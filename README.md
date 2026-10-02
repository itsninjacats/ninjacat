# NinjaCat

An open-source, self-hosted observability backend that speaks the Datadog Agent's
wire protocol. Point an agent you already run at NinjaCat, change nothing else, and
your telemetry lands in your own ClickHouse instead of someone else's cloud.

> **Status: early.** The ingest path works end to end for the signals listed below,
> and a panel renders them. Roughly forty Datadog endpoints are still unhandled, and
> nothing here has been run in anger yet. Treat it as a working prototype, not a
> product.

## Why

Observability data is the most vendor-locked thing in an average stack: the agents are
everywhere, the bills scale with cardinality, and moving off means re-instrumenting
everything. NinjaCat attacks that from the only angle that avoids the rewrite —
compatibility with the agent protocol itself. The agent stays; only the destination
changes.

## What works today

- **Metrics** — points and DDSketch distributions, stored in ClickHouse.
- **Logs**, **service checks**, **events**, **host metadata**, **process snapshots**.
- **Kubernetes** — the cluster agent's orchestrator collections (pods, nodes,
  deployments, services, RBAC, autoscalers and the rest), raw object manifests,
  the cluster summary, and the actions the cluster agent reports running. Stored
  append-only, with a materialised view maintaining the current-state projection.
- **Containers** — lifecycle events (starts, stops, OOM kills, with the exit code
  kept distinguishable from "none reported") and the image inventory, keyed by
  digest.
- **API keys** — Datadog-shaped, created in the panel, enforced at intake.
- **Query API + panel** — metric search, per-host series, charts over an arbitrary range.
- **Self-monitoring** — NinjaCat reports its own metrics through its own pipeline.
- **Capture lab** — a sealed network that records exactly what a real agent sends, which
  is how the protocol is being worked out endpoint by endpoint.

## Quick start

Prerequisites: Docker. That is all — the toolchains live in the images.

```bash
git clone <this repo> && cd ninjacat
docker compose up
```

That brings up ClickHouse, Postgres, the server's two processes and the panel, applies
both schemas, and watches both source trees: saving an F# file rebuilds and restarts the
server (via `dotnet watch`), saving a Svelte file hot-reloads the browser.

| | |
|---|---|
| Panel | http://localhost:5173 |
| Agent intake | http://localhost:8080 |
| Panel and query API (internal, loopback only) | http://localhost:8081 |

To work on the host instead, run just the databases with
`docker compose up -d postgres clickhouse`, then `dotnet run --project src/Server -- intake`
and `dotnet run --project src/Server -- query` in `api/`, and `bun run dev` in `frontend/`. Development needs the .NET 10 SDK and
[Bun](https://bun.sh).

Nobody can register: accounts are made on the server. Create the first one, then sign in:

```bash
docker compose exec frontend bun run user:create -- you@example.com '<password>' 'Your Name'
# with the shipping image: node create-user.js you@example.com '<password>' 'Your Name'
```

Create an API key in the panel under Settings, then send an agent at it. The intake
routes on the hostname, as Datadog does, so each product needs its Datadog-shaped name
(`app.`, `trace.agent.`, `process.`, …) resolving to NinjaCat:

```bash
DD_API_KEY=<the key you just created>
DD_DD_URL=http://app.ninjacat.example:8080
DD_APM_DD_URL=http://trace.agent.ninjacat.example:8080
DD_PROCESS_CONFIG_PROCESS_DD_URL=http://process.ninjacat.example:8080
DD_LOGS_CONFIG_LOGS_DD_URL=agent-http-intake.logs.ninjacat.example:8080
DD_LOGS_CONFIG_USE_HTTP=true
DD_LOGS_CONFIG_LOGS_NO_SSL=true
```

`compose.lab.yaml` does exactly that with a real agent, on a network with
`internal: true`, so nothing can escape to Datadog regardless of how the agent is
configured. That is the recommended way to try this the first time:

```bash
DD_API_KEY=<key> docker compose -f compose.yaml -f compose.lab.yaml up
```

### NinjaCat watching itself

`compose.self.yaml` makes the installation its own first customer. The panel reports
through Datadog's browser SDKs (RUM, browser logs), the server's two processes through
Datadog's .NET tracer (traces, runtime metrics, profiles, logs), and an agent reports the
host — all of it to this installation's own intake, none of it to Datadog. It is off
unless the file is named:

```bash
NINJACAT_SELF_KEY=<key> docker compose -f compose.yaml -f compose.self.yaml up
```

Use a key made for this alone: the panel hands it to every browser that opens it.

## Architecture

```
Datadog Agent ──:8080──▶ intake ──▶ one writer per table ──▶ ClickHouse
                                                                 ▲
SvelteKit panel ──:8081──▶ query (panel and query API) ─────────┘
       │
       └──▶ Postgres (users, sessions, API keys)
```

The server is one F# program (`api/`) run as two processes: agents reach `intake` and only
`intake`; the panel's API and Datadog's query API are served by `query`, on a port of its own. The agent
gets its answer before anything is written — rows are buffered per table and inserted in
batches, so a burst of logs cannot delay metrics.

Intake routes on the `Host` header, mirroring the way Datadog puts every product on its
own hostname. That is what lets a single `DD_SITE` reconfigure an entire agent.

See [CLAUDE.md](CLAUDE.md) for the full architecture notes, the commands, and the
reasoning behind the load-bearing decisions.

## Language

English is the canonical language of this project — UI, code, comments, commits and
docs. The panel is localised (Paraglide/inlang) with `en` as the base locale; other
locales are translations of it. Parts of the tree are still Polish and are being
converted as they are touched.

## License

[GNU AGPL v3](LICENSE).

If you run a modified NinjaCat as a network service, AGPL section 13 requires you to
offer its users the corresponding source. The panel does not link to its source yet
(`TODO.md`); until it does, that offer is yours to make.

Copyright (C) 2026 Michal Hodur.

## Contributing

Not open for pull requests yet — the contribution terms are still being settled. Issues
and protocol findings are welcome in the meantime.

## Trademark

NinjaCat is not affiliated with, endorsed by, or sponsored by Datadog, Inc.
Datadog is a trademark of Datadog, Inc. The name is used here only to describe protocol
compatibility.
