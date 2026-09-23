# RUM tables

Everything the browser, iOS and Android SDKs send lands on one host —
`browser-intake.<site>` — and in six ClickHouse tables. The router is
`intake/router_rum.go`, the rows are `apps/storage/rows_rum.go`, the schema is
`schema/migrations/0013_rum.sql`. Pointing an SDK at us is
[docs/rum-sdk-setup.md](../rum-sdk-setup.md).

## The one thing to know before reading further

**Every table carries the whole payload it came from in a `String` column**
(`event`, `span`, `segment`). This is not redundancy. A RUM event variant has
well over a hundred optional fields, several of them arrays of objects, and new
ones arrive with every SDK release — while the mobile SDKs ship against schema
snapshots months older than ours. The typed columns exist so a query can
**filter** without parsing JSON; the raw column exists so nothing is lost,
including keys the generated types do not declare yet.

**The date is the sender's date, never arrival.** iOS and Android buffer events
to disk and upload them up to eighteen hours later — an offline device, tracking
consent still pending, a crash reported at the next app launch. An old `date` is
correct data. Arrival is kept separately in `received_at`, so the lag is
measurable instead of invented.

**Absent is not zero.** A view that sends no `crash` sub-object has not measured
zero crashes; it is a platform with no crashes to measure. Those columns are
`Nullable` with Go pointer fields behind them, and a missing timestamp is `NULL`
rather than 1970.

## The request block, on every table

A RUM event has no hostname, no agent and no container id. The only thing that
identifies its sender is the request it arrived in, so all six tables start with
the same fifteen columns:

| column | source |
|---|---|
| `tenant_id` | the API key's tenant (`TenantFromContext`) |
| `received_at` | arrival, for measuring how stale a batch was |
| `ddsource` | `?ddsource=` — `browser`, `ios`, `android`, `flutter`, … |
| `evp_origin`, `evp_origin_version` | `?dd-evp-origin[-version]=` (browser) or `DD-EVP-ORIGIN[-VERSION]` (mobile) |
| `evp_encoding` | `?dd-evp-encoding=deflate` (browser, which sets no headers) or `Content-Encoding` (mobile). `""` means the batch was sent uncompressed |
| `request_id` | `?dd-request-id=` or `DD-REQUEST-ID` — fresh per attempt |
| `idempotency_key` | `DD-IDEMPOTENCY-KEY` (`sha1(body)`, RUM only) — stable across retries, so the pair tells a retry from a new batch |
| `dd_api` | `?_dd.api=fetch\|beacon` (browser) |
| `batch_time` | `?batch_time=` ms epoch, `Nullable` |
| `retry_count`, `retry_after` | `?_dd.retry_*` (browser) or `?ddtags=retry_count:N,retry_after:CODE` (mobile). `Nullable`: a first attempt has no retry count, and "retried zero times" is the same number and a different fact. `retry_after` is stored **as sent** — a delay in milliseconds from the browser, the HTTP status that caused the retry from mobile |
| `remote_addr`, `user_agent` | the connection and its header |
| `query_extra` | every query parameter without a column of its own, as JSON, **with `dd-api-key` and `api_key` removed** |

The payload columns hold the **decoded** bytes, so `evp_encoding` is the only
record that compression happened at all — an SDK release that quietly stops
compressing is a bandwidth regression that would otherwise show up in a network
bill and nowhere else.

`query_extra` is the `event` column's bargain applied to the URL: a parameter an
SDK release adds must not become invisible. The credential is stripped because
with query-string auth the credential *is* a query parameter, and a table with a
30-day TTL must not be where it becomes durable.

## rum_views

```
ReplacingMergeTree(document_version)
PARTITION BY toYYYYMM(date)
ORDER BY (tenant_id, application_id, session_id, view_id)
TTL date + 30 DAY
```

Fed by `type:"view"` and `type:"view_update"` lines on `POST /api/v2/rum`.

**A view is upserted, not appended.** It lives as long as the user stays on the
page: the SDK re-sends the *same* `view.id` with a growing
`_dd.document_version` and updated counters, and all three SDKs keep only the
highest version per id inside one batch (browser's in-batch replace, iOS's
`RUMViewEventsFilter`, Android's `RumViewEventFilter`). One `view.id` is a
series of updates to one logical row. Readers must use `FINAL` or `argMax(...)`
for exactness, the same caveat `hosts` carries.

**`view_update` merges into the same row as `view`.** It is the newer wire
spelling of an update this table already models as a replacement: same
application/session/view identity, same `_dd.document_version` counter, the same
declared field set. Keeping them apart would split one view's history across two
rows whose counters disagree. `event_type` records which spelling won the merge.

**Why monthly partitions.** ReplacingMergeTree only collapses rows *within* one
partition. `date` is the view's start and stays constant across its updates, so
daily partitions would in fact hold — but the whole guarantee would then rest on
an SDK detail nobody here controls. A month costs one coarser pruning step and
removes the question.

Columns, in short: `date`, the identity triple, `document_version`, `event_type`,
`service`/`version`/`build_version`/`build_id`/`source`, the `session.*` block,
`usr.*` and `account.*`, `view.url`/`name`/`referrer`, `view.loading_type`
(worth its own column — averaging `loading_time` across `initial_load` and
`route_change` produces a number that means nothing), `loading_time`,
`time_spent`, `is_active`, `is_slow_rendered`, the seven per-kind counters, the
Web Vitals, the device/os/connectivity block, `context` and `feature_flags` as
JSON, `ddtags`, `event`.

**Web Vitals read the modern block first, the deprecated field second.**
`lcp` ← `view.performance.lcp.timestamp`, else `view.largest_contentful_paint`;
`cls` ← `performance.cls.score`, else `cumulative_layout_shift`;
`inp` ← `performance.inp.duration`, else `interaction_to_next_paint`;
`fcp` ← `performance.fcp.timestamp`, else `first_contentful_paint`;
`fid` ← `performance.fid.duration`, else `first_input_delay`;
`fbc` ← `performance.fbc.timestamp`; `ttfb` ← `view.first_byte`. Both spellings
are still on the wire, so one column answers for either SDK generation.

`source` is **not** the same thing as `evp_origin`: a WebView event folded into
a mobile batch keeps `source:"browser"` while the request says `ios`/`android`.

## rum_events

```
MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, event_type, session_id, date)
TTL date + 30 DAY
```

Fed by `action`, `error`, `resource`, `long_task`, `vital` and `transition`
lines on `POST /api/v2/rum`. Append-only: unlike a view, each of these happened
once.

One table for six kinds, because they share the whole common block and differ
only in one sub-object. Per-kind columns:

- **action** — `action.type`, `action.target.name` (the label a user sees is
  there, not on `action` itself), `action.id`, and
  `action.frustration.type` as an **array**: one click can be a rage click *and*
  a dead click.
- **error** — `id`, `message`, `type`, `source`, `stack`, `is_crash`,
  `handling`, `fingerprint`. Crash and ANR reports arrive here as ordinary
  errors with `is_crash` set, generated at the *next* app launch — which is why
  their `date` can predate the session they belong to.
- **resource** — `id`, `type`, `url`, `method`, `status_code`, `duration`,
  `size`.
- **long_task** — `id`, `duration`.
- **vital** — `id`, `type` (the sub-discriminator: `duration`,
  `operation_step`, `app_launch`), `name`, `duration`.

`transition` is browser-only and fills no per-kind column; it is in `event`.

## rum_telemetry

```
MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, status, date)
TTL date + 90 DAY
```

`type:"telemetry"` arrives on the same `/api/v2/rum` track from every SDK,
including one configured for Logs only. It is not product data — it is how a
support question gets answered. `configuration` events carry the SDK's whole
resolved config (~150 keys that change every release, kept whole in
`configuration` rather than given columns), `usage` events name which API the
app actually called (`usage_feature`, a ~40-value enum), `error` events are the
SDK's own failures. Ninety days, the audit class: "what was this SDK configured
to do three months ago" outlives the data it produced.

Device and OS here come from `telemetry.device.*` / `telemetry.os.*`, not from
the top level — telemetry events have no top-level device block.

## rum_timeseries

```
MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, name, session_id, date)
TTL date + 14 DAY
```

`type:"timeseries"` is mobile-only (`cpu`, `memory`) and structurally unlike a
vital: one event carries a whole run of samples. `timestamps` stays an
`Array(Int64)` in wire order and `values` stays JSON, so the pairing between
`timestamps[i]` and `values.<name>[i]` survives; exploding it into one row per
sample would multiply the row count by the batch size for no query we have.

**`start`/`end` are `DateTime64(9)` — nanoseconds**, unlike `date`, which is
milliseconds. That is what the wire carries here.

## rum_replay_segments

```
MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, application_id, session_id, view_id, received_at)
TTL received_at + 14 DAY
```

Fed by `POST /api/v2/replay`. **The blobs are not decoded.** Datadog's own
intake stores segments "without any processing", and the browser SDK's encoder
deliberately emits zlib streams built to be *concatenated* later — inflating and
re-deflating would throw that property away, and the record schema (94 structs,
8 unions) is not something an intake should parse under load. `rumevents/replay`
models both formats for the day a viewer needs them; nothing at ingest does.

**One row per blob part, never per request.** The mobile SDKs send N segments in
one multipart body (one per view in the on-disk batch) with a parallel **array**
of metadata objects; entry *i* describes part *i*. Collapsing them would lose the
per-segment metadata.

Three shapes on one path:

| shape | parts | `variant` |
|---|---|---|
| browser segment | `segment` (zlib) + `event` (one object) | `segment` |
| mobile segment | `file0`…`fileN` (zlib) + `event` (array) | `segment` |
| canvas resource | `image` (raw, repeated on mobile) + `event` | `resource` |

`segment` holds the zlib bytes exactly as received; `image` holds the raw blob.
Both are `CODEC(NONE)` — ZSTD over a deflate stream costs CPU and saves nothing.
`event` is this blob's own metadata object as JSON. `part_name`/`part_filename`
record where in the envelope the blob sat.

This table sorts, partitions and expires on **arrival**, a deliberate exception
to the sender's-date rule: replay metadata has no `date`, and `start`/`end`
describe the recorded window and are absent entirely on the resource variant.

`index_in_view` is `Nullable` for a three-way reason: the browser sends a number,
Android sends an explicit `null` (an upstream TODO), iOS omits the key.

## rum_spans

```
MergeTree
PARTITION BY toDate(start)
ORDER BY (tenant_id, service, name, start)
TTL start + 14 DAY
```

Fed by `POST /api/v2/spans`: NDJSON of `{"spans":[...],"env":"..."}` envelopes.
This is **not** the agent's `/v0.4/traces` — it is a flat, hand-rolled span
shape produced only by dd-sdk-ios and dd-sdk-android, covered by no published
JSON Schema. It lives here because it shares a host, a credential and a sender
with mobile RUM, and shares nothing at all with the msgpack tracer payloads.

**The ids are `String`.** The SDKs encode `trace_id`/`span_id`/`parent_id` as
decimal or hex text of 64- and 128-bit numbers; parsing them would need a guess
about the base, and going anywhere near a `float64` silently drops digits above
2^53.

`start` is `DateTime64(9)` — nanoseconds, because rounding a span start to
milliseconds makes short spans overlap. `meta` is a plain `Map(String, String)`
(**not** the tag multiset shape: this is a JSON object, and a JSON object cannot
repeat a key) with `meta.device` and `meta.os` flattened to dotted keys, so
`meta['device.brand']` is reachable rather than dropped. `metrics` is
`Map(String, Float64)`. `envelope_extra` keeps whatever the envelope carried
besides `spans` and `env`, as JSON — usually empty, there so a key a future SDK
adds is stored rather than counted.

## Tags

`ddtags` travels **inside each event** for every SDK, not in the query string
the way the agent sends it. It is stored as
`Map(LowCardinality(String), Array(LowCardinality(String)))` with the
`bloom_filter` index pair on `mapKeys` and `arrayFlatten(mapValues)`, like
everywhere else — see `docs/decisions/0001-tags-are-a-multiset.md`. Query with
`has(ddtags['k'], 'v')`, not `ddtags['k'] = 'v'`.

## What does not decode

Per `intake/raw.go`, anything the router cannot turn into rows goes to
`raw_payloads` under the intake label `"rum"`, never to a log line alone:

| reason | when |
|---|---|
| `unknown_event` | a `/api/v2/rum` line whose `type` matches no variant — an SDK newer than this build. `note` is the type it claimed. **The rest of the batch is still stored.** |
| `decode_error` | a line that is not JSON, a span envelope whose `spans` is not an array, a replay `event` part that is neither object nor array |
| `unexpected_shape` | a replay body that is not multipart, a span envelope with no `spans` key |

Decode *success* paths never call `storeRaw` — this is a fallback, not a mirror
of the traffic.

## Sensitive columns

- `usr_id`, `usr_name`, `usr_email`, `usr_anonymous_id` identify the **person**
  (`session_id` identifies only the visit). Plain `String` with heavy ZSTD
  rather than `LowCardinality`: a dictionary of every user id is a dictionary
  that never stops growing.
- `remote_addr` is the browser's only network identity — kept because it is the
  one input a geo lookup would need later (the SDKs send no geo at all) and
  because it is how abuse of a public, write-only credential is traced.
- `context` and `feature_flags` are application-supplied and can hold anything
  the page put there.
- `segment` / `image` are a recording of a real user's screen.
- `query_extra` and the header allowlist in `intake/raw.go` both strip
  credentials by construction.

## dropped_by_decision

Everything the handler decodes reaches a column, a structured column or the raw
`event`/`span`/`segment` column. These are the things deliberately **not** given
their own typed column or their own table:

1. **Session Replay record contents.** Stored as the zlib bytes that arrived,
   never inflated or parsed. Reason above; `rumevents/replay` exists for the day
   a viewer needs typed records. Nothing is lost — the bytes are the record.
2. **A typed column per RUM field.** Each variant has 100+ optional fields
   (`view.in_foreground_periods`, `error.threads`, `error.binary_images`,
   `resource.graphql`, `_dd.page_states`, `display.viewport`, `synthetics`,
   `ci_test`, `tab`, `container`, `privacy`, `accessibility`, `custom_timings`,
   …). They are in `event`, complete, and gain a column when a query needs one.
3. **`@session.frustration.count`.** Datadog shows it but no SDK sends it: it is
   a server-side rollup of `view.frustration.count` grouped by `session.id`. If
   we want it, we compute it — inventing a column would imply the wire carries
   something it does not.
4. **A session table.** Sessions are derivable from `rum_views` /`rum_events`
   (`session_id` is on every row) and the SDKs send no session object of their
   own. Materialising one before a query asks for it would be a rollup with no
   reader.
5. **`/api/v2/logs`, `/api/v2/profile`, `/api/v2/debugger` on this host** reuse
   the existing handlers and their existing tables. The SDK-specific gaps in
   those decoders (browser logs are NDJSON where the agent sends an array;
   `date` is a number on browser/iOS and an ISO-8601 string on Android, while
   `HandleLogs` reads `timestamp`) belong to those routers, not this one — they
   are listed as open items in `docs/rum-sdk-setup.md`.
6. **The exploded timeseries sample.** `timestamps` and `values` stay arrays; see
   `rum_timeseries` above.
7. **A parsed `retry_after`.** Stored as the integer sent, because the browser's
   milliseconds and the mobile SDKs' HTTP status are two different meanings for
   one field and normalising would pick one and lose the other. The platform is
   in `evp_origin` beside it.
8. **`Access-Control-*` request headers and the `Origin` header** are not stored
   per row. They describe the CORS handshake, not the telemetry; `remote_addr`
   and `user_agent` already identify the sender.
