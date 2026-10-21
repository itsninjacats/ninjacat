//! From ClickHouse rows to the response: series, the shared time axis, limits.
//!
//! Gaps stay gaps. ClickHouse returns only buckets that had points, and a series with nothing in a
//! bucket gets null there, not zero, which would draw a drop that never happened.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::Hash;

use ninjapaws_anomaly::Band;

use crate::api::v2::timeseries::{ParsedLimit, SortOrder};
use crate::api::v2::wire::{
    NinjacatBounds, NinjacatForecast, TimeseriesFormulaQueryResponse, TimeseriesResponse, TimeseriesResponseAttributes,
    TimeseriesResponseSeries,
};
use crate::query::compile::Row;
use crate::query::plan::{Aggregation, Fill, FillMethod};

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub group_tags: Vec<String>,
    /// Bucket start (unix ms) → value.
    pub points: BTreeMap<i64, f64>,
}

/// The value shown for a series that has no value for a `by` key. Datadog lower-cases tag values
/// on intake, so no real value can be "N/A".
///
/// WARNING(undocumented): the exact group_tags string of an N/A group. The docs name the group
/// "N/A"; `key:N/A` is our spelling.
pub const NOT_APPLICABLE: &str = "N/A";

/// Groups items by a key, the groups in the order their keys first appear.
pub(crate) fn grouped<T, K: Eq + Hash + Clone>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<(K, Vec<T>)> {
    let mut index: HashMap<K, usize> = HashMap::new();
    let mut groups: Vec<(K, Vec<T>)> = Vec::new();
    for item in items {
        let k = key(&item);
        match index.get(&k) {
            Some(&i) => groups[i].1.push(item),
            None => {
                index.insert(k.clone(), groups.len());
                groups.push((k, vec![item]));
            }
        }
    }
    groups
}

/// Floats ordered with NaN below every number.
pub(crate) fn compare(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or_else(|| b.is_nan().cmp(&a.is_nan()))
}

/// Summed from 0.0.
pub(crate) fn sum(values: &[f64]) -> f64 {
    values.iter().fold(0.0, |acc, v| acc + v)
}

pub(crate) fn mean(values: &[f64]) -> f64 {
    sum(values) / values.len() as f64
}

fn group_tags(group_by: &[String], groups: &[Option<String>]) -> Vec<String> {
    group_by.iter().zip(groups).map(|(k, v)| format!("{k}:{}", v.as_deref().unwrap_or(NOT_APPLICABLE))).collect()
}

/// Groups rows by their group values; each group is one series.
///
/// A bucket that starts before `from_ms` is dropped: it holds only the part of its interval
/// inside the window, so its value covers less time than every other point.
///
/// WARNING(undocumented): what Datadog does with a partial first bucket. Its one recorded v1
/// response starts at the first bucket after `from`, and we do the same.
pub fn from_rows(group_by: &[String], from_ms: i64, rows: Vec<Row>) -> Vec<Series> {
    let kept: Vec<Row> = rows.into_iter().filter(|r| r.bucket_ms >= from_ms).collect();
    grouped(kept, |r| r.groups.clone())
        .into_iter()
        .map(|(groups, rs)| Series {
            group_tags: group_tags(group_by, &groups),
            points: rs.into_iter().map(|r| (r.bucket_ms, r.value)).collect(),
        })
        .collect()
}

// --- filling and combining, in the engine ----------------------------------------------------

/// Fills one series at `times` it has no point at, within the limit after its previous real
/// point. `times` and the series are both in time order, so one pass with a cursor does it.
fn fill_at(method: FillMethod, limit_ms: i64, times: &[i64], points: BTreeMap<i64, f64>) -> BTreeMap<i64, f64> {
    let real: Vec<(i64, f64)> = points.iter().map(|(&t, &v)| (t, v)).collect();
    // real[i] is the first real point after the current time.
    let mut i = 0;
    let mut filled = Vec::new();
    for &t in times {
        while i < real.len() && real[i].0 <= t {
            i += 1;
        }
        if points.contains_key(&t) || i == 0 {
            continue;
        }
        let (tp, vp) = real[i - 1];
        if t - tp > limit_ms {
            continue;
        }
        match method {
            FillMethod::Last => filled.push((t, vp)),
            FillMethod::Zero => filled.push((t, 0.0)),
            FillMethod::Linear if i < real.len() => {
                let (tn, vn) = real[i];
                filled.push((t, vp + (vn - vp) * (t - tp) as f64 / (tn - tp) as f64));
            }
            // Nothing after the gap to interpolate towards.
            FillMethod::Linear => {}
        }
    }
    let mut points = points;
    points.extend(filled);
    points
}

/// The first value no later value is greater than.
pub(crate) fn max_of(values: &[f64]) -> f64 {
    values[1..].iter().fold(values[0], |acc, &x| if x > acc { x } else { acc })
}

pub(crate) fn min_of(values: &[f64]) -> f64 {
    values[1..].iter().fold(values[0], |acc, &x| if x < acc { x } else { acc })
}

fn combine_values(agg: Aggregation, values: &[f64]) -> f64 {
    match agg {
        Aggregation::Avg => mean(values),
        Aggregation::Sum => sum(values),
        Aggregation::Min => min_of(values),
        Aggregation::Max => max_of(values),
        Aggregation::Count => values.len() as f64,
    }
}

/// Per-series rows → one series per group: each series filled, then the series of a group
/// combined bucket by bucket with the space aggregator.
///
/// Interpolation is "aligning several series together, to make it possible to perform
/// aggregation across sources" (docs: metrics/guide/interpolation-the-fill-modifier-explained).
/// So a series is filled only at the times another series of its group has a real point, and a
/// series alone in its group is never filled.
///
/// WARNING(undocumented): whether Datadog also fills to align series of different groups ("group
/// queries for easier comparisons"). We do not.
///
/// WARNING(undocumented): a gap at the start of the window, whose previous real point lies before
/// `from`. The window's reads are not extended for it, so it stays a gap.
pub fn combine(group_by: &[String], space_agg: Aggregation, fill: Fill, from_ms: i64, rows: Vec<Row>) -> Vec<Series> {
    let kept: Vec<Row> = rows.into_iter().filter(|r| r.bucket_ms >= from_ms).collect();
    grouped(kept, |r| r.groups.clone())
        .into_iter()
        .map(|(groups, rs)| {
            let series: Vec<BTreeMap<i64, f64>> = grouped(rs, |r| r.series_id)
                .into_iter()
                .map(|(_, xs)| xs.into_iter().map(|r| (r.bucket_ms, r.value)).collect())
                .collect();
            let times: Vec<i64> =
                series.iter().flat_map(|s| s.keys().copied()).collect::<BTreeSet<_>>().into_iter().collect();

            let filled = match fill {
                Fill::Within { method, limit_seconds } if series.len() > 1 => {
                    series.into_iter().map(|s| fill_at(method, limit_seconds as i64 * 1000, &times, s)).collect()
                }
                _ => series,
            };

            Series {
                group_tags: group_tags(group_by, &groups),
                points: times
                    .iter()
                    .filter_map(|t| {
                        let values: Vec<f64> = filled.iter().filter_map(|s| s.get(t).copied()).collect();
                        (!values.is_empty()).then(|| (*t, combine_values(space_agg, &values)))
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The window's bucket starts: multiples of the step, from the first at or after `from_ms`, up to
/// `to_ms`. ClickHouse's toStartOfInterval aligns second-sized buckets the same way.
pub fn grid(step_ms: i64, from_ms: i64, to_ms: i64) -> impl Iterator<Item = i64> {
    let first = from_ms.saturating_add(step_ms - 1) / step_ms * step_ms;
    std::iter::successors(Some(first), move |t| t.checked_add(step_ms)).take_while(move |&t| t < to_ms)
}

/// A value of 0 in every bucket of `times` a series has none in.
pub fn zero_fill(times: &[i64], s: Series) -> Series {
    let mut points = s.points;
    for &t in times {
        points.entry(t).or_insert(0.0);
    }
    Series { points, ..s }
}

/// A formula's `limit`: rank the series by their mean, keep `count`. Without a count only the
/// order applies.
///
/// WARNING(undocumented): what Datadog ranks a formula's `limit` by. We use the mean.
pub fn limit(l: Option<ParsedLimit>, series: Vec<Series>) -> Vec<Series> {
    let Some(l) = l else { return series };
    let rank = |s: &Series| {
        if s.points.is_empty() { f64::NEG_INFINITY } else { mean(&s.points.values().copied().collect::<Vec<_>>()) }
    };
    let mut ranked: Vec<(f64, Series)> = series.into_iter().map(|s| (rank(&s), s)).collect();
    match l.order {
        SortOrder::Desc => ranked.sort_by(|a, b| compare(b.0, a.0)),
        SortOrder::Asc => ranked.sort_by(|a, b| compare(a.0, b.0)),
    }
    if let Some(n) = l.count {
        ranked.truncate(n.max(0) as usize);
    }
    ranked.into_iter().map(|(_, s)| s).collect()
}

/// One forecast point, past the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Predicted {
    pub time_ms: i64,
    pub value: f64,
    pub range: Band,
}

/// What a series carries beside its values, when its formula asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeriesExtra {
    /// The expected range by time, within the window.
    pub band: Option<BTreeMap<i64, Band>>,
    pub forecast: Option<Vec<Predicted>>,
}

/// One series of an output, with what its formula adds beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputSeries {
    pub data: Series,
    pub extra: SeriesExtra,
}

/// One output of a plan: a formula, or a query when there are no formulas.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputResult {
    pub query_index: usize,
    pub lines: Vec<OutputSeries>,
}

fn bounds(times: &[i64], band: &BTreeMap<i64, Band>) -> NinjacatBounds {
    NinjacatBounds {
        upper: times.iter().map(|t| band.get(t).map(|b| b.upper)).collect(),
        lower: times.iter().map(|t| band.get(t).map(|b| b.lower)).collect(),
    }
}

fn forecast(points: &[Predicted]) -> NinjacatForecast {
    NinjacatForecast {
        times: points.iter().map(|p| p.time_ms).collect(),
        values: points.iter().map(|p| p.value).collect(),
        lower: points.iter().map(|p| p.range.lower).collect(),
        upper: points.iter().map(|p| p.range.upper).collect(),
    }
}

/// Every output's series with their extras, laid out on one time axis.
pub fn response_with(outputs: &[OutputResult]) -> TimeseriesFormulaQueryResponse {
    let flat: Vec<(usize, &OutputSeries)> =
        outputs.iter().flat_map(|o| o.lines.iter().map(move |s| (o.query_index, s))).collect();
    let times: Vec<i64> =
        flat.iter().flat_map(|(_, s)| s.data.points.keys().copied()).collect::<BTreeSet<_>>().into_iter().collect();
    let any_band = flat.iter().any(|(_, s)| s.extra.band.is_some());
    let any_forecast = flat.iter().any(|(_, s)| s.extra.forecast.is_some());

    TimeseriesFormulaQueryResponse {
        data: TimeseriesResponse {
            id: "0".into(),
            kind: "timeseries_response".into(),
            attributes: TimeseriesResponseAttributes {
                series: flat
                    .iter()
                    .map(|(index, s)| TimeseriesResponseSeries {
                        group_tags: s.data.group_tags.clone(),
                        query_index: *index,
                    })
                    .collect(),
                values: flat
                    .iter()
                    .map(|(_, s)| times.iter().map(|t| s.data.points.get(t).copied()).collect())
                    .collect(),
                ninjacat_bounds: any_band
                    .then(|| flat.iter().map(|(_, s)| s.extra.band.as_ref().map(|b| bounds(&times, b))).collect()),
                ninjacat_forecast: any_forecast
                    .then(|| flat.iter().map(|(_, s)| s.extra.forecast.as_deref().map(forecast)).collect()),
                times,
            },
        },
    }
}

/// Every output's series, laid out on one time axis.
pub fn response(outputs: Vec<(usize, Vec<Series>)>) -> TimeseriesFormulaQueryResponse {
    let outputs: Vec<OutputResult> = outputs
        .into_iter()
        .map(|(query_index, series)| OutputResult {
            query_index,
            lines: series.into_iter().map(|data| OutputSeries { data, extra: SeriesExtra::default() }).collect(),
        })
        .collect();
    response_with(&outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(groups: &[&str], bucket_ms: i64, value: f64) -> Row {
        Row { groups: groups.iter().map(|g| Some(g.to_string())).collect(), series_id: None, bucket_ms, value }
    }

    fn keys(ks: &[&str]) -> Vec<String> {
        ks.iter().map(|k| k.to_string()).collect()
    }

    fn points(ps: &[(i64, f64)]) -> BTreeMap<i64, f64> {
        ps.iter().copied().collect()
    }

    // --- series from rows ----------------------------------------------------------------

    #[test]
    fn rows_group_into_one_series_per_group() {
        let series = from_rows(
            &keys(&["host"]),
            0,
            vec![row(&["web-1"], 0, 1.0), row(&["web-1"], 20, 3.0), row(&["web-2"], 20, 7.0)],
        );
        assert_eq!(
            series.iter().map(|s| s.group_tags.clone()).collect::<Vec<_>>(),
            [keys(&["host:web-1"]), keys(&["host:web-2"])]
        );
        assert_eq!(series[0].points, points(&[(0, 1.0), (20, 3.0)]));
    }

    #[test]
    fn a_series_without_the_tag_is_the_na_group() {
        let rows = vec![Row { groups: vec![Some("web-1".into()), None], series_id: None, bucket_ms: 0, value: 1.0 }];
        let series = from_rows(&keys(&["host", "role"]), 0, rows);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].group_tags, keys(&["host:web-1", "role:N/A"]));
    }

    #[test]
    fn no_group_by_is_one_series_without_tags() {
        let series = from_rows(&[], 0, vec![row(&[], 0, 1.0), row(&[], 20, 2.0)]);
        assert_eq!(series.len(), 1);
        assert!(series[0].group_tags.is_empty());
    }

    #[test]
    fn a_bucket_starting_before_from_is_dropped() {
        let series = from_rows(&[], 1000, vec![row(&[], 980, 1.0), row(&[], 1000, 2.0)]);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].points.keys().copied().collect::<Vec<_>>(), [1000]);
    }

    // --- the response --------------------------------------------------------------------

    #[test]
    fn gaps_are_null_not_zero_on_a_shared_time_axis() {
        // web-1 misses 12:00:20, web-2 misses 12:00:00.
        let series = from_rows(
            &keys(&["host"]),
            0,
            vec![row(&["web-1"], 0, 1.0), row(&["web-1"], 40, 3.0), row(&["web-2"], 20, 7.0), row(&["web-2"], 40, 9.0)],
        );
        let r = response(vec![(0, series)]).data.attributes;
        assert_eq!(r.times, [0, 20, 40]);
        assert_eq!(r.values, [vec![Some(1.0), None, Some(3.0)], vec![None, Some(7.0), Some(9.0)]]);
    }

    #[test]
    fn no_data_is_an_empty_response_not_an_error() {
        let r = response(vec![(0, vec![])]).data;
        assert_eq!(r.kind, "timeseries_response");
        assert!(r.attributes.series.is_empty());
        assert!(r.attributes.times.is_empty());
    }

    #[test]
    fn limit_keeps_the_top_series_by_mean() {
        let s = |name: &str, v: f64| Series { group_tags: keys(&[name]), points: points(&[(0, v)]) };
        let kept = limit(
            Some(ParsedLimit { count: Some(2), order: SortOrder::Desc }),
            vec![s("a", 1.0), s("b", 3.0), s("c", 2.0)],
        );
        assert_eq!(kept.iter().map(|s| s.group_tags.clone()).collect::<Vec<_>>(), [keys(&["b"]), keys(&["c"])]);
    }

    // --- filling and combining -----------------------------------------------------------

    fn series_row(id: u64, bucket_ms: i64, value: f64) -> Row {
        Row { groups: vec![Some("prod".into())], series_id: Some(id), bucket_ms, value }
    }

    /// Two hosts in one group, every 20 s. Host 1 misses the bucket at 40 s.
    ///   host 1:  10   20   --   40
    ///   host 2:   1    2    3    4
    fn two_hosts() -> Vec<Row> {
        vec![
            series_row(1, 0, 10.0),
            series_row(1, 20_000, 20.0),
            series_row(1, 60_000, 40.0),
            series_row(2, 0, 1.0),
            series_row(2, 20_000, 2.0),
            series_row(2, 40_000, 3.0),
            series_row(2, 60_000, 4.0),
        ]
    }

    fn summed(fill: Fill, rows: Vec<Row>) -> Vec<(i64, f64)> {
        let series = combine(&keys(&["env"]), Aggregation::Sum, fill, 0, rows);
        assert_eq!(series.len(), 1);
        series[0].points.iter().map(|(&t, &v)| (t, v)).collect()
    }

    fn within(method: FillMethod, limit_seconds: i32) -> Fill {
        Fill::Within { method, limit_seconds }
    }

    #[test]
    fn without_a_fill_the_sum_dips_where_a_host_missed_a_bucket() {
        assert_eq!(summed(Fill::NoFill, two_hosts()), [(0, 11.0), (20_000, 22.0), (40_000, 3.0), (60_000, 44.0)]);
    }

    #[test]
    fn linear_fill_aligns_the_host_first() {
        // Host 1 at 40 s: halfway between 20 and 40 → 30; sum 30 + 3.
        assert_eq!(
            summed(within(FillMethod::Linear, 300), two_hosts()),
            [(0, 11.0), (20_000, 22.0), (40_000, 33.0), (60_000, 44.0)]
        );
    }

    #[test]
    fn last_and_zero_fills() {
        assert_eq!(
            summed(within(FillMethod::Last, 300), two_hosts()),
            [(0, 11.0), (20_000, 22.0), (40_000, 23.0), (60_000, 44.0)]
        );
        assert_eq!(
            summed(within(FillMethod::Zero, 300), two_hosts()),
            [(0, 11.0), (20_000, 22.0), (40_000, 3.0), (60_000, 44.0)]
        );
    }

    #[test]
    fn a_gap_longer_than_the_limit_stays_a_gap() {
        // 20 s after host 1's last point, with a 10 s limit: not filled.
        assert_eq!(
            summed(within(FillMethod::Linear, 10), two_hosts()),
            [(0, 11.0), (20_000, 22.0), (40_000, 3.0), (60_000, 44.0)]
        );
    }

    #[test]
    fn a_series_alone_in_its_group_is_not_filled() {
        // Nothing to align with: interpolation is for combining series.
        let alone = vec![series_row(1, 0, 10.0), series_row(1, 40_000, 30.0)];
        assert_eq!(summed(within(FillMethod::Linear, 300), alone), [(0, 10.0), (40_000, 30.0)]);
    }

    #[test]
    fn the_window_grid_and_zero_alignment() {
        assert_eq!(grid(20_000, 1, 60_000).collect::<Vec<_>>(), [20_000, 40_000]);
        let s = zero_fill(&[0, 20_000], Series { group_tags: vec![], points: points(&[(20_000, 5.0)]) });
        assert_eq!(s.points, points(&[(0, 0.0), (20_000, 5.0)]));
    }

    // --- extras in the response ----------------------------------------------------------

    #[test]
    fn extras_appear_only_when_asked_as_ninjacat_fields() {
        let s = Series { group_tags: keys(&["host:a"]), points: points(&[(0, 1.0), (20_000, 2.0)]) };
        let plain = serde_json::to_string(&response(vec![(0, vec![s.clone()])])).unwrap();
        assert!(!plain.contains("ninjacat_"));

        let extra = SeriesExtra {
            band: Some([(20_000, Band { lower: 1.5, upper: 2.5 })].into_iter().collect()),
            forecast: Some(vec![Predicted { time_ms: 40_000, value: 3.0, range: Band { lower: 2.0, upper: 4.0 } }]),
        };
        let outputs = [OutputResult { query_index: 0, lines: vec![OutputSeries { data: s, extra }] }];
        let json = serde_json::to_string(&response_with(&outputs)).unwrap();
        assert!(json.contains("\"times\":[0,20000]"), "{json}");
        assert!(json.contains("\"ninjacat_bounds\":[{\"upper\":[null,2.5],\"lower\":[null,1.5]}]"), "{json}");
        // System.Text.Json wrote whole floats as 3, serde_json writes 3.0.
        assert!(
            json.contains(
                "\"ninjacat_forecast\":[{\"times\":[40000],\"values\":[3.0],\"upper\":[4.0],\"lower\":[2.0]}]"
            ),
            "{json}"
        );
    }
}
