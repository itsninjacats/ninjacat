# The `logs` table

What `intake/router_logs.go` stores, and what it deliberately does not.
Migration: `schema/migrations/0014_api_fidelity.sql`. The `api.<site>` intake
has its own page, `docs/tables/api.md`.

Routes, all three aliasing one handler: `POST /api/v2/logs`, `POST /v1/input`,
`POST /v1/input/:apikey`, on `http-intake.logs.<site>` and
`agent-http-intake.logs.<site>`. The response is unchanged: **`202 {}`**, always
— Datadog's logs intake answers with an empty object here, not with the error
structure the metrics endpoints use.

## Table

`logs` — `MergeTree`, `PARTITION BY toDate(timestamp)`,
`ORDER BY (tenant_id, service, host, timestamp)`, TTL 14 days (0001). Fed only by
this router. Two columns are new.

| Column | Source |
|---|---|
| `tenant_id` | the API key's tenant |
| `timestamp` | see "Timestamp resolution" below |
| `host` | `hostname`, or the `host` attribute, or the `hostname`/`host` query parameter, in that order |
| `service` | `service`, or the `service` query parameter |
| `source` | `ddsource`, or the `ddsource` query parameter |
| `status` | the `status` attribute (not a declared field of `HTTPLogItem`, despite being a reserved name in Datadog's own docs) |
| `message` | `message`, the only required field |
| `tags` | the query `ddtags` and the item's `ddtags`, MERGED, comma-split, as the usual multiset |
| `attributes JSON` | **new** — every attribute that did not become a column |
| `timestamp_source LowCardinality(String)` | **new** — which wire field produced `timestamp` |

### `attributes`

`datadogV2.HTTPLogItem` declares five fields. Everything else a sender includes
— `dd.trace_id`, `usr.*`, `http.*`, `error.*`, whole nested objects — used to be
COUNTED and dropped, one log line per batch saying how many. `dd.trace_id` is
the one key that joins a log line to its trace, so counting it was the worst
available outcome.

A native `JSON` column rather than a `String`: values are read back by attribute
(`attributes.usr.id`), nested objects keep their shape, and ClickHouse stores
each discovered path as its own subcolumn, so reading one attribute does not
read the others. `json.Number` values survive re-encoding as their original
digits, so a 64-bit `dd.trace_id` is written exactly as it arrived rather than
rounded through `float64`.

The keys lifted into real columns are REMOVED from `attributes` — a value in two
places is a value that can disagree with itself — but only when they were
actually used. A `timestamp` we could not parse stays here, because a value
dropped by both the column and the map is a value nobody can find again.

Empty attributes are written as `{}`: a JSON column rejects an empty string.

### `timestamp_source`

One of `timestamp_ms`, `timestamp_string`, `date_ms`, `date_string`, `arrival`.

Without it, "the sender did not send a timestamp" and "the sender sent one we
could not parse" are the same row — and only the second is a bug report.

## Timestamp resolution

In order, first match wins:

1. `timestamp` as a millisecond number (what the agent sends)
2. `timestamp` as an RFC 3339 string
3. `date` as a millisecond number
4. `date` as an ISO-8601 string (what the browser SDK sends)
5. arrival time

The handler used to read step 1 only. A sender using any of the other three got
ARRIVAL TIME silently — which is not a missing value but a wrong one, and one
that survives every sanity check because it always looks plausible. Step 5 is
still a real case: `HTTPLogItem` has no timestamp field at all, so an official
client cannot always send one. The difference is that the row now says so.

A candidate that does not parse falls through to the next rather than ending the
search, and the key it came from is left in `attributes`.

## Framing

Three framings arrive on this path and all three are accepted, told apart by the
first significant byte:

- `[{…},{…}]` — the agent and every official client
- `{…}` — a single bare object, which the API also accepts
- `{…}\n{…}` — NDJSON, what the browser SDK sends

A streaming decoder covers the last two at once: it reads consecutive JSON
values until EOF, which is NDJSON with the bare object as its one-line case.

## Query parameters

`ddsource`, `ddtags`, `hostname` (or `host`) and `service` are accepted as query
parameters by Datadog's intake, which is how a sender that cannot shape its body
still says what it is. They were never read here, so tags supplied only in the
URL were lost with no trace at all — not even a count, since they are not part
of any decoded item.

The body wins for the scalars. `ddtags` is the exception and MERGES, with the
query's tags first: the query form tags a whole batch, each item tags itself.

## TCP transport

`agent-intake.logs.<site>`, handled by `intake/tcplogs.go`, not this file —
the Datadog Agent's other logs transport. An agent falls back to it whenever
`logs_config.logs_dd_url` points at a custom endpoint and
`logs_config.use_http` is not forced true (and HTTPS was not reachable at
startup), which is the common case for anyone who set `logs_dd_url` without
also reading the fine print. Before this listener existed, those installs
lost every log with no error on either side — the agent believed it was
shipping, the HTTP intake never saw a connection.

**Framing.** One log per line: `<api_key> <json>\n` — the api key, one space
(`0x20`), the JSON-encoded log, one `\n` (`0x0A`). Sourced from
`server/docs/spis-endpointow-datadoga.md` §4.8's reverse-engineering notes on
the agent's own `comp/logs-library/client/tcp/` (prefixer + delimiter); see
`intake/tcplogs.go`'s header comment for what could and could not be
independently verified against agent source in this module's cache. The
agent's other framing for this transport — a 4-byte big-endian length prefix
around a protobuf payload, behind an undocumented `dev_mode_use_proto` flag —
is **not implemented**; that doc's research reads it as the default, which
this codebase could not confirm one way or the other with the sources
available to it (see the GAP note in `tcplogs.go`). Until it is verified and
added, `logs_config.dev_mode_use_proto: false` is required for this listener
to understand what an agent sends.

**TLS vs. plain.** TLS on port 10516 is the agent's own default for this
transport; set `logs_config.logs_no_ssl: true` on the agent to talk plain
instead. This server mirrors that choice rather than picking one: the
listener is plain unless both `NINJACAT_TLS_CERT` and `NINJACAT_TLS_KEY` are
set (env vars read once in `cmd/ninjacat/main.go`), in which case it serves
TLS with that certificate. Listener address: `NINJACAT_LOGS_TCP_ADDR`,
default `:10516`; set it to `off` to disable the listener entirely.

**Agent-side config, for `logs_dd_url: <endpoint>:<port>`:**

| key | for this transport |
|---|---|
| `logs_config.logs_dd_url` | `host:10516` (or your `NINJACAT_LOGS_TCP_ADDR` port) |
| `logs_config.logs_no_ssl` | `true` to match a plain (no cert configured) listener |
| `logs_config.use_http` | must NOT be `true` — that would send this agent to `http-intake.logs.<site>` instead |

**Identical to HTTP.** Every row goes through the exact same conversion HTTP
items do — `decodeLogItemJSON` (shared with `router_logs.go`) into `logRow`,
so `LogRow`'s fields, the timestamp resolution order, the `attributes` JSON
column and the tenant-from-key model are all unchanged from the rest of this
page. `storage.LogsWriter` is the same writer either transport sends to.

**Different from HTTP**, because there is no `gin.Context` or `Host` header
on a raw TCP connection:

- Batching is per CONNECTION, not per request: rows are buffered and flushed
  at 500 lines or 1 second, whichever comes first, rather than once per
  HTTP body.
- An unknown API key CLOSES THE CONNECTION right after logging it, rather
  than answering 403 to one request — the agent never reads back from this
  socket, so there is no channel to answer on, and it is not this server's
  call to keep accepting bytes under a key nobody issued.
- Undecodable JSON is stored as a `raw_payloads` row with `intake:
  "logs-tcp"` and always `reason: "decode_error"` (HTTP's `unexpected_shape`
  split is not made here), with the connection's remote address in
  `headers["Remote-Addr"]` standing in for the `Host` header HTTP would have
  recorded.

## Undecodable input

| intake | reason | when |
|---|---|---|
| `logs` | `decode_error` | the body is not JSON, or an NDJSON line is not (the error names which item) |
| `logs` | `unexpected_shape` | an item with `UnparsedObject` set, or one whose attributes cannot be re-encoded |

An item that decoded as JSON but not as a log has none of the declared fields
populated, so a row built from it would be empty strings pretending to be a log
line. Its bytes go to `raw_payloads` and its well-formed neighbours still become
rows — the item, not the batch, is the unit of failure. A batch whose tenant
cannot be resolved stores nothing, as everywhere else.

## Dropped by decision

1. **A raw copy of every event.** Deliberate, and the one place where the
   default "store it" is overridden by arithmetic: `logs` is the highest-volume
   table in the schema, and a second copy of each line would double it to
   protect against a decode path that already keeps every attribute. Only items
   we could NOT decode are copied.
2. **The `:apikey` path parameter of `/v1/input/:apikey`.** It is never read —
   `RequireAPIKey` looks at the header and the query, so a request that reaches
   the handler was already authenticated by one of those, and the path segment
   is a duplicate credential. Storing it would put a key in a table.
3. **TCP-mode logs, from this handler's point of view.** Not a decision of
   `router_logs.go`: without `logs_config.use_http=true` the agent uses its
   own TCP+TLS transport and never reaches this file's HTTP endpoints at
   all. It is not dropped overall — see "TCP transport" above for where it
   actually goes (`intake/tcplogs.go`).
