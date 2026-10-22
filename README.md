# NinjaCat

An open-source, self-hosted observability backend that speaks the Datadog Agent's
wire protocol. Point an agent you already run at NinjaCat, change nothing else, and
your telemetry lands in your own ClickHouse instead of someone else's cloud.

> **Status: early.** The ingest path works end to end for the signals listed below,
> and a panel renders them. Roughly forty endpoints the agent calls are still unhandled, and
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
- **API keys** — in the format the agent expects (32 hex characters), created in the
  panel, enforced at intake.
- **Query API + panel** — metric search, per-host series, charts over an arbitrary range.
- **Monitors** — metric monitors (threshold, change, anomaly, forecast, outlier),
  defined in the panel and evaluated by a service of their own. Notifications are a
  stub so far.
- **Self-monitoring** — NinjaCat reports its own metrics through its own pipeline.
- **Capture lab** — a sealed network that records exactly what a real agent sends, which
  is how the protocol is being worked out endpoint by endpoint.

## Quick start

Prerequisites: Docker. That is all — the toolchains live in the images.

```bash
git clone <this repo> && cd ninjacat
docker compose up
```

That brings up ClickHouse, Postgres, the server's programs and the panel, and applies both
schemas. Saving a Svelte file hot-reloads the browser; after a change to the server,
`docker compose up --build` rebuilds its image.

| | |
|---|---|
| Panel | http://localhost:5173 |
| Agent intake | http://localhost:8080 |
| Panel and query API (internal, loopback only) | http://localhost:8081 |

To work on the host instead, run just the databases with
`docker compose up -d postgres clickhouse`, then `cargo run --bin ninjapaws_migrate`,
`cargo run --bin ninjapaws_internal` and the intake programs you need (`cargo run --bin
ninjapaws_api`, …) in `backend/`, and `bun run dev` in `frontend/`. Development needs Rust and
[Bun](https://bun.sh).

Nobody can register: accounts are made on the server. Create the first one, then sign in:

```bash
docker compose exec frontend bun run user:create -- you@example.com --name 'Your Name'
# with the shipping image: node create-user.js you@example.com --name 'Your Name'
```

It asks for the password, as Django's `createsuperuser` does. Where nobody is there to type
(a Job, a script), it takes it from `NINJACAT_USER_PASSWORD`.

The first account on an empty database is the instance admin and owner of the tenant
`default`; later ones take `--admin` (an instance admin, who creates tenants and accounts),
`--tenant <id|slug>` (repeatable) and `--role owner|admin|member` (default `member`).

Create an API key in the panel under Settings, then send an agent at it. Each product has
its own hostname, as with Datadog (`app.`, `trace.agent.`, `process.`, …), and each is served
by a program of its own, so each needs to resolve to NinjaCat:

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

## Architecture

```
Datadog Agent ──:8080──▶ ingress ──host──▶ ninjapaws_<host> ──▶ one insert per table ──▶ ClickHouse
                                                                                            ▲
SvelteKit panel ──:8081──▶ ninjapaws_internal (panel and query API) ───────────────────────┤
       │                                                                                    │
       │                     ninjapaws_monitors ────────────────────────────────────────────┘
       │                         ▲ reads `monitor`, LISTEN ninjacat_monitors
       └──▶ Postgres (users, sessions, tenants and their members, API keys, monitors)
```

The server is in Rust (`backend/`): one image, one program per intake host. Datadog puts
every product on its own hostname, which is what lets a single `DD_SITE` reconfigure an
entire agent; here each of those hosts is a program of its own, deployed, scaled and stopped
on its own, and the ingress in front sends each name to its program. The panel's API and
Datadog's query API are served by `ninjapaws_internal`, on a port of its own;
`ninjapaws_monitors` evaluates the monitors and writes what they went through. A handler
inserts its rows before it answers, and ClickHouse's asynchronous inserts batch the many small
inserts on its side; when ClickHouse refuses one, the agent is told to send again.

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

Code and protobuf definitions taken from Datadog's and other open-source repositories
keep their own licences; see `backend/THIRD_PARTY_NOTICES.md`.
