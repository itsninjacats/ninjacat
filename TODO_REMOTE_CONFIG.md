# Remote Configuration

The one channel where data flows **down**: the agent polls `config.<site>` and applies what
comes back. Today `Routers/Config.fs` parses the poll, drops it and answers 404, so the agent
keeps the configuration it has. Nothing here is started.

Sources: the agent's code at tag 7.84.0 (`pkg/config/remote`, `pkg/remoteconfig/state`), the
schema in `api/src/Proto/protos/datadog-agent/datadog/remoteconfig/remoteconfig.proto`, and
one experiment on agent 7.83.2 (`server/docs/zadania/remote-config-tuf.md`, which predates
the `.proto` files and is out of date where it says the exchange is unknown).

## What travels through it

It is a pipe, not a feature. Each kind of content is a *product*; the agent asks for the
ones it and its clients want. The ones that matter to us, by who consumes them:

| Consumer | Products | What they change |
| --- | --- | --- |
| Tracers (through the agent) | `APM_TRACING` | the tracer's own settings: sampling rules, log injection, header tags, tracing on or off |
| | `LIVE_DEBUGGING`, `LIVE_DEBUGGING_SYMBOL_DB` | Dynamic Instrumentation probes |
| | `ASM`, `ASM_DD`, `ASM_DATA`, `ASM_FEATURES` | application security rules, and switching it on |
| Trace agent | `APM_SAMPLING` | sampling rates per service and environment |
| Core agent | `AGENT_CONFIG`, `AGENT_TASK`, `AGENT_INTEGRATIONS` | log level, a flare on request, integration configs |
| Cluster agent | `K8S_ACTIONS` | delete a pod, restart, roll back or patch a workload (`kubeactions.proto`) |
| | `CONTAINER_AUTOSCALING_SETTINGS`, `CONTAINER_AUTOSCALING_VALUES`, `CLUSTER_AUTOSCALING_VALUES` | autoscaling |
| Security agent | `CWS_DD`, `CWS_CUSTOM`, `CWS_REMEDIATION` | runtime security rules |
| Others | `NETWORK_PATH`, `SYNTHETIC_TEST`, `NDM_DEVICE_PROFILES_CUSTOM`, `METRIC_CONTROL`, `DO_QUERY_ACTIONS`, `DATA_SECURITY_DB_SCAN_TASKS`, the fleet installer's (`UPDATER_*`, `INSTALLER_CONFIG`) | |

A tracer never talks to us for this: it polls its agent (`/v0.7/config`), and the agent's
poll carries every such client (service, env, version, the products it wants) in
`active_clients`.

## What the agent does, as read from its source

- **Three requests**, all on `config.<site>`, with `DD-Api-Key` (and `DD-Application-Key`
  when set):
  - `POST /api/v0.1/configurations`: `LatestConfigsRequest` → `LatestConfigsResponse`;
  - `GET /api/v0.1/org`: → `OrgDataResponse` (the organisation's UUID);
  - `GET /api/v0.1/status`: → `OrgStatusResponse` (`enabled`, `authorized`). The agent logs
    which of the four combinations it got and does not poll for configs while either is false.
- **The answer** holds two sets of TUF metadata — `config_metas` (roots, timestamp, snapshot,
  top targets, delegated targets) and `director_metas` (roots, timestamp, snapshot, targets)
  — and `target_files` (path and raw bytes).
- **It is Uptane**, TUF with two repositories: *config* says what exists, *director* says
  what this agent gets. After the TUF checks on each (signatures, versions, expiry), the
  agent also requires:
  - every director target to exist in the config repository with the same length and hashes;
  - the snapshot's `custom.org_uuid`, when present, to equal the UUID from `/api/v0.1/org`;
  - every director target path to carry the agent's organisation id.
- **Paths**: `datadog/<org_id>/<PRODUCT>/<config_id>/<file>`.
- **Both roots can be replaced**: `remote_configuration.director_root` and
  `remote_configuration.config_root` (raw `root.json`). An override is used *instead of* the
  embedded root, never beside it — confirmed by experiment: a malformed override makes the
  agent panic rather than fall back.

## Not known yet

- A real poll. Remote Config was off in every lab run, so not one request is recorded.
- What the agent does with a first answer whose root version differs from the override's,
  and how it wants root rotation served (`director_root_version`).
- Which fields of `targets.custom` and of each target's `custom` it needs (`opaque_backend_state`,
  per-target version, expiry). The agent sends some of them back in its next poll.
- The cluster agent: same client, but never run here with Remote Config on.
- How each tracer verifies what its agent hands it (the agent passes the director's targets
  metadata through).

## Steps

- [ ] **Store the poll.** No signing needed: which agents exist, their products, and every
      tracer client with service, env and version. Worth having on its own (it is the only
      place a tracer's presence is reported before it sends a span). A table, a decoder, a
      real payload in `Fixtures/real/`.
- [ ] **Answer `org` and `status`** truthfully: a stable UUID per tenant; `enabled` and
      `authorized` false until the rest exists.
- [ ] **A signing service.** Our own roots for both repositories, the four roles each, keys
      kept outside the intake process. Decide where keys live and who may use them before
      writing any of it: this is the component that can change what every agent does.
- [ ] **Lab**: an agent on an internal network with both roots overridden; serve one
      harmless config (`AGENT_CONFIG`, a log level) until the agent applies it. Record what
      it rejects and why — its error messages name the failed check.
- [ ] **`APM_TRACING` / `APM_SAMPLING`**: the first products with a use here, once there is
      something to configure from the panel.
- [ ] **`K8S_ACTIONS`**: the other half of `k8s_actions`, which already stores the cluster
      agent's reports. Needs its own permission model first: who may ask for a pod to be
      deleted, and an audit trail (`requested_by` is part of the payload).
- [ ] Operator documentation: the two settings, and how to hand an agent our roots.

## The browser SDK has one too, and it is a different thing

Read from `@datadog/browser-rum` 7.15.0 (`remoteConfiguration.js`); never run here.

- Off unless the page passes `remoteConfigurationId` (or `remoteConfiguration.id`) to
  `init`. Without it the SDK makes no such request, and our panel does not set it.
- One plain `GET` of `https://sdk-configuration.browser-intake-<site>/v1/<id>.json`. No TUF,
  no signatures: the page trusts what HTTPS gives it. `remoteConfigurationProxy` replaces
  the whole URL.
- The document's `rum` object overrides a fixed list of init options: `applicationId`,
  `service`, `env`, `version`, the sample rates (session, replay, telemetry, trace),
  `trackUserInteractions`, `trackResources`, `trackLongTasks`, `defaultPrivacyLevel`,
  `allowedTracingUrls`, and a few more. A value can also be *dynamic*: read at start from a
  cookie, the DOM, a JS path or `localStorage`.
- The answer is cached in the browser and refreshed in the background; with
  `remoteConfiguration.required` RUM does not start until there is one.

- [ ] Serve it: a route on the browser intake, a document per RUM application kept in
      Postgres, an editor in the panel. Needs none of the signing above, so it can come
      long before the agent's. What it buys: changing sample rates and privacy level of a
      customer's application without them shipping a new build.
- [ ] Record the request a real browser makes (path, headers, caching headers it expects).

## Why it waits

Everything above the first two steps turns NinjaCat from something that only receives into
something that commands the machines it watches. That is a different risk, and none of the
read side depends on it.
