# misc: apm_telemetry, and what the other misc/install routes store

Covers `intake/router_misc.go` and `intake/router_install.go`. One ClickHouse
table (`apm_telemetry`), migration `schema/migrations/0011_apm_telemetry.sql`,
writer file `apps/storage/rows_apm_telemetry.go`. Everything else in these two
routers either has nothing to store (the install host's own routes) or has no
published schema and goes to the shared `raw_payloads` fallback table instead.

## apm_telemetry

Fed by `POST /api/v2/apmtelemetry` on `instrumentation-telemetry-intake.<site>`
(`HandleTelemetry`, `intake/router_misc.go`).

- **Engine**: `MergeTree` — append-only facts, like `events`/`logs`.
- **PARTITION BY** `toDate(received_at)`.
- **ORDER BY** `(tenant_id, received_at)` — the dominant query shape is
  "everything in a time window", same class as `events`; nothing here is
  queried "one metric, one host" the way `metrics` is.
- **TTL** 14 days (the `logs` class): this is diagnostic telemetry about the
  tracers/agents/installer themselves, produced by every instrumented process
  on its own heartbeat, not a record anything else depends on.
- **Writer tuning**: `MaxRows 2000, FlushInterval 5s, BufferLimit 50_000,
  MaxInFlight 2` — the `k8s_resources` class (bursty-per-pass): a deploy makes
  many processes report within the same second, and a single `message-batch`
  request turns into several rows on top of its own (see below).

### One request, one or more rows

Every request produces exactly one row for itself. `request_type
"message-batch"` additionally produces one more row per entry of its
`payload` array:

- the **parent row** keeps `request_type = "message-batch"` and `payload` =
  the whole batch array, verbatim — "what did this HTTP request contain" is
  always answerable from one row;
- each **child row** gets that entry's own `request_type` and `payload`,
  `batch_index` = its 0-based position, `parent_request_type =
  "message-batch"`, and otherwise the same envelope-level fields as the
  parent (`tenant_id`, `runtime_id`, `application`, `host`, …) — a batch
  entry has no envelope of its own, so there is nothing else to give it.

`batch_index` is `Nullable(UInt32)`, `NULL` on every non-child row. A pointer
(`*uint32`) on the Go side, never a `0` sentinel: entry 0 of a real batch must
stay distinguishable from "not a batch entry at all".

### Column-to-source-field mapping

| Column(s) | Source |
|---|---|
| `tenant_id` | `TenantFromContext(c)` |
| `received_at` | arrival time (`time.Now().UTC()`) — nothing on the wire is trustworthy as *the* row timestamp, since only some producers send `tracer_time`/`event_time` |
| `request_type`, `api_version`, `runtime_id` | envelope keys of the same name |
| `producer` | derived by `telProducer(env, request_type)` — `request_type` alone is ambiguous for `logs`/`traces`/`message-batch`, shared by more than one producer; the fallback reads `event_time`/`origin`/`runtime_id` presence |
| `seq_id` | envelope `seq_id`, parsed through `json.Number` (`apmSeqID`) — never through `float64` or a bare `interface{}` decode, either of which would round a large sequence number past 2^53 |
| `tracer_time`, `event_time` | envelope keys of the same name, parsed as Unix seconds with an optional fractional part (`apmUnixTime`) |
| `tracer_time_raw`, `event_time_raw` | the literal JSON text of the same two keys, kept even when the parse above fails — a parser bug must not erase the value |
| `service_name`…`tracer_version` | `application.{service_name,service_version,env,language_name,language_version,tracer_version}` |
| `hostname`, `host_os`, `host_architecture` | `host.{hostname,os,architecture}` |
| `host_extra` | every other key of the `host` object, JSON-text-encoded by name (gohai and the tracer both send more than three host fields) |
| `payload` | the request's own `payload`, raw JSON text (the whole batch array for a parent row, one entry's payload for a child row) |
| `debug` | envelope `debug`, raw JSON text (documented on the tracer/installer envelope; not parsed, no fixed type) |
| `origin` | envelope `origin` — presence (not value) is the fleet-installer signal used by `telProducer`'s fallback |
| `via`, `dd_agent_hostname`, `dd_agent_env`, `datadog_container_id`, `x_datadog_container_tags` | the trace-agent proxy's own headers (`Via`, `DD-Agent-Hostname`, `DD-Agent-Env`, `Datadog-Container-Id`, `X-Datadog-Container-Tags`) — never in the JSON body |
| `extra` | every envelope key besides the ones above, JSON-text-encoded by name (`apmExtraKeys`) — an undeclared key from a newer producer stays visible instead of disappearing |
| `batch_index`, `parent_request_type` | see "One request, one or more rows" above |

### Semantics worth knowing

- No upsert key: `MergeTree`, append-only, one row per request/entry.
- No sensitive columns by design — the JSON envelope this table stores does
  not carry credentials, and the proxy headers kept are identity/routing
  headers (agent hostname, container id), not secrets. `raw_payloads` (below)
  is the table with an explicit header allowlist for that reason; this one
  has none because nothing here needed excluding.
- `payload`/`debug`/`host_extra`/`extra` are raw JSON text, not a typed
  column: none of the five producers' Go types is importable from this
  module (nested in internal agent packages, or unexported), so the schema
  claims no more authority over their shape than the JSON already has. See
  `intake/router_misc.go`'s header comment for the full per-producer shape
  reference.

## Other routes in these two files

- **`resources-intake.<site>` `/api/v2/genresources`** (`HandleGenResources`)
  — no `.proto` exists for this track anywhere upstream (the agent forwards
  an integration's bytes untouched). Every request goes to `raw_payloads`
  with `reason = "no_schema"`; the note carries `DD-EVP-ORIGIN`,
  `DD-EVP-ORIGIN-VERSION` and `Content-Type`, since there is no row of
  columns for them to land in.
- **`eudm-intake.<site>` `/api/v2/aiusage`** (`handleAIUsage`,
  `router_install.go`) — same treatment: no published schema, every request
  to `raw_payloads` with `reason = "no_schema"` and the same origin-header
  note.
- **`llmobs-intake.<site>` `/api/v2/llmobs`** (`handleLLMObs`,
  `router_install.go`) — the agent's only real caller is the connectivity
  diagnose (empty body or `X-Requested-With: datadog-agent-diagnose`), which
  stores **nothing at all**: it is a probe, not a payload. A real payload
  (not expected from the agent) still goes to `raw_payloads` with `reason =
  "no_schema"`, as a safety net.
- **`install.datadoghq.com` / `install.datad0g.com`** (`router_install.go`'s
  `routeInstall` group: the HEAD probe, `/v2/`, `/v2/:repo/manifests/:tag`,
  `/v2/:repo/blobs/:digest`, `/btfs/...`) — **nothing is stored here, by
  design, not as a gap.** Every response is a fixed protocol answer (`200`
  probe/version-check acks, or a `404` in the OCI or BTF error shape) and no
  route reads a request body at all — there is nothing to store. This was
  true before this change and stays true after it.

## dropped_by_decision

Nothing decoded by these two files is dropped. Everything that reaches a
handler either gets a real column (`apm_telemetry`'s envelope fields) or
travels whole into `raw_payloads` (`genresources`, `aiusage`, an unexpected
`llmobs` payload, and any `apmtelemetry` body that is not a JSON object,
`reason = "decode_error"`). The one thing genuinely not stored is the
`llmobs` diagnose/empty-body probe and the install host's fixed-response
routes — both are protocol handshakes with no payload to keep, not data being
discarded.
