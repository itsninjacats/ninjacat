# ninjacat-api

The NinjaCat server, in F#. Two programs, three processes, sharing only
ClickHouse and Postgres: `ninjacat-api intake` takes what agents send,
`ninjacat-api query` answers the panel (never the same port or container), and
`ninjacat-monitors` evaluates monitors as a cluster of its own.

| Process | Port | Who reaches it | What it serves |
| --- | --- | --- | --- |
| `ninjacat-api intake` | 8080 (`NINJACAT_ADDR`) | agent machines | the intake: Datadog's wire protocol, behind an API key |
| | 10516 (`NINJACAT_LOGS_TCP_ADDR`) | agent machines | the agent's TCP transport for logs; `off` disables it |
| `ninjacat-api query` | 8081 (`NINJACAT_INTERNAL_ADDR`) | the panel only | `/internal/*` for the panel and Datadog's query API, behind a token the panel signs (below); still on an internal network. It only reads, and can be given a ClickHouse user that cannot write |
| `ninjacat-api migrate` | | | applies the ClickHouse migrations and the metrics retention, and exits |
| `ninjacat-monitors` | 2552 (`NINJACAT_MONITOR_PORT`) | the other monitor nodes | Akka remoting, the cluster's own traffic |
| | 8558 (`NINJACAT_MONITOR_MANAGEMENT_PORT`) | the other monitor nodes | Akka.Management, for the cluster bootstrap in Kubernetes only |
| | 8559 (`NINJACAT_MONITOR_HEALTH_ADDR`) | the kubelet | `/health/ready` and `/health/live`, nothing else |

The query side speaks Datadog's public API (`/api/v1/...`, `/api/v2/...`)
rather than one of our own, so the panel, `datadog-api-client`, Terraform and
Grafana's Datadog plugin can all point at it unchanged.

## Layout

- `src/Proto` — the agent's protobuf schemas and the C# generated from them.
  The only C# in the tree, and none of it written by hand
  (`protos/README.md` says where each schema comes from).
- `src/Engine` — parsers, AST, compiler to SQL. No reference to ASP.NET,
  Oxpecker, Akka or the ClickHouse driver: it returns SQL text plus typed
  parameters (`Sql.fs`) and nothing else, so it is tested without a server.
  `Api/V2/Wire.fs` holds Datadog's v2 query shapes, each type named exactly
  after its schema in Datadog's OpenAPI spec; `Panel.fs` holds the panel's
  own queries (its metric series go through the same planner as the query
  API). `Monitor/` is everything a monitor means, as pure functions: the
  definition, its parser, options and validation, the evaluation of each
  monitor type, and the state machine of its groups. The query process
  validates with it and the monitor service evaluates with it.
- `src/Storage` — the write path. `Rows/*.fs` has one record and one
  `Table` per ClickHouse table; `Writer.fs` buffers and flushes one table;
  `ClickHouseSink.fs` owns one writer per table; `Migrations.fs` applies
  `schema/migrations/*.sql`, which are embedded in the binary.
- `src/Intake` — the agent-facing side. `Routes.fs` is the whole map: every
  host, and every path on it as an Oxpecker route (ASP.NET's router does the
  matching); `Auth.fs` is the API key as an ASP.NET authentication scheme;
  `Routers/*.fs` hold the handlers, Oxpecker's own, which turn a payload into
  rows.
- `src/Platform` — what every process shares: configuration, the web host's
  defaults, self-monitoring. Every ClickHouse client is made in its
  `ClickHouse.fs`, and every Postgres URL becomes a connection string in its
  `Postgres.fs`.
- `src/Server` — `ninjacat-api`: the two HTTP surfaces and the API key keeper.
- `src/Monitors` — `ninjacat-monitors`: the actors, the reads of the
  `monitor` table, the writes of what the monitors went through.
- `tests/Engine.Tests`, `tests/Intake.Tests`, `tests/Query.Tests`,
  `tests/Monitors.Tests` — xunit v3 on Microsoft.Testing.Platform.

Postgres (users, tenants, API keys, monitors) is reached through plain
Npgsql, no ORM. Its schema belongs to Drizzle in `frontend/`: this
service never migrates it, and a table it needs is added there first.

## The intake

**Routing follows Datadog's hostnames.** Datadog puts every product on its own
host (`app.<site>`, `trace.agent.<site>`, `http-intake.logs.<site>`, about
forty more), and so does `Routes.fs`: a plain list of host prefixes, each with
a router of its own. A host it does not know is refused by name, so a wrong
`DD_SITE` fails loudly. Paths are the framework's to match, so they ignore
case and a trailing slash, and a known path under another method is a 405. That is also why
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
`api_key.tenant_id` and is written into every row. A tenant is the panel's
Better Auth organization, and `tenant_id` is its id. The query process takes
the tenant from the panel's token (see "What `query` serves"), never from the
request, and nothing falls back to `default`.

### Adding a router

1. Rows: a record and a `Table` in `src/Storage/Rows/<X>.fs` (columns in
   INSERT order), and a migration in `schema/migrations/` if the table is new.
   A migration that has been applied anywhere is never edited; add the next one.
   The history was squashed into one `0001_initial.sql` on 2026-10-13: a database
   built before that has a ledger that no longer matches, and must be dropped.
2. Handlers (`byte[] -> EndpointHandler`, bound with `bindBody`) in
   `src/Intake/Routers/<X>.fs`.
3. In `Routes.fs`: the host's routes, and a line for the host in `configure`.
4. Tests in `tests/Intake.Tests/<X>Tests.fs`.

`Routers/Logs.fs` with `Rows/Logs.fs` is the example to read first.

## The monitor service

`ninjacat-monitors` evaluates metric monitors: threshold,
change, anomaly, forecast and outlier, simple or multi alert. The panel owns
the definitions (`/app/monitors`); this process only reads them, evaluates,
and stores what happened.

```
panel ──(monitor row + pg_notify, one transaction)──▶ Postgres `monitor`
                                                          │ LISTEN ninjacat_monitors + re-read every 30 s
manager (cluster singleton) ── Ensure / Delete ──▶ shard region "metric-monitor", one entity per monitor
entity: aligned tick → node's gate → Engine/Monitor → ClickHouse
        → monitor_transitions, monitor_evaluations
notifier (cluster singleton): monitor_transitions → monitor_notifications
query process: /internal/monitors/* reads those tables for the panel
```

**It is an Akka.NET cluster.** Every node carries the `metrics` and `notifier`
roles; each monitor is one sharded entity (`Cluster.fs`, 100 shards, fixed:
changing the count moves every monitor). The manager and the notifier are
cluster singletons. Nothing asks an actor for its state, so the panel never
depends on the cluster being up: it reads ClickHouse through the query
process. There is no Akka.Persistence: Postgres holds the definitions,
ClickHouse holds the transitions, and an entity rebuilds its groups from their
latest transitions when it starts.

An entity is given the tick time, since the engine never reads the clock; the
tenant comes from the definition and is bound into every statement.

**Load is capped per node.** Loads, evaluations and transition inserts take a
slot of one `SemaphoreSlim` per node (`NINJACAT_MONITOR_CONCURRENCY`). A tick
that finds no slot within one interval, or finds the previous evaluation still
running, is recorded as skipped: overload shows as skipped evaluations, not as
errors, so watch `ninjacat.monitor.evaluations{outcome:skipped}`.

**Transitions are awaited, evaluations are not.** The buffered writer drops a
failed batch by design, so transitions are inserted directly and awaited
(`ClickHouseSink.InsertAsync`), and an evaluation is not adopted until its
transitions are stored. A failed insert is retried with the same rows before
anything new is evaluated, for as long as the entity lives. On a hand-off it
makes one more attempt and stops; the next owner starts from the last stored
state, so a change that still holds is recorded again on its next tick and one
that reverted in between is not. A crash loses what was pending the same way.
The key is `(tenant, monitor, group, at)`: a tick's transitions are keyed by
the aligned tick, a definition change or a delete by when it was handled.
`ReplacingMergeTree` collapses a retried batch's identical rows; read with
`FINAL` or `argMax`.

**The notifier is a stub.** Every 5 s it finds the transitions that would be
notified, logs them and writes a `stub` row to `monitor_notifications`.
Message rendering, channels, renotify and downtimes are open work (`TODO.md`).

**Formation.** Without `NINJACAT_MONITOR_DISCOVERY`, a node seeds a cluster of
its own at `NINJACAT_MONITOR_HOST:NINJACAT_MONITOR_PORT`: one node, which is
how compose runs it. With `NINJACAT_MONITOR_DISCOVERY=kubernetes`,
Akka.Management's cluster bootstrap lists the pods through the Kubernetes API
(the chart's ServiceAccount may `get/list/watch` pods), and the first
formation waits for two contact points; `NINJACAT_MONITOR_HOST` must then be
the pod's IP. Split-brain resolution is keep-majority, so run three replicas
on three nodes (the chart's default anti-affinity). Remoting is plain TCP;
the chart admits only monitor pods to 2552 and 8558 (`TODO.md`, section 0).

**Health.** Akka.Management for .NET serves no health routes, so the process
has a small listener of its own: `/health/ready` is 200 once the node has
joined the cluster, `/health/live` while its actor system runs. Discovery
lists running pods, not ready ones, so readiness does not hold up the
bootstrap. A stop runs Akka's coordinated shutdown, given 45 s (the chart
gives the pod 60).

**Databases.** Read-only on Postgres's `monitor` table; on ClickHouse it reads
the metrics and writes its own tables and self-monitoring, with no DDL: the
intake (or `ninjacat-api migrate`) creates them. The statements are in
`helm/ninjacat/values.yaml` (`clickhouse.monitorsUser`,
`monitors.ownDatabaseUrl`).

**Self-monitoring** adds `ninjacat.monitor.*` (`src/Monitors/Telemetry.fs`) to
the usual `ninjacat.node.*` and `ninjacat.storage.*`.

### Running it

In the dev stack it is the compose service `monitors` (one node, no published
port; `docker exec ninjacat-monitors curl -s localhost:8559/health/ready`).
Stop it before running a node by hand (`dotnet run --project src/Monitors`)
against the same databases: two clusters would both evaluate every monitor.
The image is the `monitors` target of `api/Dockerfile`.

Its own settings:

| Variable | Default |
| --- | --- |
| `NINJACAT_MONITOR_CONCURRENCY` | `8` — loads, evaluations and transition inserts at once on this node |
| `NINJACAT_MONITOR_MAX_GROUPS` | `1000` — the most groups one evaluation may yield; more is an error (`too_many_groups`), never a truncation |
| `NINJACAT_MONITOR_HOST` | `localhost` — the address other nodes reach this one at; required, as the pod's IP, with discovery |
| `NINJACAT_MONITOR_PORT` | `2552` — Akka remoting |
| `NINJACAT_MONITOR_DISCOVERY` | unset: the node seeds itself. `kubernetes`: cluster bootstrap over the Kubernetes API |
| `NINJACAT_MONITOR_POD_SELECTOR` | `app=ninjacat-monitors` — the label selector discovery lists pods by (the chart sets its own labels) |
| `NINJACAT_MONITOR_MANAGEMENT_PORT` | `8558` — Akka.Management, used only with discovery |
| `NINJACAT_MONITOR_HEALTH_ADDR` | `:8559` — where the health routes are served |

A set value that is not a positive whole number, or a discovery other than
`kubernetes`, stops the start.

## Tests

```bash
dotnet test                                         # everything
cd tests/Intake.Tests && dotnet test                # the intake and storage
NINJACAT_GOLDEN_ROUTES=routeLogs,routeAPI dotnet test   # golden fixtures of those route sets only
cd tests/Monitors.Tests && dotnet test              # the monitor service
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
- **Monitors**: `Engine.Tests` covers what a monitor means; `Monitors.Tests`
  runs the actors on Akka's TestKit and three nodes on localhost, and skips a
  test whose database is not running.
- **Query** (`Query.Tests`): the query token, that a request answers for
  its token's tenant alone, and `/internal/monitors/*`.

## Calling the engine from F#

In-process code uses `Query.Engine`, not HTTP: the same query and formula
strings go in, plain records come out. Monitors go one level lower
(`Monitor/Evaluate.fs` calls `Plan.plan` and `Execute`), since they cache
anomaly models between ticks and reduce `.as_count()` queries before the
formula.

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

`execute` is how the engine reaches ClickHouse — `Platform/ClickHouse.fs`
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
grep -rn "WARNING(undocumented)" src   # a choice we made; check against a documented source or a recorded payload
grep -rn "FIXME(" src                   # a choice we doubt
```

Each such comment names the question, then the answer we picked. When one is
settled by a public source (documentation, an open-source client, a recorded
payload), replace the warning with that source.

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
- JSON is read by one rule (`JsonFields`): a member is found by its exact
  name, the last of a repeated name counts, and null is the same as absent.
  Go matched names without regard to case. Where a column keeps a
  member's JSON text, an explicit null is kept as `null`: an empty string
  is a value too, and not the one that was sent.
- JSON of a protobuf message (`/api/v2/series`, `/api/beta/sketches`, the
  agent's health report) is read by protobuf's own JSON parser: a field under
  the schema's name or in camelCase, an enum by number or by name, unknown
  keys passed over. A null inside a list or a map refuses the payload, which
  is kept raw.
- An event with an `alert_type` or `priority` outside Datadog's list is a row,
  with the sender's word in `alert_type_raw`/`priority_raw`; Go kept it raw.
- A time with a one-digit hour (`T1:30:00Z`) is not RFC 3339 and is not read
  as one.
- Notes on a kept payload say what the JSON was (`series[1].points: expected
  a list of [timestamp, value] pairs, got string`); none quotes a Go error or
  names a Go type.
- The summaries of a trace-stats sketch are computed with `System.Math`; they
  differ from Go's from about the 13th digit.
- `HEAD /support/flare` answers 200 without a key: the agent's connectivity
  check sends it with none.
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
export DATABASE_URL=postgres://root:mysecretpassword@localhost:5433/local
dotnet run --project src/Server -- intake    # :8080
dotnet run --project src/Server -- query     # :8081
dotnet watch --project src/Server run -- intake   # with reload
dotnet run --project src/Server -- migrate   # apply the ClickHouse migrations and exit
dotnet run --project src/Monitors            # the monitor service, one node
```

Package versions live in `Directory.Packages.props` only.

## Environment

| Variable | Default |
| --- | --- |
| `NINJACAT_ADDR` | `:8080` — where `intake` listens |
| `NINJACAT_INTERNAL_ADDR` | `:8081` — where `query` listens |
| `NINJACAT_LOGS_TCP_ADDR` | `:10516`; `off` disables the listener |
| `NINJACAT_TLS_CERT`, `NINJACAT_TLS_KEY` | PEM files; set both and the logs TCP listener speaks TLS |
| `CLICKHOUSE_HTTP_ADDR` | `localhost:8123` |
| `CLICKHOUSE_DB`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD` | `ninjacat` |
| `DATABASE_URL` | required — the same `postgres://` URL the panel uses |
| `NINJACAT_QUERY_TOKEN_SECRET` | required by `query` — at least 32 bytes, the key the panel signs its tokens with (below) |
| `NINJACAT_AUTO_MIGRATE` | `true`; with several replicas set `false` and run `migrate` as a job |
| `NINJACAT_METRICS_RETENTION_DAYS` | `30` — days `metrics` keeps raw points; a value that is not 1–36500 stops the start |
| `NINJACAT_METRICS_ROLLUP_RETENTION_DAYS` | `400` — days `metrics_1m`, the one-minute rollup, and `metric_tag_keys` keep their rows; likewise, and not shorter than the raw retention |
| `NINJACAT_TRUSTED_PROXIES` | none — reverse proxies in front of the intake, as addresses or networks (`10.0.0.0/8`), comma-separated. Only their `X-Forwarded-For` is believed; the loopback is always trusted |
| `NINJACAT_MAX_BODY_BYTES` | the largest request body the intake reads, 64 MiB by default; a larger one is answered 413. A body may inflate to 64 MiB |
| `NINJACAT_ACK_UNKNOWN` | `true` answers 202 instead of 404 on unknown intake paths |
| `DEBUG` | `true` dumps every intake request to `NINJACAT_CAPTURE_DIR` (default `captures`) |
| `NINJACAT_SELFMON_INTERVAL`, `NINJACAT_SELFMON_TENANT`, `NINJACAT_SELFMON_HOST` | `15s`, `default`, the machine's name |

The connection, retention and self-monitoring variables apply to
`ninjacat-monitors` as well; its own `NINJACAT_MONITOR_*` are listed under
"The monitor service".

## What `query` serves (:8081)

| | |
| --- | --- |
| `GET /ping` | liveness |
| `GET /health` | ClickHouse and Postgres reachability; 503 if either is down |
| `GET /api/v1/metrics?from=<unix s>[&host=]` | Datadog's active metrics list; `tag_filter` not yet |
| `POST /api/v2/query/timeseries`, `/api/v2/query/scalar` | Datadog's v2 query API: queries, formulas, functions |
| `GET /internal/metrics/{names,tags,tag-values,query}` | the panel's metric pages; each takes `from`/`to`, by default the last day for names, tags and tag values and the last hour for the query |
| `GET /internal/logs/{search,facets}` | the panel's log pages |
| `POST /internal/monitors/validate` | checks a monitor in Datadog's JSON with the code the service evaluates with: `{problems, notActedOn}`, the options stored but not acted on |
| `GET /internal/monitors/status?ids=` | per monitor: its state, its open groups by state, when it last triggered |
| `GET /internal/monitors/{id}/groups` | the open groups, with their state and latest value |
| `GET /internal/monitors/{id}/transitions?from&to[&group]` | the state timeline |
| `GET /internal/monitors/{id}/evaluations?from&to[&group]` | the evaluated values, for the graph |

**Metrics come from three tables.** `metrics` holds every point, sorted by
tenant, metric, the 15-minute bucket, host and series. ClickHouse does not
derive the bucket from a condition on `timestamp`, so every read of it states
the bucket as well (`Metrics.rawWindow`); without that a query reads every
granule of its metric. `env`, `service`, `kube_namespace`, `kube_deployment`
and `pod_name` are also kept as array columns of their own, each with every
value of the tag, and a filter or a group on them reads the column rather than
the map. `metrics_1m` holds one row per series per minute, with the tags and
plain counts, sums, minima, maxima and a rate's events for `as_count()` (an
average is the sum over the count), filled by a materialized view as points
arrive. A query whose step is a whole number of minutes reads the rollup and
adds its rows up, which gives the raw table's answer from about a quarter of
the rows (`Plan.tableFor`); a finer step reads the raw points.
`metric_tag_keys`, filled by another view, holds every metric and its tag
keys per hour (a metric without tags under the key `''`); the panel lists
metric names and tag keys from it, and tag values from the raw points. A window that reaches
back past the raw retention gets whole-minute steps, since only the rollup
still holds it. The process that migrates sets the two retentions as TTLs;
the query process and the monitor service plan by the same two settings and
do not read the TTLs back, so give every process the same values (the chart's
`metrics.*` does). The rollup is never kept shorter than the raw points (the
start stops if it would be). `metrics` is partitioned by day, so keep the raw
retention short and let the rollup hold the long range. The other tables'
TTLs are fixed in their migrations.

**Upgrading past 0005.** 0005 swaps `metrics` and `metrics_1m` for tables in a
new layout, which a release from before it cannot read. With the chart's
pre-upgrade migration Job the old `query` and `monitors` pods run against the
new tables until they are replaced: charts fail and monitors record errors,
without transitions. A rollback to an older release stays broken the same way.
The old tables are left as `metrics_old` and `metrics_1m_old`, and nothing
reads them, so every query starts at the swap, a monitor's window included:
mute monitors over long windows for that long, or restore the rollup's history
right after the upgrade with the `INSERT … SELECT` in 0005's header (the raw
points are not carried over). Run it once only, since a second run doubles every
sum, and with `SETTINGS max_block_size = 65536, min_insert_block_size_rows =
500000, min_insert_block_size_bytes = 0, max_insert_threads = 1`: without them
it ran out of 4 GB after 4 M rows, while with them 100 M rows take about 8
minutes. `metrics_old`
empties itself through its TTL; drop `metrics_1m_old` by hand once copied, or
when its history is not wanted.

If 0005 stops part-way, every later start fails on a table it already made.
Its statements run in order and the ledger row is written last, so find the
first one that has not taken effect (`SHOW TABLES LIKE 'metric%'`,
`SHOW CREATE TABLE metrics_1m`), run it and the rest by hand, and record the
migration. When `metrics_old` already exists, a retry may have made an empty
`metrics_next` or `metrics_1m_next`: drop it rather than renaming it.

```sql
INSERT INTO schema_migrations (version, name, checksum) VALUES (5, 'metrics_layout', 2867969176439395894)
```

Every route but `/ping` and `/health` asks for `Authorization: Bearer <token>`:
an HS256 JWT the panel signs with `NINJACAT_QUERY_TOKEN_SECRET` for each call
(`frontend/src/lib/server/ninjacat.ts`), checked by ASP.NET's JwtBearer
(`src/Server/QueryToken.fs`). Its claims are `sub` (the user), `tenant`, `role`,
`iss` `ninjacat-panel` and `aud` `ninjacat-query`, and it lives 60 seconds.
The tenant every query runs for is the token's `tenant`; nothing in the request
can name another. A request without a valid token, or with one that does not name
exactly one tenant, is answered 401.

To ask by hand, mint one with the panel's own library (here for an hour, and
the tenant `default`). `SECRET` is the dev stack's, from `compose.yaml`; the
kind lab generates its own (`lab/k8s/README.md` says how to read it).

```bash
cd frontend
export SECRET=dev-only-query-token-secret-change-me-32ch
TOKEN=$(bun -e "
import { SignJWT } from 'jose';
const key = new TextEncoder().encode(process.env.SECRET);
console.log(await new SignJWT({ tenant: 'default', role: 'owner' })
  .setProtectedHeader({ alg: 'HS256' }).setSubject('curl')
  .setIssuer('ninjacat-panel').setAudience('ninjacat-query')
  .setExpirationTime('1h').sign(key));
")
curl -H "Authorization: Bearer $TOKEN" 'http://127.0.0.1:8081/internal/metrics/names?limit=10'
```

The intake's API keys are not here. The keeper lives in `intake`; the panel
tells it a key changed with `NOTIFY ninjacat_api_keys` in Postgres, and it
re-reads them every 30 seconds regardless.
