# Porting the Go intake to F#: the guide every porting task follows

## What is going on

NinjaCat is a drop-in replacement for Datadog: the Datadog Agent is pointed at
it and it stores what the agent sends in ClickHouse. The intake is written in
Go (`server/`). The owner wants ONE language and one server, so the Go intake
is being moved into the F# service (`api/`), router by router. The Go code is
the specification: the F# code must answer the same and store the same rows.

You are porting one group of routers. Other tasks port the other groups at the
same time, each in its own git worktree, and the results are merged afterwards.
That is why the rules about which files you may touch matter.

Repository root: your current working directory (a git worktree). Paths below
are relative to it.

## Read these first (the foundation, already written)

- `api/src/Intake/Http.fs` — `Request`, `Response`, `Handler`, `Route`, `Body`, `Multipart`.
- `api/src/Intake/Common.fs` — `Tags`, `Text`, `Time`, `Json`, `Wire`, `Raw.store`, `SelfMetrics`, `Diagnose`.
- `api/src/Intake/Engine.fs` — auth, route matching, the engine. `api/src/Intake/Routes.fs` — host dispatch and the list of route sets.
- `api/src/Intake/Msgpack.fs`, `api/src/Intake/DDSketch.fs`, `api/src/Intake/ProcessFrame.fs` (the process-agent / orchestrator envelope: Go's `process.DecodeMessage`, `process.EncodeMessage`, `MessageType.String()`).
- `api/src/Intake/Routers/Logs.fs` + `api/src/Storage/Rows/Logs.fs` — THE WORKED EXAMPLE. It is the port of `server/intake/router_logs.go` + `server/apps/storage/rows_logs.go`. Read the Go and the F# side by side before you start; your port should look like it.
- `api/src/Intake/Routers/Flare.fs` + `api/src/Storage/Rows/Flares.fs` — a second example (multipart).
- `api/src/Storage/Table.fs` — `Table<'row>`, `ISink`, `Sink.write`, `Col`, `UnixNanos`.
- `api/src/Storage/Rows/Raw.fs`, `Rows/Metrics.fs` — more row examples.
- `api/tests/Intake.Tests/Fixtures/README.md` and `api/tests/Intake.Tests/Golden/*.fs` — the golden fixtures and how they are compared.
- `api/src/Proto/protos/README.md` — the protobuf messages available (generated C#, namespaces like `Datadog.Agentpayload`, `Datadog.ProcessAgent`, `Datadog.Trace`, `Datadog.Trace.Idx`, `Pb`, `Datadoghq.Api.Metrics.V3`, `Datadog.Contlcycle`, `Datadog.Contimage`, `Datadog.Sbom`, `Cyclonedx.V14`, `Datadog.Cws.Dumpsv1`, `Datadog.Healthplatform`, `Datadog.Agentdiscovery`, `Datadog.Kubeactions`, `Datadog.Config`, `Perftools.Profiles`, `Test` (DDSketch)). To see a generated class, build `api/src/Proto` and look under `api/src/Proto/obj/Debug/net10.0/`.

The Go side:

- `server/intake/router_<x>.go` — the handlers you port. Their comments hold hard-won knowledge about the agent's protocol.
- `server/intake/router_<x>_test.go` — their tests.
- `server/apps/storage/rows_<x>.go` — the row structs, the INSERT statements, `AppendTo`.
- `server/intake/server.go`, `payloads.go`, `raw.go`, `body.go`, `apikey_mw.go`, `routes.go` — the shared Go code (already ported: see the foundation).
- Go libraries the handlers use are in the module cache: `$(cd server && go env GOMODCACHE)` (e.g. `github.com/!data!dog/datadog-api-client-go/v2@v2.65.0/api/datadogV1/model_*.go`, `github.com/!data!dog/agent-payload/v5@v5.0.207/...`). When Go decodes into a Datadog-published model, READ that model's `UnmarshalJSON` and reproduce what it does (declared fields, required fields, `AdditionalProperties`, `UnparsedObject`), as `Routers/Logs.fs` does for `HTTPLogItem`.

## What you produce

For each Go row file you own → `api/src/Storage/Rows/<X>.fs` (a stub exists):

- One F# record per Go row struct, with EXACTLY the Go struct's exported field
  names (the golden test compares field by field; `TenantID`, not `TenantId`).
  A nested/embedded Go struct is a nested record under the same field name.
- One `Table` per Go writer: `Writer` = the Go writer atom's string
  (`"storage_x"`), `Name` = the table, `Columns` = the INSERT column list in
  order, `Values` = what `AppendTo` passes, in order. Take `MaxRows`,
  `FlushInterval`, `BufferLimit`, `MaxInFlight` from the Go `WriterConfig`
  (only the ones it sets; `Table.create` has the Go defaults).
- Type mapping:

  | Go                       | F#                                         |
  |--------------------------|--------------------------------------------|
  | `string`                 | `string` (or `byte[]` if it carries raw, possibly non-UTF-8 bytes: bodies, archives, pprof) |
  | `[]byte`                 | `byte[]`                                   |
  | `time.Time`              | `DateTime` (UTC). For a `DateTime64(9)` column: `UnixNanos` |
  | `*time.Time`, `*float64`, `*string`… | `'T option`; in `Values` pass `Col.opt x` |
  | `[]*time.Time`           | `DateTime option[]` → in `Values` map to `Nullable<DateTime>[]` |
  | `[]string`, `[]int64`…   | arrays (`string[]`)                        |
  | `map[string]string`      | `Map<string, string>`                      |
  | `map[string][]string`    | `Map<string, string[]>`                    |
  | `uint8/16/32/64`, `int32/64`, `float32/64`, `bool` | the same .NET type      |
  | `[16]byte` / `uuid.UUID` | `Guid`                                     |
  | `orEmpty(x)`, `orEmptySlice(x)` | nothing: F# maps/arrays are never nil |
  | `orJSONObject(s)`        | `Col.jsonObject s`                         |

  Check the column's ClickHouse type in `server/schema/migrations/*.sql` when
  in doubt. The ClickHouse driver accepts F# `Map`, arrays, tuples
  (`System.Tuple`), `Guid`, `DateTime`, `byte[]` for String columns, and
  numerics of any width.

For each Go router file you own → `api/src/Intake/Routers/<X>.fs` (a stub
exists, with the names `Routes.fs` already refers to — keep those names and
their types):

- Handlers are `Request -> Response`: they RETURN the answer. The body is
  `r.Body` (already decompressed). `r.Header`, `r.Query`, `r.Param`, `r.Tenant`.
- Rows go out with `Sink.write r.Sink <Table> rows` (it skips empty batches,
  like Go's `a.store`). Keep the ORDER of writes the same as Go's `a.store`
  calls: the golden test compares batches in order.
- `a.storeRaw(c, intake, reason, note, body)` → `Raw.store r intake reason note body`.
- `c.JSON(code, gin.H{...})` → `Response.json code "..."` or `Response.jsonOf code {| ... |}`.
- `time.Now().UTC()` → `DateTime.UtcNow`; `uuid.New()` → `Guid.NewGuid()`.
- `isDiagnose(c)` → `Diagnose.isSweep r`; `wireTime` → `Time.wireSeconds`;
  `wireTimeMillis` → `Time.wireMillis`; `tagsToMultiMap` → `Tags.toMultiMap`;
  `kvToMap` → `Tags.toMap`; `splitTag` → `Tags.split`; `boolToUint8` → `Text.flag`;
  `lcTimePtr` → `Time.optionalSeconds`; `orchVersionSpread` → `Wire.versionSpread`;
  `metricTypeName` → `Wire.metricTypeName`; `decodeJSONList` → `Json.tryParseList`;
  `civDecodeMsgpack` → `Msgpack.decode`; `sketch.Build` → `DDSketch.build`;
  `a.countSelf` → `SelfMetrics.count`; `process.DecodeMessage` → `ProcessFrame.decode`; `bodyIsJSON` → `Body.isJson`;
  multipart bodies → `Multipart.boundary` + `Multipart.parts`.
- JSON: `System.Text.Json` (`JsonElement`, `JsonNode`). When a row stores JSON
  text, write it with `JavaScriptEncoder.UnsafeRelaxedJsonEscaping` so text
  stays readable in ClickHouse; the golden test compares JSON text by meaning
  (key order and escaping do not matter, numbers must keep their digits —
  never route a 64-bit id through a float).
- Protobuf: `Some.Message.Parser.ParseFrom(bytes)`; wrap in try/with for
  `InvalidProtocolBufferException`.

Tests → `api/tests/Intake.Tests/<X>Tests.fs` (a stub exists, already in the fsproj):

- The golden fixtures cover what the Go tests sent through HTTP. Make every
  fixture of your route sets pass.
- Go tests that call pure functions directly (table-driven converters,
  parsers) leave no fixture: port those as xunit tests against your F#
  functions. Also add a test for any route of yours that no fixture touches.
  Use `CapturingSink` and `Replay.testStore`/`Replay.testKey` from the Golden
  folder to drive a handler; look at how `Golden/Replay.fs` builds a request.
- xunit v3, plain `Assert`, test names in backticks saying what holds.

## Rules

1. **Parity, not improvement.** Same status codes, same bodies, same rows,
   same raw_payloads behaviour, same order of writes. If the Go code looks
   wrong, port it as it is and mention it in your report.
2. **Touch only your files**: your `Routers/<X>.fs`, your `Rows/<X>.fs`, your
   `<X>Tests.fs`, and `Fixtures/overrides.json` (your own entries only). Do
   NOT edit `Http.fs`, `Common.fs`, `Engine.fs`, `Routes.fs`, `Msgpack.fs`,
   `DDSketch.fs`, `ProcessFrame.fs`, `Table.fs`, `Golden/*.fs`, other tasks' routers or rows, or
   anything under `server/`. If you truly need a shared helper changed or
   added, write the helper privately in your own file and list it in your
   report so it can be hoisted after the merge. If a stub's signature in your
   router must change, say so in the report.
   You may add a new source file of your own (e.g. a private helper module)
   and its line in the `.fsproj`, next to your router's line.
3. **Style — the owner's explicit wishes:**
   - Plain, explicit F#. No custom operators, no home-made DSLs or
     computation expressions, no clever point-free chains. `match`, `if`,
     loops, small named functions. Longer and obvious beats short and magic.
   - FEW comments. One to three lines where the WHY is not obvious from the
     code — and the Go comments are full of such whys about the agent's
     protocol: keep that knowledge, condensed. Do not narrate what the code
     does, do not copy the Go comments wholesale, no banner comments.
   - English everywhere.
   - `TreatWarningsAsErrors` is on: zero warnings.
4. **Logging**: `r.Log` (`ILogger`). Keep a warning where Go logged a decode
   problem or something unhandled. The chatty per-request summaries
   (`log.Printf("[x] host=… %d series…")`) and the helpers that only format
   them (`describe`, `apiKV`, `apiTally`, …) are NOT ported, unless a test
   asserts on them.
5. **overrides.json** is for differences that cannot be reproduced: the text
   of a Go library's error message quoted in a note or a response. Each entry
   needs a `why`. Never use it to hide a difference in behaviour. Note that
   paths under `/sends/N/args/...` mirror `/sends/N/rows/...` and both need
   an entry.
6. No new NuGet packages unless unavoidable; if you add one, pin it in
   `api/Directory.Packages.props` and say so in the report.
7. Environment variables Go reads with `os.Getenv` in your routers: read the
   same variable the same way (`Environment.GetEnvironmentVariable`).

## How to build and test

```bash
cd api/tests/Intake.Tests
NINJACAT_GOLDEN_ROUTES=routeX,routeY dotnet test      # only your route sets' fixtures (+ all non-golden tests)
dotnet test                                           # everything; other tasks' fixtures fail until merged — ignore those
```

A golden failure prints the fixture id and each difference as a JSON path:
`/sends/0/rows/0/Host: Go "h1", F# ""`. The fixture itself is
`api/tests/Intake.Tests/Fixtures/go/<TestName>/<n>.json`; `routes` in it says
which route sets it belongs to. The Go test of the same name shows the intent.

`dotnet build` from `api/` must succeed with 0 warnings and 0 errors, and the
existing tests in `api/tests/Engine.Tests` must still pass.

## When you are done

1. `git add` your files and commit in your worktree:
   `feat(api): port the <name> intake to F#` (end the message with the line
   `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`). Do not push.
2. Report, as your final message (it is read by the coordinator, not by the owner):
   - the branch/commit you made;
   - golden fixtures of your route sets: how many pass / how many there are,
     and the exact reason for each one that does not;
   - tests you added (count, what they cover) and which Go tests you did not
     port and why;
   - every `overrides.json` entry you added, with its reason;
   - every route you ported (method + path), and any you did not;
   - shared-code changes you needed (helpers you wrote privately that belong
     in Common, signature changes), with file and function names;
   - anything in the Go code that looked like a bug, and anything you are
     unsure about. Be precise about what is verified and what is assumed.
