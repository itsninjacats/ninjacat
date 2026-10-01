# ninjacat-api

The NinjaCat server, in F#: one process that takes what Datadog Agents send,
stores it in ClickHouse and answers queries about it.

| Port | Who reaches it | What it serves |
| --- | --- | --- |
| 8080 (`NINJACAT_ADDR`) | agent machines | the intake: Datadog's wire protocol, behind an API key |
| 8081 (`NINJACAT_INTERNAL_ADDR`) | the panel only | `/internal/*` for the panel and Datadog's query API. No key is asked for, so it stays on an internal network |
| 10516 (`NINJACAT_LOGS_TCP_ADDR`) | agent machines | the agent's TCP transport for logs; `off` disables it |

The query side speaks Datadog's public API (`/api/v1/...`, `/api/v2/...`)
rather than one of our own, so the panel, `datadog-api-client`, Terraform and
Grafana's Datadog plugin can all point at it unchanged.

## Layout

- `src/Proto` — the agent's protobuf schemas and the C# generated from them.
  The only C# in the tree, and none of it written by hand
  (`protos/README.md` says where each schema comes from).
- `src/Engine` — parsers, AST, compiler to SQL. No reference to ASP.NET,
  Oxpecker or the ClickHouse driver: it returns SQL text plus typed parameters
  (`Sql.fs`) and nothing else, so it is tested without a server.
  `Api/V2/Wire.fs` holds Datadog's v2 query shapes, each type named exactly
  after its schema in Datadog's OpenAPI spec; `Panel.fs` holds the panel's
  own queries.
- `src/Storage` — the write path. `Rows/*.fs` has one record and one
  `Table` per ClickHouse table; `Writer.fs` buffers and flushes one table;
  `ClickHouseSink.fs` owns one writer per table; `Migrations.fs` applies
  `schema/migrations/*.sql`, which are embedded in the binary.
- `src/Intake` — the agent-facing side. `Routes.fs` dispatches on the `Host`
  header, `Engine.fs` matches the route and checks the key, `Routers/*.fs`
  turn a payload into rows.
- `src/Server` — the host: configuration, the two HTTP surfaces, the API key
  keeper, self-monitoring. `ClickHouse.fs` and `Postgres.fs` are the only
  files that open a database connection.
- `tests/Engine.Tests`, `tests/Intake.Tests` — xunit v3 on
  Microsoft.Testing.Platform.

Postgres (users, API keys, later monitors and dashboards) is reached through
plain Npgsql, no ORM. Its schema belongs to Drizzle in `frontend/`: this
service never migrates it, and a table it needs is added there first.

## The intake

**Routing follows Datadog's hostnames.** Datadog puts every product on its own
host (`app.<site>`, `trace.agent.<site>`, `http-intake.logs.<site>`, about
forty more), and so does the table in `Routes.fs`. A host it does not know is
refused by name, so a wrong `DD_SITE` fails loudly. That is also why
`curl localhost:8080/ping` answers 404: send
`-H 'Host: api.ninjacat.local'`.

**A handler is a function `Request -> Response`.** The engine has already read
and decompressed the body and found the key's tenant. The handler turns the
payload into rows, hands them to the sink and returns: the agent gets its 202
before anything reaches ClickHouse.

**One writer per table, never a pool.** Separate buffers stop a burst of logs
from delaying metrics; several writers for one table would shrink the batches
and bring back ClickHouse's "Too many parts". A slow insert is handled by
flushing in the background, up to `MaxInFlight` at once, not by adding writers.

**A payload that cannot be stored as rows is kept whole** in `raw_payloads`,
with the reason, so nothing an agent sent is lost to a decoder we have not
written yet.

**API keys are looked up without a lock.** The keeper reads Postgres every
30 seconds (and at once when the panel says a key changed) and publishes an
immutable snapshot; a request reads the current one. Postgres holds only the
SHA-256 of a key.

**`tenant_id` is the first `ORDER BY` column everywhere.** It comes from
`api_key.tenant_id` and is written into every row. Today it is always
`"default"`.

### Adding a router

1. Rows: a record and a `Table` in `src/Storage/Rows/<X>.fs` (columns in
   INSERT order), and a migration in `schema/migrations/` if the table is new.
   A migration that has been applied anywhere is never edited; add the next one.
2. Handlers and their route list in `src/Intake/Routers/<X>.fs`.
3. The host in `Routes.fs`.
4. Tests in `tests/Intake.Tests/<X>Tests.fs`.

`Routers/Logs.fs` with `Rows/Logs.fs` is the example to read first.

## Tests

```bash
dotnet test                                         # everything
cd tests/Intake.Tests && dotnet test                # the intake and storage
NINJACAT_GOLDEN_ROUTES=routeLogs,routeAPI dotnet test   # golden fixtures of those route sets only
```

- **Golden fixtures** (`tests/Intake.Tests/Fixtures`): requests recorded from
  the Go server this one replaced, each with the answer it gave and the rows
  it stored. Every one is replayed and compared field by field. See
  `Fixtures/README.md`.
- **ClickHouse round trip**: with the dev ClickHouse running, a scratch
  database is built from the migrations and every fixture's rows are inserted
  through the real sink.
- **Real senders** (`Fixtures/real`): payloads recorded from a real browser
  SDK, the agent's DBM integrations against seven databases, its SNMP,
  NetFlow, traceroute and data security checks, and system-probe. They are
  what the decoders are held to where the Go recordings say nothing.
  `lab/README.md` says how they were recorded.
- One `<Router>Tests.fs` per router for what the fixtures do not reach.

## Calling the engine from F#

Monitors and other in-process code use `Query.Engine`, not HTTP: the same
query and formula strings go in, plain records come out.

```fsharp
let! result =
    Engine.timeseries execute tenant
        { From = now.AddHours -1.0; To = now; Interval = None
          Queries = [ { Name = "cpu"; Query = "avg:system.cpu.user{env:prod} by {host}" } ]
          Formulas = [ "anomalies(cpu, 'agile', 2)" ] }
// Ok [ { Formula = "anomalies(cpu, 'agile', 2)"
//        Series = [ { Tags = [ { Key = "host"; Value = Some "web-1" } ]
//                     Points = [ { Time = …; Value = 41.2; Expected = Some { Lower = 35.0; Upper = 47.1 } }; … ]
//                     Forecast = [] }; … ] } ]
// or Error [ "queries[0] (cpu): …" ] — every problem, named

let! values = Engine.scalar execute tenant { …; Queries = [ { …; Aggregator = ScalarAvg } ] }
// Ok [ { Formula = "cpu"; Values = [ { Tags = [ … ]; Value = 41.7 }; … ] } ]
```

`execute` is how the engine reaches ClickHouse — `Server/ClickHouse.fs`
`metricRows` in the service, a list in tests.

## Known deviations from Datadog

Decisions, not gaps: places where NinjaCat behaves differently on purpose.

- **`trace.*` metrics are not metrics here.** Datadog turns the agent's APM
  stats into ordinary metrics (`sum:trace.http.request.hits{service:web}`),
  queryable like any other. NinjaCat keeps APM stats in their own table
  (`apm_stats`) and does not merge them into the metrics path: a metric query
  for `trace.*` finds nothing. APM is to be queried through its own data
  sources (`apm_metrics`, `apm_resource_stats`, `apm_dependency_stats`, as
  Datadog's v2 query API defines them), not built yet. Decided 2026-09-27.
- **RUM likewise** stays in its own tables (`rum_events`, `rum_views`, …),
  queried as events, not as metrics.

## Where Datadog is silent

Datadog documents its query language unevenly. Wherever we had to decide
behaviour the docs do not pin down, the code says so at that spot:

```bash
grep -rn "WARNING(undocumented)" src   # a choice we made; check against Datadog when possible
grep -rn "FIXME(" src                   # a choice we doubt
```

Each such comment names the question, then the answer we picked. When one is
checked against a real Datadog account, replace the warning with the source.

## Differences from the Go server

This server replaced one written in Go and was held to its behaviour by the
golden fixtures. Where it differs:

- Process-agent frames compressed with zstd 0.x (encoding 2) are refused by
  name and kept in `raw_payloads`. Agent 7.84 sends zstd 1.x (encoding 4),
  which is read.
- Invalid UTF-8 in a text column is stored as U+FFFD; Go stored the bytes.
- A few malformed payloads land in `raw_payloads` where Go stored rows, or the
  reverse: protobuf nested deeper than 100, invalid UTF-8 in a v0.4 trace's
  protobuf strings, a declared RUM field of the wrong type.
- Notes on a refused payload quote .NET's error text, not Go's
  (`Fixtures/overrides.json` lists each).
- Capture dumps redact the API key headers.
- Self-monitoring: the Ergo node's metrics are gone, `ninjacat.node.goroutines`
  became `ninjacat.node.threads`, and `ninjacat.storage.rows.{written,dropped,failed,buffered}`
  are new.
- ClickHouse is reached over HTTP (`CLICKHOUSE_HTTP_ADDR`, :8123), not the
  native protocol (`CLICKHOUSE_ADDR`, :9000).

Where the Go server was wrong, this one is not held to it:

- `GET /api/v2/validate` is served (Go had only POST) and answers the same
  org id on every call: the trace-agent derives its Org Propagation Marker
  from it.
- A connection's container and remote-service tags are read from
  `encodedTags`, where the agent writes them; Go read `encodedConnectionsTags`.
  The agent's placeholder set `"-"` at index 0 means "none".
- A `null` in a list of series, points, resources, sketches or checks is left
  out; Go lost the whole batch on it, or stored an empty element.
- One v1 series or distribution that is not a series is kept raw alone; Go
  kept the whole body raw and stored nothing.
- `/api/v2/intake-key` gives back the key that came in the query string too.
- `event_management_events.attributes` is `{}` when the event has none, not
  the text `null`.
- A packfile upload without a `packfile` part is kept raw and stores no pack.
- A test-cycle body with bad events is kept raw once, not once per bad event.
- The agent's `{}` probe on a DBM route stores no row.
- A DBM plan whose definition is EXPLAIN output inside a string (postgres,
  mysql) is a plan; Go knew only the oracle check's list of steps and marked
  the others undecoded.
- `GET /api/v2/profiling/quota` answers the browser profiler, and
  browser-intake takes the key from `DD-CLIENT-TOKEN` too.
- A JSON body nested deeper than `Json.maxDepth` (512) is kept in
  `raw_payloads`. Go read to 10 000, and so did this server until it turned
  out that four routes answered 500 past 1000.
- MessagePack is read by the MessagePack library, not by a port of Go's msgp:
  notes on a refused APM stats or Data Streams payload name the field
  (`Stats/0/Stats/2/Hits: expected an unsigned integer, got the integer -5`),
  and a nil element of a Data Streams array is left out instead of becoming
  an all-zero row.
- A NaN or an infinity among a span's attributes is written as the text
  `"NaN"`, `"+Inf"` or `"-Inf"`; Go replaced the whole column with an error
  marker and lost the other attributes.

And what the Go server kept as bytes or in `raw_payloads` is decoded:

- What system-probe saw inside a connection (HTTP, HTTP/2, Kafka, Postgres,
  Redis) is a row per endpoint, topic or operation: `connection_http_stats`,
  `connection_kafka_stats`, `connection_database_stats`.
- Sensitive Data Scanner results are `sds_scans`, `sds_results` and
  `sds_matches`.
- The resources snapshot on `/intake/` is `process_groups`.

## Commands

```bash
dotnet build
dotnet test
DATABASE_URL=postgres://root:mysecretpassword@localhost:5433/local \
  dotnet run --project src/Server        # :8080 intake, :8081 internal
dotnet watch --project src/Server run    # with reload
dotnet run --project src/Server -- migrate   # apply the ClickHouse migrations and exit
```

Package versions live in `Directory.Packages.props` only.

## Environment

| Variable | Default |
| --- | --- |
| `NINJACAT_ADDR` | `:8080` — the intake |
| `NINJACAT_INTERNAL_ADDR` | `:8081` — panel and query API |
| `NINJACAT_LOGS_TCP_ADDR` | `:10516`; `off` disables the listener |
| `NINJACAT_TLS_CERT`, `NINJACAT_TLS_KEY` | PEM files; set both and the logs TCP listener speaks TLS |
| `CLICKHOUSE_HTTP_ADDR` | `localhost:8123` |
| `CLICKHOUSE_DB`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD` | `ninjacat` |
| `DATABASE_URL` | required — the same `postgres://` URL the panel uses |
| `NINJACAT_AUTO_MIGRATE` | `true`; with several replicas set `false` and run `migrate` as a job |
| `NINJACAT_ACK_UNKNOWN` | `true` answers 202 instead of 404 on unknown intake paths |
| `DEBUG` | `true` dumps every intake request to `NINJACAT_CAPTURE_DIR` (default `captures`) |
| `NINJACAT_SELFMON_INTERVAL`, `NINJACAT_SELFMON_TENANT`, `NINJACAT_SELFMON_HOST` | `15s`, `default`, the machine's name |

## Internal endpoints (:8081)

| | |
| --- | --- |
| `GET /ping` | liveness |
| `GET /health` | ClickHouse and Postgres reachability; 503 if either is down |
| `GET /api/v1/metrics?from=<unix s>[&host=]` | Datadog's active metrics list; `tag_filter` not yet |
| `POST /api/v2/query/timeseries`, `/api/v2/query/scalar` | Datadog's v2 query API: queries, formulas, functions |
| `GET /internal/metrics/{names,hosts,tags,tag-values,query}` | the panel's metric pages |
| `GET /internal/logs/{search,facets}` | the panel's log pages |
| `POST /internal/apikeys/refresh`, `GET /internal/apikeys/status` | the panel says a key changed; the keeper's state |

Tenant is `"default"` on this port until requests carry one.
