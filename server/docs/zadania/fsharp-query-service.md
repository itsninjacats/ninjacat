# The query side in F# — plan

**Plan, 2026-09-27. No code yet.** What it takes for NinjaCat to stop being a
(good) Datadog shim and start answering questions: explorers, formulas,
monitors. Written down so the order of work is decided once.

Related: [silnik-kwerend.md](silnik-kwerend.md) — the original query engine
design. Its direction holds; one of its decisions is reversed below.
[json-log-attributes.md](json-log-attributes.md) — blocks the log explorer.

## The split

**Ingest stays in Go. Everything that reasons about the data moves to F#.**

Ingest is dumb by design: decode, translate, hand to a writer, answer 202. Go
does that well and there is nothing there that a rewrite would improve. The
parts that are still to be written — query engine, formulas, correlation,
monitors — are compilers and state machines, which is exactly what F# is good
at: discriminated unions for the AST, exhaustive pattern matching when
rewriting it, FParsec for the grammars.

Ingest will not stay entirely dumb. Checks such as "does this RUM application
exist" will need to be made at intake. That is deferred until RUM, and nothing
in this plan closes it off.

Porting ingest 1:1 later stays possible. The protocol knowledge lives in the Go
code, so a port is translation rather than rediscovery — **provided** there is
a corpus of captures to replay through both implementations and compare the
resulting rows. Building that corpus is worth doing anyway, as test data.

**The contract between the two sides is the ClickHouse schema.** Go owns the
migrations; the F# service only reads. `tenant_id` handling has to match on
both sides.

## The decision reversed from silnik-kwerend.md

That document says the text parser is optional and late, because the panel can
build the AST from controls. That no longer holds. Datadog stores monitors,
formulas and dashboard widgets as **query text**:

```
avg(last_5m):avg:system.cpu.user{env:prod} by {host} > 90
```

So the parser for Datadog's query syntax is the foundation, not a convenience.
Together with the engine behind it, it is also the hardest part of this plan.

## Phases

### 0. Service foundation

- F# service on ASP.NET Core, a ClickHouse client, read-only.
- Takes over panel endpoints one at a time: `frontend/src/lib/server/ninjacat.ts`
  points each endpoint at F# once it reaches parity; Go `query`/`panelapi`
  keeps serving the rest until then.
- Keep the HTTP layer thin. The engine lives in plain F# modules with no
  dependency on the web framework, so it can be tested without a server and
  the framework can be swapped cheaply.

**Decided: Oxpecker.** Both it and Falco are thin functional layers over
ASP.NET Core endpoint routing with similar performance, and both depend on a
single maintainer; Oxpecker had the stable net10 release while Falco 6 was
still in beta. For an internal JSON API the choice matters little as long as
the rule above holds.

**Started 2026-09-27** in `api/`: Engine / Server / tests split, ClickHouse
over HTTP via ClickHouse.Driver with `readonly=2`, `/health`, and the first
Datadog endpoint, `GET /api/v1/metrics`.

**Open, recommended yes: speak Datadog's public query API.**
`/api/v2/query/timeseries`, `/api/v2/logs/events/search`,
`/api/v2/spans/events/search`, `/api/v1/monitor`, … instead of an API of our
own. Then the panel, `datadog-api-client`, Terraform and Grafana's Datadog
plugin work against us unchanged. The Go intake forwards those paths to F#
instead of answering 404 (see [brakujace-endpointy-api.md](brakujace-endpointy-api.md)).

### Status, 2026-09-27

`POST /api/v2/query/timeseries` runs metric queries end to end in `api/`:
the query language (filters in both syntaxes, wildcards, `by`, rollups,
`as_count`/`as_rate`, `fill`), formulas with arithmetic between queries,
and Datadog's function families — pointwise, rank, count/exclusion,
rate/smoothing/cumulative with lookback, timeshift/calendar_shift,
interpolation and default_zero, regression, autosmooth, outliers,
anomalies (basic, agile, robust, with seasonality) and forecast (linear,
seasonal). Anomaly bands and forecasts travel as `ninjacat_bounds` and
`ninjacat_forecast` beside the values.

Every behaviour Datadog does not document is marked in the code
(`grep -rn "WARNING(undocumented)" api/src`); `integral` is a FIXME.

Not yet: `dt()`, `.weighted()`, percentiles (`p95:`, from `sketches`),
calendar rollups, `/api/v2/query/scalar`, v1 `/api/v1/query`, the
`metrics_1m` table for windows past raw retention, and the metric catalog.

### 1. Metric explorer

- Metric catalog: name, type, unit, last seen. Probably a ClickHouse
  materialized view, which needs no change on the Go side. Type rules
  (UNSPECIFIED behaves as GAUGE, resolved at read time) as in silnik-kwerend.md.
- Parser for metric query syntax: tag filters, wildcards, negation, `IN`,
  `AND`/`OR`, `by {}`.
- Time and space aggregation constrained by metric type; `.rollup()`,
  `.as_count()` / `.as_rate()`.
- Compiler AST → SQL with bound parameters: `metrics` vs `metrics_1m`, limits
  on points and cardinality (truncation is reported, never silent), a
  mandatory time window.
- Distributions and percentiles (`p95:`). First check how sketches are stored —
  it decides whether ClickHouse or F# computes the percentile.

### 2. Formulas and functions

- **Each individual query compiles to SQL; formulas are evaluated in F#, in
  memory, over aligned series.** ClickHouse does the heavy scan, F# does the
  algebra.
- Arithmetic between queries (`a / b`): time axis alignment and a policy for
  gaps.
- `timeshift`, `week_before`, `rate` / `derivative` / `per_second`, `cumsum`,
  `moving_rollup`, `top()`.
- Later: `anomalies`, `forecast`, `outliers`.

### 3. Log explorer

- Parser for log search syntax: free text, tags, `@attributes`, ranges,
  `-negation`.
- Blocked by the storage decision in json-log-attributes.md — make it at the
  start of this phase.
- Analytics (group by, count, timeseries), then patterns and live tail.

### 4. Trace explorer

- Span search uses **the same syntax as log search**, so the phase 3 parser
  carries over almost unchanged.
- Single trace view by `trace_id`, APM metrics, later the service map.

### 5. Monitors

- Parser for monitor definitions: phases 1–4 syntax plus condition and window.
- Metric monitors first, then log and APM monitors.
- Per-group state machine: OK / WARN / ALERT / NO DATA, recovery thresholds,
  evaluation delay, renotify. A good fit for Orleans — one grain per monitor.
- Downtimes and mutes.
- Message templates (`{{#is_alert}}`, `{{host.name}}`) — another small parser.
- Notification channels: webhook, e-mail, Slack.

The hard part here is Datadog's semantics, not the language.

### 6. Integrations

To be scoped. In Datadog the word covers three different things:

- agent checks — already arrive through the intake,
- cloud crawlers (AWS, GCP) — pulled by the backend, nothing exists yet,
- notification channels — belong to phase 5.
