# TODO

Open work, grouped by what it unblocks. Scoped write-ups of single items live in
`docs/protocol/zadania/`; this file is the list.

**Focus for now: infrastructure** — the agent, Kubernetes, network, databases — and the
backend that receives it. What applications send (tracers, profiles, RUM, error tracking) and
the panel come after. Sections 1 to 3 first; within 3, Kubernetes and containers before
tracers.

## 0. Before 0.1.0 faces the internet

From a security review on 2026-10-02: three independent readings (panel, server, deployment
and repository) and probes of the shipping images. No secret is committed, in the tree or
its history; neither image calls out; the panel's pages load nothing from a third party.

Fixed that day, each checked against the shipping image:

- [x] **Anyone could read logs and metric names.** `/app` was guarded by its layout's load,
      which a data request can skip (`__data.json?x-sveltekit-invalidated=001`). One guard
      stands in front of every route now (`frontend/src/lib/server/guard.ts`): only `/login`
      is public.
- [x] **Anyone could write to Postgres** through the scaffolding routes (`/zadania`,
      `/remote-demo`). They are gone, with `/demo`, the `task` table (migration 0003) and
      the remote-functions flag.
- [x] **No limit on a request body**, on the wire or inflated: a few megabytes of gzip took
      the intake to an out-of-memory exception, and on the `install.` host no key was needed.
      `NINJACAT_MAX_BODY_BYTES` (64 MiB, 413 above it), 64 MiB inflated, and a host that
      asks for no key no longer serves the flare upload. The agent's own ceilings are far
      below (7.84.0 defaults: 2.5 MiB compressed and 4 MiB inflated per payload, 5 MiB for
      series, logs and the event-platform tracks, 10 MiB through `evp_proxy`); of 1 688
      requests recorded from real agents the largest was 397 kB. Not measured: how large a
      flare or a profile gets — those are not compressed as a body, so only the 64 MiB on
      the wire applies to them.
- [x] **Keys in the query string were stored**: `raw_payloads.query`, the path of
      `/v1/input/<key>`, capture dumps, one RUM warning.
- [x] The chart and `compose.prod.yaml` named images on Docker Hub under an account that is
      not ours. The chart names GHCR, where CI publishes.
- [x] CI published images with no test in front. `ci.yml`: server tests against a real
      ClickHouse, panel type check and tests, chart lint, then both images or neither;
      actions pinned to commits; `latest` only on a release. First run on pull request #1:
      all four jobs passed, 1 547 and 267 tests with none skipped, images built and not pushed.

- [x] **Any pod in the cluster could read every tenant's telemetry.** The chart has a
      NetworkPolicy: the query port is reached from the panel's pods alone (it takes a
      network plugin that enforces policies).
- [x] **Chart defaults.** `domainRoot` is required, TLS is on, plain HTTP is answered with a
      redirect: ingress-nginx does that for an Ingress with TLS, and for Traefik the chart
      adds a redirect Middleware and a second Ingress on the HTTP entry point (when the
      cluster has Traefik's CRDs). `trustedProxies` is one value for the intake and the
      panel. `secrets.betterAuthSecret` shorter than 32 characters is refused. Pods have
      memory limits, `seccompProfile`, a read-only root filesystem; the server logs JSON, so
      nothing a request carries can forge a log line; NOTES say how the first account is made.
- [x] **Sign-in is throttled.** The form signs in through Better Auth's own endpoint, so its
      rate limiter counts: five attempts a minute per client address
      (`frontend/src/lib/server/auth-options.ts`), in the process's memory. With more than one panel
      pod, `storage: "database"`.
- [x] **An agent through the chart's real Ingress.** The kind lab runs Traefik now and the
      agent reaches the intake through the chart's Ingress with the lab's certificate. Run on
      2026-10-02, agent 7.84.0 given only `site`: all 39 hosts answer over HTTPS; HTTP is
      301/308 to HTTPS; the agent used 39 host names, `app.<site>` for the main one and no
      `<version>-app.agent.<site>` (it prefixes its version only for Datadog's own domains,
      `ddURLRegexp` in `pkg/config/utils/endpoints.go`); every answer 200 or 202, nothing in
      `raw_payloads`, no warning; the intake saw the agents' pod addresses, not Traefik's;
      an ordinary pod got no answer from the query port; install, upgrade, `helm test`, two
      intake replicas and `create-user.js` all work on the read-only filesystem.

- [x] `create-user.js` asks for the password, twice and without echo, as Django's
      `createsuperuser` does; with no terminal it reads `NINJACAT_USER_PASSWORD`. The
      password is no longer an argument, where it stayed in the shell's history.

Open, most urgent first:

- [x] **Merge and tag.** Pull request #1 merged, `v0.2.0` tagged from `main` (2026-10-15).
- [ ] **A key has no kind** (decided 2026-10-02: do it, later). A RUM client token is a
      full API key: copied from a page it writes metrics, logs and traces for its tenant.
      A `kind` column on `api_key` (the panel's schema), carried as a claim by the keeper;
      the browser scheme takes only client keys, the other three refuse them; the keys page
      lets one be made. Until then, do not set `PUBLIC_NINJACAT_RUM_*` on a public panel.
- [ ] **Any member of a tenant manages its API keys.** The keys page
      (`frontend/src/routes/app/settings/keys/`) checks the tenant, not the role, so a
      `member` can make a key that writes into the tenant or delete the one production uses.
      Until RBAC (tenants.md defers it), limit create and delete to `owner` and `admin`
      (`locals.tenant.role`), or do not give anyone the `member` role.
- [ ] Logs over TCP (:10516): on by default in the image (the chart turns it off), plain
      text unless given a certificate, no limit on connections, no timeout, the key checked
      only when a whole frame has arrived.
- [x] The panel sends security headers: a CSP with nonces (`kit.csp`), `frame-ancestors`, HSTS, nosniff (2026-10-15).
- [ ] The server falls back to the ClickHouse password `ninjacat` when none is set
      (`backend/storage/src/lib.rs`); the chart and `compose.prod.yaml` always set one. Fail
      instead.
- [ ] `compose.prod.yaml`: the panel on every interface as plain HTTP, no health checks on
      the server's services.
- [ ] The server's programs log text lines (tracing's default format), not one JSON object
      per line: what a request puts in a path or a header can pass for a log line of its own.
- [ ] What the query process says in an error (ClickHouse's text) reaches the browser.
- [ ] The chart's NetworkPolicy covers the query port only. The intake and the panel still
      take a connection from any pod; admitting the Ingress controller alone needs to know
      its namespace (a value).
- [ ] The image build runs code fetched from jsdelivr (two inlang plugins at floating
      majors, `frontend/project.inlang/settings.json`). Pin or vendor them.
- [ ] Repository settings: `main` unprotected, no tag rules, secret scanning and push
      protection off. Two old public images, `…/frontend` and `…/server`, from September.
- [ ] The history was rewritten on 2026-10-02 so that every commit carries one author
      address; the three branches were force-pushed. GitHub still serves the old commits to
      anyone who has their hashes until it collects them; only its support removes them
      at once. Any other clone has to be made again, not pulled.
- [ ] The panel does not link to its source (AGPL section 13); `bun run lint` fails on 431
      files, so CI does not run it.
- [ ] **The monitor service has not run at its capacity target** (1 000 monitors × 20
      groups, windows under 24 h; `docs/protocol/zadania/monitors.md`, "Capacity"). On a real
      cluster: load 1 000 monitors over 20 000 series; measure read latency
      (`system.query_log`, the monitors user), `ninjacat.monitor.evaluation.lag`, the skip
      rate and `monitor_evaluations` rows a day, into `backend/README.md`.

## 1. The server in Rust

The server is `backend/` (2026-10-21): one program per intake host (`ninjapaws_<host>`), the
query process, the monitor service and `ninjapaws_migrate`, all in one image. It replaced the F#
server, which had replaced the Go one; the golden fixtures recorded from Go are replayed against
every host (`backend/intake/tests/golden.rs`) and their rows inserted into a scratch ClickHouse.

- [x] Every host and route of the F# intake, the query process, the monitor service, the
      ClickHouse migrations; compose and the chart with a program per host; CI.
- [ ] **Run it.** Not yet in front of a real agent, nor on ninjapaw: the dev stack with
      `compose.lab.yaml`, then the cluster, comparing table by table with what the F# intake
      stored.
- [ ] **Self-observability.** The F# processes were traced by Datadog's .NET tracer, injected
      from outside (the old `compose.self.yaml`, `docs/self-observability.md` in the history).
      The Rust programs send nothing about themselves: traces through OpenTelemetry (OTLP to
      the agent), their logs, and `ninjacat.node.*`/`ninjacat.storage.*` from every program,
      not only the monitor service.
- [ ] **Postgres without TLS.** Every program connects with `NoTls`: a `DATABASE_URL` that asks
      for TLS (`sslmode=require`) does not connect. tokio-postgres with rustls and the
      operator's CA.
- [ ] **An unknown host** gets the proxy's 404 (Traefik's, the Ingress controller's), not the
      F# intake's `no intake for host "<name>"`, which told an operator what DD_SITE built.
- [ ] **The hosts are listed three times**: `helm/ninjacat/values.yaml` (`intake.programs`),
      `dev/intake-routes.yml` and `host_process` in `intake/tests/golden.rs`. One list.
- [ ] serde_json nests at most 128 deep, System.Text.Json allowed 512: a body between the two
      is kept raw now.
- [ ] `docs/protocol/**` describe Go files that no longer exist; say what the Rust equivalent
      is as each note is translated.

## 2. Intake: not served yet

- [ ] **Remote Config** (`config.<site>`). Answered 404 on purpose: the agent verifies the
      answer against a TUF root. Its own list: `TODO_REMOTE_CONFIG.md` (what travels through
      it, what the agent checks, the steps; the first two need no signing).
- [ ] **OTLP** (`otlp.<site>`): metrics, traces, logs, the host profiler's profiles. Its own
      host and router (`otlp-osobny-router.md`).
- [ ] **`GET /api/v1/query`** for the cluster agent's External Metrics Provider answers an
      empty result. The query engine is in the same process now: answer it for real, so HPA
      on NinjaCat metrics works.
- [ ] Serverless flare (`/api/ui/support/serverless/flare`).
- [ ] `HEAD` on OCI registry manifests (the cluster agent's admission controller).
- [ ] `evp_proxy` with a subdomain that is not in the host table: decide between refusing by
      name (today) and keeping the payload raw.
- [ ] Process-agent frames compressed with zstd 0.x (kept raw today). Only if an agent that
      sends them turns up.

No published schema, kept raw until a real payload shows the shape: `genresources`,
`aiusage`, `llmobs`.

## 3. Verify against real senders

Decoders written from Datadog's source and never run against the real thing. Every lab run so
far found something, so expect findings. How to record: `lab/README.md`.

- [ ] **Kubernetes**: orchestrator collections and manifests arrived and decoded in the
      kind lab (2026-10-02, agent 7.84.0); record them into `backend/intake/tests/fixtures/real/`. Cluster-agent
      actions and container lifecycle events were not produced by that run.
- [x] The agent's connectivity check posts `{}` to every event-platform intake at start,
      and `sbom`, `contlcycle`, `contimage` and `data-streams` each logged a decode warning.
      A kept payload is logged once now, by `raw::store`, which already knew a probe; the
      82 handler warnings that said the same thing first are gone.
- [x] `HEAD /support/flare` answers 200 without a key: the agent's connectivity check
      (`pkg/diagnose/connectivity`, 7.84.0) sends it with none and takes anything but 200 or
      a redirect for a failure. The upload itself still asks for the key.
- [ ] **Containers**: lifecycle events, image inventory, SBOM.
- [ ] **Tracers** — one small app with `dd-trace` covers: profiles, the debugger and symbol
      uploads, Data Streams Monitoring, tracer telemetry, CI Visibility (test cycle,
      coverage, git endpoints).
- [x] **NinjaCat sending to NinjaCat**, checked on 2026-10-01 with the F# server under
      `dd-trace-dotnet` 3.54.0 and the panel under `@datadog/browser-rum`/`-logs`: the
      browser talks to the panel and the intake only. First finding: the tracer's logs have
      no `message` (Serilog's compact form), which the logs intake refused. The Rust server's
      own telemetry is section 1's. Left:
  - [ ] `dd-trace` in the SvelteKit server, and `allowedTracingUrls` so a RUM resource
        links to the server span that answered it;
  - [ ] the tracer's telemetry and profiles arrive, but nothing reads them yet (sections 3
        and 4).
- [ ] **Agent and tracer telemetry** (`instrumentation-telemetry-intake`,
      `/api/v2/apmtelemetry`). Useful on its own: it says which agents and tracers run, in
      which versions, with which configuration, and what went wrong inside them. Five
      producers share the path and are told apart by `request_type`; they do not follow
      `dd_url`, which is why the labs never saw them:
  - `compose.self.yaml` already sets `apm_config.telemetry.dd_url`; the .NET tracer's
    telemetry arrives through it as `apm_telemetry` rows. Seen there on 2026-10-02 and not
    looked into: the trace-agent also posts to the same host over https and logs "server
    gave HTTP response to HTTPS client", and the agent's own telemetry
    (`agent_telemetry`) is not redirected at all. The labs do not redirect either yet:
  - [ ] redirect them in `compose.lab.yaml` and `lab/`: `agent_telemetry.dd_url` (the agent's
        own metrics, logs and message batches, zstd) and `apm_config.telemetry.dd_url` (the
        tracers' proxy, trace-agent onboarding, the cluster agent's patch events), and add
        the host alias; document both keys where the agent's settings are listed;
  - [ ] record a real payload of each producer into `backend/intake/tests/fixtures/real/` and hold the decoder
        (`intake/src/routers/misc.rs`) to them;
  - [ ] the tracers' proxy passes any subpath through, not only `/api/v2/apmtelemetry`:
        serve the host by prefix;
  - [ ] the fleet installer's telemetry goes to the same host with no key to redirect it.
- [ ] **Security**: CWS activity dumps and runtime events, CSPM/compliance.
- [ ] **Metrics v3** (`/api/intake/metrics/v3/*`), behind the agent's experimental flag.
- [ ] **Process agent**: realtime frames, the discovery check.
- [ ] **USM**: Kafka and HTTP/2 (HTTP, Postgres and Redis are verified).
- [ ] **Logs over TCP** (:10516).
- [ ] **DBM leftovers**: Postgres column statistics (needs the `datadog` schema function),
      MongoDB explain plans and schemas, SQL Server deadlocks and extended events.
- [ ] **RUM**: `vital` events and the browser profiler's upload (the SDK sent neither in the
      lab); WebKit (needs `libevent-2.1-7t64 libavif16`); the Android SDK in an emulator. iOS
      needs a Mac.
- [ ] The small event-platform tracks: agent discovery, agent health, event management,
      software inventory (Windows), synthetics, OpenLineage, device configuration backups,
      the Private Action Runner.

## 4. Decode deeper

Stored whole today; the columns a product needs are still inside a blob.

- [ ] **DBM query metrics** — with the Database Explorer, not before. Datadog exposes them
      as ordinary metrics: `postgresql.queries.count`, `.time`, `.rows` and about fifteen
      more, "per query_signature, db, and user (DBM only)" (`postgres/metadata.csv` in
      DataDog/integrations-core; the other databases have their own `*.queries.*`). So their
      backend turns each row of `<dbms>_rows` into metric points. Doing the same would make
      them work in the metrics explorer with no new table: points tagged with the query's
      signature, database and user. To think through first:
  - whether the signature really is a tag there, and under which name (the description
    says "per query_signature"; the tag name itself is not verified);
  - cardinality: every distinct query is a series, on every host;
  - where the query text lives, since a tag cannot hold it (the `fqt` events carry
    signature → statement);
  - whether the agent's rows are deltas or running totals for each database (the
    integrations compute the difference between two collections before sending — check).
  Real events of seven databases are in `backend/intake/tests/fixtures/real/`.
- [ ] **DBM, the rest**: one row per session from `<dbms>_activity`; `name`, `status`,
      `category` of health events as columns.
- [ ] **Profiles (pprof)**: samples and stacks. Today the profile is kept as bytes with a
      summary beside it (sample types, sample count, duration, number of functions);
      `intake/src/routers/pprof.rs` already walks samples, locations and functions to count them. Open
      is the table: one row per sample or per distinct stack, how a stack is encoded, the
      TTL. Record a real profile from a tracer first. Needed for flame graphs.
- [ ] **Profiles (JFR)**: the Java tracer uploads Java Flight Recorder files, not pprof.
      They are stored as bytes and not recognised at all: no summary, `attach_is_pprof` 0.
      (From memory of dd-trace-java, not checked against its source: check first.) Decide
      between a JFR reader and leaving Java to whoever needs it.
- [ ] **Session Replay**: segments are stored as they arrive, on purpose, with their
      metadata (view, times, record count, whether a full snapshot is inside). The record
      format is known (Datadog's schema: full DOM snapshot, incremental mutations, pointer,
      scroll). What is missing is a player: Datadog's is not open, so either one of our own
      or rrweb's adapted, since the format descends from it. Panel work, with an endpoint
      that returns a view's segments in order.
- [ ] **NDM**: device scan OIDs, diagnoses and NetFlow exporters out of the generic
      `ndm_metadata_objects`.
- [ ] **Logs**: JSON bodies into attributes, for `@key:value` search (`json-log-attributes.md`).
- [ ] Kubernetes manifests, agent inventories (`/api/v1/metadata`), APM telemetry payloads.
- [ ] CI: coverage bitmaps, git packfiles. Flares (the zip), sourcemaps (the ELF).

## 5. Error Tracking (applications; later)

Not an intake of its own: Datadog builds it on errors that already arrive three ways. The
data is here; the product is not.

- [ ] Grouping: a fingerprint from the error's type, its message with the variable parts
      taken out, and the top frames of its stack; an `issues` table; a job that assigns
      errors to issues. Sources: `rum_events` (type `error`; stack, crash flag and a
      fingerprint column are there), `spans` with an error (`error.type`, `error.message`,
      `error.stack` in meta), logs.
- [ ] Logs carry `error.kind` and `error.stack` inside their JSON body, which is not split
      into attributes yet (section 4, Logs): until then log errors cannot be grouped.
- [ ] JavaScript source maps: `datadog-ci` uploads them to `sourcemap-intake` too, and
      `/api/v2/srcmap` knows only the ELF symbol shape. Record a real upload, store it, and
      unminify stacks before fingerprinting.
- [ ] Mobile symbolication: dSYM (iOS), ProGuard/R8 mapping and NDK symbols (Android), the
      same question.
- [ ] **Sentry-compatible intake** (`sentry-intake.<site>`). Datadog documents it
      (`error_tracking/guides/sentry_sdk` in DataDog/documentation): an application keeps its
      Sentry SDK and changes only the DSN to `https://<TOKEN>@sentry-intake.<site>/1`. Events
      and messages come in as **logs** with `source:sentry-sdk` and feed Error Tracking
      through its logs path; traces, attachments and sessions are not supported. Verified
      there against `@sentry/node` and `@sentry/browser` 9.13, `sentry-sdk` 2.26 (Python),
      Java 8.6, .NET 5.5, Go 0.32, Ruby 5.23. For us: a new program and host (`intake.programs`, `dev/intake-routes.yml`), Sentry's
      envelope endpoint (`POST /api/<project>/envelope/`, key in `X-Sentry-Auth` or
      `sentry_key`), event → log row. The protocol is Sentry's own and public; record a
      real SDK's envelope first. Fits on-prem well: many applications already carry a
      Sentry SDK.
- [ ] Crash reports from the tracers' crash tracker arrive through the telemetry intake
      (unverified; with the telemetry item in section 3).

## 6. Known rough edges

- [ ] **Bug: `k8s_resources_current` keeps deleted objects.** Seen on 2026-10-15 with real
      agents: pods replaced in a rollout stay there as `Failed` (the old Datadog agent pods
      on the jose cluster). The table collapses as it should (`ReplacingMergeTree` on
      `collected_at`, live objects refreshed every cycle); what is missing is expiry: an
      object no later collection lists keeps its last row forever. Simplest fix, at read
      time: count as current only what was collected within a few cycles.
- [ ] **Host aliases are not resolved.** An agent that finds Docker on a Kubernetes node
      takes the Docker daemon's host name (`jose1`), while the kubelet-based checks and the
      cluster agent send the alias (`jose1-jose`, node name plus cluster name). The agent
      lists the alias in its metadata (`host_aliases` in `hosts`). Datadog joins the two
      into one host; NinjaCat shows two (1.17M points as `jose1`, 181k as `jose1-jose` on
      2026-10-15). Map aliases to the canonical host, at query time or on write.
- [ ] **`containers.name` and `containers.image` are empty from agent 7.84.** The
      collector message no longer fills them; the values are in the tags
      (`container_name`, `short_image`, `image_name`, `kube_namespace`). Fill the columns
      from the tags when the fields are empty. With the Kubernetes explorer, not before.
- [ ] **A ClickHouse outage loses data silently.** The intake answers 202 before it writes,
      and a batch whose insert fails is counted and dropped (`storage/src/writer.rs`), so the
      agent, already answered, never resends it. Every restart of ClickHouse (an upgrade,
      the operator rolling the pod, a crash) leaves a gap in every table as long as the
      outage. Needs a decision before anyone relies on NinjaCat: retry with backoff in the
      writer, a spill to local disk, or a durable queue between intake and the writers
      (NATS JetStream or similar), which would also let the writers restart on their own.
- [x] The query port asks for the panel's HS256 token (`ninjapaws_internal/token.rs`,
      2026-10-13); its `tenant` claim is the only source of the tenant.
- [ ] **Datadog's query API on the query port takes only the panel's token.**
      `/api/v1/metrics` and `/api/v2/query/timeseries|scalar` answer the panel alone, so no
      Datadog client can call them. Datadog's clients send an API key and an application
      key (`DD-API-KEY`, `DD-APPLICATION-KEY`); that needs application keys in the panel
      (per user, per tenant), a second authentication scheme next to JwtBearer, and a way to
      reach the port that is not the panel's network.
- [ ] **Logs over TCP (:10516) are plain unless a certificate is given**, so the API key
      travels in clear text, and the agent's default (TLS) does not even connect. Proposed,
      not decided: without `NINJACAT_TLS_CERT`/`NINJACAT_TLS_KEY` the listener does not
      start; plain TCP only when asked for by name, for labs and closed networks.
- [ ] **Log status is stored as the sender spelled it**, except for the compact form of
      Datadog's .NET tracer (`@l`), which is mapped to `info`/`warn`/`error`…. Datadog
      normalises every status (its status remapper: by first letters, syslog numbers);
      until we do, `Warning`, `warn` and `WARN` are three facet values.
- [ ] The dev stack builds the server up to three times on every save, once per process,
      each into its own volume. Executables that share only what they need would cut that
      (`TODO_POSSIBLY.md`).
- [ ] `/api/v2/validate`: the org id is derived from the tenant, so two installations that
      both use `default` share an Org Propagation Marker. Needs an installation id.
- [ ] A `null` in a v2 series' tag list is stored as an empty tag.
- [ ] Connections: DNS names are read only from version 1 of the DNS buffer.
- [ ] `container_host_type` is empty on process discovery frames, `notSpecified` elsewhere.
- [ ] Redis key names arrive only when the agent is told to send them: document the option.
- [ ] RUM: an event type the schema does not know goes to `raw_payloads` as `unknown_event`.
      Nothing is lost, but nothing says so either: watch that reason when the SDKs are
      upgraded.
- [ ] **The lab scripts write keys for tenants that may not exist.** `api_key.tenant_id`
      references `organization.id` since 2026-10-13, but `lab/server.sh` inserts a key for
      the tenant `lab`, which nothing creates, so the insert fails on the foreign key; the
      script has to make its tenant first, as `lab/k8s/scripts/up.sh` now does.
- [ ] `lab/` scripts for MySQL, MariaDB, SQL Server, Oracle, MongoDB, ClickHouse, SNMP and
      system-probe ran before the key and network were renamed; only Postgres and the data
      security run were repeated afterwards.
- [ ] Exceptions used as control flow inside decoders. Done: `UnsupportedNumber` (Trace),
      and the two msgpack readers, which are gone: the MessagePack library reads the bytes
      and decoders take fields from the tree (`MsgFields`). Left: `Misfit` (ApiPayloads) and
      `HealthReportMismatch` (Evp), which go when those decoders move to the shared struct
      reader (section 1a). `NotAProfile` (Pprof) went with the hand-written decoder.
      `Msgpack.value` still raises the library's own exception in three places (too deep,
      a map key that is not a string, a byte that starts no value), caught once in
      `Msgpack.decode` together with the library's.
- [ ] **No size limit anywhere on the intake.** `MaxRequestBodySize` is off (flares and
      profiles are tens of megabytes) and `Body.decompress` inflates without a ceiling, so
      one small compressed request can ask for gigabytes. The RUM intake is the exposed
      one: its key is in every page. A limit per route, on the decompressed size.
- [x] **JSON depth**: one limit, `Json.maxDepth` (512), for every parser. It was four
      (64, 512, 10 000), and four routes answered 500 between 1000 and 10 000 levels: the
      parser let through what `Utf8JsonWriter` then refused to write back. A limit has to
      stay as long as code walks a document by recursion — measured: a plain recursive walk
      of a 20 kB body nested 10 000 deep overflows a 1.5 MB stack, and that ends the process.
      Deeper bodies are kept in raw_payloads. To lift the limit, the recursive walkers
      (`writeSorted`, `canonicalJson`, `Msgpack.value`) would have to become loops.

- [ ] **`snmp_traps.timestamp` is `DateTime` (seconds)**, but the router reads the trap's
      time in milliseconds (`intake/src/routers/ndm.rs`), so the sub-second part is dropped. Found in
      the squash of 2026-10-13, which had to keep the schema as it was.
- [ ] **`processes.has_create_time` + `create_time`** would be one `Nullable(DateTime)`.
      The squash kept the pair to stay identical to the old chain; folding it changes the
      row writers in `storage/src/rows/process.rs`.
- [ ] **0002 refills `metrics_1m` in one `INSERT … SELECT … GROUP BY`** over every raw
      point kept. Fine on the databases that exist; on a large `metrics` it could run past
      the migration client's hour or out of memory, and the next start would redo it. Batch
      it by day if such a database ever has to take it.
- [ ] **The query process and the monitor service do not read the metrics TTLs back.**
      They plan by the retention env vars; the chart passes `metrics.*` to all, the compose
      files rely on defaults. Reading `system.tables` at start would end the duplication.
- [ ] **The intake's API-key keeper does not notice a LISTEN link that died silently.**
      `backend/intake/src/keys.rs` waits on its connection with no keepalive, so a
      half-open link stops notifications until the process restarts; the 30 s re-read
      keeps keys current meanwhile. The monitor service sets TCP keepalives on its
      connection; the keeper should do the same.
- [ ] **The databases the chart makes (`postgres.create`, `clickhouse.create`) have no
      NetworkPolicy.** It must admit their operators from another namespace, or
      reconciliation stops silently.
- [ ] **Nothing backs those databases up.** CloudNativePG's Barman Cloud plugin or volume
      snapshots to an on-prem object store; ClickHouse's `BACKUP` or clickhouse-backup.
- [ ] **The ClickHouse the chart makes is one replica.** More need `Replicated*MergeTree`
      engines in the migrations and a Keeper (Altinity's `ClickHouseKeeperInstallation`).
- [ ] **With `postgres.create` the monitors use the application's role.** A SELECT-only
      one needs a Drizzle migration for the GRANT and a role CloudNativePG manages.
- [ ] **Nothing checks CloudNativePG's certificate.** The server connects in plain text
      (section 1), and so does the panel; verifying means its `<cluster>-ca` Secret.
- [ ] **An unaligned `to` older than the raw retention** loses its last part minute: the
      rollup stops at the minute holding `to`, and the raw points that would finish it
      are gone.
- [ ] **Anomaly monitors keep their models in the entity's memory**
      (`engine/src/monitor/evaluate.rs`, `ninjapaws_monitors/entity.rs`). So: a group that appears between
      refits is learning until the next refit (up to an hour); a hand-off or a restart
      refits every model; a seasonal forecast refits on every evaluation, as nothing
      caches it; and refits are not counted in `ninjacat.monitor.*`, so their cost is
      visible only in `evaluation.time`.

## 7. Panel

- [ ] Global time picker.
- [x] Drag-to-zoom on charts (2026-10-15): a zoom is Datadog's `from_ts`/`to_ts`/`live=false`
      in the URL, beside `view`; the monitor editor keeps its zoom in the page, as its
      queries are not in the URL. A double-click goes back to the range before the zoom.
- [ ] Crash when switching the display type in the metrics explorer (not reproduced yet).
- [ ] Explorer strings into Paraglide (`en` first, then `es`, `pl`).
- [ ] Remove the unused `app/metrics/data` route and `fetchMetricSeries`.
- [ ] Prettier: ignore shadcn and generated files.
- [ ] Log search.
- [x] Tenants (2026-10-13; planned in `docs/protocol/zadania/tenants.md`):
      `frontend/docs/auth-and-route-guard.md`.
- [x] Monitors (2026-10-14; planned in `docs/protocol/zadania/monitors.md`): `backend/README.md`,
      "The monitor service"; what is left is section 7a.
- [ ] **RBAC.** The tenant roles are the organization plugin's (owner, admin, member), and
      only Better Auth's own organization routes check them: the panel's pages do not, and
      the query port ignores the token's `role` claim. Decide what each role may do (keys,
      members, monitors, later the query API) and check it where the action runs; the
      plugin's access control (`ac`, `roles`, `hasPermission`) is the place to start. Until
      then, section 0's item on keys.
- [ ] **Deleting a tenant.** Not offered (`disableOrganizationDeletion`), because its API
      keys (`on delete restrict`) and its ClickHouse rows would be left without an owner.
      Needs: what happens to the rows (drop by `tenant_id`, or keep until TTL), its keys
      revoked first, and `default` refused while self-monitoring writes there. Its members'
      sessions need nothing: the hook moves a session off a tenant it no longer finds.
- [ ] **Invitations by e-mail.** Members are added directly by the instance admin; the
      plugin's invitation routes are in `disabledPaths` because an on-prem install may have
      no e-mail. With an optional mail channel (`sendInvitationEmail`), re-enable them and
      let a tenant's owner or admin invite.
- [ ] A tenant's own owner or admin cannot add members: only the instance admin can, under
      `/app/admin`. A members page for the current tenant (`locals.tenant`, owner or admin)
      would reuse `changeMember` in `lib/server/admin.ts` and `auth.api.addMember`.
- [ ] Better Auth's own `organization/update-member-role`, `remove-member` and `leave`
      routes skip the row lock `changeMember` takes, so an owner there and the instance
      admin at the same moment could leave a tenant with no owner. Route both through one
      path, or disable those routes once the members page above exists.
- [ ] Nothing writes `api_key.last_used_at`, so the keys page shows every key as never
      used. The intake looks a key up on every request and the keeper is its only reader of
      Postgres; a write per request is too many, one per key per keeper cycle is enough.
- [ ] Accounts on the admin pages: ban, delete, reset a password. The admin plugin has the
      calls (`banUser`, `removeUser`, `setUserPassword`); the pages offer none yet.
- [ ] Data retention set in the panel. For now `NINJACAT_METRICS_RETENTION_DAYS` and
      `NINJACAT_METRICS_ROLLUP_RETENTION_DAYS`; other tables keep a fixed TTL.
- [ ] On-call, built in-house (on-prem sovereignty): external services only as optional
      channels.
- [ ] "NinjaCat on NinjaCat": a stateless watchdog that alerts through one simple channel
      when the main stack is down.

## 7a. Monitors

What the first version of monitors left out, on purpose. The run at the capacity target is in
section 0; the anomaly model cache in section 6.

- [ ] **Notifications for real.** The notifier is a stub: it logs and writes a `stub` row.
      Missing: rendering the message template, channels (built in, external ones only as
      optional plug-ins), renotify, `notify_by`, escalations. The options are stored and
      validated, not acted on, and the panel says so.
- [ ] **Manual resolve.** The `resolved` reason and the `actor` column of
      `monitor_transitions` are ready for it.
- [ ] **Downtimes**, decided when notifying (a `muted` notification), not on the transition.
- [ ] **Composite monitors.**
- [ ] **Groups removed on purpose still alert.** A deleted deployment or node is No Data
      under `notify_no_data` until the group expires after 24 h, as in Datadog; Datadog
      auto-mutes cloud hosts shut down by autoscaling, and Kubernetes knows a deployment is
      gone. Tell a deliberate removal from an outage.
- [ ] **Custom schedules and cumulative windows** (`scheduling_options`, `current_*`
      windows): refused today.
- [ ] **Retention of the tables other than `metrics` and `metrics_1m`** is fixed in their
      migrations (the monitor tables: 400 days of transitions and notifications, 7 of
      evaluations). Make it configurable as the metrics' is.
- [ ] **Percentile and distribution monitors.** The `percentile` time aggregator is
      refused by the monitor parser (`engine/src/monitor/parse.rs`).
- [ ] **A Datadog-compatible `/api/v1/monitor`** on the `app.` host, so Terraform and
      Datadog's clients can manage monitors. The stored shape is already Datadog's.
- [ ] **Log monitors.**
- [ ] The monitor list loads every monitor of the tenant and asks for all their states in
      one call; no paging.

## 8. Language debt

English is canonical; what is still Polish is listed in `CLAUDE.md` ("Language policy"):
`docs/protocol/**`, `experiments/goja-sandbox/README.md` and the copy of the two home pages
(`/`, `/app`). Translate when touched; rename routes in one deliberate batch.
