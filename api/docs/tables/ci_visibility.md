# CI Visibility — `ci_test_events`, `ci_coverage`, `git_commits`, `git_packfiles`, `ci_settings_requests`, `ci_webhook_events`, `ci_pipeline_events`

Feeds: three new intake hosts plus ten routes on `api.<site>`.

| host | route | handler | table |
|---|---|---|---|
| `citestcycle-intake.<site>` | `POST /api/v2/citestcycle` | `HandleCITestCycle` | `ci_test_events` |
| `citestcov-intake.<site>` | `POST /api/v2/citestcov` | `HandleCITestCov` | `ci_coverage` |
| `webhook-intake.<site>` | `POST /api/v2/webhook`, `/api/v2/webhook/` | `HandleCIWebhook` | `ci_webhook_events` |
| `api.<site>` | `POST /api/v2/libraries/tests/services/setting` | `HandleCISettings` | `ci_settings_requests` |
| `api.<site>` | `POST /api/v2/ci/tests/skippable` | `HandleCISkippableTests` | `ci_settings_requests` |
| `api.<site>` | `POST /api/v2/ci/libraries/tests` | `HandleCIKnownTests` | `ci_settings_requests` |
| `api.<site>` | `POST /api/v2/ci/libraries/tests/flaky` | `HandleCIFlakyTests` | `ci_settings_requests` |
| `api.<site>` | `POST /api/v2/test/libraries/test-management/tests` | `HandleCITestManagement` | `ci_settings_requests` |
| `api.<site>` | `POST /api/v2/git/repository/search_commits` | `HandleGitSearchCommits` | `git_commits` |
| `api.<site>` | `POST /api/v2/git/repository/packfile` | `HandleGitPackfile` | `git_packfiles` + `git_commits` |
| `api.<site>` | `POST /api/v2/ci/pipeline/tags` | `HandleCIPipelineTags` | `ci_pipeline_events` |
| `api.<site>` | `POST /api/v2/ci/pipeline/metrics` | `HandleCIPipelineMetrics` | `ci_pipeline_events` |
| `api.<site>` | `POST /api/intake/ci/custom_spans` | `HandleCICustomSpans` | `ci_pipeline_events` |

`schema/migrations/0017_ci_visibility.sql` creates the tables,
`apps/storage/rows_ci.go` is the writer, `intake/router_civisibility.go` and
`intake/router_ciwebhook.go` are the handlers.

## What makes this different from every other intake

**Most of these endpoints are reads.** Everywhere else in this server the
agent tells us something and we answer 202. Here a tracer *asks* — is coverage
on, which tests may I skip, which commits do you already have — and then
behaves according to the answer. A wrong answer somewhere else costs a stored
row; a wrong answer here changes what a customer's test suite does. That is
why every response struct in `router_civisibility.go` mirrors the field names
of the client struct that parses it, with the source file named above it, and
why the safe answer is uniformly "feature off, list empty".

**Two transports, one set of paths.** Each endpoint reaches a real Datadog
either *agentless* (the tracer talks to `<subdomain>.<site>` directly with
`dd-api-key`) or *tunnelled* through the local agent's EVP proxy (the tracer
talks to `<agent>/evp_proxy/v2/<path>` and `X-Datadog-EVP-Subdomain` tells the
agent which host to forward to — `req.URL.Host = subdomain + "." + endpoint.Host`,
`pkg/trace/api/evp_proxy.go:189`). We serve the agentless spelling, which is
the one that arrives by hostname. The four `evp_*` columns on `ci_test_events`
and `ci_coverage` say which path a payload took.

Three header names in that hop are not the obvious ones, all from
`datadog-agent/pkg/trace/api/evp_proxy.go` and
`pkg/trace/api/internal/header/headers.go:19`:

- the container id header is **`Datadog-Container-ID`**, with no `X-` prefix.
  `X-Datadog-Container-Id` does not exist in the agent at all.
- there is **no `X-Datadog-AgentVersion`**. The proxy announces itself in
  `Via: trace-agent <version>`, which is where `agent_version` comes from.
- `X-Datadog-EVP-Subdomain` is **consumed** by the proxy and not forwarded, so
  a non-empty `evp_subdomain` means something other than a datadog-agent sent
  the request.

## Sources

Every field was read from a client that sends it, at `main` on 2026-09-23:

- `DataDog/dd-trace-go` — `ddtrace/tracer/civisibility_tslv.go` (the event
  envelope and `tslvSpan`), `internal/civisibility/utils/net/{client,
  settings_api,skippable,known_tests_api,test_management_tests_api,
  searchcommits_api,sendpackfiles_api,coverage}.go`
- `DataDog/dd-trace-py` — `ddtrace/internal/ci_visibility/{encoder,writer,
  git_client,_api_client}.py`, `ddtrace/internal/evp_proxy/constants.py`
- `DataDog/datadog-agent` — `pkg/trace/api/evp_proxy.go`
- `jenkinsci/datadog-plugin` — `DatadogApiClient.java`, `BatchSender.java`,
  `DatadogWebhookBuildLogic.java`, `DatadogWebhookPipelineLogic.java`
- `DataDog/datadog-ci` — `packages/base/src/commands/{tag,measure,trace,
  git-metadata}/…`, `packages/base/src/helpers/request/datadog-route.ts`

**`DataDog/datadog-ci-spec` — the repo dd-trace-go's own comment calls "the"
spec — is not publicly reachable (404).** So the authoritative backend-side
field catalogue does not exist for us. "Complete" below means complete with
respect to what the shipped clients write, which is why every table keeps a
raw column.

---

## `ci_test_events` — the test results

One row per **event**, not per payload. A payload is an envelope

```
{ "version": 1,
  "metadata": { "<event type>": {"<k>": "<v>"}, "*": {...} },
  "events": [ {"type": ..., "version": ..., "content": {...}}, ... ] }
```

and each event is one of five shapes, documented verbatim in
`civisibility_tslv.go:47-148`:

| `type` | `version` | carries |
|---|---|---|
| `test` | **2** | session + module + suite ids, `trace_id`/`span_id`/`parent_id`, `itr_correlation_id` |
| `test_suite_end` | 1 | session + module + suite ids |
| `test_module_end` | 1 | session + module ids |
| `test_session_end` | 1 | session id |
| `span` | 1 | `trace_id`/`span_id`/`parent_id` |

The version split is `civisibility_tslv.go:298` against `:323`/`:348`/`:371`;
dd-trace-py agrees (`TEST_EVENT_VERSION = 2`, `TEST_SUITE_EVENT_VERSION = 1`).
The session/module/suite ids are **promoted out of `meta`** into top-level
`content` fields by the tracer (`getAndRemoveMetaToUInt64`; dd-trace-py's
`_filter_ids` makes the identical move), and the `*_end` types zero their
span/trace ids, which `msgp`'s `omitempty` then omits.

### Wire format

`Content-Type: application/msgpack`, gzip'd **only in agentless mode**
(`civisibility_transport.go:154-178`; the EVP path is uncompressed because the
agent handles it). JSON is accepted too — dd-trace-go's Bazel mode converts one
to the other, and a hand-rolled client or a replayed capture sends JSON. The
handler decides by `Content-Type` and falls back to sniffing the first byte.

The encoder bisects a payload at **5 MB** rather than sending one oversized
POST (`encoder.py:67,118-180`), and truncates string `meta` values at **5000
characters** (`encoder.py:68`). Neither is enforced here; both are why a
payload arrives split.

### The decoder's one hard rule

`test_session_id` is a full 64-bit value. It is read through msgpack's integer
types or `json.Number`, **never through `float64`** — a round trip through a
float turns `9007199254740993` into `...992` and the row then joins to nothing,
silently. `TestCITestCycleKeepsFullWidthIDs` pins it for both formats and
`TestCITestEventRoundTrip` pins it again through the column.

### Columns

| column | source |
|---|---|
| `event_type`, `event_version` | the outer event |
| `payload_version` | the envelope's `version` |
| `session_id`, `module_id`, `suite_id` | `content.test_*_id`, `UInt64`. `0` means "this event type does not carry it" — structural absence, so not `Nullable` |
| `trace_id`, `span_id`, `parent_id` | `content.*`, `UInt64` |
| `itr_correlation_id` | echoed back from a skippable-tests response |
| `service`, `name`, `resource`, `span_type` | `content.*` |
| `env` | `meta["env"]`, falling back to the envelope's `*` metadata |
| `start` | `content.start`, ns since epoch → **`DateTime64(9)`**. Nanoseconds are not decoration: two tests in the same millisecond are otherwise unorderable |
| `duration_ns`, `error` | `content.*`. `error = 0` means no error |
| `test_*` | lifted from `meta` — `test.name`, `test.suite`, `test.module`, `test.framework`, `test.framework_version`, `test.status`, `test.type`, `test.source.file`, `test.parameters`, `test.codeowners`, `test.command`, `test_session.name` |
| `test_source_start`, `test_source_end` | `Nullable(Int64)`, read from **`metrics` first, then `meta`** — a numeric `SetTag` in dd-trace-go routes to `metrics`, and a decoder reading only `meta` leaves both NULL |
| `git_*` | `git.repository_url`, `git.branch`, `git.tag`, `git.commit.sha`, `git.commit.message`, `git.commit.{author,committer}.{name,email,date}` |
| `ci_*` | `ci.provider.name`, `ci.pipeline.{id,name,number,url}`, `ci.job.{id,name,url}`, `ci.stage.name`, `ci.workspace_path`, `ci.node.{name,labels}` |
| `os_*`, `runtime_*` | `os.platform`, `os.version`, `os.architecture`, `runtime.name`, `runtime.version` |
| `language`, `runtime_id`, `library_version` | the **envelope metadata**, with the span's own tag winning if it set one |
| `test_is_new`, `test_is_retry`, `test_is_modified`, `test_skipped_by_itr`, `itr_unskippable`, `itr_forced_run`, `code_coverage_enabled` | `Nullable(UInt8)`. **Absent is not false**: a tracer with the feature off writes no tag at all, and a report counting retried tests has to tell "nobody was looking" from "no" |
| `test_retry_reason`, `early_flake_abort_reason` | `test.retry_reason` (`attempt_to_fix`/`early_flake_detection`/`auto_test_retry`/`external`), `test.early_flake.abort_reason` |
| `evp_subdomain`, `container_id`, `agent_hostname`, `agent_version` | the EVP-proxy hop, see above |
| `meta`, `metrics` | **every** key, verbatim. The hot columns above are copies for query speed, not extractions — a tracer that renames a tag loses a column and keeps the data |
| `metadata` | the envelope's entry for this event type, with `*` merged underneath it and the per-type entry winning |
| `content` | the event's `content`, rendered as **JSON** whatever the request's format was — the tracer's own doc comment documents the JSON shape, so that is what a reader will recognise |

`meta` is `Map(k, String)`, **not** the tag multiset shape: the wire type is
`map[string]string`, so a duplicate key is not representable and the multiset
would cost lookup speed for a collision that cannot happen.

`ORDER BY (tenant_id, service, start)`, monthly partitions, **90 day TTL**,
bloom filters on `session_id` and `test_name` — the two lookups that do not
start from the ORDER BY ("everything in this CI run", "is this test flaky").

**Known gap:** a `content.start` of 0 produces a row dated 1970 that the TTL
drops at once. That is deliberate: `evpParseTime`'s rule applies here too —
never invent a timestamp nobody sent.

---

## `ci_coverage` — per-test line coverage

`multipart/form-data` with two parts:

1. a dummy JSON `event` part, literally `{"dummy": true}` in both clients;
2. the coverage payload, msgpack or JSON.

**The coverage part's form name differs between the two shipped clients** —
`coveragex` in dd-trace-go (`coverage.go:80`) and `coverage1` in dd-trace-py
(`encoder.py:312`). Both ship. So the decoder finds it by *not* being `event`,
and reads its format from the part's own `Content-Type` (sniffing the first
byte when the part has none). `TestCITestCovFindsCoveragePartByContentType`
covers all four combinations.

Payload shape (`encoder.py:337-380`):

```json
{"version": 2, "coverages": [
  {"test_session_id": 123, "test_suite_id": 456, "span_id": 789,
   "files": [{"filename": "a.go", "bitmap": "<raw bytes>"}, {"filename": "b.go"}]}]}
```

- **`span_id` is omitted** when `itr_suite_skipping_mode` is on
  (`encoder.py:375`) — suite-level coverage has no per-test span. Hence
  `Nullable(UInt64)`: `0` is a legal span id.
- `files[].bitmap` is present only when line-level coverage is available. A
  filename-only entry is valid.

`files_filename` and `files_bitmap` are **parallel arrays that always have the
same length**: a file with no bitmap keeps an empty string at its index rather
than shifting the two out of step.

**The bitmap is never decoded.** dd-trace-go states the contract outright
(`skippable.go:235`): *"Go coverage metadata is stored and returned by the
backend as FileBitmap bytes; the backend does not translate bitmap
encodings."* Our job is to keep bytes we can hand back verbatim on a
skippable-tests response.

`raw` holds **this entry's** bytes, delimited out of the part without
re-encoding (msgpack via `msgp.Skip` on the byte slice, JSON via
`json.RawMessage`), and `raw_format` says which. Per entry, not per request:
a payload carries one entry per test, and storing the whole part on every row
would cost the square of the payload size.

---

## `git_commits` and `git_packfiles`

### `POST /api/v2/git/repository/search_commits`

Request and response are the **same envelope**
(`searchcommits_api.go:22-33` and `:84-93`):

```json
{"data": [{"id": "<sha>", "type": "commit"}], "meta": {"repository_url": "..."}}
```

The client reads only `data[].id`; dd-trace-py additionally filters on
`type == "commit"`. It subtracts what we return from its local commit list to
decide whether to unshallow and upload packs.

**We answer an empty list, and store the candidates.** The honest answer to
"which of these do you have" is a *read* against ClickHouse, and the intake has
no read path by design — reads live in the query app so a slow query can never
stall ingest. Answering empty makes the client upload its packs, which is
where `git_packfiles` gets filled from, so it is also the behaviour we want
today. When the query app grows a lookup, `HandleGitSearchCommits` is the one
handler to change and the rows it already writes are the index it will read.

The client sets `ExpectJSONResponse` on this request, so the response **must**
carry a JSON content type: a 200 with anything else is treated as transient and
retried (`http.go:375-385`).

`git_commits` is a `ReplacingMergeTree(seen_at)` keyed on
`(tenant_id, repository_url, sha)`. A row means *"this sha was mentioned for
this repository"* — **not** that we hold the objects. `packfile_id` is NULL for
a `search_commits` sighting and set for a `packfile` one, which is the only
path where we do. Readers need `FINAL` or `argMax`. **No TTL**: a commit is an
identity, like a host, and an answer that expires becomes a wrong answer rather
than a missing one.

### `POST /api/v2/git/repository/packfile`

`multipart/form-data`, one request per `.pack` file
(`sendpackfiles_api.go:54-89`, byte-identical in `git_client.py:103-131`):

| field | content type | body |
|---|---|---|
| `pushedSha` | `application/json` | `{"data":{"id":"<sha>","type":"commit"},"meta":{"repository_url":"..."}}` |
| `packfile` | `application/octet-stream` | raw `git pack-objects` output, filename = the `.pack` basename |

**The answer is `204 No Content` with an empty body.** dd-trace-go accepts any
2xx (`sendpackfiles_api.go:107`) but dd-trace-py checks for exactly 204
(`git_client.py:430,458`), so 204 is the only status both call success.

`packfile_id` is sha256 of the pack bytes, hex — the upload carries no id of
its own and `git_commits.packfile_id` has to point at something. The handler
also writes a `git_commits` row for the pushed sha *with* that id, which
`ReplacingMergeTree` lets overwrite the NULL a `search_commits` sighting left.

---

## `ci_settings_requests` — the questions, and our answers

Five endpoints, one table. These are reads from the tracer's point of view, so
there is no telemetry to keep — what there is instead is the question and our
answer. Worth a table for one reason: the day any of these features is
implemented, "which tracers asked, for which repo, at which granularity, and
what did we tell them" is the entire migration plan. It also makes a tracer
stuck in a retry loop visible.

### Settings — `POST /api/v2/libraries/tests/services/setting`

Request (`settings_api.go:26-43`):

```json
{"data":{"id":"<uuid>","type":"ci_app_test_service_libraries_settings",
  "attributes":{"service","env","repository_url","branch","sha",
    "configurations":{"os.platform","os.version","os.architecture",
      "runtime.name","runtime.architecture","runtime.version","custom":{}}}}}
```

**Every flag in the response is `false`, and that is a feature, not a stub.**
Each of the other four endpoints is individually gated by one of these flags in
the tracer, so an all-false answer makes a real tracer behave exactly as it
does against a backend with no Test Optimization: it still posts its results to
`/api/v2/citestcycle` (unconditional — that is the data, not a feature) and
still uploads git metadata when configured to, but never asks for skippable
tests, known tests or test-management state. Turning one true is what a future
release does when the feature behind it exists; doing it earlier makes a
customer's suite skip tests on the strength of data we do not have.

The shape is confirmed by the client itself: dd-trace-go returns a zero-value
`SettingsResponseData` when its Bazel cache is unavailable
(`settings_api.go:88`), field for field what we send:

```json
{"data":{"id":"<echoed>","type":"ci_app_test_service_libraries_settings","attributes":{
  "code_coverage": false, "coverage_report_upload_enabled": false,
  "early_flake_detection": {"enabled": false,
    "slow_test_retries": {"5s":0,"10s":0,"30s":0,"5m":0},
    "faulty_session_threshold": null},
  "flaky_test_retries_enabled": false, "itr_enabled": false,
  "require_git": false, "tests_skipping": false,
  "known_tests_enabled": false, "impacted_tests_enabled": false,
  "test_management": {"enabled": false, "attempt_to_fix_retries": 0}}}}
```

`faulty_session_threshold` is `*int` in the client, so `null` is a legal value
and means "no threshold". The four `slow_test_retries` keys are literally
`"5s"`, `"10s"`, `"30s"`, `"5m"`.

### Skippable tests — `POST /api/v2/ci/tests/skippable`

Answered `{"meta":{"correlation_id":"","coverage":{}},"data":[]}` — nothing is
skipped. `meta` is **sent, not omitted**: dd-trace-py treats a missing `meta` as
a malformed response (`_api_client.py:548-552`).

For whoever implements this: `meta.coverage` is `{"<repo-relative path>":
"<base64 FileBitmap bytes>"}` and the backend never decodes it — the bytes come
from a `/api/v2/citestcov` upload and go back out unchanged.
`ci_coverage.files_bitmap` is where they already are.

### Known tests — `POST /api/v2/ci/libraries/tests`

Answered with an empty `tests` map and
`page_info: {"cursor":"","size":0,"has_next":false}`.

**`has_next: false` is the one field in this whole file where the wrong answer
is worse than useless.** The client loop (`known_tests_api.go:114-165`) keeps
POSTing with the previous cursor for as long as `has_next` is true, and
dd-trace-go has no iteration cap; dd-trace-py caps at 10 000 pages. A response
that set it true without changing the cursor would spin a customer's test
process forever.

### Flaky tests — `POST /api/v2/ci/libraries/tests/flaky`

**UNVERIFIED.** Unlike every other route here, this path was not read from a
shipped client — it appears in our own task spec and in older dd-trace-py
releases, and no current source we could reach names it. Implemented anyway,
answering the same empty known-tests envelope, because an empty list is
harmless whatever the exact shape turns out to be and a 404 in the middle of a
test run is not. The request body reaches `ci_settings_requests` regardless,
which is how we will learn the real shape the first time a tracer calls it.

### Test management — `POST /api/v2/test/libraries/test-management/tests`

Request carries `repository_url`, `sha`, `module`, `commit_message`, `branch`
(`test_management_tests_api.go:36-42`). Answered
`{"data":{"id":"<echoed>","type":"ci_app_libraries_tests","attributes":{"modules":{}}}}` —
nothing quarantined, disabled or being attempted-to-fix. The real shape nests
`modules → suites → tests → properties{quarantined, disabled, attempt_to_fix}`.

### Retry behaviour we should respect

From `http.go:200-397` (Go) and `_api_client.py:290-333` (Python), for all
five endpoints — **not** for `/api/v2/citestcycle`, which has no retry at all:

- `2xx` success; `403` terminal in both (bad key, do not retry)
- `429` → Go reads `x-ratelimit-reset` and sleeps that long
- `>= 500` retried by both; Go also retries `>= 406`
- `400`–`405` terminal
- Go: `100ms * 2^attempt` capped at 10s, 4 attempts. Python: Fibonacci with
  jitter, 5 attempts.

So an overloaded ninjacat should prefer `503`/`429` over `500` to land on the
friendlier path, and must never answer a 4xx it does not mean.

---

## `ci_webhook_events` — CI provider webhooks

`webhook-intake.<site>/api/v2/webhook/` is Datadog's one *generic* CI-provider
webhook intake — the Jenkins plugin posts here and Datadog's GitLab
documentation gives the identical URL for GitLab's legacy webhook integration.

The Jenkins plugin (`DatadogApiClient.java:298-304`, `BatchSender.java:44-88`):

- URL `webhook-intake.<site>/api/v2/webhook/?service=<ci instance name>`
- headers `DD-API-KEY`, `DD-CI-PROVIDER-NAME: jenkins`,
  `Content-Encoding: gzip`, `Content-Type: application/json`
- body a **JSON array** batched to 5 MB uncompressed, gzip on the wire
  (unwrapped by the `Decompress` middleware before the handler runs)

**One batch mixes levels.** Each element carries `"level": "pipeline" |
"stage" | "job"` and the backend tells them apart by that field, not by URL or
header — which is why one table with a `level` column is the shape the producer
already assumes, the same argument `0001_initial.sql` makes for
`k8s_resources`.

Both path spellings are registered rather than relying on Gin's trailing-slash
redirect: the plugin's URL ends in a slash and a 301 on a POST drops the body,
which here is a CI run that never appears.

Pipeline-level fields (`DatadogWebhookBuildLogic.java:59-206`): `payload_version`,
`url`, `start`/`end` (ISO8601, `end` omitted while running), `partial_retry`,
`queue_time` (ms), `status`, `is_manual`, `trace_id`, `span_id`, `pipeline_id`,
`unique_id`, `name`, `user{name,email}`, `parameters{}`, `tags[]`,
`node{name,hostname,workspace,labels[]}`, `git{…}`,
`parent_pipeline{trace_id,url}`.

Stage/job-level (`DatadogWebhookPipelineLogic.java:37-192`) adds
`parent_span_id`, `id`, `pipeline_unique_id`, `pipeline_name`,
`parent_stage_id` (nested stages), `stage_id`/`stage_name` (jobs) and
`error{message,type,domain,stack}`.

### Two decisions worth stating

**The ids are `String`, not `UInt64`**, even though Jenkins sends `trace_id`
and `span_id` as decimals: the same fields carry UUIDs from other providers,
and a numeric column would turn those into `0` without anybody noticing.
`civString` renders a `json.Number` by its exact literal, so a numeric id keeps
every digit.

**`tags` is a genuine multiset.** The wire form is an array of `"key:value"`
strings and the plugin repeats keys for per-configuration axes, so the column
is `Map(k, Array(k))` and the values go through `tagsToMultiMap` — see
`docs/decisions/0001-tags-are-a-multiset.md` for what a plain map costs.
`parameters`, by contrast, is a JSON object and stays a plain map.

### GitLab — UNVERIFIED

Datadog's docs give the URL and say the secret token is left empty, but
GitLab's own emitter lives in `gitlab-org/gitlab` and was not read. So whether
GitLab sends Datadog's level-shaped schema or its own native webhook JSON
(`object_kind`, `project`, `builds`) is not confirmed, and neither is which
headers it sets. The decoder reads **both**: it looks for `level` and falls
back to `object_kind`, sniffs `gitlab` from `object_kind` when no
`DD-CI-PROVIDER-NAME` header is set, keeps whatever hot fields match, and puts
everything it does not name into `extra` — with the element stored verbatim in
`body` regardless. `provider` is `"unknown"` when nothing says otherwise;
never a guess that produces a real provider name.

One part of the GitLab path **is** verified, and it shapes the route's
authentication: the URL Datadog documents carries the key in the query string,
`webhook-intake.<site>/api/v2/webhook/?dd-api-key=<key>`, because GitLab's
generic Project Webhook UI has no field for an arbitrary header. So this host
does not take the default `Dd-Api-Key` / `api_key` pair — `routes.go` builds it
with `RequireAPIKeyFrom` and adds the `dd-api-key` query parameter, the same
spelling the browser RUM SDK uses. Without it a correctly configured GitLab
project is answered 403 by the middleware, before the handler or `storeRaw`
runs, and every pipeline it reports is lost with no log line anywhere. The
header stays first in the source list so a Jenkins request never parses a query
string, and the exception stays at this one host: a key in a URL lands in every
proxy access log, which is why `RequireAPIKey` does not offer it by default.

`headers` is an allowlist, the same rule `intake/raw.go` states: a 90-day table
must never become a place a credential is durable. `DD-API-KEY` and
`Authorization` are absent from it, and a test asserts that.

---

## `ci_pipeline_events` — the `datadog-ci` CLI

Three routes, one table (`packages/base/src/helpers/request/datadog-route.ts`
is the CLI's complete route allowlist, so this is the full inventory of its CI
write endpoints):

| CLI command | route | `data.type` |
|---|---|---|
| `datadog-ci tag` | `POST /api/v2/ci/pipeline/tags` | `ci_custom_tag` |
| `datadog-ci measure` | `POST /api/v2/ci/pipeline/metrics` | `ci_custom_metric` |
| `datadog-ci trace` | `POST /api/intake/ci/custom_spans` | `ci_app_custom_span` |

All three are `{"data": {"type", "attributes"}}` with `DD-API-KEY` (plus
`DD-APPLICATION-KEY` when configured). `ci_level` is a number — 1 pipeline,
2 stage, 3 job (`LEVEL_TO_NUMBER`) — and `Nullable`, because a custom span
sends none. `ci_env` is the provider's raw environment-variable snapshot and is
**the join key** back to whatever pipeline a webhook or a tracer already
reported, which is why it is a real `Map` and not just part of `body`.

`tags` here is a JSON **object**, so keys are unique and the column is a plain
map — not the `"key:value"` list that makes a multiset. `measure` calls its map
`metrics`; a custom span may call the same idea `measures`, and both land in
the `metrics` column.

**UNVERIFIED:** the custom-span `Payload` type was not readable from a primary
source, so `span_name`, `span_start_raw` and `span_end_raw` are filled from the
obvious key names (`name`, `start_time`, `end_time`) and left empty otherwise.
`attributes` holds the truth either way, and any key this decoder does not name
is kept with its value in `extra`.

The CLI retries five times with 5–30s backoff on a thrown error and surfaces a
hard failure to the CI job, so these answer `202` — a `5xx` here stalls
somebody's build for half a minute.

---

## Not implemented

Found while reading the clients, in scope for a complete CI Visibility but not
built here:

- **`POST /api/v2/logs` on `http-intake.logs.<site>`** as CI Visibility's own
  log channel (`DD_CIVISIBILITY_LOGS_ENABLED`, `logs_api.go:15-35`). Same wire
  shape as the regular logs intake, which `router_logs.go` already serves, but
  always gzip'd — the only endpoint in this protocol that forces compression
  regardless of transport. It would land in `logs`, not here.
- **`POST /api/v2/cicovreprt` on `ci-intake.<site>`** — whole coverage-report
  upload (lcov and friends), a *third* coverage subdomain distinct from both
  `citestcov-intake` and `citestcycle-intake` (`dd-trace-py constants.py:42,49`).
  Gated by `coverage_report_upload_enabled`, which we report false.
- **`POST /api/v2/cireport` on `cireport-intake.<site>`** — `datadog-ci junit
  upload` (`packages/plugin-junit/src/api.ts:14-15`), multipart with a gzip'd
  JUnit XML part.
- **A generic `evp_proxy` of our own.** If a datadog-agent is ever put *in
  front of* ninjacat, every endpoint here also arrives as
  `/evp_proxy/v2/<path>` on whatever host that agent's `dd_url` points at.
  That is a reverse proxy keyed off a header, architecturally unlike every
  other router in `intake/`.
- **Reading back what we stored.** Nothing in `panelapi` or `query` serves any
  of these tables yet, and `HandleGitSearchCommits` answering empty is the one
  place where that gap is visible to a client rather than to a person.
