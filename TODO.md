# TODO

Open work, grouped by what it unblocks. Scoped write-ups of single items live in
`server/docs/zadania/` (to move, see below); this file is the list.

**Focus for now: infrastructure** — the agent, Kubernetes, network, databases — and the
backend that receives it. What applications send (tracers, profiles, RUM, error tracking) and
the panel come after. Sections 1 to 3 first; within 3, Kubernetes and containers before
tracers.

## 1. Finish the move to one server

The F# server (`api/`) took over from the Go one; compose already runs only it, as two
processes (`intake`, `query`). What is left before `server/` can go:

- [ ] `lab/k8s/`: build and deploy `api/` instead of `server/` (image, `CLICKHOUSE_HTTP_ADDR`,
      `ninjacat-api migrate`), then run it. Also the first live check of the Kubernetes
      intake on F#.
- [ ] `helm/ninjacat`: the same switch in the chart (`templates/server.yaml`, the migrate
      job, `values.yaml`).
- [ ] Move `server/docs/` to `api/docs/` and fix the links to it (`CLAUDE.md`, `api/README.md`,
      comments).
- [ ] Delete `server/`, the root `go.mod` and the empty `query/`. The ClickHouse migrations
      already live in `api/schema/migrations/`; the copy under `server/` goes with it.
- [ ] Consolidate what the parallel port wrote several times: section 1a.
- [ ] Comments that explain a behaviour by "as Go did": say why the behaviour is right, or
      change it.

## 1a. One copy of each helper

From an independent audit of `api/` on 2026-10-01. About 1 300 to 1 450 lines of
`src/Intake` and 320 of the tests can go. Where the copies differ, the difference is a bug or
a decision; those come first. Before anything else: move `GoCompat.fs`, `Lenient.fs` and
`ProcessEnum.fs` up beside `Common.fs` in `Intake.fsproj` — they compile too late for the
routers that needed them, which is how the copies came about.

- [ ] **JSON parsing**: seven parsers, four depth limits (64, 512, 10 000), three repair
      policies. One: `Common.Json` (repair, 10 000) plus `tryParseFirst` for "the first
      value, whatever follows". `CiWebhook` uses the 512 one by accident of `open` order;
      `Evp.healthIssuesRaw` re-parses a body with different rules than its own decoder.
      Decide what `Lenient` keeps of a half surrogate pair in raw text (`a\ud800` today,
      `a�` everywhere else).
- [ ] **Go-style struct reading**: five implementations (`GoCompat.Fields`, ApiPayloads
      `fill`/`Declared`, `CiVisibility.GoStruct`, Evp `go*`, `Kubeops.decodeAction`), five
      wordings of the same note, and a real disagreement: for `{"v":"a","v":null}` three
      give `""` or absent and two give `"a"`. Go leaves a scalar alone on null, so the two
      are right. One reader, the rule fixed once; note texts change, fixtures get `edited`.
- [ ] **Multipart**: four readers. They disagree on a body that breaks off (silently partial
      or an error), on a part name without `form-data`, on directories in a file name.
      One strict `Multipart.tryParts`, the lenient one as a wrapper where flares want it.
      Profiling's private copy of Go's `mime.ParseMediaType` (125 lines) goes with it.
- [ ] **Time**: clamps repeated in `Api`, `Rum`, `Process`, `Kubeops` that
      `Time.fromUnix*` already does; `try/with ArgumentOutOfRangeException` around calls
      that no longer throw, under comments promising `None` (`Security`, `Evp`,
      `Profiling`); four protobuf-`Timestamp` readers with four rules (`sdsTime` can
      overflow); `Lenient.rfc3339` beside `Time.tryRfc3339`, which wrongly refuses a
      one-digit hour.
- [ ] **protobuf → JSON text** written twice with byte-identical output: `Kubeops.GoJson`
      (generic) and `Process.GoJson` (by hand, 150 lines).
- [ ] **`JsonElement` accessors**: every router has its own `text`, string list, flag,
      integer, "keys without a column" (nine names). Decide first whether a null among the
      extra keys is stored as `""` or `"null"`: both exist.
- [ ] **Varints and wire walkers**: six varint readers of two semantics, four schema-less
      protobuf walkers.
- [ ] **Enum names**: five implementations; `ProcessEnum` throws for an enum outside its
      two descriptor files; Evp has three hand-typed tables.
- [ ] **Writing JSON**: the same writer block at thirteen sites; two functions both named
      `GoJson.compact` that give different output (duplicate keys, order), picked by `open`
      order.
- [ ] Small ones: `accepted` defined eleven times; `if r.Tenant <> "" then Sink.write` 57
      times; Go's `%q` twice with different escaping; `FormatFloat` twice; a body split into
      items seven ways.
- [ ] **Names**: five modules called `GoJson`, three things called `Json`, three called
      `Engine`. `Rows/ApiExtra.fs` and `Rows/ProcessExtra.fs` are named after the porting
      agents' file boundaries.
- [ ] **Tests**: the request sender exists in fourteen copies (some add the API key, some do
      not; some split the query string, some do not) → one in `Golden/Replay.fs`; the
      multipart builder in four; string → `JsonElement` in ten with three parsers.
- [ ] **Engine**: two SQL parameter collectors, two tag-filter models, three duration
      parsers, `maxPoints` 2000 in `Panel` and 1500 in `Plan`. `Panel.querySeries`
      aggregates in one pass, which `Compile.fs` itself calls wrong for multi-host series.
- [ ] **Dead code**: `Text.joinOrDash`, `ProcessFrame.typeName`,
      `Json.tryInt64`; used only by tests: `Json.tryParseList`. Unused
      endpoint: `/internal/metrics/hosts` (check for other callers first).
- [ ] **Parity machinery with nothing left to match**: `TraceSketch.GoMath` (125 lines
      re-implementing Go's `math` to the last bit), `ApiPayloads.wrongWireType` and
      `hasUnknownFields`, notes that name Go types (`cannot unmarshal … into Go value of type
      gogen.MetricPayload`). The Go route set names (`routeAPI`…) are out of the server:
      `Routes.fs` is a plain chain of hosts, and only `Golden/Replay.fs` knows the names.
- [ ] 141 comment lines in `src` cite Go. Half describe Go's libraries as a format spec and
      stay; fix the ones that are false or present tense: `Server/Config.fs:60`,
      `Intake/Routes.fs:14`, `Engine/Query/Compile.fs:102,123`, `Routers/Lenient.fs:104`,
      `Routers/Rum.fs:353` (trusting any `X-Forwarded-For`, justified only by Gin's default
      — a security decision that needs its own reason).

## 1b. Use what the framework and the libraries already do

From a second audit on 2026-10-01, of home-made replacements for things .NET, ASP.NET or a
referenced library provides. Each replacement API was checked against the pinned version;
the items themselves are not started. (Done already: the intake's own HTTP router, replaced
by Oxpecker's.)

- [ ] **Pprof**: `Routers/Pprof.fs` decodes by hand a message `src/Proto` already generates
      (`Perftools.Profiles.Profile`). `Profile.Parser.ParseFrom` plus the soundness checks;
      about 200 lines go. A few verdicts on malformed input change (`pprofVerdicts`).
- [ ] **Protobuf to JSON**: three hand writers (`Kubeops.GoJson`, `Process.GoJson`,
      `ApiPayloads.sketchJson`) beside `JsonFormatter.Default`, which `Security.fs` and
      `Evp.fs` already use, so two dialects are stored. `JsonFormatter` cannot write the
      Go-shaped one (64-bit integers as strings, spaces). Decide: one writer of ours, or the
      library's everywhere and every kubeops and process fixture `edited`.
- [ ] **Logs over TCP**: own accept loop, TLS handshake and a buffer that is quadratic on a
      busy connection. Kestrel's `Listen` + `UseHttps` + `UseConnectionHandler`; the framing
      code stays.
- [ ] **RUM CORS**: hand-written headers; ASP.NET's `AddCors`/`UseCors`. Differences to
      check against 20 fixtures: what it sends without an `Origin`, and on non-preflights.
- [ ] **RUM client address**: re-implements Gin's trust-every-proxy default.
      `UseForwardedHeaders`, which is also where the trust decision belongs.
- [ ] **ClickHouse inserts**: the driver already gets the rows (`InsertBinaryAsync` with
      `object[]`); nothing is hand-built there. Inserting records instead is possible
      (`InsertBinaryAsync<T>`, a `ClickHouseColumn` attribute per field) and would remove
      the ~900 lines of `Values` lambdas and the pairing of columns by position, but 207
      option fields would have to become `Nullable`/null, nested records be flattened, and
      the driver cannot write nanosecond `DateTime64` at all. A migration, not a deletion.
      Small things that can go now: `Table.Writer` (89 of 90 are `"storage_" + Name`), the
      backtick trimming for one column, `WriterLimits`.
- [ ] **Engine request errors** are three regexes over the JSON deserializer's exception
      text (`Api/V2/Timeseries.fs`). Reading the two request bodies by hand is longer and
      does not depend on message wording.
- [ ] **Kubeops** reads protobuf fields by name string to treat 24 collector kinds as one.
      Plain: one branch per kind, about 100 lines more.
- [ ] Small: `Trace.parseHex64` → `UInt64.TryParse(…, AllowHexSpecifier)`;
      `Profiling.isSpace` → `Rune.IsWhiteSpace`; `Ndm.formatIPv6` → `IPAddress.ToString()`
      (differs only for embedded IPv4); `median`, `movingAverage`, `dist` → MathNet;
      FNV-1a written twice; the hand-written varint readers → `CodedInputStream`.

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

- [ ] **Kubernetes**: orchestrator collections, manifests, cluster-agent actions (with the
      `lab/k8s` item above).
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
    telemetry arrives through it as `apm_telemetry` rows. The labs do not yet:
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
      reader (section 1a); `NotAProfile` (Pprof), with the shared protobuf field walker.
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
`server/docs/**`, `frontend/docs/**`, route segments (`/zadania`, `/app/ustawienia/klucze`),
identifiers in `frontend/src/lib/server/api-keys.ts`. Translate when touched; rename routes
in one deliberate batch.
