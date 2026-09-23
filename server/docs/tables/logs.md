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
3. **TCP-mode logs.** Not a decision of this handler: without
   `logs_config.use_http=true` the agent uses its own TCP+TLS transport and
   never reaches an HTTP endpoint at all.
