# metrics v3 — the columnar intake on `app.<site>`

What `intake/router_app.go` stores, and what it deliberately does not.

Three routes share one handler and one payload type
(`intake_v3.Payload`, `github.com/DataDog/agent-payload/v5/metrics/intake_v3`):

| route | agent config that turns it on |
|---|---|
| `POST /api/intake/metrics/v3/series` | `use_v3_api.series.enabled: true` (default `datadog_only` keeps a third-party `dd_url` on `/api/v2/series`) |
| `POST /api/intake/metrics/v3/sketches` | the URL listed under `serializer_experimental_use_v3_api.sketches.endpoints` |
| `POST /api/intake/metrics/v3beta/sketches` | a shadow copy of v2 sketch traffic; route overridable via `...sketches.beta_route` |

Series and sketches are not different payloads. They differ only in the
`metricType` nibble of each `Types` entry, so all three routes decode
identically and the label only changes the log prefix and the `intake` column
of a raw payload.

The response contract is unchanged: **`202` with `{}`**, whatever happens to
the body. Nothing a sender does makes this handler answer anything else.

## Tables

**No new table, and no migration.** v3 is the same signal as v1/v2 in a denser
encoding, so it lands in the two tables that already hold it. A v3 series and a
v2 series of the same metric are indistinguishable once stored, which is the
point — a dashboard cannot ask which intake a point arrived on.

| table | engine | ORDER BY | TTL | fed by |
|---|---|---|---|---|
| `metrics` | `MergeTree` | `(tenant_id, metric, host, timestamp)` | 30 days | v3 `Count` / `Rate` / `Gauge` series, via `storage.MetricsWriter` (`WriteMetrics{Points}`) |
| `sketches` | `MergeTree` | `(tenant_id, metric, host, timestamp)` | 30 days | v3 `Sketch` series, via `storage.SketchesWriter` (`WriteSketches{Sketches}`) |
| `raw_payloads` | `MergeTree` | `(tenant_id, intake, received_at)` | 30 days | bodies this handler could not turn into rows, via `a.storeRaw` |

Both writers keep the tuning migration `0001` sized them for: `metrics` 5000
rows / 2 s, `sketches` 1000 rows / 5 s. v3 changes the encoding, not the
volume class.

## Column mapping

One row per **point**, not per series: a v3 series carries `NumPoints[i]`
points and each one is its own row, exactly as `Dogsketches` are in v2.

### `metrics` ← a `Count` / `Rate` / `Gauge` series

| column | source |
|---|---|
| `tenant_id` | `TenantFromContext(c)`, from the API key |
| `timestamp` | `Timestamps`, delta-accumulated across the whole array, through `wireTime` |
| `metric` | `DictNameStr[nameRef]` |
| `host` | the `host` pair of the series' resource set, else the `host` pair in `Metadata.Resources` |
| `metric_type` | `Types[i] & 0xF` through `metricTypeName` — the v3 numbers for Count/Rate/Gauge are the v2 ones, so the word is the same on both intakes |
| `source_type` | `DictSourceTypeName[sourceTypeNameRef]` |
| `unit` | `DictUnitStr[unitRef]`, only for a series carrying `flagHasUnit` (see below) |
| `interval` | `Intervals[i]` |
| `value` | `ValsSint64` / `ValsFloat32` / `ValsFloat64`, selected by the ValueType nibble; `ValueType_Zero` is a zero that is not on the wire at all |
| `tags` | the series' tagset **plus** `Metadata.Tags`, through `tagsToMultiMap` |

### `sketches` ← a `Sketch` series

| column | source |
|---|---|
| `tenant_id`, `timestamp`, `metric`, `host`, `tags` | as above |
| `count` | always one entry of `ValsSint64`, whatever the summary's value type |
| `sum`, `min`, `max` | three **consecutive** entries of the ValueType-selected column, in that order |
| `avg` | not on the wire — reconstructed as `sum / count`, which is what Datadog's own intake does |
| `bucket_keys` | `SketchBinKeys`, delta-decoded **per point** |
| `bucket_counts` | `SketchBinCnts`, verbatim |

## Semantics worth knowing

**Reference columns are base-1.** Index 0 is the implicit empty value in every
dictionary, so a series with no name, no tags and no resources is legal, not an
error, and `dict[0]` must be `""` rather than the first real entry.

**Delta encoding is not uniform, and this is the part that silently corrupts
data when read wrong.** `NameRefs`, `TagsetRefs`, `ResourcesRefs`,
`SourceTypeNameRefs`, `OriginInfoRefs`, `UnitRefs` and `Timestamps` accumulate
across the **whole** array. Resource `Type`/`Name` accumulators restart at each
resource **set**. `SketchBinKeys` restarts at each **point**. Getting the
resource rule wrong renames every host after the first, and nothing about the
result looks broken.

**A tagset can include another tagset.** In `DictTagsets` a negative
accumulated index is not a tag index: it is an earlier tagset, spliced in
whole. That is how the agent avoids repeating the tags every series of a host
shares, and a reader that treats it as an out-of-range tag index loses most of
the tags in a real payload.

**Tags are a multiset here too.** The series' own tags and the payload-wide
`Metadata.Tags` are concatenated before `tagsToMultiMap`, so `env:prod` on the
series and `env:staging` on the payload become `env: [prod, staging]` rather
than one overwriting the other — see `docs/decisions/0001-tags-are-a-multiset.md`.

**A zero timestamp means "not given", not 1970.** It goes through `wireTime`
like every other intake, so the row gets arrival time. Once stored, "the sender
sent 0" and "the sender sent nothing" are indistinguishable — the same
documented limitation the v1/v2/logs paths have.

**`UnitRefs` is the one ambiguous column in the format.** The proto says only
"value present if flagHasUnit is set, entire array is delta encoded", which
reads either as one entry per series or one entry per *flagged* series, and the
agent's reference reader predates the column entirely. Picking wrong puts every
unit on the wrong metric, so the handler lets the length decide per payload:
`len(UnitRefs) == len(Types)` is read as one per series, `len(UnitRefs) ==
<flagged series>` as one per flagged series, and anything else drops the units
and sends the payload to `raw_payloads`. When every series is flagged — the
common case — the two readings coincide. If a real agent ever produces the
third case, the stored payload settles the question.

### Raw columns: when a v3 body reaches `raw_payloads`

| `reason` | when |
|---|---|
| `decode_error` | `proto.Unmarshal` refused the body |
| `unexpected_shape` | the payload decoded to zero series (usually a still-compressed body — `proto.Unmarshal` fails open on this format), an odd `Metadata.Resources` list, a per-series column shorter than `Types`, a reference outside its dictionary, a truncated dictionary, an unreadable `UnitRefs` length, or a value column that ran out mid-walk |
| `int64_precision` | the walk widened a `sint64` past 2^53 into the `Float64` value column — the rows are still written, rounded, and the payload keeps the exact integers |

`int64_precision` extends the three-word vocabulary migration `0002` documents
for `raw_payloads.reason`. The column is a plain `LowCardinality(String)`, so
this needs no schema change, but the comment in `0002_raw_payloads.sql` now
lists one reason fewer than the code produces.

A **partial** walk failure stores both: the rows it managed to read go to their
tables, and the body goes to `raw_payloads`. Losing forty good series because
the forty-first column is short is the failure this table exists to prevent.
A clean decode never writes a raw payload — that table is a fallback, not a
mirror of the traffic.

No column here is sensitive. The raw body is the payload as the handler saw it
(decompressed), and the header allowlist in `intake/raw.go` keeps `Dd-Api-Key`
and `Authorization` out of it.

## dropped_by_decision

| what | why |
|---|---|
| **origin info** (`DictOriginInfo` product / category / service) | Neither `metrics` nor `sketches` has an origin column today, and another branch is adding them. The triple is decoded and tallied into the log line (`origins (product/category/service, no column yet)`), not invented into a tag. |
| **non-host resources** (`device`, and anything else in a resource set) | No column on either table — same as the v2 path, which tallies them the same way. The tally names every distinct `type=name`, so adding a column later is a schema change, not an archaeology exercise. |
| **`flagNoIndex`** | The agent's "do not index this metric" hint (v2's `agent_hidden` origin metric type). It is a query-time policy with no column on `metrics`; counted per batch in the log, exactly as the v2 handler counts the reserved `Origin` field. |
| **sketch `interval`, `source_type` and `unit`** | `SketchRow` has none of the three, though a v3 sketch series carries all of them in its per-series columns. Decoded and then dropped; adding them means widening the `sketches` table, which is not this branch's to widen. |
| **`Types` bits outside the three nibbles** | Only `metricType` (`0xF`), `valueType` (`0xF0`), `flagNoIndex` and `flagHasUnit` are defined. An unknown bit is ignored rather than treated as an error; an unknown *valueType* nibble is a shape error and keeps the payload. |
| **v3 has no `Distributions`** | Worth stating because v2 does: the pre-DDSketch encoding `gogen.SketchPayload` still carries does not exist in v3, so nothing is lost on this route by `SketchRow` being keyed on DDSketch buckets. |
| **Sender identity** | v3 has no `CommonMetadata`: no agent version, timezone, epoch or IPs anywhere in the payload. There is nothing to drop. The request headers that do identify the sender are kept on any raw payload through `intake/raw.go`'s allowlist. |
