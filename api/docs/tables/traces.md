# APM tables (`trace.agent.<site>`)

Six tables, all created by `schema/migrations/0003_traces.sql`, all fed by
`intake/router_trace.go`, all written through `apps/storage/rows_traces.go`.

Until this work none of the four routes on this host stored a byte; they
decoded, logged and dropped. The logs stayed — they are how the protocol was
read in the first place — but they are no longer the only sink, and anything
that fails to decode now goes to `raw_payloads` under the intake label
`trace` instead of vanishing.

**The zero-payload guard.** Protobuf carries no type marker and unmarshals
"fail open" — unknown fields are silently skipped — so a body meant for a
completely different endpoint can decode without error into an empty
`AgentPayload`. `HandleTraces` treats a decode with both `TracerPayloads` and
`IdxTracerPayloads` empty as that case: instead of quietly calling `store()`
with zero rows (a no-op that looks identical to a healthy, traceless agent),
it goes to `raw_payloads` under reason `unexpected_shape`, the same guard
`HandleContainerLifecycle` (`intake/router_containers.go`) uses for the same
reason. The agent still gets its `{"rate_by_service":{}}` 200 either way.

| table | engine | ORDER BY | TTL | fed by |
|---|---|---|---|---|
| `spans` | MergeTree, `PARTITION BY toDate(start)` | `(tenant_id, service, name, start, trace_id)` | 14 days on `start` | `POST /api/v0.2/traces` |
| `apm_stats` | MergeTree, `PARTITION BY toDate(bucket_start)` | `(tenant_id, service, bucket_start, name, resource)` | 30 days on `bucket_start` | `POST /api/v0.2/stats` |
| `dsm_pipeline_stats` | MergeTree, `PARTITION BY toDate(bucket_start)` | `(tenant_id, service, bucket_start, hash)` | 30 days on `bucket_start` | `POST /api/v0.1/pipeline_stats` |
| `dsm_backlogs` | MergeTree, `PARTITION BY toDate(bucket_start)` | `(tenant_id, service, bucket_start)` | 30 days on `bucket_start` | same request, bucket's `Backlogs` |
| `dsm_bucket_transactions` | MergeTree, `PARTITION BY toDate(bucket_start)` | `(tenant_id, service, bucket_start)` | 30 days on `bucket_start` | same request, bucket's two packed blobs |
| `dsm_messages` | MergeTree, `PARTITION BY toDate(received_at)` | `(tenant_id, received_at)` | 30 days on `received_at` | `POST /api/v2/data_streams_messages` |

Response contracts are unchanged, because senders depend on them:

* `/api/v0.2/traces` — **200** with exactly `{"rate_by_service":{}}`. The agent
  parses this body; `rate_by_service` feeds its priority sampler. It is
  returned on every path out of the handler, decode failure included — failing
  the request would only make the agent retry the same undecodable payload.
* `/api/v0.2/stats` — **200** with `{}`. 200, not 202: that is what the handler
  answered before this write layer existed (`f10ee4d:server/intake/router_trace.go`,
  `defer c.JSON(http.StatusOK, gin.H{})`), and the agent's stats writer only
  checks for 2xx. Changing it would be the behavioural change this section
  exists to prevent, so it stays until someone has an agent-side reason.
* `/api/v0.1/pipeline_stats` — **202** with `{}`.
* `/api/v2/data_streams_messages` — **202** with `{}`.

`isDiagnose` (the agent's startup connectivity sweep, `X-Requested-With:
datadog-agent-diagnose`) still short-circuits the first three handlers before
the body is read, so a restarting agent does not fill any table.

A request with no tenant in context answers exactly as before and stores
nothing: `tenant_id` is the first `ORDER BY` column of every table and a
guessed value would put rows where no query looks.

---

## `spans`

One row per span. Two wire shapes reach it and `wire_format` says which:

* `v04` — `pb.TracerPayload`, strings inline.
* `idx` — `idx.TracerPayload` (`pbgo/trace/idx`), every string an index into a
  per-payload table where index 0 is the empty string, so `Ref == 0` means
  "unset" and is never used as an index.

The column is load-bearing, not decoration: several columns exist in only one
of the two formats, so a zero means "this format has no such field" for one row
and "the tracer left it unset" for the other.

### Column → source field

| columns | source |
|---|---|
| `received_at` | arrival time (`time.Now()`); the span's own clock is `start` |
| `agent_hostname`, `agent_env`, `agent_version`, `target_tps`, `error_tps`, `rare_sampler_enabled`, `agent_tags` | `pb.AgentPayload` |
| `container_id`, `language`, `language_version`, `tracer_version`, `runtime_id`, `tracer_env`, `tracer_hostname`, `app_version`, `tracer_tags` | `TracerPayload` (idx: via the string table) |
| `tracer_attributes_json` | idx `TracerPayload.Attributes`, full typed form |
| `container_debug_*` (5, all `Nullable`) | `TracerPayload.ContainerDebug`; idx resolves `ErrorRef` / `BufferEvictionReasonRef` through the string table |
| `priority`, `origin`, `dropped_trace`, `chunk_tags` | `TraceChunk` |
| `sampling_mechanism` | idx `TraceChunk.SamplingMechanism`; v0.4 carries the same thing in `meta['_dd.p.dm']` |
| `chunk_attributes_json` | idx `TraceChunk.Attributes`, full typed form |
| `service`, `name`, `resource`, `span_type` | `Span.Service/Name/Resource/Type` |
| `trace_id`, `trace_id_high` | v0.4: `Span.TraceID` + hex `meta['_dd.p.tid']`; idx: `TraceChunk.TraceID`, 16 big-endian bytes split in half |
| `trace_id_hex` | MATERIALIZED, `leftPad(hex(high),16,'0') ‖ leftPad(hex(low),16,'0')`, lowercased |
| `span_id`, `parent_id`, `start`, `duration_ns`, `error` | `Span` |
| `kind` | idx `Span.Kind` as the lowercase OTel name; v0.4 `meta['span.kind']` |
| `span_env`, `span_version`, `component` | idx only, per-span overrides |
| `meta`, `metrics`, `meta_struct` | v0.4 maps verbatim; idx scalars projected back into them |
| `attributes_json` | idx `Span.Attributes`, full typed form |
| `link_*` (6 arrays) | `Span.SpanLinks` / idx `Span.Links`, index = wire order |
| `event_*` (3 arrays) | `Span.SpanEvents` / idx `Span.Events`, index = wire order |

### Semantics worth knowing

* **`parent_id = 0` means ROOT**, not "absent". The wire has no has-parent bit,
  so the zero is kept as it came rather than turned into a NULL that would
  claim a distinction the tracer never made.
* **`ORDER BY` serves "one service over a time range"**, which every dashboard
  asks, and compresses well because service/name repeat for millions of
  consecutive rows. "Every span of one trace" is served by
  `INDEX idx_trace_id trace_id TYPE bloom_filter(0.001)` (and the same for
  `span_id`), because trace ids are random and leading the key with one would
  destroy time locality for every other query. `trace_id` still ends the key so
  a trace's spans sit together inside a granule.
* **`trace_id_hex` is the form people paste** out of a Datadog URL.
  `leftPad` is required: ClickHouse's `hex()` on an integer drops leading zero
  bytes, so an unpadded concatenation would produce an id of the wrong width.
* **`*_attributes_json` is the record for idx, `meta`/`metrics` are the
  projection.** An idx attribute can be bytes, an array or a nested key-value
  list; scalars go to `meta` (strings and bools) and `metrics` (ints and
  doubles) so a query written for v0.4 keeps working, and the JSON keeps every
  attribute in its declared type. Values are TAGGED — `{"k":{"int":5}}` — so
  `5`, `5.0` and `"5"` stay distinguishable. The same encoding is used for
  v0.4 span-event attributes and for both formats' link attributes.
  An `int` attribute in `metrics` is a `float64` and loses precision above
  2^53; the exact `int64` is in `attributes_json`.
* **`meta_struct` holds bytes, not text.** ClickHouse `String` is a byte
  string; these are msgpack blobs (AppSec events, process tags) with no
  published per-key type.
* **`agent_tags` / `tracer_tags` / `chunk_tags` are plain `Map`, not the tag
  multiset.** They are protobuf `map<string,string>`, which cannot repeat a
  key — the multiset shape in `0001` exists for flat `"key:value"` lists that
  can. `apm_stats.client_tags` is the one field on this host that is a genuine
  Datadog tag list, and it gets the multiset plus the bloom-filter pair.
* **What arrives is a SAMPLE.** The agent runs its samplers before it ships
  spans. The complete counts are in `apm_stats`, which is why the two tables
  exist separately and why `apm_stats` keeps rows twice as long.

---

## `apm_stats`

One row per `pb.ClientGroupedStats`, with the three enclosing levels
(`StatsPayload` → `ClientStatsPayload` → `ClientStatsBucket`) denormalized onto
it. All 21 group fields are columns (23 proto field ids, but 14 does not exist
in this version).

* **`is_trace_root` is `Enum8('not_set','true','false')`** — a genuine
  three-state `pb.Trilean`. Collapsing it to a bool would silently turn every
  older tracer's rows into non-roots. Enum8 travels by NAME through the driver.
* **`client_tags` is the tag multiset**, built with `tagsToMultiMap`, with
  `mapKeys` / `arrayFlatten(mapValues)` bloom filters.
* **`client_process_tags_hash` is a `UInt64`** end to end, never through a
  float.
* **The two latency sketches are kept RAW plus decoded.** `ok_summary` /
  `error_summary` hold the concentrator's protobuf bytes verbatim; a DDSketch
  merges with other sketches and answers any quantile later, which a stored
  p50 and p99 cannot. Beside them: `*_summary_state`
  `Enum8('absent','undecodable','empty','ok')`, `*_count/_sum/_min/_max`
  (`Nullable`, present only in state `ok`) and `*_bin_keys` / `*_bin_counts`
  (the positive-value store as parallel arrays).
  `undecodable` is a real signal — bytes arrived and were not a DDSketch,
  usually version skew — and the bytes are kept so it can be investigated.
* Values are **span durations in nanoseconds**, and the sketches are built by
  `sketches-go`, NOT by the agent's own `pkg/util/quantile` used for metric
  distributions. Mixing the two decoders would be wrong.

---

## `dsm_pipeline_stats`, `dsm_backlogs`, `dsm_bucket_transactions`

`POST /api/v0.1/pipeline_stats` is proxied by the trace-agent untouched, so the
schema is **dd-trace-go's**, not the agent's: msgpack of
`internal/datastreams.StatsPayload`, generated by `tinylib/msgp` with no field
tags, which means the map keys are the Go field names verbatim. The decoder is
hand-written over `msgp.Reader` in `router_trace.go`; dd-trace-go itself is not
a dependency of this module and pulling a whole tracer in to read a tracer's
payload would be the wrong trade.

Three tables because a bucket has three kinds of child and they have different
cardinality:

* `dsm_pipeline_stats` — one row per `StatsPoint`. `hash` / `parent_hash`
  rebuild the pathway graph by self-join, `edge_tags` describe the hop.
  **Three** DDSketches per row, each with the same raw-plus-decoded treatment
  as `apm_stats`: `pathway_latency`, `edge_latency` and `payload_size`.
  Note the units: the latencies are **seconds** here (APM's are nanoseconds),
  and `payload_size` is a distribution of message sizes in bytes — dd-trace-go's
  field name reads like a scalar, its `[]byte` type does not.
  The four proxy headers (`via`, `additional_tags`, `container_tags`,
  `content_encoding`) are columns because the container identity they carry
  appears nowhere in the body.
* `dsm_backlogs` — one row per `Backlog` (`tags`, `value`). Its own table
  because a bucket can carry backlogs and no stats points; folding them into a
  point row would duplicate or lose them.
* `dsm_bucket_transactions` — the bucket's `Transactions` and
  `TransactionCheckpointIds` blobs, kept **byte for byte**. They are hand-rolled
  binary (`[checkpointId uint8][timestamp int64 BE][idLen uint8][id bytes]`,
  and `[id uint8][nameLen uint8][name bytes]`) that dd-trace-go says must match
  the Java tracer exactly; decoding them now against today's comment would
  silently mis-read tomorrow's records.

**Unknown keys are kept with their values.** `unknown_keys Array(String)` and
`unknown_json String` on `dsm_pipeline_stats` carry every field this decoder
did not recognise, from all three levels, prefixed `payload.` / `bucket.` /
`point.`. dd-trace-go adds fields here with no version negotiation, so an
unknown key is expected — and a name without a value would say a field exists
and nothing about what it carries.

A body that does not decode goes to `raw_payloads` with reason `decode_error`
and the msgp error in `note`.

---

## `dsm_messages`

`POST /api/v2/data_streams_messages` is not the trace-agent at all — an Event
Platform track that happens to live on this host, so the forwarder batches and
the body is one JSON array.

The producer is not in the open agent repository, so **no shape is invented**:
each element is stored as the JSON text it arrived as (`message`), with its
top-level key names (`keys`) beside it. `keys` is the one thing extractable
without guessing at semantics and turns "what does this track actually send?"
into a `GROUP BY`. `position` keeps the batch order, which is the only ordering
these messages have. `dd_evp_origin` / `dd_evp_origin_version` are the closest
thing the track has to a schema identifier.

A body that is not an array goes to `raw_payloads` with reason
`unexpected_shape`.

---

## dropped_by_decision

Everything below was considered and deliberately not given a column. The
default was to store; each of these has a reason.

1. **Map insertion order** in `Span.Meta`, `Metrics`, `MetaStruct` and in every
   idx attribute map. Already lost at decode time — the generated types are Go
   maps — so it is not recoverable here, only in a different decoder.
   Repeated-field order (links, events, edge tags, backlog tags, process tags,
   array attributes) IS preserved, in arrays.
2. **Protobuf / msgpack unknown fields on the trace payloads.** The generated
   `UnmarshalVT` and `UnmarshalMsg` keep them in an unexported
   `unknownFields`, which the public API does not expose; adding a column would
   mean forking the generator. The DSM decoder is hand-written, so there the
   unknown keys ARE kept (`unknown_keys` / `unknown_json`).
3. **Negative-value store bins and zero count of every DDSketch.** Span
   durations and message sizes are non-negative by construction, so the
   positive store is the whole distribution in practice. The raw protobuf bytes
   are kept regardless, so nothing is actually lost — only not broken out.
4. **Sketch index mapping (gamma, relative accuracy, interpolation).** Same
   reason: it lives in the raw bytes, and a column would be a copy of a
   constant the concentrator sets (0.01, 2048 bins).
5. **`proto.Size(p)` / body length of a traces request.** A property of the
   request, not of any span; `raw_payloads.body_bytes` records it for the
   requests that end up there.
6. **Unknown keys on a DSM payload with no `StatsPoint` at all.** They ride the
   pipeline-stat rows, so a payload whose buckets contain only backlogs would
   carry none. Accepted rather than adding an `unknown_*` pair to three tables;
   a payload with a new field and no stats points has not been seen.
7. **A decoded form of `dsm_bucket_transactions`' two blobs.** See above — the
   layout is a Java-tracer wire format, kept verbatim on purpose.
8. **`rate_by_service` sampling rates in the traces response.** Not incoming
   data. The response stays `{}` until there is a sampler to feed it from.
9. **idx attribute nesting past 32 levels.** `AnyValue` is recursive on the
   wire and the protobuf bounds it nowhere, so the renderer stops at
   `traceAttrMaxDepth` (32) and writes `{"_depth_exceeded":32}` in place of the
   tail — visible in the column, never silently empty. A Go stack overflow is a
   fatal runtime error that would take the whole node down, not one request,
   and real tracers nest two or three levels. Note this guards OUR conversion
   only: `idx.TracerPayload` is decoded by vtprotobuf's generated `UnmarshalVT`,
   which has no nesting limit of its own, so a payload deep enough would fail
   there first. That is upstream generated code.
