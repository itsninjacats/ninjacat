//! app.<site>: what api.<site> serves, and columnar metrics, intake v3.
//!
//!   agent config: dd_url, plus use_v3_api.series.enabled: true. The default, "datadog_only",
//!   keeps a third-party dd_url on /api/v2/series (api.rs). Sketches need the URL listed under
//!   serializer_experimental_use_v3_api.sketches.endpoints.
//!
//! Three routes, one message: intake_v3.Payload. Series and sketches differ only in the metricType
//! nibble of each Types entry, and land in the same two tables the v1/v2 handlers feed: it is the
//! same signal, denser.
//!
//! The body is not one compressed blob. The agent compresses every column as its own zstd/gzip
//! frame and concatenates them, relying on the decompressor to splice consecutive frames. Length
//! prefixes inside describe the UNCOMPRESSED data, so the body is decompressed first and parsed
//! second.
//!
//! A body that does not decode, a payload with no series, a per-series column that is short or
//! missing, and a walk that runs off the end of a value column all go to raw_payloads.

use ninjapaws_proto::datadoghq::api::metrics::v3::{MetricFlags, MetricType, Payload, ValueType};
use ninjapaws_storage as storage;
use ninjapaws_storage::rows::metrics::{MetricPoint, SketchRow};
use prost::Message;
use tako::extractors::state::State;
use tako::types::Response;

use crate::raw;
use crate::server::{Body, Ctx, accepted, storage_failed};
use crate::wire::{metric_type_name, tags, time, varint};

/// The two ways a column can be wrong. Notes in raw_payloads quote them.
pub const TRUNCATED: &str = "column ends mid-element";
pub const BAD_REFERENCE: &str = "reference outside its dictionary";

const SKETCH_TYPE: u64 = MetricType::Sketch as u64;
const ZERO_VALUE: u64 = ValueType::Zero as u64;
const SINT64_VALUE: u64 = ValueType::Sint64 as u64;
const FLOAT32_VALUE: u64 = ValueType::Float32 as u64;
const FLOAT64_VALUE: u64 = ValueType::Float64 as u64;
const FLAG_HAS_UNIT: u64 = MetricFlags::FlagHasUnit as u64;

/// The largest integer a Float64 holds exactly. The value column is a Float64, so a sint64 past
/// this is rounded on the way in.
const MAX_EXACT_INT: i64 = 1 << 53;

/// A "varint length + bytes" string dictionary.
///
/// Every reference column is base-1 with the empty string implicit at 0, so the array is laid out
/// to be indexed directly. Truncated input gives the entries that were read AND the problem: the
/// good entries are still used, and the payload is kept raw.
pub fn str_dict(raw: &[u8]) -> (Vec<String>, Option<String>) {
    let mut entries = vec![String::new()];
    let mut at = 0;
    while at < raw.len() {
        match varint::read(raw, at) {
            Some((length, taken)) if length <= (raw.len() - at - taken) as u64 => {
                let start = at + taken;
                entries.push(String::from_utf8_lossy(&raw[start..start + length as usize]).into_owned());
                at = start + length as usize;
            }
            _ => return (entries, Some(TRUNCATED.into())),
        }
    }
    (entries, None)
}

/// A base-1 dictionary entry, or None for a reference outside it.
fn lookup<T: Clone>(dict: &[T], reference: i64) -> Option<T> {
    usize::try_from(reference).ok().and_then(|i| dict.get(i)).cloned()
}

/// The tagset dictionary: a flat sequence of "length, then that many delta-encoded indexes into
/// the tag strings".
///
/// The delta accumulator restarts at every set. A NEGATIVE accumulated index is not a tag but an
/// earlier tagset, included whole: that is how the agent avoids repeating the tags every series of
/// a host shares. Entry 0 is the implicit empty set, so back-references are base-1 as well.
pub fn tagsets(packed: &[i64], tag_str: &[String]) -> (Vec<Vec<String>>, Option<String>) {
    let mut sets: Vec<Vec<String>> = vec![Vec::new()];
    let mut at = 0;
    while at < packed.len() {
        let size = packed[at];
        at += 1;
        if size < 0 || size > (packed.len() - at) as i64 {
            return (sets, Some(TRUNCATED.into()));
        }
        let mut set = Vec::new();
        let mut index: i64 = 0;
        for delta in &packed[at..at + size as usize] {
            index = index.wrapping_add(*delta);
            if index < 0 {
                if index == i64::MIN || -index >= sets.len() as i64 {
                    return (sets, Some(BAD_REFERENCE.into()));
                }
                set.extend(sets[(-index) as usize].iter().cloned());
            } else if index >= tag_str.len() as i64 {
                return (sets, Some(BAD_REFERENCE.into()));
            } else {
                set.push(tag_str[index as usize].clone());
            }
        }
        sets.push(set);
        at += size as usize;
    }
    (sets, None)
}

/// One [Type, Name] pair of a resource set.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub kind: String,
    pub name: String,
}

/// The resource-set dictionary: `lens` gives the size of each set, `type_col` and `name_col` are
/// parallel delta-encoded columns covering all sets end to end.
///
/// The accumulators restart at each SET: the one delta column of this format that does not run
/// across the whole array. Read the other way, every host after the first gets the wrong name.
pub fn resources(
    lens: &[i64],
    type_col: &[i64],
    name_col: &[i64],
    strs: &[String],
) -> (Vec<Vec<Resource>>, Option<String>) {
    let mut sets: Vec<Vec<Resource>> = vec![Vec::new()];
    let mut start = 0usize;
    for size in lens {
        let size = *size;
        if size < 0 || size > (type_col.len() - start) as i64 || size > (name_col.len() - start) as i64 {
            return (sets, Some(TRUNCATED.into()));
        }
        let mut set = Vec::new();
        let mut type_ref: i64 = 0;
        let mut name_ref: i64 = 0;
        for i in 0..size as usize {
            type_ref = type_ref.wrapping_add(type_col[start + i]);
            name_ref = name_ref.wrapping_add(name_col[start + i]);
            match (lookup(strs, type_ref), lookup(strs, name_ref)) {
                (Some(kind), Some(name)) => set.push(Resource { kind, name }),
                _ => return (sets, Some(BAD_REFERENCE.into())),
            }
        }
        sets.push(set);
        start += size as usize;
    }
    (sets, None)
}

/// One entry of DictOriginInfo. The numbers are Datadog's private origin.proto enums, the same
/// ones a v2 series carries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Origin {
    pub product: u32,
    pub category: u32,
    pub service: u32,
}

/// DictOriginInfo: flattened (product, category, service) triples, base-1 like every other
/// dictionary.
pub fn origins(raw: &[i32]) -> (Vec<Origin>, Option<String>) {
    let mut triples = vec![Origin::default()];
    for triple in raw.as_chunks::<3>().0 {
        triples.push(Origin { product: triple[0] as u32, category: triple[1] as u32, service: triple[2] as u32 });
    }
    let problem = (!raw.len().is_multiple_of(3)).then(|| TRUNCATED.to_string());
    (triples, problem)
}

/// How the UnitRefs column is laid out.
///
/// THE ONE AMBIGUITY OF THIS FORMAT. The proto says only "value present if flagHasUnit is set,
/// entire array is delta encoded", which reads either way: one entry per series, like every other
/// ref column, or one entry per FLAGGED series. The agent's reference reader predates the column
/// and settles nothing, and choosing wrong shifts every unit of the payload onto the wrong metric.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnitLayout {
    NoUnits,
    /// One entry per series.
    Parallel,
    /// One entry per series with flagHasUnit.
    Compact,
}

/// The length decides, per payload. A length that fits neither reading means no units at all and
/// a trip to raw_payloads: a wrong unit is worse than a missing one.
pub fn unit_layout(types: &[u64], refs: usize) -> (UnitLayout, Option<String>) {
    if refs == 0 {
        return (UnitLayout::NoUnits, None);
    }
    let flagged = types.iter().filter(|packed| *packed & FLAG_HAS_UNIT != 0).count();
    if refs == types.len() {
        (UnitLayout::Parallel, None)
    } else if refs == flagged {
        (UnitLayout::Compact, None)
    } else {
        (
            UnitLayout::NoUnits,
            Some(format!(
                "unitRefs has {refs} entries: neither one per series ({}) nor one per flagHasUnit series ({flagged}), units dropped",
                types.len()
            )),
        )
    }
}

/// One payload turned into rows.
#[derive(Debug, Default)]
pub struct Decoded {
    pub points: Vec<MetricPoint>,
    pub sketches: Vec<SketchRow>,
    /// sint64 values beyond 2^53, stored rounded.
    pub wide_ints: usize,
    /// The FIRST inconsistency the walk met. It never stops the rows already decoded from being
    /// stored; it means the payload ALSO goes to raw_payloads, so the part that could not be read
    /// is not gone.
    pub problem: Option<String>,
}

/// A sketch point's sum, min and max, which share one value type.
struct SketchTotals {
    sum: f64,
    min: f64,
    max: f64,
}

struct SketchPoint {
    keys: Vec<i32>,
    counts: Vec<u32>,
    totals: SketchTotals,
    count: u64,
}

/// Where the walk stands in the columns all series share. A short column therefore spoils the
/// rest of the walk, not one series: the walk stops at the first one.
struct Walk<'a> {
    vals_sint64: &'a [i64],
    vals_float32: &'a [f32],
    vals_float64: &'a [f64],
    num_bins: &'a [u64],
    bin_keys: &'a [i32],
    bin_counts: &'a [u32],
    sint64: usize,
    float32: usize,
    float64: usize,
    bins_seen: usize,
    bins: usize,
    wide_ints: usize,
    problem: Option<String>,
}

impl Walk<'_> {
    fn fail(&mut self, text: String) {
        fail(&mut self.problem, text);
    }

    fn next_sint64(&mut self) -> Option<f64> {
        let value = *self.vals_sint64.get(self.sint64)?;
        self.sint64 += 1;
        if !(-MAX_EXACT_INT..=MAX_EXACT_INT).contains(&value) {
            self.wide_ints += 1;
        }
        Some(value as f64)
    }

    fn next_float32(&mut self, n: usize) -> Option<&[f32]> {
        let values = self.vals_float32.get(self.float32..self.float32 + n)?;
        self.float32 += n;
        Some(values)
    }

    fn next_float64(&mut self, n: usize) -> Option<&[f64]> {
        let values = self.vals_float64.get(self.float64..self.float64 + n)?;
        self.float64 += n;
        Some(values)
    }

    /// A point's value, or the note of the column that ran out.
    fn scalar(&mut self, value_type: u64, series: usize, point: u64) -> Result<f64, String> {
        match value_type {
            ZERO_VALUE => Ok(0.0),
            SINT64_VALUE => self
                .next_sint64()
                .ok_or_else(|| format!("valsSint64 ran out at series {series} point {point}: {TRUNCATED}")),
            FLOAT32_VALUE => self
                .next_float32(1)
                .map(|v| f64::from(v[0]))
                .ok_or_else(|| format!("valsFloat32 ran out at series {series} point {point}: {TRUNCATED}")),
            FLOAT64_VALUE => self
                .next_float64(1)
                .map(|v| v[0])
                .ok_or_else(|| format!("valsFloat64 ran out at series {series} point {point}: {TRUNCATED}")),
            _ => {
                self.fail(format!("series {series}: unknown valueType 0x{value_type:x}"));
                Ok(0.0)
            }
        }
    }

    fn summary(&mut self, value_type: u64, series: usize, point: u64) -> Result<SketchTotals, String> {
        let totals = |v: [f64; 3]| SketchTotals { sum: v[0], min: v[1], max: v[2] };
        match value_type {
            ZERO_VALUE => Ok(totals([0.0; 3])),
            SINT64_VALUE => {
                let sum = self.next_sint64();
                let min = sum.and_then(|_| self.next_sint64());
                let max = min.and_then(|_| self.next_sint64());
                match (sum, min, max) {
                    (Some(sum), Some(min), Some(max)) => Ok(totals([sum, min, max])),
                    _ => Err(format!(
                        "valsSint64 ran out in a sketch summary at series {series} point {point}: {TRUNCATED}"
                    )),
                }
            }
            FLOAT32_VALUE => {
                self.next_float32(3).map(|v| totals([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])).ok_or_else(
                    || format!("valsFloat32 ran out in a sketch summary at series {series} point {point}: {TRUNCATED}"),
                )
            }
            FLOAT64_VALUE => self.next_float64(3).map(|v| totals([v[0], v[1], v[2]])).ok_or_else(|| {
                format!("valsFloat64 ran out in a sketch summary at series {series} point {point}: {TRUNCATED}")
            }),
            _ => {
                self.fail(format!("series {series}: unknown valueType 0x{value_type:x}"));
                Ok(totals([0.0; 3]))
            }
        }
    }

    fn sketch_point(&mut self, value_type: u64, series: usize, point: u64) -> Result<SketchPoint, String> {
        let Some(&bins) = self.num_bins.get(self.bins_seen) else {
            return Err(format!("sketchNumBins ran out at series {series} point {point}: {TRUNCATED}"));
        };
        self.bins_seen += 1;
        if bins > (self.bin_keys.len() - self.bins) as u64 || bins > (self.bin_counts.len() - self.bins) as u64 {
            return Err(format!("sketch bins ran out at series {series} point {point}: {TRUNCATED}"));
        }
        let bins = bins as usize;
        // The keys are delta-encoded per point: each starts from zero.
        let mut key: i32 = 0;
        let keys = self.bin_keys[self.bins..self.bins + bins]
            .iter()
            .map(|delta| {
                key = key.wrapping_add(*delta);
                key
            })
            .collect();
        let counts = self.bin_counts[self.bins..self.bins + bins].to_vec();
        self.bins += bins;

        let totals = self.summary(value_type, series, point)?;
        // The count is ALWAYS a sint64, whatever the summary's value type: the one part of the
        // sketch layout the ValueType nibble does not tell.
        let count = self.next_sint64().ok_or_else(|| {
            format!("valsSint64 ran out reading a sketch count at series {series} point {point}: {TRUNCATED}")
        })?;
        Ok(SketchPoint { keys, counts, totals, count: if count > 0.0 { count as u64 } else { 0 } })
    }
}

/// Only the first problem is kept: it is the one the later ones follow from.
fn fail(problem: &mut Option<String>, text: String) {
    if problem.is_none() {
        *problem = Some(text);
    }
}

/// A dictionary's entries; what was wrong with it, prefixed by its column, is the problem.
fn keep<T>(problem: &mut Option<String>, column: &str, (entries, found): (Vec<T>, Option<String>)) -> Vec<T> {
    if let Some(text) = found {
        fail(problem, format!("{column}: {text}"));
    }
    entries
}

/// Walks the columnar payload and builds the rows.
///
/// The encoding has no published reader outside the agent, so the walk follows the agent's own
/// MetricDataReader (comp/dogstatsd/http/impl/internal/reader), because these rules cannot be
/// guessed from the .proto:
///
///   - every dictionary index is base-1, the empty value implicit at 0;
///   - delta columns accumulate across the whole array, except the resource Type/Name columns
///     (restart per set) and SketchBinKeys (per POINT);
///   - the value column is chosen by the ValueType nibble, and Zero consumes nothing at all;
///   - a sketch point spends three consecutive entries of its value column on sum, min, max and
///     always one entry of ValsSint64 on the count. Avg is not on the wire; the intake
///     reconstructs it as sum/count.
///
/// It is deliberately more forgiving than the agent's reader, which drops the payload on the first
/// error: losing forty good series because the forty-first column is short is the failure this
/// project exists to avoid.
pub fn decode(tenant: &str, payload: &Payload) -> Decoded {
    let md = payload.metric_data.clone().unwrap_or_default();
    let metadata = payload.metadata.clone().unwrap_or_default();
    let mut problem: Option<String> = None;

    // A truncated dictionary is used as far as it goes: the series that reference the part that
    // was read are still good rows.
    let names = keep(&mut problem, "dictNameStr", str_dict(&md.dict_name_str));
    let tag_str = keep(&mut problem, "dictTagStr", str_dict(&md.dict_tag_str));
    let tagset_dict = keep(&mut problem, "dictTagsets", tagsets(&md.dict_tagsets, &tag_str));
    let resource_str = keep(&mut problem, "dictResourceStr", str_dict(&md.dict_resource_str));
    let resource_sets = keep(
        &mut problem,
        "dictResource",
        resources(&md.dict_resource_len, &md.dict_resource_type, &md.dict_resource_name, &resource_str),
    );
    let source_types = keep(&mut problem, "dictSourceTypeName", str_dict(&md.dict_source_type_name));
    let units = keep(&mut problem, "dictUnitStr", str_dict(&md.dict_unit_str));
    let origin_dict = keep(&mut problem, "dictOriginInfo", origins(&md.dict_origin_info));

    // Metadata.Tags apply to EVERY series, and Metadata.Resources carries the host of a payload
    // whose series do not name one themselves.
    let meta_tags = &metadata.tags;
    let meta_resources = &metadata.resources;
    let meta_host = meta_resources
        .chunks(2)
        .find(|pair| pair.len() == 2 && pair[0] == "host" && !pair[1].is_empty())
        .map(|pair| pair[1].clone())
        .unwrap_or_default();
    if !meta_resources.len().is_multiple_of(2) {
        fail(
            &mut problem,
            format!("metadata.resources has {} elements, want [Type, Name] pairs", meta_resources.len()),
        );
    }

    let types = &md.types;

    // Each of these carries one entry per series, unconditionally: the agent's reader refuses the
    // whole payload when any of them is shorter, an empty one included. Reading an empty column as
    // "empty for every series" corrupts silently: no nameRefs files every series under the empty
    // name, no numPoints walks no points and acks a body that contributed nothing. unitRefs is the
    // one column that may be absent.
    for (column, length) in [
        ("nameRefs", md.name_refs.len()),
        ("tagsetRefs", md.tagset_refs.len()),
        ("resourcesRefs", md.resources_refs.len()),
        ("numPoints", md.num_points.len()),
        ("sourceTypeNameRefs", md.source_type_name_refs.len()),
        ("originInfoRefs", md.origin_info_refs.len()),
        ("intervals", md.intervals.len()),
    ] {
        if length < types.len() {
            fail(&mut problem, format!("{column} has {length} entries for {} series", types.len()));
        }
    }

    let (layout, layout_problem) = unit_layout(types, md.unit_refs.len());
    if let Some(text) = layout_problem {
        fail(&mut problem, text);
    }

    let mut walk = Walk {
        vals_sint64: &md.vals_sint64,
        vals_float32: &md.vals_float32,
        vals_float64: &md.vals_float64,
        num_bins: &md.sketch_num_bins,
        bin_keys: &md.sketch_bin_keys,
        bin_counts: &md.sketch_bin_cnts,
        sint64: 0,
        float32: 0,
        float64: 0,
        bins_seen: 0,
        bins: 0,
        wide_ints: 0,
        problem,
    };

    let mut decoded = Decoded::default();

    // Accumulators of the columns delta-encoded across the whole array.
    let (mut name_ref, mut tagset_ref, mut resource_ref, mut source_ref, mut origin_ref, mut unit_ref) =
        (0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
    let mut timestamp: i64 = 0;
    let mut point_at = 0usize;
    let mut unit_at = 0usize;

    // A column too short for the series array was reported above; here it only has to give a
    // neutral delta.
    let delta = |column: &[i64], series: usize| column.get(series).copied().unwrap_or(0);

    'series: for (i, packed) in types.iter().enumerate() {
        let kind = packed & 0xF;
        let value_type = packed & 0xF0;

        name_ref = name_ref.wrapping_add(delta(&md.name_refs, i));
        tagset_ref = tagset_ref.wrapping_add(delta(&md.tagset_refs, i));
        resource_ref = resource_ref.wrapping_add(delta(&md.resources_refs, i));
        source_ref = source_ref.wrapping_add(delta(&md.source_type_name_refs, i));
        origin_ref = origin_ref.wrapping_add(delta(&md.origin_info_refs, i));

        let metric = lookup(&names, name_ref).unwrap_or_else(|| {
            walk.fail(format!("series {i} nameRef {name_ref}: {BAD_REFERENCE}"));
            String::new()
        });
        let series_tags = lookup(&tagset_dict, tagset_ref).unwrap_or_else(|| {
            walk.fail(format!("series {i} tagsetRef {tagset_ref}: {BAD_REFERENCE}"));
            Vec::new()
        });
        // A key may appear in both lists; the multiset keeps both values.
        let tag_map = tags::to_multi_map(series_tags.iter().chain(meta_tags));

        let host = match lookup(&resource_sets, resource_ref) {
            None => {
                walk.fail(format!("series {i} resourcesRef {resource_ref}: {BAD_REFERENCE}"));
                meta_host.clone()
            }
            Some(set) => {
                set.iter().rev().find(|r| r.kind == "host").map_or_else(|| meta_host.clone(), |r| r.name.clone())
            }
        };

        let source_type = lookup(&source_types, source_ref).unwrap_or_else(|| {
            walk.fail(format!("series {i} sourceTypeNameRef {source_ref}: {BAD_REFERENCE}"));
            String::new()
        });

        // Checked but not stored: a v3 row carries neither its origin nor its non-host resources
        // yet.
        if lookup(&origin_dict, origin_ref).is_none() {
            walk.fail(format!("series {i} originInfoRef {origin_ref}: {BAD_REFERENCE}"));
        }

        let has_unit = packed & FLAG_HAS_UNIT != 0;
        let unit = match layout {
            UnitLayout::Parallel => {
                unit_ref = unit_ref.wrapping_add(md.unit_refs[i]);
                if has_unit { lookup(&units, unit_ref).unwrap_or_default() } else { String::new() }
            }
            UnitLayout::Compact if has_unit && unit_at < md.unit_refs.len() => {
                unit_ref = unit_ref.wrapping_add(md.unit_refs[unit_at]);
                unit_at += 1;
                lookup(&units, unit_ref).unwrap_or_default()
            }
            UnitLayout::Compact | UnitLayout::NoUnits => String::new(),
        };

        let interval = md.intervals.get(i).map_or(0, |v| *v as u32);
        let point_count = md.num_points.get(i).copied().unwrap_or(0);

        for p in 0..point_count {
            let Some(step) = md.timestamps.get(point_at) else {
                walk.fail(format!("timestamps ran out at series {i} point {p}: {TRUNCATED}"));
                break 'series;
            };
            timestamp = timestamp.wrapping_add(*step);
            point_at += 1;
            let at = time::wire_seconds(timestamp);

            if kind != SKETCH_TYPE {
                match walk.scalar(value_type, i, p) {
                    Err(text) => {
                        walk.fail(text);
                        break 'series;
                    }
                    Ok(value) => decoded.points.push(MetricPoint {
                        host: host.clone(),
                        // The v3 numbers are the v2 ones for Count, Rate and Gauge.
                        metric_type: metric_type_name(kind as i64).into(),
                        source_type: source_type.clone(),
                        unit: unit.clone(),
                        interval,
                        tags: tag_map.clone(),
                        ..MetricPoint::new(tenant, at, &metric, value)
                    }),
                }
            } else {
                match walk.sketch_point(value_type, i, p) {
                    Err(text) => {
                        walk.fail(text);
                        break 'series;
                    }
                    Ok(sketch) => decoded.sketches.push(SketchRow {
                        tenant_id: tenant.into(),
                        timestamp: at,
                        metric: metric.clone(),
                        host: host.clone(),
                        tags: tag_map.clone(),
                        count: sketch.count,
                        min: sketch.totals.min,
                        max: sketch.totals.max,
                        avg: if sketch.count > 0 { sketch.totals.sum / sketch.count as f64 } else { 0.0 },
                        sum: sketch.totals.sum,
                        bucket_keys: sketch.keys,
                        bucket_counts: sketch.counts,
                        ..Default::default()
                    }),
                }
            }
        }
    }

    decoded.wide_ints = walk.wide_ints;
    decoded.problem = walk.problem;
    decoded
}

/// `label` names the route in raw_payloads and in the log.
pub async fn handle(label: &str, ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    let payload = match Payload::decode(&body[..]) {
        Err(message) => {
            raw::store(&clickhouse, &ctx, label, "decode_error", &message.to_string(), &body).await;
            return accepted();
        }
        Ok(payload) => payload,
    };
    let series = payload.metric_data.as_ref().map_or(0, |md| md.types.len());
    if series == 0 {
        // A protobuf parser fails OPEN: a body that is still compressed, or meant for another
        // intake, often parses as a valid Payload with no columns. The bytes are the only
        // evidence of what the sender meant.
        let tags = payload.metadata.as_ref().map_or(0, |m| m.tags.len());
        let resources = payload.metadata.as_ref().map_or(0, |m| m.resources.len());
        tracing::warn!(
            "[{label}] decoded to zero series ({} bytes): wrong payload type or still compressed?",
            body.len()
        );
        raw::store(
            &clickhouse,
            &ctx,
            label,
            "unexpected_shape",
            &format!("protobuf decoded but MetricData carries no series; metadata tags={tags} resources={resources}"),
            &body,
        )
        .await;
        return accepted();
    }

    let decoded = decode(ctx.tenant(), &payload);
    if let Err(e) = storage::insert(&clickhouse, &decoded.points).await {
        return storage_failed(e);
    }
    if let Err(e) = storage::insert(&clickhouse, &decoded.sketches).await {
        return storage_failed(e);
    }

    // The rows above are what the walk could read; the two notes below keep what it could not. A
    // payload that decodes cleanly is never mirrored into raw_payloads.
    if let Some(problem) = &decoded.problem {
        raw::store(&clickhouse, &ctx, label, "unexpected_shape", problem, &body).await;
    }
    if decoded.wide_ints > 0 {
        // The row is still written (a rounded value beats no value) and the payload is kept, so
        // the exact integer stays recoverable.
        tracing::warn!("[{label}] {} sint64 values beyond 2^53: stored rounded, payload kept raw", decoded.wide_ints);
        raw::store(
            &clickhouse,
            &ctx,
            label,
            "int64_precision",
            &format!("{} sint64 values beyond 2^53 widened to Float64", decoded.wide_ints),
            &body,
        )
        .await;
    }
    accepted()
}
