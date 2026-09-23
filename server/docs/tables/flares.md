# agent_flares

Feeds: `HEAD`/`POST /support/flare`, registered once on every intake engine
(`intake/server.go`'s `engineAuth`, not a per-product route file) because the
agent's flare uploader talks to whichever host its `dd_url` — or the core
forwarder's versioned `<maj>-<min>-<patch>-flare.agent.<site>` rewrite, see
`intake/routes.go` — happens to resolve to. `schema/migrations/0016_agent_flares.sql`
creates the table; `apps/storage/rows_flares.go` is the writer.

Before this work `HandleFlare` answered `c.Status(200)` and discarded the
body. Every upload was accepted and every archive was lost.

## What the agent actually sends

Read from the module cache, `comp/core/flare/helpers/send_flare.go`
(`$(go env GOMODCACHE)/github.com/!data!dog/datadog-agent@v0.0.0-20260521051500-e70f483e68d2/comp/core/flare/helpers/send_flare.go`),
`getFlareReader`:

1. `HEAD /support/flare[/<case_id>]` first (`resolveFlarePOSTURL`) — a
   redirect probe. It accepts either `200` or `404` as "reachable", so a bare
   `200` with no body is the whole contract; nothing is stored.
2. `POST /support/flare`, `Content-Type: multipart/form-data`,
   `Transfer-Encoding: chunked` (`ContentLength = -1`), parts **in this
   order**:

   | order | field | written when |
   |---|---|---|
   | 1 | `case_id` | non-empty (a brand-new upload sends none at all) |
   | 2 | `email` | non-empty |
   | 3 | `source` | always (`"local"` or `"remote-config"`, `FlareSource.sourceType`) |
   | 4 | `rc_task_uuid` | remote-config-triggered uploads only |
   | 5 | `flare_file` (**file**) | always — the zip archive |
   | 6 | `agent_version` | always, **after** the archive |
   | 7 | `hostname` | always, **after** the archive |

   `agent_version`/`hostname` arriving after the file part is the one thing
   worth calling out explicitly: a handler that assumed metadata precedes the
   archive (reasonable, since it does for every other field) would silently
   lose both. `HandleFlare` reads every part with a plain `multipart.Reader`
   and switches on `part.FormName()`, so order does not matter to it — see
   `intake/router_security.go`'s `HandleSecDump` for the same pattern against
   CWS's two-part body.

3. The response, parsed by `analyzeResponse`: the agent's own (unexported)
   `flareResponse{CaseID int, Error string, RequestUUID string}` — same file —
   unmarshalled from the body. It requires `Content-Type: application/json`
   on a `200`; anything else reads as `"could not deserialize response body"`
   regardless of what the body actually contains. A non-empty `Error` is
   reported to the operator as the upload having failed even though the HTTP
   status was `200`. `HandleFlare` mirrors this struct (`case_id`, `error`,
   `request_uuid`, all three keys always present — `omitempty` is
   deliberately not used, since an explicit empty `error` on success is part
   of the contract, not an absent key).

## Columns

| Column | Source | Notes |
|---|---|---|
| `tenant_id` | `TenantFromContext(c)` | first `ORDER BY` column, per CLAUDE.md |
| `received_at` | `time.Now().UTC()` | arrival time |
| `hostname` | form field `hostname` | written after the archive on the wire |
| `case_id` | form field `case_id`, kept **as the string sent** | empty means "new case" — see below |
| `email` | form field `email` | |
| `source` | form field `source` | `local` \| `remote-config` |
| `agent_version` | form field `agent_version` | written after the archive |
| `filename` | the `flare_file` part's own filename | e.g. `datadog-agent-2026-09-23-10-30-00.zip` |
| `size_bytes` | `len(archive)` | |
| `fields` | every other multipart field | `rc_task_uuid` today; whatever a future agent version adds tomorrow, kept rather than dropped |
| `archive` | the `flare_file` part's bytes, verbatim | `ZSTD(3)`, same bulky-cold-column treatment as `raw_payloads.body`/`k8s_manifests.content` |

`case_id` stays a `String`, not parsed to an integer: an absent value is a
real, distinct fact ("no case exists yet"), and coercing it to `0` would
manufacture a case id that means something to nobody. This is separate from
the **response** `case_id`, which the agent's `analyzeResponse` requires as a
number: `intake/server.go`'s `flareCaseIDNumber` echoes the form value back
when it parses as an integer, and otherwise mints one the same way
`HandleEvents` (`router_api.go`) mints an event id — the low 63 bits of a
fresh UUID, kept inside `int64`, unique but not meaningful beyond that.

## Engine / ORDER BY / PARTITION BY / TTL

`MergeTree`, `PARTITION BY toYYYYMM(received_at)`,
`ORDER BY (tenant_id, received_at)`,
`TTL toDateTime(received_at) + INTERVAL 90 DAY`.

90 days and monthly partitions, not the 30-day/daily class the bulky-document
tables (`raw_payloads`, `k8s_manifests`) otherwise resemble: a flare is a
support artifact a human explicitly asked for, not a debugging sample or a
telemetry stream, so it gets the same retention class as `check_runs` /
`events` / `k8s_actions` rather than the "reverse-engineer a format" class
`raw_payloads` sits in. Volume is a few uploads per incident, never a steady
stream, so monthly partitioning costs nothing.

## Writer tuning

`MaxRows: 200`, `FlushInterval: 10s`, `BufferLimit: 5_000`, `MaxInFlight: 1` —
the same numbers as `k8s_manifests` (`apps/storage/rows_k8s.go`): a row is a
whole zip archive, so these counts stand for megabytes, not row volume, and a
single in-flight flush is plenty for something this infrequent.

## Failure paths

* Content-Type is not `multipart/form-data`, or has no boundary → `storeRaw`
  under `flare`/`decode_error`; the response is still the documented
  `200` + JSON, with a non-empty `error` — refusing the request would only
  make the agent retry the same unreadable upload.
* The multipart body parses but carries no `flare_file` part → `storeRaw`
  under `flare`/`decode_error`; same response shape.
* No tenant in context → nothing is stored (the standing `storeRaw`/`store`
  rule: `tenant_id` is the first `ORDER BY` column everywhere, and a guessed
  value would put the row where no query looks), but the response is still
  answered — the agent's contract does not depend on our multi-tenancy.

## Not verified against the agent's own receiver code

The response contract above (`{case_id, error, request_uuid}`, `Content-Type`
requirement) is read directly from the agent's **sender** —
`comp/core/flare/helpers/send_flare.go`'s `analyzeResponse`, in this repo's
module cache — which is authoritative for what the agent itself parses.
There is no separate server-side reference implementation to compare against
in this codebase.
