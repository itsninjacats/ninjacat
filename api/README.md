# ninjacat-api

The read side of NinjaCat: an F# service that answers queries against the
ClickHouse tables the Go server writes. Plan and order of work:
[`server/docs/zadania/fsharp-query-service.md`](../server/docs/zadania/fsharp-query-service.md).

It speaks Datadog's public API (`/api/v1/...`, `/api/v2/...`) rather than one
of our own, so the panel, `datadog-api-client`, Terraform and Grafana's Datadog
plugin can all point at it unchanged.

## Layout

- `src/Engine` — parsers, AST, compiler to SQL. No reference to ASP.NET,
  Oxpecker or the ClickHouse driver: it returns SQL text plus typed parameters
  (`Sql.fs`) and nothing else, so it is tested without a server.
- `src/Engine/Api/V2` — Datadog's v2 query API. `Wire.fs` holds the JSON
  shapes, each type named exactly after its schema in Datadog's OpenAPI spec
  (`TimeseriesFormulaQueryRequest`, `MetricsTimeseriesQuery`, …) so one grep
  finds it in the spec and the clients. What survives validation gets our own
  `Parsed…` names, which can never be mistaken for a spec schema.
- `src/Server` — Oxpecker on ASP.NET Core. `Handlers.fs` turns HTTP into an
  engine call; `ClickHouse.fs` and `Postgres.fs` are the only files that know
  a database driver.
- `tests/Engine.Tests` — xunit v3 on Microsoft.Testing.Platform.

The ClickHouse schema is the contract with the Go side. Go owns the
migrations; this service only reads, and every request runs with
`readonly=2`.

Postgres (users, API keys, later monitors and dashboards) is reached through
plain Npgsql, no ORM. Its schema belongs to Drizzle in `frontend/`: this
service never migrates it, and a table it needs is added there first.

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

## Where Datadog is silent

Datadog documents its query language unevenly. Wherever we had to decide
behaviour the docs do not pin down, the code says so at that spot:

```bash
grep -rn "WARNING(undocumented)" src   # a choice we made; check against Datadog when possible
grep -rn "FIXME(" src                   # a choice we doubt
```

Each such comment names the question, then the answer we picked. When one is
checked against a real Datadog account, replace the warning with the source.

## Commands

```bash
dotnet build
dotnet test
DATABASE_URL=postgres://root:mysecretpassword@localhost:5433/local \
  dotnet run --project src/Server        # http://localhost:8082
dotnet watch --project src/Server run    # with reload
```

Package versions live in `Directory.Packages.props` only.

## Environment

| Variable | Default |
| --- | --- |
| `NINJACAT_API_URL` | `http://localhost:8082` |
| `CLICKHOUSE_HTTP_ADDR` | `localhost:8123` — HTTP, not the native :9000 the Go server uses |
| `CLICKHOUSE_DB`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD` | `ninjacat` |
| `DATABASE_URL` | required — the same `postgres://` URL the Go server and panel use |

## Endpoints

| | |
| --- | --- |
| `GET /ping` | liveness |
| `GET /health` | ClickHouse and Postgres reachability; 503 if either is down |
| `GET /api/v1/metrics?from=<unix s>[&host=]` | Datadog's active metrics list; `tag_filter` not yet |
| `POST /api/v2/query/timeseries` | parses and validates; answers 501 until execution exists |

Tenant is `"default"` until requests carry one, as on the Go side.
