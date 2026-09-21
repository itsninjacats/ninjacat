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

That brings up ClickHouse, Postgres, the backend and the panel, applies both schemas,
and watches both source trees: saving a `.go` file rebuilds and restarts the server
(via Air), saving a Svelte file hot-reloads the browser.

| | |
|---|---|
| Panel | http://localhost:5173 |
| Agent intake | http://localhost:8080 |
| Panel API | http://localhost:8081 |
| Ergo observer | http://localhost:9911 |

To work on the host instead, run just the databases with
`docker compose up -d postgres clickhouse`, then `go run ./cmd/ninjacat` in `server/`
and `bun run dev` in `frontend/`. Development needs Go 1.26+ and [Bun](https://bun.sh);
the build requires `CGO_ENABLED=1`, because the process-agent frames use a legacy zstd
that is not pure Go.

Create an API key in the panel under Settings, then send an agent at it:

```bash
DD_API_KEY=<the key you just created>
DD_DD_URL=http://ninjacat-host:8080
DD_APM_DD_URL=http://ninjacat-host:8080
DD_PROCESS_CONFIG_PROCESS_DD_URL=http://ninjacat-host:8080
DD_LOGS_CONFIG_LOGS_DD_URL=ninjacat-host:8080
DD_LOGS_CONFIG_USE_HTTP=true
DD_LOGS_CONFIG_LOGS_NO_SSL=true
```

`server/docker-compose.datadog-lab.yml` runs a real agent against NinjaCat on a network
with `internal: true`, so nothing can escape to Datadog regardless of how the agent is
configured. That is the recommended way to try this the first time.

## Architecture

```
Datadog Agent ──:8080──▶ intake ──▶ storage actors ──▶ ClickHouse
                                          ▲
SvelteKit panel ──:8081──▶ panel API ──▶ query pool ──┘
       │
       └──▶ Postgres (users, sessions, API keys)
```

The backend runs as a single [Ergo](https://ergo.services) node: every component is a
supervised application, so a failure is contained and restarted rather than taking the
process down. Ingest and querying are deliberately separate applications — a slow
dashboard query must never be able to stall the write path, and the two are meant to be
splittable across nodes later.

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
offer its users the corresponding source. The panel links back to this repository for
that purpose; if you modify it, that link must point at your source, not this one.

Copyright (C) 2026 Michal Hodur.

## Contributing

Not open for pull requests yet — the contribution terms are still being settled. Issues
and protocol findings are welcome in the meantime.

## Trademark

NinjaCat is not affiliated with, endorsed by, or sponsored by Datadog, Inc.
Datadog is a trademark of Datadog, Inc. The name is used here only to describe protocol
compatibility.
