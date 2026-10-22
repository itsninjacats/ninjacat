//! Evaluates a formula's `Node` over the series its queries returned.
//!
//! Functions run after time and space aggregation, on the series the response will show. A value
//! a function cannot produce becomes a gap: absent from the series, null in the response.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::api::v2::timeseries::SortOrder;
use crate::query::plan::{ArithOp, Node, PointwiseFn, RankBy, TimewiseFn, fetches, lookback};
use crate::query::series::{NOT_APPLICABLE, Series, compare, grid, max_of, mean, min_of, sum, zero_fill};

/// One value through one function; None where there is no value to show.
///
/// WARNING(undocumented): log of zero or a negative number. It is a gap: JSON cannot carry
/// -Infinity or NaN, and neither is a value to draw.
///
/// WARNING(undocumented): how round() treats halves. It rounds them away from zero (2.5 → 3,
/// -2.5 → -3), as people expect, not to even as .NET does by default.
fn apply(f: PointwiseFn, v: f64) -> Option<f64> {
    let finite = |x: f64| x.is_finite().then_some(x);
    match f {
        PointwiseFn::Abs => Some(v.abs()),
        PointwiseFn::Log2 => finite(v.log2()),
        PointwiseFn::Log10 => finite(v.log10()),
        PointwiseFn::Ceil => Some(v.ceil()),
        PointwiseFn::Floor => Some(v.floor()),
        PointwiseFn::Round(decimals) => Some(round(v, decimals)),
        PointwiseFn::ClampMin(t) => Some(if v < t { t } else { v }),
        PointwiseFn::ClampMax(t) => Some(if v < t { v } else { t }),
        // "The cutoff functions do not replace values that are equal to the threshold value."
        // (docs: dashboards/functions/exclusion)
        PointwiseFn::CutoffMin(t) => (v >= t).then_some(v),
        PointwiseFn::CutoffMax(t) => (v <= t).then_some(v),
    }
}

/// .NET's Math.Round(v, decimals, AwayFromZero): scaled by a power of ten, rounded, scaled back,
/// and left alone at 1e16 and beyond, where a double holds no fraction.
fn round(v: f64, decimals: i32) -> f64 {
    if v.abs() >= 1e16 {
        return v;
    }
    let power = 10f64.powi(decimals);
    (v * power).round() / power
}

fn map_values(f: PointwiseFn, s: Series) -> Series {
    Series { points: s.points.into_iter().filter_map(|(t, v)| apply(f, v).map(|r| (t, r))).collect(), ..s }
}

fn values(s: &Series) -> Vec<f64> {
    s.points.values().copied().collect()
}

// --- across series ---------------------------------------------------------------------------

/// The number a series is ranked by. None for a series with no points: it has nothing to rank
/// by.
///
/// WARNING(undocumented): where top() puts a series with no points. It goes last, whichever the
/// direction.
///
/// WARNING(undocumented): how `area` is integrated. It is the sum of value × bucket width, in
/// seconds. Buckets are equal within one query, so it orders series the way the true area would.
fn rank(by: RankBy, s: &Series) -> Option<f64> {
    if s.points.is_empty() {
        return None;
    }
    let values = values(s);
    Some(match by {
        RankBy::Max => max_of(&values),
        RankBy::Min => min_of(&values),
        RankBy::Mean => mean(&values),
        RankBy::Sum => sum(&values),
        RankBy::Last => values[values.len() - 1],
        RankBy::L2norm => sum(&values.iter().map(|v| v * v).collect::<Vec<_>>()).sqrt(),
        RankBy::Area => {
            let keys: Vec<i64> = s.points.keys().copied().collect();
            match keys.as_slice() {
                [_] => values[0],
                _ => sum(&values) * ((keys[1] - keys[0]) as f64 / 1000.0),
            }
        }
    })
}

/// WARNING(undocumented): how top() breaks ties. The sort is stable over the series' group
/// order, which is alphabetical (the SQL orders by group).
fn top(count: usize, by: RankBy, order: SortOrder, series: Vec<Series>) -> Vec<Series> {
    let mut ranked = Vec::new();
    let mut empty = Vec::new();
    for s in series {
        match rank(by, &s) {
            Some(r) => ranked.push((r, s)),
            None => empty.push(s),
        }
    }
    match order {
        SortOrder::Desc => ranked.sort_by(|a, b| compare(b.0, a.0)),
        SortOrder::Asc => ranked.sort_by(|a, b| compare(a.0, b.0)),
    }
    ranked.into_iter().map(|(_, s)| s).chain(empty).take(count).collect()
}

/// One series with no group: at each time any input series has a point, how many of them pass
/// `counts`. A time with no point in any series stays a gap (the docs count "tag values with
/// non-null values at each point").
fn count_across(counts: impl Fn(f64) -> bool, series: Vec<Series>) -> Vec<Series> {
    let times: BTreeSet<i64> = series.iter().flat_map(|s| s.points.keys().copied()).collect();
    let points: BTreeMap<i64, f64> = times
        .into_iter()
        .map(|t| {
            let n = series.iter().filter(|s| s.points.get(&t).is_some_and(|&v| counts(v))).count();
            (t, n as f64)
        })
        .collect();
    if points.is_empty() { vec![] } else { vec![Series { group_tags: vec![], points }] }
}

fn has_not_applicable(s: &Series) -> bool {
    let suffix = format!(":{NOT_APPLICABLE}");
    s.group_tags.iter().any(|t| t.ends_with(&suffix))
}

// --- along time ------------------------------------------------------------------------------

/// One function over one series' points, in time order.
///
/// WARNING(undocumented): how diff-like functions handle a gap. They work on consecutive points,
/// so the diff after a gap is against the last point before it.
///
/// WARNING(undocumented): monotonic_diff at zero and below. It keeps zero (a counter that did not
/// move); a negative delta (a counter reset) is a gap.
///
/// WARNING(undocumented): whether median_N and rollingavg_N windows are trailing or centred. They
/// are trailing: each point looks at itself and the N-1 before it, as a live graph must.
///
/// WARNING(undocumented): how ewma starts. At the first point's value, then
/// s = alpha·v + (1 - alpha)·s, alpha = 2 / (span + 1): "twice the weighted average age", the
/// usual span definition.
///
/// FIXME(integral): the docs define integral as "the cumulative sum of [time delta] x [value
/// delta] over all consecutive pairs of points", and that is what this does. It makes the
/// integral of a constant zero, so the docs likely mean [time delta] x [value]: the area under
/// the curve. Unverified: nobody here has a Datadog account to compare against. The fix is the
/// `Integral` case below: `dt * dv` → `dt * v1`.
fn along_time(f: TimewiseFn, step_ms: i64, s: Series) -> Series {
    let pts: Vec<(i64, f64)> = s.points.iter().map(|(&t, &v)| (t, v)).collect();
    let seconds = |dt_ms: i64| dt_ms as f64 / 1000.0;
    let deltas = |f: &dyn Fn(f64, f64) -> Option<f64>| -> Vec<(i64, f64)> {
        pts.windows(2).filter_map(|w| f(seconds(w[1].0 - w[0].0), w[1].1 - w[0].1).map(|r| (w[1].0, r))).collect()
    };
    let window = |n: i32, f: &dyn Fn(&[f64]) -> f64| -> Vec<(i64, f64)> {
        let values: Vec<f64> = pts.iter().map(|p| p.1).collect();
        (0..pts.len())
            .map(|i| {
                let start = (i as i64 - n as i64 + 1).clamp(0, i as i64 + 1) as usize;
                (pts[i].0, f(&values[start..=i]))
            })
            .collect()
    };
    let cumulative = |first: (i64, f64), steps: Vec<(i64, f64)>| -> Vec<(i64, f64)> {
        let mut acc = first.1;
        std::iter::once(first)
            .chain(steps.into_iter().map(|(t, x)| {
                acc += x;
                (t, acc)
            }))
            .collect()
    };

    let out: Vec<(i64, f64)> = match f {
        TimewiseFn::Cumsum => {
            let mut acc = 0.0;
            pts.iter()
                .map(|&(t, v)| {
                    acc += v;
                    (t, acc)
                })
                .collect()
        }
        TimewiseFn::Integral => match pts.first() {
            None => vec![],
            Some(&(t0, _)) => cumulative((t0, 0.0), deltas(&|dt, dv| Some(dt * dv))),
        },
        TimewiseFn::Diff => deltas(&|_, dv| Some(dv)),
        TimewiseFn::MonotonicDiff => deltas(&|_, dv| (dv >= 0.0).then_some(dv)),
        TimewiseFn::Derivative | TimewiseFn::PerSecond => deltas(&|dt, dv| Some(dv / dt)),
        TimewiseFn::PerMinute => deltas(&|dt, dv| Some(dv / dt * 60.0)),
        TimewiseFn::PerHour => deltas(&|dt, dv| Some(dv / dt * 3600.0)),
        TimewiseFn::Throughput => pts.iter().map(|&(t, v)| (t, v / seconds(step_ms))).collect(),
        TimewiseFn::Ewma(span) => {
            let alpha = 2.0 / (span as f64 + 1.0);
            match pts.first() {
                None => vec![],
                Some(&first) => {
                    let mut e = first.1;
                    std::iter::once(first)
                        .chain(pts[1..].iter().map(|&(t, v)| {
                            e = alpha * v + (1.0 - alpha) * e;
                            (t, e)
                        }))
                        .collect()
                }
            }
        }
        TimewiseFn::Median(n) => window(n, &ninjapaws_anomaly::median),
        TimewiseFn::RollingAvg(n) => window(n, &mean),
        TimewiseFn::TrendLine | TimewiseFn::RobustTrend if !pts.is_empty() => {
            let t0 = pts[0].0;
            let xs: Vec<f64> = pts.iter().map(|&(t, _)| seconds(t - t0)).collect();
            let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
            let (slope, intercept) = if f == TimewiseFn::TrendLine {
                ninjapaws_anomaly::ols(&xs, &ys)
            } else {
                ninjapaws_anomaly::huber(&xs, &ys)
            };
            pts.iter().zip(&xs).map(|(&(t, _), x)| (t, intercept + slope * x)).collect()
        }
        TimewiseFn::PiecewiseConstant => {
            let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
            pts.iter().zip(ninjapaws_anomaly::piecewise_constant(&ys)).map(|(&(t, _), v)| (t, v)).collect()
        }
        // Autosmooth picks one window across all series; see `eval_shifted`.
        TimewiseFn::Autosmooth => pts.clone(),
        TimewiseFn::TrendLine | TimewiseFn::RobustTrend => vec![],
    };
    Series { points: out.into_iter().collect(), ..s }
}

fn trim(from_ms: i64, series: Vec<Series>) -> Vec<Series> {
    series
        .into_iter()
        .map(|s| Series { points: s.points.into_iter().filter(|&(t, _)| t >= from_ms).collect(), ..s })
        .collect()
}

// --- arithmetic ------------------------------------------------------------------------------

/// One operation on two values; None where it has no finite answer.
///
/// WARNING(undocumented): division by zero. It is a gap, not ±Infinity, which JSON cannot carry
/// and a graph cannot draw.
fn arith(op: ArithOp, a: f64, b: f64) -> Option<f64> {
    let r = match op {
        ArithOp::Plus => a + b,
        ArithOp::Minus => a - b,
        ArithOp::Times => a * b,
        ArithOp::Divide => {
            if b == 0.0 {
                f64::NAN
            } else {
                a / b
            }
        }
        ArithOp::Minimum => {
            if a < b {
                a
            } else {
                b
            }
        }
        ArithOp::Maximum => {
            if a < b {
                b
            } else {
                a
            }
        }
        ArithOp::Power => a.powf(b),
    };
    r.is_finite().then_some(r)
}

/// An operand of arithmetic: series, or a value per time with no series of its own (a number,
/// time(), or arithmetic on those).
enum Operand<'a> {
    Many(Vec<Series>),
    Scalar(Box<dyn Fn(i64) -> f64 + 'a>),
}

/// Point by point where both have a value: a gap on either side is a gap in the result.
///
/// WARNING(undocumented): operands at different steps (one query with `.rollup(sum, 60)`, the
/// other at the request's 20 s). Their times rarely meet, and most points become gaps; Datadog
/// does not say how it aligns them.
fn zip_series(op: ArithOp, a: &Series, b: &Series, group_tags: Vec<String>) -> Series {
    Series {
        group_tags,
        points: a
            .points
            .iter()
            .filter_map(|(&t, &av)| b.points.get(&t).and_then(|&bv| arith(op, av, bv)).map(|r| (t, r)))
            .collect(),
    }
}

fn with_scalar(f: impl Fn(i64, f64) -> Option<f64>, s: Series) -> Series {
    Series { points: s.points.into_iter().filter_map(|(t, v)| f(t, v).map(|r| (t, r))).collect(), ..s }
}

fn tag_set(s: &Series) -> BTreeSet<String> {
    s.group_tags.iter().cloned().collect()
}

/// Two operands combined, series matched by group:
///   - a number or time() applies to every series;
///   - one ungrouped series (a query with no `by`) applies to every series of the other side;
///   - otherwise series pair up by their group tags, as a set, so `by {a, b}` meets `by {b, a}`.
///
/// WARNING(undocumented): a group on one side only. It is dropped: there is nothing to combine it
/// with.
fn combine_operands<'a>(op: ArithOp, left: Operand<'a>, right: Operand<'a>) -> Operand<'a> {
    let ungrouped = |series: &[Series]| match series {
        [s] if s.group_tags.is_empty() => Some(s.clone()),
        _ => None,
    };
    match (left, right) {
        (Operand::Scalar(f), Operand::Scalar(g)) => {
            Operand::Scalar(Box::new(move |t| arith(op, f(t), g(t)).unwrap_or(f64::NAN)))
        }
        (Operand::Scalar(f), Operand::Many(b)) => {
            Operand::Many(b.into_iter().map(|s| with_scalar(|t, v| arith(op, f(t), v), s)).collect())
        }
        (Operand::Many(a), Operand::Scalar(g)) => {
            Operand::Many(a.into_iter().map(|s| with_scalar(|t, v| arith(op, v, g(t)), s)).collect())
        }
        (Operand::Many(a), Operand::Many(b)) => match (ungrouped(&a), ungrouped(&b)) {
            (Some(sa), _) => Operand::Many(b.iter().map(|sb| zip_series(op, &sa, sb, sb.group_tags.clone())).collect()),
            (_, Some(sb)) => Operand::Many(a.iter().map(|sa| zip_series(op, sa, &sb, sa.group_tags.clone())).collect()),
            (None, None) => {
                let by_group: HashMap<BTreeSet<String>, &Series> = b.iter().map(|s| (tag_set(s), s)).collect();
                Operand::Many(
                    a.iter()
                        .filter_map(|sa| {
                            by_group.get(&tag_set(sa)).map(|sb| zip_series(op, sa, sb, sa.group_tags.clone()))
                        })
                        .collect(),
                )
            }
        },
    }
}

/// What evaluation reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fetched {
    /// Each (query, offset)'s series, with times already moved to the present window.
    pub series: HashMap<(usize, i64), Vec<Series>>,
    /// Each query's bucket width.
    pub step_ms: HashMap<usize, i64>,
    /// The window's end, for functions that fill it to the end.
    pub to_ms: i64,
}

/// The bucket width of the first query `node` reads. The plan refuses a formula that reads none,
/// and every query read has its step in `fetched`.
fn step_of(fetched: &Fetched, node: &Node) -> Option<i64> {
    fetches(node).first().and_then(|(i, _)| fetched.step_ms.get(i).copied())
}

fn operand<'a>(fetched: &'a Fetched, shift: i64, from_ms: i64, node: &'a Node) -> Operand<'a> {
    match node {
        Node::Constant(c) => {
            let c = *c;
            Operand::Scalar(Box::new(move |_| c))
        }
        // Times are milliseconds; time() is seconds.
        Node::TimeOfPoint => Operand::Scalar(Box::new(|t| t as f64 / 1000.0)),
        Node::Arith(op, l, r) => {
            combine_operands(*op, operand(fetched, shift, from_ms, l), operand(fetched, shift, from_ms, r))
        }
        other => Operand::Many(eval_shifted(fetched, shift, from_ms, other)),
    }
}

/// A time-wise function asks its input for earlier points (its lookback) and trims its own output
/// back to `from_ms`, so nothing above it sees the history: `top` ranks, and the response shows,
/// the visible window only.
fn eval_shifted(fetched: &Fetched, shift: i64, from_ms: i64, node: &Node) -> Vec<Series> {
    match node {
        Node::Fetch(i) => trim(from_ms, fetched.series.get(&(*i, shift)).cloned().unwrap_or_default()),
        Node::Shift { offset_ms, inner } => eval_shifted(fetched, shift + offset_ms, from_ms, inner),
        // WARNING(undocumented): what outliers() returns. On a Datadog graph it colours the
        // outlying series and greys the rest; the v2 response has nowhere to carry that mark. It
        // returns the outlying series only: what a monitor alerts on.
        Node::Outliers { inner, algorithm, tolerance } => {
            let series = eval_shifted(fetched, shift, from_ms, inner);
            let points: Vec<Vec<(i64, f64)>> =
                series.iter().map(|s| s.points.iter().map(|(&t, &v)| (t, v)).collect()).collect();
            let flags = ninjapaws_anomaly::outliers(*algorithm, *tolerance, &points);
            series.into_iter().zip(flags).filter(|(_, outlying)| *outlying).map(|(s, _)| s).collect()
        }
        Node::Arith(..) | Node::Constant(_) | Node::TimeOfPoint => match operand(fetched, shift, from_ms, node) {
            Operand::Many(series) => series,
            // The plan refuses a formula made only of numbers.
            Operand::Scalar(_) => vec![],
        },
        // "Fills empty time intervals using the value 0": every bucket of the window, after time
        // and space aggregation (docs: dashboards/functions/interpolation).
        //
        // WARNING(undocumented): default_zero over a query with no series in the window. It gives
        // one series without tags, all zeros, so a sparse metric reads as 0 rather than as
        // nothing.
        //
        // WARNING(undocumented): the docs say default_zero fills "with interpolation" where
        // interpolation is enabled. Interpolation has already run before space aggregation here;
        // default_zero fills what is left with 0.
        Node::DefaultZero(inner) => {
            let times: Vec<i64> =
                step_of(fetched, inner).map(|step| grid(step, from_ms, fetched.to_ms).collect()).unwrap_or_default();
            match eval_shifted(fetched, shift, from_ms, inner) {
                series if series.is_empty() => {
                    vec![zero_fill(&times, Series { group_tags: vec![], points: BTreeMap::new() })]
                }
                series => series.into_iter().map(|s| zero_fill(&times, s)).collect(),
            }
        }
        Node::Pointwise(f, inner) => {
            eval_shifted(fetched, shift, from_ms, inner).into_iter().map(|s| map_values(*f, s)).collect()
        }
        Node::Top { inner, count, by, order } => top(*count, *by, *order, eval_shifted(fetched, shift, from_ms, inner)),
        // Every value that reaches here is finite (see `apply`), so "not null" and "non-zero
        // finite" need no further check.
        Node::CountNonzero(inner) => count_across(|v| v != 0.0, eval_shifted(fetched, shift, from_ms, inner)),
        Node::CountNotNull(inner) => count_across(|_| true, eval_shifted(fetched, shift, from_ms, inner)),
        Node::ExcludeNull(inner) => {
            eval_shifted(fetched, shift, from_ms, inner).into_iter().filter(|s| !has_not_applicable(s)).collect()
        }
        Node::Timewise(f, inner) => {
            let step_ms = step_of(fetched, inner).unwrap_or(0);
            // Saturating: `from_ms` may already be the lowest there is.
            let inner_from = from_ms.saturating_sub(lookback(*f) as i64 * step_ms);
            let series = eval_shifted(fetched, shift, inner_from, inner);
            match f {
                TimewiseFn::Autosmooth => trim(from_ms, autosmooth(series)),
                _ => trim(from_ms, series.into_iter().map(|s| along_time(*f, step_ms, s)).collect()),
            }
        }
    }
}

/// "When you apply Auto Smoother to several timeseries using a group by query, the same window
/// size is applied on all the timeseries" (https://www.datadoghq.com/blog/auto-smoother-asap/).
///
/// WARNING(undocumented): how that one window is chosen. Each series gets its own ASAP window,
/// and all use the median of them. The window is capped at the lookback autosmooth reads, so the
/// first visible point always has a full window behind it.
fn autosmooth(series: Vec<Series>) -> Vec<Series> {
    let windows: Vec<f64> = series
        .iter()
        .map(values)
        .filter(|v| !v.is_empty())
        .map(|v| ninjapaws_anomaly::autosmooth_window(lookback(TimewiseFn::Autosmooth) as usize, &v) as f64)
        .collect();
    if windows.is_empty() {
        return series;
    }
    let w = ninjapaws_anomaly::median(&windows) as usize;
    series
        .into_iter()
        .map(|s| {
            let smoothed = ninjapaws_anomaly::moving_average(w, &values(&s));
            Series { points: s.points.keys().copied().zip(smoothed).collect(), ..s }
        })
        .collect()
}

/// Evaluates `node`, keeping points from `from_ms` on.
pub fn eval_from(fetched: &Fetched, from_ms: i64, node: &Node) -> Vec<Series> {
    eval_shifted(fetched, 0, from_ms, node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(i: usize) -> Box<Node> {
        Box::new(Node::Fetch(i))
    }

    fn series(values: &[f64]) -> Series {
        Series {
            group_tags: vec!["host:a".into()],
            points: values.iter().enumerate().map(|(i, &v)| (i as i64, v)).collect(),
        }
    }

    fn one(series: Vec<Series>) -> Series {
        assert_eq!(series.len(), 1, "{series:?}");
        series.into_iter().next().unwrap()
    }

    /// Evaluates with every point the queries returned: there is no window to trim to.
    fn eval(series: HashMap<usize, Vec<Series>>, node: &Node) -> Vec<Series> {
        let fetched = Fetched {
            step_ms: series.keys().map(|&i| (i, 20_000)).collect(),
            series: series.into_iter().map(|(i, s)| ((i, 0), s)).collect(),
            to_ms: i64::MAX,
        };
        eval_from(&fetched, i64::MIN, node)
    }

    fn only(i: usize, series: Vec<Series>) -> HashMap<usize, Vec<Series>> {
        HashMap::from([(i, series)])
    }

    /// The values after applying one function to [-2.5; -1; 0; 1; 2.5; 100], in time order; None
    /// where the function left a gap.
    fn through(f: PointwiseFn) -> Vec<Option<f64>> {
        let input = series(&[-2.5, -1.0, 0.0, 1.0, 2.5, 100.0]);
        let out = one(eval(only(0, vec![input]), &Node::Pointwise(f, fetch(0))));
        (0..6).map(|t| out.points.get(&t).copied()).collect()
    }

    fn some(values: &[f64]) -> Vec<Option<f64>> {
        values.iter().map(|&v| Some(v)).collect()
    }

    #[test]
    fn abs_ceil_floor() {
        assert_eq!(through(PointwiseFn::Abs), some(&[2.5, 1.0, 0.0, 1.0, 2.5, 100.0]));
        assert_eq!(through(PointwiseFn::Ceil), some(&[-2.0, -1.0, 0.0, 1.0, 3.0, 100.0]));
        assert_eq!(through(PointwiseFn::Floor), some(&[-3.0, -1.0, 0.0, 1.0, 2.0, 100.0]));
    }

    #[test]
    fn round_takes_halves_away_from_zero() {
        assert_eq!(through(PointwiseFn::Round(0)), some(&[-3.0, -1.0, 0.0, 1.0, 3.0, 100.0]));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn round_to_decimals() {
        let out = one(eval(only(0, vec![series(&[3.14159])]), &Node::Pointwise(PointwiseFn::Round(2), fetch(0))));
        assert_eq!(out.points[&0], 3.14);
    }

    #[test]
    fn log_of_zero_or_less_is_a_gap() {
        assert_eq!(through(PointwiseFn::Log10), [None, None, None, Some(0.0), Some(2.5f64.log10()), Some(2.0)]);
        assert_eq!(through(PointwiseFn::Log2), [None, None, None, Some(0.0), Some(2.5f64.log2()), Some(100f64.log2())]);
    }

    #[test]
    fn clamp_raises_or_lowers_to_the_threshold() {
        assert_eq!(through(PointwiseFn::ClampMin(0.0)), some(&[0.0, 0.0, 0.0, 1.0, 2.5, 100.0]));
        assert_eq!(through(PointwiseFn::ClampMax(10.0)), some(&[-2.5, -1.0, 0.0, 1.0, 2.5, 10.0]));
    }

    #[test]
    fn cutoff_removes_beyond_the_threshold_and_keeps_it_equal() {
        assert_eq!(through(PointwiseFn::CutoffMin(0.0)), [None, None, Some(0.0), Some(1.0), Some(2.5), Some(100.0)]);
        assert_eq!(
            through(PointwiseFn::CutoffMax(2.5)),
            [Some(-2.5), Some(-1.0), Some(0.0), Some(1.0), Some(2.5), None]
        );
    }

    fn s(name: &str, points: &[(i64, f64)]) -> Series {
        Series { group_tags: vec![name.into()], points: points.iter().copied().collect() }
    }

    fn points(ps: &[(i64, f64)]) -> BTreeMap<i64, f64> {
        ps.iter().copied().collect()
    }

    #[test]
    fn gaps_stay_gaps_and_every_series_is_mapped() {
        let a = s("host:a", &[(0, -1.0), (20, -2.0)]);
        let b = s("host:b", &[(20, -3.0)]);
        let out = eval(only(0, vec![a, b]), &Node::Pointwise(PointwiseFn::Abs, fetch(0)));
        assert_eq!(out[0].points, points(&[(0, 1.0), (20, 2.0)]));
        assert_eq!(out[1].points, points(&[(20, 3.0)]));
    }

    #[test]
    fn nested_functions_apply_inside_out() {
        // abs(log10(x)) on 0.01 → log10 = -2 → abs = 2
        let node = Node::Pointwise(PointwiseFn::Abs, Box::new(Node::Pointwise(PointwiseFn::Log10, fetch(0))));
        let out = one(eval(only(0, vec![series(&[0.01])]), &node));
        assert!((out.points[&0] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn fetches_lists_the_queries_a_node_reads_with_their_offsets() {
        let node = Node::Pointwise(PointwiseFn::Abs, Box::new(Node::Pointwise(PointwiseFn::Floor, fetch(3))));
        assert_eq!(fetches(&node), [(3, 0)]);
        let shifted = Node::Shift {
            offset_ms: -3_600_000,
            inner: Box::new(Node::Pointwise(
                PointwiseFn::Abs,
                Box::new(Node::Shift { offset_ms: -86_400_000, inner: fetch(3) }),
            )),
        };
        assert_eq!(fetches(&shifted), [(3, -90_000_000)]);
    }

    // --- across series -------------------------------------------------------------------

    fn names(out: &[Series]) -> Vec<String> {
        out.iter().map(|s| s.group_tags[0].clone()).collect()
    }

    /// Three series built so each ranking picks a different winner:
    ///   a: 1, 1, 10    max 10, mean 4,   sum 12, last 10, l2norm √102 ≈ 10.1
    ///   b: 5, 5, 5     max 5,  mean 5,   sum 15, last 5,  l2norm √75  ≈ 8.7
    ///   c: -9, 8, 0    max 8,  mean -1/3, sum -1, last 0,  l2norm √145 ≈ 12.0, min -9
    fn abc() -> Vec<Series> {
        vec![
            s("a", &[(0, 1.0), (20_000, 1.0), (40_000, 10.0)]),
            s("b", &[(0, 5.0), (20_000, 5.0), (40_000, 5.0)]),
            s("c", &[(0, -9.0), (20_000, 8.0), (40_000, 0.0)]),
        ]
    }

    fn top_of(count: usize, by: RankBy, order: SortOrder) -> Vec<String> {
        names(&eval(only(0, abc()), &Node::Top { inner: fetch(0), count, by, order }))
    }

    #[test]
    fn each_ranking_picks_its_own_winner() {
        assert_eq!(top_of(1, RankBy::Max, SortOrder::Desc), ["a"]);
        assert_eq!(top_of(1, RankBy::Mean, SortOrder::Desc), ["b"]);
        assert_eq!(top_of(1, RankBy::Sum, SortOrder::Desc), ["b"]);
        assert_eq!(top_of(1, RankBy::Last, SortOrder::Desc), ["a"]);
        assert_eq!(top_of(1, RankBy::L2norm, SortOrder::Desc), ["c"]);
        assert_eq!(top_of(1, RankBy::Min, SortOrder::Asc), ["c"]);
        assert_eq!(top_of(1, RankBy::Area, SortOrder::Asc), ["c"]); // signed: c's area is negative
    }

    #[test]
    fn top_keeps_the_order_and_the_count() {
        assert_eq!(top_of(5, RankBy::Mean, SortOrder::Desc), ["b", "a", "c"]);
        assert_eq!(top_of(2, RankBy::Mean, SortOrder::Asc), ["c", "a"]);
    }

    #[test]
    fn a_series_with_no_points_goes_last_either_way() {
        let series = vec![s("empty", &[]), s("one", &[(0, 1.0)])];
        for order in [SortOrder::Asc, SortOrder::Desc] {
            let node = Node::Top { inner: fetch(0), count: 5, by: RankBy::Mean, order };
            assert_eq!(names(&eval(only(0, series.clone()), &node)), ["one", "empty"]);
        }
    }

    #[test]
    fn count_nonzero_and_count_not_null_count_series_per_time() {
        // At t=0: a=1, b=5, c=-9 → 3 non-zero. At 40000: a=10, b=5, c=0 → 2 non-zero, 3 not null.
        let nonzero = one(eval(only(0, abc()), &Node::CountNonzero(fetch(0))));
        let not_null = one(eval(only(0, abc()), &Node::CountNotNull(fetch(0))));
        assert!(nonzero.group_tags.is_empty());
        assert_eq!(nonzero.points, points(&[(0, 3.0), (20_000, 3.0), (40_000, 2.0)]));
        assert_eq!(not_null.points, points(&[(0, 3.0), (20_000, 3.0), (40_000, 3.0)]));
    }

    #[test]
    fn count_not_null_counts_gaps_out() {
        let series = vec![s("a", &[(0, 1.0), (20_000, 1.0)]), s("b", &[(20_000, 0.0)])];
        let out = one(eval(only(0, series), &Node::CountNotNull(fetch(0))));
        assert_eq!(out.points, points(&[(0, 1.0), (20_000, 2.0)]));
    }

    #[test]
    fn counting_nothing_is_no_series() {
        assert!(eval(only(0, vec![]), &Node::CountNonzero(fetch(0))).is_empty());
    }

    #[test]
    fn exclude_null_drops_groups_with_any_na() {
        let tagged = |tags: &[&str]| Series {
            group_tags: tags.iter().map(|t| t.to_string()).collect(),
            points: points(&[(0, 1.0)]),
        };
        let series =
            vec![tagged(&["host:a", "role:web"]), tagged(&["host:b", "role:N/A"]), tagged(&["host:N/A", "role:db"])];
        let kept = eval(only(0, series), &Node::ExcludeNull(fetch(0)));
        assert_eq!(
            kept.iter().map(|s| s.group_tags.clone()).collect::<Vec<_>>(),
            [vec!["host:a".to_string(), "role:web".into()]]
        );
    }

    // --- along time ----------------------------------------------------------------------

    /// Points every 20 s: 0 → v0, 20000 → v1, …
    fn every20(values: &[f64]) -> Series {
        s("a", &values.iter().enumerate().map(|(i, &v)| (i as i64 * 20_000, v)).collect::<Vec<_>>())
    }

    fn along(f: TimewiseFn, values: &[f64]) -> Vec<(i64, f64)> {
        one(eval(only(0, vec![every20(values)]), &Node::Timewise(f, fetch(0)))).points.into_iter().collect()
    }

    fn close(expected: &[(i64, f64)], actual: &[(i64, f64)]) {
        assert_eq!(expected.len(), actual.len(), "{actual:?}");
        for ((te, ve), (ta, va)) in expected.iter().zip(actual) {
            assert_eq!(te, ta);
            assert!((ve - va).abs() < 1e-9, "{expected:?} != {actual:?}");
        }
    }

    #[test]
    fn cumsum() {
        close(&[(0, 1.0), (20_000, 3.0), (40_000, 6.0)], &along(TimewiseFn::Cumsum, &[1.0, 2.0, 3.0]));
    }

    #[test]
    fn diff_and_its_rates() {
        // 10 → 16 → 13 over 20 s steps.
        let v = [10.0, 16.0, 13.0];
        close(&[(20_000, 6.0), (40_000, -3.0)], &along(TimewiseFn::Diff, &v));
        close(&[(20_000, 0.3), (40_000, -0.15)], &along(TimewiseFn::Derivative, &v));
        close(&[(20_000, 0.3), (40_000, -0.15)], &along(TimewiseFn::PerSecond, &v));
        close(&[(20_000, 18.0), (40_000, -9.0)], &along(TimewiseFn::PerMinute, &v));
        close(&[(20_000, 1080.0), (40_000, -540.0)], &along(TimewiseFn::PerHour, &v));
    }

    #[test]
    fn monotonic_diff_a_reset_is_a_gap_standing_still_is_zero() {
        close(&[(20_000, 6.0), (60_000, 0.0)], &along(TimewiseFn::MonotonicDiff, &[10.0, 16.0, 2.0, 2.0]));
    }

    #[test]
    fn throughput_divides_by_the_bucket_width() {
        close(&[(0, 1.0), (20_000, 0.5)], &along(TimewiseFn::Throughput, &[20.0, 10.0]));
    }

    #[test]
    fn integral_as_the_docs_define_it() {
        // Δt × Δvalue: 20 × (16 − 10) = 120, then 20 × (13 − 16) = −60.
        close(&[(0, 0.0), (20_000, 120.0), (40_000, 60.0)], &along(TimewiseFn::Integral, &[10.0, 16.0, 13.0]));
    }

    #[test]
    fn ewma() {
        // span 3 → alpha 0.5: 10, 0.5·20 + 0.5·10 = 15, 0.5·20 + 0.5·15 = 17.5
        close(&[(0, 10.0), (20_000, 15.0), (40_000, 17.5)], &along(TimewiseFn::Ewma(3), &[10.0, 20.0, 20.0]));
        // span 1 is the series itself
        close(&[(0, 10.0), (20_000, 20.0)], &along(TimewiseFn::Ewma(1), &[10.0, 20.0]));
    }

    #[test]
    fn median_and_rolling_average_are_trailing() {
        close(
            &[(0, 5.0), (20_000, 3.0), (40_000, 1.0), (60_000, 1.0)],
            &along(TimewiseFn::Median(3), &[5.0, 1.0, 1.0, 9.0]),
        );
        close(
            &[(0, 5.0), (20_000, 3.0), (40_000, 2.0), (60_000, 3.0)],
            &along(TimewiseFn::RollingAvg(5), &[5.0, 1.0, 0.0, 6.0]),
        );
    }

    #[test]
    fn a_gap_is_bridged_by_diff() {
        let out = one(eval(
            only(0, vec![s("a", &[(0, 1.0), (60_000, 4.0)])]),
            &Node::Timewise(TimewiseFn::Derivative, fetch(0)),
        ));
        assert_eq!(out.points, points(&[(60_000, 0.05)])); // 3 over 60 s
    }

    // --- lookback ------------------------------------------------------------------------

    fn with_history() -> Fetched {
        // Buckets from −40 s; the visible window starts at 0.
        Fetched {
            series: HashMap::from([(
                (0, 0),
                vec![s("a", &[(-40_000, 1.0), (-20_000, 2.0), (0, 10.0), (20_000, 12.0)])],
            )]),
            step_ms: HashMap::from([(0, 20_000)]),
            to_ms: i64::MAX,
        }
    }

    #[test]
    fn the_first_visible_diff_uses_the_point_before_the_window() {
        let out = one(eval_from(&with_history(), 0, &Node::Timewise(TimewiseFn::Diff, fetch(0))));
        assert_eq!(out.points, points(&[(0, 8.0), (20_000, 2.0)]));
    }

    #[test]
    fn cumsum_starts_at_the_visible_window() {
        let out = one(eval_from(&with_history(), 0, &Node::Timewise(TimewiseFn::Cumsum, fetch(0))));
        assert_eq!(out.points, points(&[(0, 10.0), (20_000, 22.0)]));
    }

    #[test]
    fn nested_cumsum_of_diff_reads_one_bucket_back_sums_from_the_window() {
        let node = Node::Timewise(TimewiseFn::Cumsum, Box::new(Node::Timewise(TimewiseFn::Diff, fetch(0))));
        let out = one(eval_from(&with_history(), 0, &node));
        assert_eq!(out.points, points(&[(0, 8.0), (20_000, 10.0)]));
    }

    // --- shifts --------------------------------------------------------------------------

    #[test]
    fn a_shift_reads_the_shifted_fetch() {
        let fetched = Fetched {
            series: HashMap::from([
                ((0, 0), vec![s("now", &[(0, 1.0)])]),
                ((0, -3_600_000), vec![s("hour ago", &[(0, 7.0)])]),
            ]),
            step_ms: HashMap::from([(0, 20_000)]),
            to_ms: i64::MAX,
        };
        assert_eq!(names(&eval_from(&fetched, 0, &Node::Fetch(0))), ["now"]);
        assert_eq!(
            names(&eval_from(&fetched, 0, &Node::Shift { offset_ms: -3_600_000, inner: fetch(0) })),
            ["hour ago"]
        );
    }

    // --- default_zero --------------------------------------------------------------------

    fn window60(series: Vec<Series>) -> Fetched {
        Fetched { series: HashMap::from([((0, 0), series)]), step_ms: HashMap::from([(0, 20_000)]), to_ms: 60_000 }
    }

    #[test]
    fn default_zero_fills_every_bucket_of_the_window() {
        let out = one(eval_from(&window60(vec![s("a", &[(20_000, 5.0)])]), 0, &Node::DefaultZero(fetch(0))));
        assert_eq!(out.points, points(&[(0, 0.0), (20_000, 5.0), (40_000, 0.0)]));
    }

    #[test]
    fn default_zero_with_nothing_at_all_is_one_zero_series() {
        let out = one(eval_from(&window60(vec![]), 0, &Node::DefaultZero(fetch(0))));
        assert!(out.group_tags.is_empty());
        assert_eq!(out.points, points(&[(0, 0.0), (20_000, 0.0), (40_000, 0.0)]));
    }

    // --- autosmooth across series --------------------------------------------------------

    #[test]
    fn autosmooth_uses_one_window_for_every_series() {
        // Same noise, different levels: one window, so both smooth alike.
        let noisy = |level: f64| {
            s(
                &level.to_string(),
                &(0..60).map(|i| (i * 20_000, level + if i % 2 == 0 { 1.0 } else { -1.0 })).collect::<Vec<_>>(),
            )
        };
        let out = eval(only(0, vec![noisy(10.0), noisy(50.0)]), &Node::Timewise(TimewiseFn::Autosmooth, fetch(0)));
        let spread = |s: &Series| {
            let v: Vec<f64> = s.points.values().skip(20).copied().collect();
            max_of(&v) - min_of(&v)
        };
        assert!((spread(&out[0]) - spread(&out[1])).abs() < 1e-9);
        assert!(spread(&out[0]) < 2.0);
    }

    // --- arithmetic ----------------------------------------------------------------------

    fn two(a: Vec<Series>, b: Vec<Series>, node: &Node) -> Vec<Series> {
        let fetched = Fetched {
            series: HashMap::from([((0, 0), a), ((1, 0), b)]),
            step_ms: HashMap::from([(0, 20_000), (1, 20_000)]),
            to_ms: i64::MAX,
        };
        eval_from(&fetched, i64::MIN, node)
    }

    fn host(name: &str, ps: &[(i64, f64)]) -> Series {
        s(&format!("host:{name}"), ps)
    }

    fn arith_of(op: ArithOp, l: Node, r: Node) -> Node {
        Node::Arith(op, Box::new(l), Box::new(r))
    }

    #[test]
    fn series_pair_up_by_group() {
        let a = vec![host("x", &[(0, 10.0)]), host("y", &[(0, 20.0)]), host("only-a", &[(0, 1.0)])];
        let b = vec![host("y", &[(0, 4.0)]), host("x", &[(0, 5.0)])];
        let out = two(a, b, &arith_of(ArithOp::Divide, Node::Fetch(0), Node::Fetch(1)));
        let pairs: Vec<(Vec<String>, f64)> = out.iter().map(|s| (s.group_tags.clone(), s.points[&0])).collect();
        assert_eq!(pairs, [(vec!["host:x".to_string()], 2.0), (vec!["host:y".to_string()], 5.0)]);
    }

    #[test]
    fn an_ungrouped_side_applies_to_every_series() {
        let a = vec![host("x", &[(0, 10.0)]), host("y", &[(0, 20.0)])];
        let total = vec![Series { group_tags: vec![], points: points(&[(0, 40.0)]) }];
        let out = two(a, total, &arith_of(ArithOp::Divide, Node::Fetch(0), Node::Fetch(1)));
        assert_eq!(out.iter().map(|s| s.points[&0]).collect::<Vec<_>>(), [0.25, 0.5]);
    }

    #[test]
    fn numbers_and_time_apply_to_every_point() {
        let a = vec![host("x", &[(0, 0.25), (60_000, 0.5)])];
        let times100 = two(a.clone(), vec![], &arith_of(ArithOp::Times, Node::Fetch(0), Node::Constant(100.0)));
        assert_eq!(times100[0].points.values().copied().collect::<Vec<_>>(), [25.0, 50.0]);
        // time() is seconds: at 60 000 ms, time() − value = 60 − 0.5.
        let minus = two(a, vec![], &arith_of(ArithOp::Minus, Node::TimeOfPoint, Node::Fetch(0)));
        assert_eq!(minus[0].points[&60_000], 59.5);
    }

    #[test]
    fn a_gap_on_either_side_or_a_zero_divisor_is_a_gap() {
        let a = vec![host("x", &[(0, 1.0), (20_000, 2.0), (40_000, 3.0)])];
        let b = vec![host("x", &[(0, 1.0), (40_000, 0.0)])];
        let out = two(a, b, &arith_of(ArithOp::Divide, Node::Fetch(0), Node::Fetch(1)));
        assert_eq!(out[0].points, points(&[(0, 1.0)]));
    }

    #[test]
    fn minimum_maximum_and_pow() {
        let a = || vec![host("x", &[(0, 3.0)])];
        let b = || vec![host("x", &[(0, 2.0)])];
        let at0 = |op| two(a(), b(), &arith_of(op, Node::Fetch(0), Node::Fetch(1)))[0].points[&0];
        assert_eq!(at0(ArithOp::Minimum), 2.0);
        assert_eq!(at0(ArithOp::Maximum), 3.0);
        assert_eq!(at0(ArithOp::Power), 9.0);
    }

    #[test]
    fn a_function_over_arithmetic() {
        let a = vec![host("x", &[(0, 1.0)])];
        let b = vec![host("x", &[(0, 4.0)])];
        let node =
            Node::Pointwise(PointwiseFn::Abs, Box::new(arith_of(ArithOp::Minus, Node::Fetch(0), Node::Fetch(1))));
        assert_eq!(two(a, b, &node)[0].points[&0], 3.0);
    }
}
