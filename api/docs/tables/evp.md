# EVP misc tables

Storage for the six single-purpose "event platform" hosts that had no table
before this file: `agentdiscovery-intake`, `agenthealth-intake`,
`event-management-intake`, `softinv-intake`, `http-synthetics` and
`data-obs-intake` (seven routes — data-obs carries two). Handlers live in
`server/intake/router_evp.go`; the schema is
`server/schema/migrations/0010_evp.sql`; row types and writers are
`server/apps/storage/rows_evp.go`.

None of the seven routes share a wire shape, so each table follows the
payload it is named for rather than a common pattern. Two things repeat
throughout, though:

- **String + parsed Nullable, everywhere a timestamp arrives as a string.**
  `EmittedAt`, `DetectedAt`, `testStartedAt`, `eventTime`, and friends are all
  producer-defined strings, unvalidated by the agent. Each gets two columns:
  the string exactly as sent, and a `Nullable(DateTime64)` holding the parsed
  value when it happens to be RFC3339 — `NULL` otherwise, never a fallback to
  "now", which would invent a timestamp nobody sent. `received_at` (arrival
  time, always present) is what `PARTITION BY` and the TTL key off instead —
  the one time value every row is guaranteed to have.
- **JSON-text columns for genuinely schemaless sub-objects** — a `structpb.Struct`,
  OpenLineage facets, a Python integration's whole result, an undeclared key's
  value. These hold the sub-object's JSON verbatim rather than being flattened
  into columns that would need a schema change every time a producer adds a
  field.

Decoding uses `json.Decoder.UseNumber()` (see `evpObject` in router_evp.go),
so a large id or counter that arrives as a JSON number is preserved exactly —
`json.Number` round-trips through `json.Marshal` byte-for-byte, unlike
`float64`, which loses precision above 2^53.

Undecodable input (a body that fails to decode, an array element that fails
`evpObject`, or a shape this track cannot use — e.g. `event_management_events`
without `data.attributes`) goes to `raw_payloads` via `a.storeRaw`, never
silently dropped. Decode success paths never call `storeRaw`.

## Tables

### agent_discovery

`ENGINE = MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, host_id, integration, runtime, received_at)`,
`TTL received_at + 30 days`.

Fed by `POST /api/v2/agentdiscovery` (protobuf, one `AgentDiscoveryPayloadBatch`
per request). One row per `AgentDiscoveryPayload`; `host_id` is the batch's
own identity and repeats onto every row.

| Column | Source |
|---|---|
| `host_id` | `AgentDiscoveryPayloadBatch.HostId` |
| `integration`, `runtime`, `runtime_id` | `AgentDiscoveryPayload.{Integration,Runtime,RuntimeId}` |
| `ingestion_timestamp` | `AgentDiscoveryPayload.IngestionTimestamp` (ptr-optional; `NULL` when absent, never epoch) |
| `config_paths`, `config_contents`, `config_truncated`, `config_formats` | `ConfigFiles[]`, four parallel arrays, index-aligned. `config_formats[i]` is `PayloadFormat.String()` (e.g. `PAYLOAD_FORMAT_YAML`) |
| `env_var_names`, `env_var_values` | `EnvVars[]`, two parallel arrays, index-aligned |

**Sensitive columns: `env_var_names`/`env_var_values` keep VALUES, not just
names.** The handler's own log line redacts them (names only) because they
are credential-shaped — API keys, database passwords an integration config
embeds — but the row is the reason this endpoint is captured at all: an
operator debugging "why is this integration not picking up config X" needs
the actual value. This is a deliberate product decision, not an oversight; an
operator who wants a shorter retention on this specific exposure should
shorten this table's TTL independently of the others.

**Two parallel arrays, not a `Map`:** `EnvVars` is a *repeated* field on the
wire, not a map, so a real agent can send the same name twice (a raw environ
block is not itself deduplicated). A `Map(String,String)` would silently
collapse a duplicate name to whichever value proto iterated over last and
lose the original order entirely; the arrays preserve both, the same way
`config_paths`/`config_contents` above do for `ConfigFiles`.

**Not recoverable:** `proto.Unmarshal` silently drops fields the linked
`agent-payload` version does not declare — there is no unknown-field sink at
the proto layer the way there is for JSON, so a newer agent sending a field
this schema predates never reaches this table, with no signal that it happened.

### agent_health_reports / agent_health_issues

`agent_health_reports`: `MergeTree`, `PARTITION BY toYYYYMM(received_at)`,
`ORDER BY (tenant_id, host, received_at)`, `TTL received_at + 90 days`.

`agent_health_issues`: `MergeTree`, `PARTITION BY toYYYYMM(received_at)`,
`ORDER BY (tenant_id, report_id, issue_key)`, `TTL received_at + 90 days`.

Fed by `POST /api/v2/agenthealth` (JSON — the agent marshals its own proto via
`encoding/json`, one `HealthReport` per request). Split into one report row
and one row per entry of the report's `Issues` map.

**Upsert key / join key:** `report_id` is a `UUID` **generated at ingest**
(`uuid.New()` in the handler) — the wire has no report-level id, only a map of
per-issue ids — and is what joins the two tables. Neither table is a
`ReplacingMergeTree`; both are append-only history, one row set per report
received.

**Issue ordering:** Go's map iteration order over `Issues` is random, so
`healthReportRows` sorts issue keys before building rows. Two ingests of "the
same" report (e.g. a retried request) therefore produce rows in the same
order — a diff between the two is meaningful instead of noise.

| Column (reports) | Source |
|---|---|
| `schema_version`, `event_type`, `service` | `HealthReport.{SchemaVersion,EventType,Service}` |
| `emitted_at` / `emitted_at_parsed` | `HealthReport.EmittedAt`, parsed when RFC3339 |
| `host` | `HealthReport.Host.Hostname` |
| `agent_version` | `HealthReport.Host.AgentVersion` (ptr-optional) |
| `par_ids` | `HealthReport.Host.ParIds` |
| `issue_count` | `len(HealthReport.Issues)` |

| Column (issues) | Source |
|---|---|
| `issue_key` | the `Issues` map key (not always equal to `id` below, though producers usually set them the same) |
| `id`, `issue_name`, `title`, `description`, `category`, `location`, `source`, `issue_type` | `Issue.*` |
| `severity` | `Issue.Severity.String()` (e.g. `ISSUE_SEVERITY_HIGH`) |
| `detected_at` / `detected_at_parsed` | `Issue.DetectedAt`, parsed when RFC3339 |
| `extra` | `Issue.Extra` (`structpb.Struct`), rendered as JSON — genuinely schemaless per issue. Rendered from a **parallel, `UseNumber`-decoded read of the same body**, not from the `structpb.Struct` itself: `structpb.Value`'s own generated type stores every JSON number as `float64`, which would silently round a large probe id or counter before this table ever saw it. See `evpHealthIssuesRaw`/`evpIssueExtra` in `router_evp.go`. |
| `remediation_summary`, `remediation_step_order`, `remediation_step_text`, `script_*` | `Issue.Remediation.*`; steps are two parallel arrays. `script_requires_root` is `Nullable(UInt8)`: `NULL` means "no script was reported at all", distinct from a script that explicitly reports `requires_root=false` |
| `tags` | `Issue.Tags`, a multiset — `tagsToMultiMap` |
| `persisted_state`, `first_seen`, `last_seen`, `resolved_at`, `issue_type` | `Issue.PersistedIssue.*` |

`persisted_state`/`first_seen`/`last_seen` are `""` (not a state name) when
the issue carries no `PersistedIssue` at all — distinct from
`ISSUE_STATE_UNSPECIFIED`, which would claim a state was reported.
`resolved_at` is `Nullable(String)`: `NULL` means "not resolved", never an
empty string standing in for "resolved, but we don't know when".

### event_management_events

`MergeTree`, `PARTITION BY toYYYYMM(received_at)`,
`ORDER BY (tenant_id, received_at)`, `TTL received_at + 90 days`.

Fed by `POST /api/v2/events` (JSON, one JSON:API-shaped envelope per request,
no Go type — each producer builds its own `map[string]any`).

| Column | Source |
|---|---|
| `data_type` | `data.type` |
| `host`, `title`, `category`, `integration_id`, `message`, `aggregation_key` | `data.attributes.*` |
| `timestamp` / `timestamp_parsed` | `data.attributes.timestamp`, parsed when RFC3339 |
| `tags` | `data.attributes.tags`, a multiset |
| `notable_event_type` | `data.attributes."system-notable-events".event_type` — the hyphenated key travels as a raw map key, never guessed at as a struct tag |
| `attributes` | the **inner** `data.attributes.attributes` object, JSON, kept nested |
| `outer_extra` | undeclared keys at **three** levels of the envelope, merged into one map — see below |

**Semantics worth knowing:** the wire uses the key `attributes` twice — once
at `data.attributes` (the envelope) and once nested inside it,
`data.attributes.attributes` (the producer's own payload: `{status, priority,
custom}` for notable events, `{changed_resource, author, prev_value, ...}`
for change events). These are kept as two separate columns (`attributes` is
the inner one) rather than merged, because flattening them would let one
clobber the other.

`outer_extra` catches undeclared keys at three levels, not just
`data.attributes`: the attributes level itself (unprefixed, as before), keys
of `data` that are siblings of `attributes` (e.g. a JSON:API `id`), prefixed
`data.`, and keys of the envelope that are siblings of `data` (e.g. JSON:API
`meta`/`included`), prefixed `top.`. The prefixes exist so a key from one
level cannot silently shadow a same-named key from another.

### host_software

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, hostname, name, received_at)`, `TTL received_at + 30 days`.

Fed by `POST /api/v2/softinv` (JSON array of
`{hostname, host_software: {software: [...]}}`). **One row per software
entry**, not per payload — "a full host inventory runs to hundreds of lines"
per the intake audit, and a query like "every host running curl < 8.0" needs
row-per-entry, not an array column.

| Column | Source |
|---|---|
| `hostname` | payload `hostname` |
| `software_type`, `name`, `version`, `publisher`, `deployment_status`, `product_code` | the software entry |
| `deployment_time` / `deployment_time_parsed` | entry `deployment_time`, parsed when RFC3339 |
| `is_64_bit` | entry `is_64_bit`, `Nullable(UInt8)` |
| `install_paths` | entry `install_paths` |
| `extra` | undeclared keys **on the software entry**, JSON per key |
| `payload_extra` | undeclared keys **at the payload level** (beyond `hostname`/`host_software`) merged with undeclared keys **on the `host_software` object itself** (beyond `software`), the latter prefixed `host_software.` so it cannot collide with a payload-level key of the same name; both repeated onto every row that payload produced |

`is_64_bit` is `Nullable`, not defaulted to `false`: "the agent never
determined the architecture" and "32-bit" are different facts, and collapsing
them would misreport bitness for software the agent could not inspect.

**dropped_by_decision:** a software entry that is not itself a JSON object
(e.g. a bare string slipped into the array) is skipped — there is no shape to
build a row from and no column it could partially fill. This is the one place
in this file where a list element is silently dropped rather than reaching
`raw_payloads`; the top-level payload it came from is still captured whole
via `storeRaw` if the *payload* itself fails to decode.

### synthetics_results

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, test_id, received_at)`, `TTL received_at + 30 days`.

Fed by `POST /api/v2/synthetics` (JSON array of `common.TestResult` — no Go
type; the shape is documented in router_evp.go's header comment). One row per
result.

| Column | Source |
|---|---|
| `test_id`, `test_name`, `test_type`, `test_subtype`, `test_version` | `test.*` |
| `location_id`, `location_name`, `location_display_name` | `location.*` |
| `result_id`, `result_initial_id`, `status`, `run_type`, `duration` | `result.*` (duration's wire type is undocumented; kept as sent) |
| `test_started_at`/`_parsed`, `test_finished_at`/`_parsed`, `test_triggered_at`/`_parsed` | `result.test*At`, parsed when RFC3339 |
| `assertion_type`, `assertion_operator`, `assertion_expected`, `assertion_actual`, `assertion_valid` | `result.assertions[]`, five parallel arrays, index-aligned — order and count both matter, which is exactly what the handler's old tally-only logging lost |
| `failure_code`, `failure_message` | `result.failure.*`, both `Nullable` — the failure object's PRESENCE is itself a signal, not just its content |
| `config` | `result.config`, JSON (the documented shape stops at `host`/`port`; `"..."` in the source comment is an explicit "unenumerated", not an oversight) |
| `netstats_*` | `result.netstats.*`, `Nullable(Int64)`/`Nullable(Float64)` via `json.Number` |
| `netpath` | `result.netpath`, JSON — "the same type netpath-intake carries" |
| `dd`, `enrichment`, `v` | top-level `_dd`, `enrichment`, `v` |
| `extra` | undeclared keys at **two** levels: top-level (unprefixed) and on the `result` object itself (prefixed `result.`, so it cannot collide with a top-level key of the same name) |

### openlineage_events

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, run_id, received_at)`, `TTL received_at + 30 days`.

Fed by `POST /api/v1/lineage` (JSON, one OpenLineage `RunEvent` per request;
an array is tolerated). One row per event.

| Column | Source |
|---|---|
| `event_type`, `producer`, `schema_url` | top-level fields |
| `event_time` / `event_time_parsed` | `eventTime`, parsed when RFC3339 |
| `run_id`, `run_facets` | `run.runId`, `run.facets` (JSON — OpenLineage's own extension point, genuinely open-ended by spec) |
| `job_namespace`, `job_name`, `job_facets` | `job.*` |
| `input_namespace`, `input_name`, `input_facets` | `inputs[]`, three parallel arrays |
| `output_namespace`, `output_name`, `output_facets` | `outputs[]`, three parallel arrays (independent from the input arrays — not a shared index) |
| `api_version` | the `?api-version` query parameter |
| `via` | the `Via` request header the trace-agent's proxy adds |
| `extra` | undeclared top-level keys, JSON per key |

`run_id` is first after `tenant_id` in `ORDER BY` because it is the natural
correlation key across a run's `START`/`RUNNING`/`COMPLETE`/`FAIL`/`ABORT`
lifecycle. **Known gap, not a decode gap:** this route currently answers 403
in some deployments because the OpenLineage transport authenticates with
`Authorization: Bearer <key>` rather than `Dd-Api-Key` — the foundation's
`KeyFromBearer` support (see `routes.go`) is what makes this table reachable
at all; if that guard regresses, this table simply stops receiving rows.

### query_action_results

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, received_at)`, `TTL received_at + 30 days`.

Fed by `POST /api/v2/query-actions` (JSON array). **The one track with zero
field names asserted anywhere upstream** — results come from Python
integrations this repo never sees the source of, so there is no shape to
decode into columns at all.

| Column | Source |
|---|---|
| `result` | the entry, verbatim, as JSON |
| `keys` | the entry's top-level keys, sorted |
| `dd_evp_origin`, `dd_evp_origin_version` | the `DD-EVP-ORIGIN`/`DD-EVP-ORIGIN-Version` request headers |

## dropped_by_decision

- **`agent_discovery`: proto fields the linked `agent-payload` version does
  not declare.** `proto.Unmarshal` discards unknown fields silently, and
  there is no wire-level signal that it happened — a newer agent's additions
  are simply invisible until the vendored schema is updated. Not something a
  handler-level decision can fix.
- **`agent_health_issues`: proto fields likewise unrecoverable** for the same
  reason — `HealthReport` is decoded with `encoding/json` into a typed proto
  struct, so a field the struct does not declare is dropped before the
  handler ever sees it (JSON, unlike protobuf, *could* preserve it, but the
  agent-payload struct is the decode target and does not expose an
  unknown-field sink).
- **`host_software`: a software entry that is not a JSON object.** See the
  table's own note above — skipped, not stored, because there is no shape to
  build a row from. The payload it belongs to is still captured whole via
  `storeRaw` if the payload itself fails to decode.
- **`synthetics_results`: `result.config` fields beyond `host`/`port`.** Not
  dropped from storage (the whole `config` object is kept as JSON), but the
  intake's own documentation of this track explicitly stops enumerating
  fields here (`"..."` in the source comment) — there is no upstream Go type
  to check against, so nothing beyond `host`/`port` is asserted to exist by
  name anywhere in this codebase.
- **Everything else in this file is stored.** Where a field's wire type or
  presence was ambiguous, the default was to keep it as a JSON-text or `Map`
  column rather than to drop it — see the two repeating patterns at the top
  of this document.
