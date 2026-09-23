# Tables fed by `api.<site>` / `app.<site>`

Everything `intake/router_api.go` stores, and everything it deliberately does
not. Migration: `schema/migrations/0014_api_fidelity.sql`. The logs intake has
its own page, `docs/tables/logs.md`.

The rule this page records: a field of a decoded payload reaches a column, or a
structured column that holds it losslessly (Map, Array, JSON), or a raw column
holding its bytes — or it appears under "Dropped by decision" below with a
reason. A log line is not a sink.

## Where each route's rows go

| Route | Tables |
|---|---|
| `POST /api/v1/series`, `POST /api/v2/series` | `metrics` |
| `POST /api/beta/sketches` | `sketches`, `agent_batch_metadata` |
| `POST /api/v1/distribution_points` | `sketches` |
| `POST /api/v1/check_run` | `check_runs` |
| `POST /api/v1/events` | `events` |
| `POST /intake/` — events variant | `events` |
| `POST /intake/` — agent_checks variant | `agent_checks`, `external_host_tags` |
| `POST /intake/` — gohai / systemStats variant | `hosts` |
| `POST /intake/` — legacy V5 resources variant | `raw_payloads` |
| `POST /api/v1/metadata` | `agent_metadata` |
| `POST /api/v2/intake-key` | `delegated_auth_requests` |
| `POST /api/v2/profiles/symbols/query` | `symbol_queries` |
| `POST /api/unstable/on_prem_runners[/api_key_only]` | `runner_enrollments` |
| `POST .../workflow-tasks/publish-task-update` | `runner_task_updates` |
| `POST .../workflow-tasks/heartbeat` | `runner_heartbeats` |
| `POST .../workflow-tasks/dequeue` | `runner_dequeues` |
| `POST /api/v2/actions/connections` | `action_connections` |
| `GET /api/v1/query` | nothing — a read endpoint with no query engine behind it |
| `GET .../runner/health-check`, `GET /api/v1/validate`, `/support/flare` | nothing — no body, no data |

Every response contract is unchanged by this work: `202 {"errors":[]}` on the
series/sketches/check_run/distribution_points family, `200 {"status":"ok"}` on
`/intake/` and `/api/v1/metadata`, the full `EventCreateResponse` body on
`/api/v1/events`, `200 {"data":{"attributes":{"api_key":…}}}` on
`intake-key`, `200 {"data":[]}` on the symbol query, and PAR's exact statuses
(200 with `createRunnerResponse` on enrollment, bare 200 with an empty body on
dequeue, exactly 200 on heartbeat, 202 elsewhere).

## Tables extended

### `metrics`, `sketches`

New columns on both, same meaning:

| Column | Source |
|---|---|
| `origin_product`, `origin_category`, `origin_service` `UInt32` | v2 `MetricSeries.Metadata.Origin` / `Sketch.Metadata.Origin`. **0 means not sent**, which is also what the proto default and the enum's "unknown" mean, so no Nullable is needed. v1 carries no origin and leaves all three at 0. |
| `resources Map(LowCardinality(String), String)` | v2's `[type, name]` list minus the `host` entry, which has had its own column since 0001. v1's undeclared `device` key is written here as `device=<name>`, so one query spans both wire versions. A plain Map, not the tag multiset: a resource list is typed slots, one name per type. |
| `extra Map(String, String)` | v1 `Series.AdditionalProperties` beyond `device`, `source_type_name` and `unit`; on `sketches`, a distribution_points series' undeclared keys. **Values are JSON-encoded**, so one column takes a string, a number, an object and an array. |

`sketches.resources` stays empty for the agent's own payload —
`gogen.SketchPayload_Sketch` has a flat `Host` field, not a resource list — and
exists so the two tables answer the same questions from a source that has one.

### `check_runs`

`extra Map(String, String)` — `datadogV1.ServiceCheck.AdditionalProperties`,
values JSON-encoded. The agent's own struct has exactly the six declared keys,
so a non-empty `extra` means a newer agent or a third-party client, which is
when the value is worth more than a count in a log.

### `events`

Two sources now feed this table: `POST /api/v1/events` and the agent's own
events on `/intake/`. They fill different subsets, which is why the columns of
both live here rather than in two tables that every chart would have to UNION.

| Column | Source |
|---|---|
| `event_type LowCardinality(String)` | `/intake/` only — `pkg/metrics/event.Event.EventType`. Empty for the public API. |
| `related_event_id Nullable(Int64)` | public API only. Nullable because Datadog's id space starts at 1 and "no parent" must not become 0. |
| `alert_type_raw`, `priority_raw String` | exactly what the sender wrote, before the coercion that fills `alert_type` / `priority`. Empty when nothing was sent. |
| `extra Map(String, String)` | undeclared keys, values JSON-encoded. |

The coercion stays — dropping an event over an invented alert type would lose
the one thing we were asked to remember — but it is no longer silent.

Worth knowing: on `/api/v1/events` the alert type is a generated ENUM, so a
value outside Datadog's vocabulary makes the whole event unparseable and the
object goes to `raw_payloads` (`events`/`unexpected_shape`) rather than to a
row of empty strings. The coercion therefore only ever fires on the `/intake/`
path, where the same fields are free strings.

### `hosts`

The host payload had nine columns and roughly twenty keys. Mapping now:

| Column | Source key |
|---|---|
| `uuid`, `agent_flavor`, `python_version` | `uuid`, `agent-flavor`, `python` |
| `meta`, `logs`, `otlp` `String` | the envelope objects of those names, verbatim JSON |
| `network String` | the ENVELOPE's `network` — the host's addresses, from `comp/metadata/host` |
| `filesystem String` | `gohai.filesystem`, the mount list |
| `system_stats`, `install_method`, `proxy_info`, `container_meta` `Map` | `systemStats`, `install-method`, `proxy-info`, `container-meta`, values JSON-encoded |
| `fips_mode`, `fips_proxy_enabled` `Nullable(UInt8)` | `fips_mode`, `fips_proxy_enabled`. NULL when the key is absent: "FIPS is off" and "this agent does not know about FIPS" are different statements. |
| `host_tags Map(LowCardinality(String), Array(String))` | `host-tags`, EVERY source. Only `system` used to reach a row, so on a cloud host the provider's tags were discarded outright. |
| `tags` (unchanged) | still the `system` source alone — it is what every chart filters on, and mixing a provider's set into it would change what existing queries mean |
| `gohai_extra Map(String, String)` | gohai sections beyond `platform`/`cpu`/`memory`/`filesystem`, values JSON-encoded. `gohai.network` lands here on purpose: the envelope's `network` owns that column and the two are different things with the same name. |
| `intake_extra Map(String, String)` | top-level envelope keys this decoder does not name. **`apiKey` is excluded explicitly**, so a credential cannot reach a column through a catch-all. |
| `resources String` | the legacy V5 process snapshot riding on the host payload, verbatim. The process intake owns that format; parsing it twice would be two decoders to keep in step. |

`hosts` is a `ReplacingMergeTree(seen_at)` keyed on `(tenant_id, host)`, so
read it with `FINAL` or `argMax`. A host leaves the table only by replacement —
there is no TTL, because a live host must not age out.

## Tables added

| Table | Engine | ORDER BY | Fed by |
|---|---|---|---|
| `agent_batch_metadata` | MergeTree, 30d | `(tenant_id, agent_version, received_at)` | `gogen.CommonMetadata` off a sketch batch |
| `agent_checks` | MergeTree, 90d | `(tenant_id, hostname, check_name, received_at)` | `/intake/` agent_checks variant |
| `external_host_tags` | MergeTree, 90d | `(tenant_id, host, source, received_at)` | `/intake/` agent_checks variant |
| `agent_metadata` | MergeTree, 30d | `(tenant_id, variant, hostname, received_at)` | `/api/v1/metadata`, all 13 variants |
| `delegated_auth_requests` | MergeTree, 90d | `(tenant_id, at)` | `/api/v2/intake-key` |
| `symbol_queries` | MergeTree, 30d | `(tenant_id, at)` | `/api/v2/profiles/symbols/query` |
| `runner_enrollments` | ReplacingMergeTree(`enrolled_at`), no TTL | `(tenant_id, runner_id)` | PAR enrollment |
| `runner_task_updates` | MergeTree, 90d | `(tenant_id, job_id, task_id, at)` | PAR publish-task-update |
| `runner_heartbeats` | MergeTree, 7d | `(tenant_id, job_id, task_id, at)` | PAR heartbeat |
| `runner_dequeues` | MergeTree, 7d | `(tenant_id, at)` | PAR dequeue |
| `action_connections` | MergeTree, 90d | `(tenant_id, runner_id, name, at)` | `/api/v2/actions/connections` |

Notes worth carrying:

- **`agent_batch_metadata`** describes the SENDER, not the metric: agent
  version, timezone, its clock and both IPs, one row per request. Compared
  against `received_at`, `current_epoch` is a clock-skew measurement for free.
  `CommonMetadata.ApiKey` is deliberately not a column.
- **`agent_checks`** is not `check_runs`. `check_runs` holds `/api/v1/check_run`,
  the current service-check path; these are the V5 collector's own view,
  arriving on the same request as the host metadata, with a status vocabulary
  no published document relates to the other. The wire shape is a POSITIONAL
  array: positions 0–4 are the columns, anything after them is
  `positional_extra` as a JSON array. `status` is `Nullable(Int64)` because the
  array may stop before position 3 and status 0 means OK — a non-nullable
  column would report every truncated entry as healthy.
- **`external_host_tags`** are tags an agent reports FOR ANOTHER host (a vSphere
  collector tagging its VMs). One row per (host, source): the host described is
  not the sender, and one host carries several sources at once. Rows are
  emitted in sorted source order, so replaying a capture produces the same rows.
- **`agent_metadata`**: one row per variant key present, the variant's own body
  kept as text in `payload`. `timestamp` is Nullable — an absent one must not
  become 1970. `clustercheck_status` and `clustercheck_integration_status` ride
  in `envelope_extra` next to the `clustercheck_metadata` row.
- **`delegated_auth_requests`** stores the SHA-256 of the delegated-auth proof,
  hex, and never the proof. `api_key_id` is the Postgres id of the key the
  request authenticated with — not the key, not its hash.
- **`runner_enrollments`** is the table that makes PAR verifiable: every OPMS
  request after enrollment is signed with the identity we hand out, and
  `public_key_pem` is what a verifier will need. ReplacingMergeTree by
  `runner_id`, because a re-enrolled runner must not leave a second public key
  behind with no rule for choosing between them. `org_id` is stored because the
  FNV-64a derivation is ours: if it ever changes, the runners enrolled under the
  old one are only recoverable from here.
- **`action_connections.credentials` is SENSITIVE.** It holds the integration's
  tokens, passwords and private keys verbatim, because a connection without its
  credentials cannot be used. The logging path prints only the KEY NAMES of
  that object and must keep doing so. Anything reading this column — a panel, an
  export, a support flare — is reading a credential store, not telemetry.

## Undecodable input

Anything that fails to decode, any batch element the model reports as
`UnparsedObject`, and any payload with no published schema goes to
`raw_payloads` through `storeRaw`. Decode success paths never do. The labels
used here:

**What is stored is the bytes the element ARRIVED as**, not a re-encoding of
`UnparsedObject`. The generated `UnmarshalJSON` fills that map with plain
`encoding/json` — only `AdditionalProperties` gets the `UseNumber` decoder — so
an integer above 2^53 inside an element that did not fit the model has already
been rounded to `float64` before a handler sees it, and re-encoding it would
store the rounded number. `jsonElements` / `jsonFieldElements` (intake/payloads.go)
re-split the body for those originals; `parseCheckRuns` returns them alongside
the decoded checks, because it skips items and a later re-split would line the
wrong bytes up with the wrong check. `apiJSONBytes` remains the fallback for a
framing that cannot be re-split.

A series is a special case worth knowing: `DistributionPointsSeries` also fills
`UnparsedObject` when only its `type` word is outside the one legal value, and
then its fields ARE populated. Such a series is stored raw **and** its points
still become rows — a typo in `type` must not cost a host its distribution.
`ServiceCheck` and `EventCreateRequest` do the same for an out-of-range
`status` and an unknown `alert_type`/`priority`, but there the coerced value
would be a guess in a column (an unreadable check stored as `OK`), so those
keep the bytes only.

| intake | reason | when |
|---|---|---|
| `series` | `decode_error` | the body is not a v1/v2 series payload |
| `series` | `unexpected_shape` | a v1 series with `UnparsedObject` set, a point with a nil timestamp or value, a payload-level undeclared key |
| `sketches` | `decode_error` / `no_schema` | undecodable body / a legacy pre-DDSketch `Distribution` |
| `check_run` | `decode_error` / `unexpected_shape` | undecodable body / an item that is not a `ServiceCheck` |
| `events` | `decode_error` / `unexpected_shape` | undecodable body / an event with `UnparsedObject` set |
| `distribution_points` | `decode_error` / `unexpected_shape` | undecodable body / a payload with `UnparsedObject` set, a series with `UnparsedObject` set, a payload-level undeclared key, a pair that is not `[timestamp, [values]]` |
| `intake` | `decode_error` / `unexpected_shape` / `no_schema` | not JSON / no known variant key / the legacy V5 resources snapshot |
| `metadata` | `decode_error` / `no_schema` | not JSON / no known variant key |
| `symbols` | `decode_error` | not a JSON:API document |
| `par` | `decode_error` | not a JSON:API document, on any PAR route |

## Dropped by decision

1. **`distribution_points` raw values.** `sketch.Build` reduces them to DDSketch
   buckets and the originals are discarded. Deliberate: a distribution exists to
   be merged with the agent's own sketches, which arrive already bucketed, and
   keeping both forms would mean two tables that disagree about one metric.
   Everything the model could not fit IS kept, whole, in `raw_payloads`: a body
   that fit no series list, a series that fit no series, and a pair that could
   not be split.
2. **`CommonMetadata.ApiKey`** (sketches) and the `/intake/` envelope's
   **`apiKey`**. The request already authenticated; storing the credential again
   would put it in a table with a TTL, which is exactly where a credential must
   never be.
3. **The PAR delegated-auth proof** itself — only its SHA-256 is stored, for the
   same reason.
4. **`Origin`'s reserved field 3** ("do not index", `metric_type=9`). The
   published proto marks it reserved, so gogo parks it in `XXX_unrecognized`
   with no name to decode into. Counted in the log line; there is nothing to put
   in a column.
5. **`MetricPayload.XXX_unrecognized` / `MetricSeries.XXX_unrecognized`.** Same
   reason: unknown protobuf wire fields have no names, and a column of anonymous
   byte strings answers no question. When a future agent starts using one, the
   published proto will name it and it gets a column then.
6. **Undeclared keys on a JSON-submitted `/api/v2/series`.** `gogen.MetricPayload`
   is a protobuf struct with no `AdditionalProperties`, so `encoding/json` drops
   what it does not declare before any code here runs. Not a decision so much as
   a limit worth writing down: the agent's protobuf path is structurally
   complete, the JSON path is not. Fixing it means decoding v2 JSON into a model
   that keeps extras, which is a change to `payloads.go`'s parser, not to a table.
7. **`GET /api/v1/query`.** A read endpoint: the query string is a question, not
   telemetry, and storing every poll of the cluster-agent's External Metrics
   Provider would fill a table with our own unanswered questions.
8. **`GET .../runner/health-check`, `/api/v1/validate`, `/support/flare`.** No
   body and no data beyond two headers already in the log line.
