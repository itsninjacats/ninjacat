# TODO

Open work, grouped by what it unblocks. Scoped write-ups of single items live in
`api/docs/zadania/`; this file is the list.

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
      actions pinned to commits; `latest` only on a release. Not yet run on GitHub.

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
      (`frontend/src/lib/server/auth.ts`), in the process's memory. With more than one panel
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

- [ ] **Merge and tag.** The chart on `main` is still the Go one. Merge this branch, tag
      from `main`; `ci.yml` has not run on GitHub yet. (The owner does this.)
- [ ] **A key has no kind** (decided 2026-10-02: do it, later). A RUM client token is a
      full API key: copied from a page it writes metrics, logs and traces for its tenant.
      A `kind` column on `api_key` (the panel's schema), carried as a claim by the keeper;
      the browser scheme takes only client keys, the other three refuse them; the keys page
      lets one be made. Until then, do not set `PUBLIC_NINJACAT_RUM_*` on a public panel.
- [ ] Logs over TCP (:10516): on by default in the image (the chart turns it off), plain
      text unless given a certificate, no limit on connections, no timeout, the key checked
      only when a whole frame has arrived.
- [ ] The panel sends no security headers (CSP, `frame-ancestors`, HSTS, nosniff).
- [ ] The server falls back to the ClickHouse password `ninjacat` when none is set
      (`Config.fs`); the chart and `compose.prod.yaml` always set one. Fail instead.
- [ ] `compose.prod.yaml`: the panel on every interface as plain HTTP, no health checks on
      the three services, logs not JSON (`Logging__Console__FormatterName=json`).
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

## 1. Finish the move to one server

The F# server (`api/`) is the only server: the Go one was removed on 2026-10-02, and its
notes moved to `api/docs/`. What still names it:

- [x] `lab/k8s/`: builds `api/` and installs NinjaCat from `helm/ninjacat`
      (`ninjacat-values.yaml`); its own manifests are only what the chart does not own
      (data stores, the panel's NodePort), with Traefik in front and a NetworkPolicy that
      lets nothing in the namespace reach the internet. Run on 2026-10-02 with agent 7.84.0: every table
      of `verify.sh` filled except `container_events` (no container died), nothing in
      `raw_payloads`. What it found is in section 3.
- [x] `helm/ninjacat`: one image, two Deployments (`intake`, `query`), a hook Job per
      schema (`node migrate.js`, `ninjacat-api migrate`), `CLICKHOUSE_HTTP_ADDR`, an
      optional read-only ClickHouse user for `query`. Checked in the lab: install, upgrade,
      `helm test`, two intake replicas.
- [ ] `api/docs/**` describe Go files that no longer exist; say what the F# equivalent is
      as each note is translated.
- [x] Consolidate what the parallel port wrote several times: section 1a.
- [x] Comments that explain a behaviour by "as Go did": say why the behaviour is right, or
      change it.

## 1a. One copy of each helper

From an independent audit of `api/` on 2026-10-01, done on 2026-10-02: `src/Intake` went
from 17 383 to 14 779 lines. Where the copies differed, the difference was a bug or a
decision; each item says which way it went.

- [x] **JSON parsing**: one parser, `Json` in `Common.fs` (`tryParse`, `tryParseFirst`,
      `tryParseObject`, `tryParseList`), one depth limit, one repair. `Lenient.fs` and
      the private parsers of `ApiPayloads`, `CiVisibility` and `GoCompat` are gone. Two
      things changed with it: raw JSON kept in a column has half a surrogate pair repaired
      (`\ufffd`) like everything else, and a time with a one-digit hour (`T1:30:00Z`) is
      not RFC 3339 and is no longer read as one.
- [x] **JSON of a protobuf message** is read by protobuf's own `JsonParser`
      (`ProtoJson.tryParse`): `/api/v2/series` and `/api/beta/sketches` as JSON, and the
      agent's health report. ~400 lines of field-by-field filling are gone. Its rules stand:
      names as the schema spells them or in camelCase, an enum by number or name, unknown
      keys passed over, and a null inside a list or a map refuses the payload (kept raw).
- [x] **Reading an object field by field**: one reader, `JsonFields` (`JsonFields.fs`,
      beside `Common.fs`), instead of five. Its rule, once: a member is found by its exact
      name, the last of a repeated name counts, null is the same as absent, an integer is
      read from its digits. Notes are one wording about the JSON (`series[2].points:
      expected a list, got string`); nothing names a Go type any more. Dropped with the
      copies: matching names without regard to case, an item refused because an unrelated
      number overflows a float64, the two levels "is not"/"did not fit" of the v1 models.
      An event with an invented `alert_type` or `priority` is now a row, with the sender's
      word in `alert_type_raw`/`priority_raw`. The version of a JSON coverage payload is read leniently (0
      when it cannot be read), as in the msgpack form.
- [x] **Multipart**: one strict reader, `Multipart.tryParts` in `Http.fs`, on ASP.NET's
      `MultipartReader`; the lenient "parts before the break" only for flares and the
      security dump. Profiling's copy of Go's `mime.ParseMediaType` is gone
      (`MediaTypeHeaderValue`). A part's name counts whatever its disposition type, and a
      file name keeps its directories.
- [x] **Time**: the repeated clamps and the dead `try/with` are gone; a protobuf
      `Timestamp` is read by one function, `Time.ofTimestamp`, over the library's
      `ToDateTime()` — an invalid one (nanos outside a second, a year past 9999) is None.
- [x] **`JsonElement` accessors**: one set in `Json` (`Common.fs`): `field`, `child`, `at`
      (a path), `lenientString`, `flag`, `text`, `textList`, `otherMembers`, …; the private
      layers of eight routers are gone. `JsonFields.fs` is only `Fields`, the reader that
      notes mismatches. Where a column keeps a member's JSON text, an explicit null stays `null`;
      a typed column that is not Nullable holds `""` or 0 for null and absent alike.
- [x] **Varints and wire walkers**: one `Varint.read` (through `CodedInputStream`);
      `Security.wireLayout` walks with the library's reader. Capture's debug dump is the
      other walker left, and does a different job.
- [x] **Enum names**: `ProtoEnum.name` in `Common.fs`, for any generated enum, from the
      `OriginalName` attribute the generator writes; a value the schema lacks is its number.
      `ProcessEnum.fs`, three copies and Evp's hand-typed tables are gone.
- [x] **Writing JSON**: one writer, `Json.write`; each output format once (`compact` as
      sent, `compactSorted` with keys sorted and the last duplicate winning, `quoted`).
- [x] Small ones: `accepted` once, in `Http.fs`; `Ctx.write ctx table rows` writes nothing
      without a tenant and replaced 62 checks; Go's `%q` and `FormatFloat` copies are gone
      (`Json.quoted`, `Text.ofFloat`); a body is split by `Json.tryParseList`,
      `tryParseArray` or `tryParseElementBytes`.
- [x] **Names**: no module is called `GoJson`; only the intake has a `Json` (the engine's
      is `Serialization`); `Rows/ProcessExtra.fs` is folded into `Rows/Process.fs`,
      `Rows/ApiExtra.fs` split into `Rows/Agent.fs` and `Rows/Runner.fs`. Left: routers and
      table modules that share a name across projects (`Logs`, `Containers`, `Config`,
      `Metrics`) need a naming rule first; `ProtoJson.message` and `Security.protoJson`
      write protobuf JSON with different escaping (`\u003C` or `<`), and unifying them
      changes stored text.
- [x] **Tests**: one request sender, one multipart builder and one string → `JsonElement`
      in `tests/Intake.Tests/Golden/Requests.fs`. Left: `RumTests.requestOf` still builds
      its own context.
- [x] **Engine**: one SQL parameter collector (`SqlParams` in `Sql.fs`), one tag-filter
      model and SQL writer (`MetricQuery/Filter.fs`), one duration parser (`Duration.fs`),
      one `maxPoints` (1500, in `Plan.fs`; that it is Datadog's cap is the comment's claim,
      not a checked fact). `/internal/metrics/query` goes through the query engine's
      planner, so the panel's series aggregate in time first and across series second, like
      every other query; `Panel.querySeries` is gone. Checked against the live ClickHouse.
- [x] **Dead code**: `Text.joinOrDash`, `ProcessFrame.typeName`, `DDSketch.bounds`,
      `/internal/metrics/hosts` with its query. Left: `SyntheticsTestConfigs.table` in
      `Rows/Ci.fs` has a table (migration 0017) and no writer.
- [x] **Parity machinery**: `TraceSketch.GoMath` is `System.Math` now (stored sketch
      summaries differ from about the 13th digit; the sketch's accuracy is 1 %),
      `hasUnknownFields` is gone, and no note names a Go type or quotes a Go library.
- [x] Comments that cited Go as the reason say why in their own terms; the ones that
      describe a Go library as the definition of a wire format stay.

## 1b. Use what the framework and the libraries already do

From a second audit on 2026-10-01, of home-made replacements for things .NET, ASP.NET or a
referenced library provides. Done on 2026-10-02 unless ticked otherwise:

- [x] The intake's own HTTP router → Oxpecker's routes, ASP.NET's matcher.
- [x] **Pprof**: the hand-written protobuf decoder → the class generated from
      `profile.proto`. Five verdicts on malformed input follow protobuf's rules now
      (a field sent twice, a wrong wire type, a group, strings that are not UTF-8).
- [x] **Protobuf to JSON**: the three hand writers → `JsonFormatter`, through `ProtoJson` in
      `Common.fs`. Every JSON column of the Kubernetes, ECS and process tables is proto3's
      JSON now: 64-bit integers are strings, enums are names, absent is "". Rows written
      before hold the Go-shaped JSON; nothing rewrites them.
- [x] **Logs over TCP**: own accept loop and TLS → a Kestrel endpoint with a connection
      handler (`TcpLogs.Connection`). Checked live: plain, TLS, split frames, a wrong key,
      a server stopping with connections open.
- [x] **RUM CORS** → ASP.NET's CORS middleware. Checked in Chromium and Firefox. No
      headers at all for a request that names no origin; Allow-Methods, Allow-Headers and
      Max-Age only on preflights.
- [x] **RUM client address** → `UseForwardedHeaders`, trusting the loopback and the
      proxies named in `NINJACAT_TRUSTED_PROXIES`. `X-Real-Ip` is no longer read.
- [x] **Engine request errors**: the regexes over the deserializer's exception text are
      gone; its message is passed on as it is.
- [x] **Kubeops**: the fields read by name string → one explicit function per kind.
- [x] Small: `parseHex64`, `isSpace`, `formatIPv6`, `median`, `movingAverage`, `dist` use
      the library; FNV-1a is one function (`Storage/Fnv.fs`); varints are read by
      `CodedInputStream` (`Varint` in `Common.fs`, `Capture.fs`); `wrongWireType` is gone.
- [x] **Native handlers**: the intake's handlers are Oxpecker's (`byte[] ->
      EndpointHandler`, bound with `bindBody`), they answer with Oxpecker's own handlers,
      and the key is an ASP.NET authentication scheme. `Request`, `Response`, `Deps` and
      the adapter are gone. Query parameter names are matched as ASP.NET matches them,
      without regard to case.
- [ ] **ClickHouse inserts**: the driver already gets the rows (`InsertBinaryAsync` with
      `object[]`); nothing is hand-built there. Inserting records instead is possible
      (`InsertBinaryAsync<T>`, a `ClickHouseColumn` attribute per field) and would remove
      the ~900 lines of `Values` lambdas and the pairing of columns by position, but 207
      option fields would have to become `Nullable`/null, nested records be flattened, and
      the driver cannot write nanosecond `DateTime64` at all. A migration, not a deletion.
      Small things that can go now: `Table.Writer` (89 of 90 are `"storage_" + Name`), the
      backtick trimming for one column, `WriterLimits`.

Looked home-made, kept on purpose: body decompression (the middleware drops
`Content-Encoding`, has no zstd, and throws on a mislabelled body), the migration runner
(nothing referenced migrates ClickHouse), the Postgres URL parser (Npgsql refuses
`postgres://` URLs), `Config.fs`, `Capture.fs`, DDSketch and the STL/DBSCAN/Huber code
(MathNet has none of them), the RFC 3339 and half-surrogate handling.

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
      kind lab (2026-10-02, agent 7.84.0); record them into `Fixtures/real/`. Cluster-agent
      actions and container lifecycle events were not produced by that run.
- [x] The agent's connectivity check posts `{}` to every event-platform intake at start,
      and `sbom`, `contlcycle`, `contimage` and `data-streams` each logged a decode warning.
      A kept payload is logged once now, by `Raw.store`, which already knew a probe; the
      82 handler warnings that said the same thing first are gone.
- [x] `HEAD /support/flare` answers 200 without a key: the agent's connectivity check
      (`pkg/diagnose/connectivity`, 7.84.0) sends it with none and takes anything but 200 or
      a redirect for a failure. The upload itself still asks for the key.
- [x] `libgssapi-krb5-2` is in the shipping image: Npgsql loads it to try GSS encryption,
      and printed `Cannot load library libgssapi_krb5.so.2` at every start without it.
- [ ] **Containers**: lifecycle events, image inventory, SBOM.
- [ ] **Tracers** — one small app with `dd-trace` covers: profiles, the debugger and symbol
      uploads, Data Streams Monitoring, tracer telemetry, CI Visibility (test cycle,
      coverage, git endpoints).
- [x] **NinjaCat sending to NinjaCat** (`compose.self.yaml`, off unless named): the panel
      through `@datadog/browser-rum` and `@datadog/browser-logs` with `proxy`, both server
      processes through `dd-trace-dotnet` 3.54.0 (traces, runtime metrics, the continuous
      profiler, logs by direct submission, tracer telemetry) and an agent on a network with
      no route out. Checked on 2026-10-01: the browser talks to the panel and the intake
      only; a traced process asks DNS for `agent`, `clickhouse`, `postgres` and its own
      intake alias and nothing else. First finding: the tracer's logs have no `message`
      (Serilog's compact form), which the logs intake refused. Left:
  - [ ] `dd-trace` in the SvelteKit server, and `allowedTracingUrls` so a RUM resource
        links to the server span that answered it;
  - [ ] the agent there sees only its own container: no container logs, no host metrics
        (both need mounts from the host, which is the operator's call);
  - [ ] `ninjacat.node.*` self-metrics come from `intake` only; `query` holds no writer;
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
  - [ ] record a real payload of each producer into `Fixtures/real/` and hold the decoder
        (`Routers/Misc.fs`) to them;
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
  Real events of seven databases are in `Fixtures/real/`.
- [ ] **DBM, the rest**: one row per session from `<dbms>_activity`; `name`, `status`,
      `category` of health events as columns.
- [ ] **Profiles (pprof)**: samples and stacks. Today the profile is kept as bytes with a
      summary beside it (sample types, sample count, duration, number of functions);
      `Routers/Pprof.fs` already walks samples, locations and functions to count them. Open
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
      Java 8.6, .NET 5.5, Go 0.32, Ruby 5.23. For us: a new host in `Routes.fs`, Sentry's
      envelope endpoint (`POST /api/<project>/envelope/`, key in `X-Sentry-Auth` or
      `sentry_key`), event → log row. The protocol is Sentry's own and public; record a
      real SDK's envelope first. Fits on-prem well: many applications already carry a
      Sentry SDK.
- [ ] Crash reports from the tracers' crash tracker arrive through the telemetry intake
      (unverified; with the telemetry item in section 3).

## 6. Known rough edges

- [ ] **The query process (:8081) asks for nothing.** No key, no session, and `?tenant=`
      lets the caller name any tenant. All that protects it is the network: its own
      container, a port compose does not publish (loopback only in the dev stack). It needs
      a credential the panel presents — a shared secret at least, the user's session and
      tenant at best — before a second tenant exists.
- [ ] **Logs over TCP (:10516) are plain unless a certificate is given**, so the API key
      travels in clear text, and the agent's default (TLS) does not even connect. Proposed,
      not decided: without `NINJACAT_TLS_CERT`/`NINJACAT_TLS_KEY` the listener does not
      start; plain TCP only when asked for by name, for labs and closed networks.
- [ ] **Log status is stored as the sender spelled it**, except for the compact form of
      Datadog's .NET tracer (`@l`), which is mapped to `info`/`warn`/`error`…. Datadog
      normalises every status (its status remapper: by first letters, syslog numbers);
      until we do, `Warning`, `warn` and `WARN` are three facet values.
- [ ] The dev stack builds the server twice on every save, once per process, each into its
      own volume. Two executables that share only what they need would halve that
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

## 7. Panel

- [ ] Global time picker, with drag-to-zoom on charts.
- [ ] Crash when switching the display type in the metrics explorer (not reproduced yet).
- [ ] Explorer strings into Paraglide (`en` first, then `es`, `pl`).
- [ ] Remove the unused `app/metrics/data` route and `fetchMetricSeries`.
- [ ] Prettier: ignore shadcn and generated files.
- [ ] Log search.
- [ ] Monitors — the owner writes these himself. Design notes: evaluation per window length,
      state transitions and evaluations in ClickHouse, definitions in Postgres, Akka.NET
      with `IWithTimers`, notifications as their own actors behind an outbox.
- [ ] On-call, built in-house (on-prem sovereignty): external services only as optional
      channels.
- [ ] "NinjaCat on NinjaCat": a stateless watchdog that alerts through one simple channel
      when the main stack is down.

## 8. Language debt

English is canonical; what is still Polish is listed in `CLAUDE.md` ("Language policy"):
`api/docs/**`, `frontend/docs/**`, route segments (`/zadania`, `/app/ustawienia/klucze`),
identifiers in `frontend/src/lib/server/api-keys.ts`. Translate when touched; rename routes
in one deliberate batch.
