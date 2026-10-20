//! Runs a plan: every query it needs, in parallel, then the response. (F#: `Query/Execute.fs`.)
//!
//! The engine never sees ClickHouse. It is handed an `Executor`, which runs SQL and returns rows;
//! the server's is backed by the driver, a test's by a list.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::TimeDelta;
use futures::future::try_join_all;
use ninjapaws_anomaly::{Anomalies, Forecast, Model, Seasonality, forecast_at};

use crate::api::v2::scalar::{ParsedScalarRequest, ScalarAggregator};
use crate::api::v2::wire::{
    DataScalarColumn, GroupScalarColumn, ScalarColumn, ScalarFormulaQueryResponse, ScalarFormulaResponseAtrributes,
    ScalarMeta, ScalarResponse, TimeseriesFormulaQueryResponse,
};
use crate::query::compile::{Row, compile, keep_from_ms};
use crate::query::evaluate::{Fetched, eval_from};
use crate::query::plan::{
    ANOMALY_WINDOW, Extra, Fill, Node, Output, Plan, QueryPlan, TimeAggregation, fetches, season_length,
    seasonal_history,
};
use crate::query::series::{
    self, OutputResult, OutputSeries, Predicted, Series, SeriesExtra, combine, from_rows, grid, max_of, mean, min_of,
    sum, zero_fill,
};
use crate::sql::{Sql, TenantId};
use crate::unix_time;

/// What runs the engine's SQL. A failure is the database's message.
pub trait Executor: Sync {
    fn rows(&self, sql: Sql) -> impl Future<Output = Result<Vec<Row>, String>> + Send;
}

/// Why a query has no answer.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryError {
    /// What is wrong with the request, every problem named: the caller's 400.
    Invalid(Vec<String>),
    /// The database failed while running it: ours, a 500.
    Failed(String),
}

/// One output's seasonal anomaly models, by the group's tags: the costly half of an anomaly
/// output, which a monitor fits once and reuses.
pub type AnomalyModels = HashMap<Vec<String>, ninjapaws_anomaly::Model>;

/// Each group's series as points, by the group's tags.
type Histories = HashMap<Vec<String>, Vec<(i64, f64)>>;

fn points_of(s: &Series) -> Vec<(i64, f64)> {
    s.points.iter().map(|(&t, &v)| (t, v)).collect()
}

/// One (query, shift)'s series. A shifted read is the same query over a shifted window; its
/// times are moved back onto the present one.
async fn fetch_one<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    window: &Plan,
    i: usize,
    offset: i64,
) -> Result<Vec<Series>, String> {
    let query = &window.queries[i];
    let shifted = Plan {
        from: unix_time::of_millis(window.from.saturating_add(offset)),
        to: unix_time::of_millis(window.to.saturating_add(offset)),
        ..window.clone()
    };
    let rows = execute.rows(compile(tenant, &shifted, query)).await?;
    let keep_from = keep_from_ms(&shifted, query);

    let combined = match query.fill {
        Fill::NoFill => from_rows(&query.group_by, keep_from, rows),
        Fill::Within { .. } => combine(&query.group_by, query.space_agg, query.fill, keep_from, rows),
    };

    // "COUNT or RATE metrics queried as as_count() or as_rate() ... are always aligned as 0"
    // (docs: dashboards/functions/interpolation): an empty bucket there is zero events, not
    // unknown.
    let aligned = match query.time_agg {
        TimeAggregation::AsCount | TimeAggregation::AsRate => {
            let times: Vec<i64> = grid(query.step.num_milliseconds(), keep_from, shifted.to).collect();
            combined.into_iter().map(|s| zero_fill(&times, s)).collect()
        }
        TimeAggregation::Plain(_) => combined,
    };

    Ok(aligned
        .into_iter()
        .map(|s| Series { points: s.points.into_iter().map(|(t, v)| (t - offset, v)).collect(), ..s })
        .collect())
}

/// Reads what `nodes` need over `window`, at the steps in `window.queries`.
///
/// Only the queries some node uses; one named but never shown is not worth a trip to ClickHouse.
/// Each (query, shift) runs once however many nodes use it, and all run in parallel.
async fn fetch_all<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    window: &Plan,
    nodes: &[&Node],
) -> Result<Fetched, String> {
    let mut keys: Vec<(usize, i64)> = Vec::new();
    for key in nodes.iter().flat_map(|n| fetches(n)) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let series = try_join_all(keys.iter().map(|&(i, offset)| fetch_one(execute, tenant, window, i, offset))).await?;

    Ok(Fetched {
        step_ms: keys.iter().map(|&(i, _)| (i, window.queries[i].step.num_milliseconds())).collect(),
        series: keys.into_iter().zip(series).collect(),
        to_ms: window.to,
    })
}

/// The node's series over `length` back from the plan's end, at the seasonal model's step, from
/// the first value returned.
async fn seasonal_read<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    season: Seasonality,
    length: TimeDelta,
    node: &Node,
) -> Result<Histories, String> {
    let step = TimeDelta::milliseconds(season.step_ms());
    let window = Plan {
        from: plan.to.saturating_sub(length.num_milliseconds()),
        step,
        queries: plan.queries.iter().map(|q| QueryPlan { step, ..q.clone() }).collect(),
        ..plan.clone()
    };
    let h = fetch_all(execute, tenant, &window, &[node]).await?;

    let series = match node {
        // Zeros only from a series' first point: before it the series did not exist, and a model
        // fitted on zeros there would skip learning.
        Node::DefaultZero(inner) => eval_from(&h, window.from, inner)
            .into_iter()
            .map(|s| match s.points.keys().next() {
                Some(&first) => zero_fill(&grid(season.step_ms(), first, h.to_ms).collect::<Vec<_>>(), s),
                None => s,
            })
            .collect(),
        _ => eval_from(&h, window.from, node),
    };
    Ok(series.into_iter().map(|s| (s.group_tags.clone(), points_of(&s))).collect())
}

/// Each group's model, fitted on the node's seasonal history, and that history.
async fn fit_groups<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    algorithm: Anomalies,
    season: Seasonality,
    node: &Node,
) -> Result<(Histories, AnomalyModels), String> {
    let histories =
        seasonal_read(execute, tenant, plan, season, seasonal_history(plan.retention, season), node).await?;
    let models = histories
        .iter()
        .filter_map(|(tags, h)| Model::fit_window(algorithm, h, plan.from, plan.to).map(|m| (tags.clone(), m)))
        .collect();
    Ok((histories, models))
}

fn seasonality(algorithm: Anomalies) -> Option<Seasonality> {
    match algorithm {
        Anomalies::Robust(season) | Anomalies::Agile(season) => Some(season),
        Anomalies::Basic => None,
    }
}

/// An output's series, with what `extra` adds beside them. The extras read history before the
/// window, so the node is evaluated from `history_from`, and the series trimmed back to the
/// window; the limit picks among the trimmed series.
fn with_extras(
    fetched: &Fetched,
    o: &Output,
    from_ms: i64,
    history_from: i64,
    extra: impl Fn(&Series) -> SeriesExtra,
) -> Vec<OutputSeries> {
    let visible = |s: &Series| Series {
        group_tags: s.group_tags.clone(),
        points: s.points.range(from_ms..).map(|(&t, &v)| (t, v)).collect(),
    };
    let full = eval_from(fetched, history_from, &o.node);
    let kept: HashSet<Vec<String>> =
        series::limit(o.limit, full.iter().map(visible).collect()).into_iter().map(|s| s.group_tags).collect();
    full.iter()
        .filter(|s| kept.contains(&s.group_tags))
        .map(|s| OutputSeries { data: visible(s), extra: extra(s) })
        .collect()
}

fn predicted(predictions: Vec<ninjapaws_anomaly::Prediction>) -> Vec<Predicted> {
    predictions.into_iter().map(|p| Predicted { time_ms: p.time, value: p.value, range: p.band }).collect()
}

async fn eval_output<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    fetched: &Fetched,
    models: Option<&HashMap<usize, AnomalyModels>>,
    o: &Output,
) -> Result<Vec<OutputSeries>, String> {
    let from_ms = plan.from;
    let to_ms = plan.to;
    let window_ms = to_ms - from_ms;
    let step_ms = fetches(&o.node)
        .first()
        .and_then(|(i, _)| fetched.step_ms.get(i).copied())
        .unwrap_or(plan.step.num_milliseconds());
    let lagging_from = from_ms.saturating_sub(ANOMALY_WINDOW as i64 * step_ms);

    let lines = match o.extra {
        Extra::None => series::limit(o.limit, eval_from(fetched, from_ms, &o.node))
            .into_iter()
            .map(|data| OutputSeries { data, extra: SeriesExtra::default() })
            .collect(),

        Extra::Anomalies { algorithm: Anomalies::Basic, bounds } => {
            with_extras(fetched, o, from_ms, lagging_from, |s| {
                let checked = ninjapaws_anomaly::anomalies(Anomalies::Basic, &points_of(s), from_ms, bounds);
                SeriesExtra { band: Some(checked.into_iter().map(|p| (p.time, p.band)).collect()), forecast: None }
            })
        }

        // The band is `bounds` standard deviations around the model's expectation: the model's
        // spread, from its fit on history, so a lasting anomaly in the window cannot widen the
        // band around itself ("predictions remain constant even through long-lasting
        // anomalies"), combined with the spread inside a model bucket, which covers a query finer
        // than the model (20 s points around an hourly expectation), measured as each point's
        // distance from its own bucket's mean, which a level shift does not change. The crate's
        // `Model::check_with` computes it.
        Extra::Anomalies { algorithm: algorithm @ (Anomalies::Robust(season) | Anomalies::Agile(season)), bounds } => {
            let empty = AnomalyModels::new();
            let given = models.map(|m| m.get(&o.query_index).unwrap_or(&empty));
            let mut fitted = AnomalyModels::new();
            let mut recent: Option<Histories> = None;
            match given {
                None => {
                    let (histories, models) = fit_groups(execute, tenant, plan, algorithm, season, &o.node).await?;
                    fitted = models;
                    recent = Some(histories);
                }
                Some(given) if matches!(algorithm, Anomalies::Agile(_)) && !given.is_empty() => {
                    // Two seasons for agile's errors to settle before the window, and one to
                    // spare for gaps.
                    let length = season_length(season) * 3 + TimeDelta::milliseconds(window_ms);
                    recent = Some(seasonal_read(execute, tenant, plan, season, length, &o.node).await?);
                }
                Some(_) => {}
            }
            let models = given.unwrap_or(&fitted);

            with_extras(fetched, o, from_ms, lagging_from, |s| {
                let recent = recent.as_ref().and_then(|r| r.get(&s.group_tags)).map(Vec::as_slice);
                let checked =
                    models.get(&s.group_tags).and_then(|m| m.check_with(recent, to_ms, &points_of(s), from_ms, bounds));
                SeriesExtra {
                    band: checked.map(|ps| ps.into_iter().map(|p| (p.time, p.band)).collect()),
                    forecast: None,
                }
            })
        }

        Extra::Forecast { deviations, method: method @ Forecast::Seasonal(season), horizon, .. } => {
            let histories =
                seasonal_read(execute, tenant, plan, season, seasonal_history(plan.retention, season), &o.node).await?;
            let horizon_ms = horizon.map_or(window_ms, |h| h.num_milliseconds());
            let future: Vec<i64> = grid(step_ms, to_ms, to_ms.saturating_add(horizon_ms)).collect();

            with_extras(fetched, o, from_ms, from_ms, |s| {
                // "Requires at least two seasons of history."
                let points = histories
                    .get(&s.group_tags)
                    .map(|h| forecast_at(method, h, to_ms, &future, deviations))
                    .unwrap_or_default();
                SeriesExtra { band: None, forecast: (!points.is_empty()).then(|| predicted(points)) }
            })
        }

        Extra::Forecast { deviations, method: method @ Forecast::Linear(_), history, horizon } => {
            let history_ms = history.map_or(window_ms, |h| h.num_milliseconds());
            let horizon_ms = horizon.map_or(window_ms, |h| h.num_milliseconds());
            let future: Vec<i64> = grid(step_ms, to_ms, to_ms.saturating_add(horizon_ms)).collect();
            let history_from = to_ms.saturating_sub(history_ms);

            with_extras(fetched, o, from_ms, from_ms.min(history_from), |s| {
                let pts: Vec<(i64, f64)> = s.points.range(history_from..).map(|(&t, &v)| (t, v)).collect();
                if pts.len() < 2 {
                    return SeriesExtra::default();
                }
                SeriesExtra {
                    band: None,
                    forecast: Some(predicted(forecast_at(method, &pts, to_ms, &future, deviations))),
                }
            })
        }
    };
    Ok(lines)
}

/// Runs a timeseries plan. With `models` from `fit` no whole history is read and a group without
/// one has no band; None fits them here.
pub async fn evaluate<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    models: Option<&HashMap<usize, AnomalyModels>>,
) -> Result<Vec<OutputResult>, String> {
    let nodes: Vec<&Node> = plan.outputs.iter().map(|o| &o.node).collect();
    let fetched = fetch_all(execute, tenant, plan, &nodes).await?;
    let lines =
        try_join_all(plan.outputs.iter().map(|o| eval_output(execute, tenant, plan, &fetched, models, o))).await?;
    Ok(plan.outputs.iter().zip(lines).map(|(o, lines)| OutputResult { query_index: o.query_index, lines }).collect())
}

/// Each seasonal anomaly output's models by query_index: the costly half of `evaluate`.
pub async fn fit<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
) -> Result<HashMap<usize, AnomalyModels>, String> {
    let seasonal = plan.outputs.iter().filter_map(|o| match o.extra {
        Extra::Anomalies { algorithm, .. } => seasonality(algorithm).map(|season| (o, algorithm, season)),
        Extra::None | Extra::Forecast { .. } => None,
    });
    let fitted = try_join_all(seasonal.map(|(o, algorithm, season)| async move {
        let (_, models) = fit_groups(execute, tenant, plan, algorithm, season, &o.node).await?;
        Ok::<_, String>((o.query_index, models))
    }))
    .await?;
    Ok(fitted.into_iter().collect())
}

/// Runs a timeseries plan into Datadog's response shape.
pub async fn run<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
) -> Result<TimeseriesFormulaQueryResponse, String> {
    let outputs = evaluate(execute, tenant, plan, None).await?;
    Ok(crate::query::series::response_with(&outputs))
}

// --- scalar ----------------------------------------------------------------------------------

/// One series' points in the window as one value.
fn reduce(agg: ScalarAggregator, step_ms: i64, points: &[(i64, f64)]) -> Option<f64> {
    let &(_, last) = points.last()?;
    let values: Vec<f64> = points.iter().map(|p| p.1).collect();
    Some(match agg {
        ScalarAggregator::Avg => mean(&values),
        ScalarAggregator::Min => min_of(&values),
        ScalarAggregator::Max => max_of(&values),
        ScalarAggregator::Sum => sum(&values),
        ScalarAggregator::Last => last,
        ScalarAggregator::L2norm => sum(&values.iter().map(|v| v * v).collect::<Vec<_>>()).sqrt(),
        ScalarAggregator::Area => sum(&values) * step_ms as f64 / 1000.0,
    })
}

/// Runs a scalar plan: each query's window reduced by its aggregator first (spec:
/// MetricsScalarQuery.aggregator), so `a / b` with avg is avg(a) / avg(b); then the formulas over
/// those one-point series.
///
/// WARNING(undocumented): a sum or area of a gauge adds up the window's buckets as a graph of the
/// query draws them.
pub async fn evaluate_scalar<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    aggregators: &[ScalarAggregator],
) -> Result<Vec<Vec<Series>>, QueryError> {
    if plan.outputs.iter().any(|o| o.extra != Extra::None) {
        return Err(QueryError::Invalid(vec!["anomalies() and forecast() are not available in scalar queries".into()]));
    }
    let nodes: Vec<&Node> = plan.outputs.iter().map(|o| &o.node).collect();
    let fetched = fetch_all(execute, tenant, plan, &nodes).await.map_err(QueryError::Failed)?;
    let (from_ms, to_ms) = (plan.from, plan.to);

    let Fetched { series, step_ms, to_ms: fetched_to } = fetched;
    let reduce_one = |i: usize, s: Series| {
        let window: Vec<(i64, f64)> = s.points.range(from_ms..to_ms).map(|(&t, &v)| (t, v)).collect();
        let value = reduce(aggregators[i], step_ms[&i], &window);
        Series { points: value.map(|v| (from_ms, v)).into_iter().collect(), ..s }
    };
    let reduced = Fetched {
        series: series
            .into_iter()
            .map(|((i, offset), ss)| ((i, offset), ss.into_iter().map(|s| reduce_one(i, s)).collect()))
            .collect(),
        step_ms: step_ms.clone(),
        to_ms: fetched_to,
    };

    Ok(plan.outputs.iter().map(|o| series::limit(o.limit, eval_from(&reduced, from_ms, &o.node))).collect())
}

fn tag_set(tags: &[String]) -> BTreeSet<String> {
    tags.iter().cloned().collect()
}

fn split_tag(tag: &str) -> (&str, &str) {
    match tag.split_once(':') {
        Some((key, value)) => (key, value),
        None => (tag, tag),
    }
}

/// Runs a scalar plan into Datadog's response shape: group columns, then one number column per
/// formula.
pub async fn run_scalar<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    plan: &Plan,
    request: &ParsedScalarRequest,
) -> Result<ScalarFormulaQueryResponse, QueryError> {
    let outputs = evaluate_scalar(execute, tenant, plan, &request.aggregators).await?;
    let from_ms = plan.from;

    // Rows: every group any column has, in the order they first appear: the first formula's,
    // limit and all, then any others'.
    //
    // WARNING(undocumented): a limit on one formula of several. The rows are the groups every
    // limited formula kept: a top 5 with a second, unlimited column is still five rows, not five
    // plus the rest with a null first column.
    let limited: Vec<HashSet<BTreeSet<String>>> = plan
        .outputs
        .iter()
        .zip(&outputs)
        .filter(|(o, _)| o.limit.is_some())
        .map(|(_, series)| series.iter().map(|s| tag_set(&s.group_tags)).collect())
        .collect();

    let mut seen = HashSet::new();
    let rows: Vec<&Vec<String>> = outputs
        .iter()
        .flatten()
        .map(|s| &s.group_tags)
        .filter(|tags| seen.insert(tag_set(tags)))
        .filter(|tags| limited.iter().all(|kept| kept.contains(&tag_set(tags))))
        .collect();

    let keys: Vec<&str> = match rows.iter().find(|tags| !tags.is_empty()) {
        Some(tags) => tags.iter().map(|t| split_tag(t).0).collect(),
        None => vec![],
    };

    let group_columns = keys.iter().map(|&key| {
        ScalarColumn::Group(GroupScalarColumn {
            name: key.to_string(),
            kind: "group".into(),
            values: rows
                .iter()
                .map(|tags| {
                    tags.iter().map(|t| split_tag(t)).filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()).collect()
                })
                .collect(),
        })
    });

    let data_columns = request.column_names.iter().zip(&outputs).map(|(name, series)| {
        let by_group: HashMap<BTreeSet<String>, &Series> = series.iter().map(|s| (tag_set(&s.group_tags), s)).collect();
        ScalarColumn::Data(DataScalarColumn {
            name: name.clone(),
            kind: "number".into(),
            values: rows
                .iter()
                .map(|tags| by_group.get(&tag_set(tags)).and_then(|s| s.points.get(&from_ms).copied()))
                .collect(),
            meta: ScalarMeta { unit: None },
        })
    });

    Ok(ScalarFormulaQueryResponse {
        data: ScalarResponse {
            kind: "scalar_response".into(),
            attributes: ScalarFormulaResponseAtrributes { columns: group_columns.chain(data_columns).collect() },
        },
    })
}

/// An executor backed by a function of the SQL, as the tests' stand-in for ClickHouse.
#[cfg(test)]
pub(crate) struct Fake<F>(pub F);

#[cfg(test)]
impl<F: Fn(&Sql) -> Vec<Row> + Sync> Executor for Fake<F> {
    fn rows(&self, sql: Sql) -> impl Future<Output = Result<Vec<Row>, String>> + Send {
        std::future::ready(Ok((self.0)(&sql)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::api::v2::{scalar, timeseries};
    use crate::query::plan::{self, DEFAULT_RETENTION};
    use crate::sql::SqlValue;

    fn tenant() -> TenantId {
        TenantId("default".into())
    }

    fn row(groups: &[&str], bucket_ms: i64, value: f64) -> Row {
        Row { groups: groups.iter().map(|g| Some(g.to_string())).collect(), series_id: None, bucket_ms, value }
    }

    fn int64_param(sql: &Sql, prefix: &str) -> i64 {
        sql.parameters
            .iter()
            .find_map(|(n, v)| match v {
                SqlValue::Int64(x) if n.starts_with(prefix) => Some(*x),
                _ => None,
            })
            .unwrap()
    }

    /// Every window here lies within the raw retention counted back from it.
    fn plan_now(body: &str) -> Plan {
        let request = timeseries::read(body).unwrap_or_else(|es| panic!("{}", es.join("\n")));
        plan::plan(DEFAULT_RETENTION, 86_400_000, &request).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    #[test]
    fn evaluate_is_send() {
        fn send<T: Send>(_: T) {}
        let fake = Fake(|_: &Sql| vec![]);
        let plan = Plan {
            from: 0,
            to: 1,
            step: TimeDelta::seconds(20),
            queries: vec![],
            outputs: vec![],
            retention: DEFAULT_RETENTION,
        };
        send(evaluate(&fake, &tenant(), &plan, None));
        send(fit(&fake, &tenant(), &plan));
        send(evaluate_scalar(&fake, &tenant(), &plan, &[]));
    }

    // --- the whole run, with ClickHouse replaced by a function ---------------------------

    #[tokio::test]
    async fn a_request_runs_end_to_end() {
        let body = r#"{"data":{"type":"timeseries_request","attributes":{"from":0,"to":3600000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*} by {host}"},
                       {"data_source":"metrics","name":"unused","query":"avg:other{*}"}],
            "formulas":[{"formula":"a"}]}}}"#;
        let asked = Mutex::new(Vec::new());
        let fake = Fake(|sql: &Sql| {
            asked.lock().unwrap().push(sql.clone());
            vec![row(&["web-1"], 0, 41.0), row(&["web-1"], 20_000, 40.5)]
        });

        let r = run(&fake, &tenant(), &plan_now(body)).await.unwrap().data.attributes;

        // Only the query a formula uses went to the database.
        let asked = asked.into_inner().unwrap();
        assert_eq!(asked.len(), 1);
        let metric = asked[0].parameters.iter().find(|(n, _)| n.starts_with("metric")).unwrap();
        assert_eq!(metric.1, SqlValue::String("cpu".into()));
        assert_eq!(r.series.len(), 1);
        assert_eq!(r.series[0].group_tags, ["host:web-1"]);
        assert_eq!(r.times, [0, 20_000]);
        assert_eq!(r.values, [vec![Some(41.0), Some(40.5)]]);
    }

    #[tokio::test]
    async fn a_shifted_read_queries_the_past_and_draws_it_now() {
        let body = r#"{"data":{"type":"timeseries_request","attributes":{"from":86400000,"to":90000000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}"}],
            "formulas":[{"formula":"a"},{"formula":"day_before(a)"}]}}}"#;
        let asked = Mutex::new(Vec::new());
        let fake = Fake(|sql: &Sql| {
            let from = int64_param(sql, "from");
            asked.lock().unwrap().push(from);
            // Whatever window was asked, one point at its start, valued by window.
            vec![Row { groups: vec![], series_id: None, bucket_ms: from, value: if from == 0 { 1.0 } else { 2.0 } }]
        });

        let r = run(&fake, &tenant(), &plan_now(body)).await.unwrap().data.attributes;

        // Two reads: the window itself, and the same window a day earlier.
        let mut asked = asked.into_inner().unwrap();
        asked.sort();
        assert_eq!(asked, [0, 86_400_000]);
        // Both drawn at the present window's start.
        assert_eq!(r.times, [86_400_000]);
        assert_eq!(r.values, [vec![Some(2.0)], vec![Some(1.0)]]);
    }

    #[tokio::test]
    async fn a_forecast_runs_end_to_end_past_the_window() {
        let body = r#"{"data":{"type":"timeseries_request","attributes":{"from":0,"to":100000,"interval":20000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}"}],
            "formulas":[{"formula":"forecast(a, 'linear', 1, horizon='40s')"}]}}}"#;
        // A line: value = bucket seconds.
        let fake = Fake(|_: &Sql| (0..5).map(|i| row(&[], i * 20_000, (i * 20) as f64)).collect());

        let a = run(&fake, &tenant(), &plan_now(body)).await.unwrap().data.attributes;
        assert_eq!(a.times, [0, 20_000, 40_000, 60_000, 80_000]); // the window only
        let f = a.ninjacat_forecast.unwrap()[0].clone().unwrap();
        assert_eq!(f.times, [100_000, 120_000]);
        assert!((f.values[0] - 100.0).abs() < 1e-6, "{f:?}");
        assert!((f.values[1] - 120.0).abs() < 1e-6, "{f:?}");
    }

    fn shape(t: i64) -> f64 {
        50.0 + 10.0 * (2.0 * std::f64::consts::PI * ((t / 60_000) % 60) as f64 / 60.0).sin()
    }

    /// The buckets of the window the SQL asks for, valued by `value`.
    fn buckets(sql: &Sql, value: impl Fn(i64) -> f64) -> Vec<Row> {
        let (from, until, step) = (int64_param(sql, "from"), int64_param(sql, "to"), int64_param(sql, "step") * 1000);
        let first = (from + step - 1) / step * step;
        (first..until).step_by(step as usize).map(|t| row(&[], t, value(t))).collect()
    }

    #[tokio::test]
    async fn a_seasonal_forecast_fits_the_history_and_continues_the_season() {
        // Hourly seasonality: every hour the same shape, minute by minute. The airline model on a
        // perfectly periodic history predicts the shape again.
        let body = r#"{"data":{"type":"timeseries_request","attributes":{"from":36000000,"to":39600000,"interval":60000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}.fill(null)"}],
            "formulas":[{"formula":"forecast(a, 'seasonal', 1, seasonality='hourly', horizon='10m')"}]}}}"#;
        let fake = Fake(|sql: &Sql| buckets(sql, shape));

        let a = run(&fake, &tenant(), &plan_now(body)).await.unwrap().data.attributes;
        let f = a.ninjacat_forecast.unwrap()[0].clone().unwrap();
        assert_eq!(f.times.len(), 10);
        for (t, v) in f.times.iter().zip(&f.values) {
            assert!((shape(*t) - v).abs() < 1e-6, "{t}: {v}");
        }
    }

    /// Hourly-seasonal data at 1-minute buckets, with a lasting +20 shift from `shift_at` on; the
    /// engine asks for whatever window and step it needs.
    fn seasonal_with_shift(shift_at: i64) -> impl Fn(&Sql) -> Vec<Row> + Sync {
        move |sql: &Sql| {
            buckets(sql, |t| {
                // a little texture, so residuals are not zero
                shape(t) + 0.3 * (t as f64 / 7e4).sin() + if t >= shift_at { 20.0 } else { 0.0 }
            })
        }
    }

    /// One query of cpu, `interval` ms apart, from `from` to `until`.
    fn plan_at(interval: i64, from: i64, until: i64, formula: &str) -> Plan {
        plan_now(&format!(
            r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":{from},"to":{until},"interval":{interval},
            "queries":[{{"data_source":"metrics","name":"a","query":"avg:cpu{{*}}.fill(null)"}}],
            "formulas":[{{"formula":"{formula}"}}]}}}}}}"#
        ))
    }

    /// Hour 10 to hour 11, every minute.
    fn hour_ten(formula: &str) -> Plan {
        plan_at(60_000, 36_000_000, 39_600_000, formula)
    }

    async fn band_after_shift(algorithm: &str) -> Vec<bool> {
        // The shift lands at 10:20.
        let plan = hour_ten(&format!("anomalies(a, '{algorithm}', 3, seasonality='hourly')"));
        let a = run(&Fake(seasonal_with_shift(37_200_000)), &tenant(), &plan).await.unwrap().data.attributes;
        let b = a.ninjacat_bounds.unwrap()[0].clone().unwrap();
        // Points 30–60 minutes into the window: well after the shift.
        (0..a.times.len())
            .filter(|&i| a.times[i] >= 37_800_000)
            .filter_map(|i| match (a.values[0][i], b.lower[i], b.upper[i]) {
                (Some(v), Some(lo), Some(hi)) => Some(v < lo || v > hi),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn robust_keeps_expecting_the_season_through_a_lasting_shift() {
        let outside = band_after_shift("robust").await;
        assert!(!outside.is_empty());
        assert!(outside.iter().all(|&o| o), "every shifted point should lie outside robust's band");
    }

    #[tokio::test]
    async fn agile_follows_a_lasting_shift() {
        let outside = band_after_shift("agile").await;
        assert!(!outside.is_empty());
        assert!(
            outside.iter().filter(|&&o| o).count() < outside.len() / 4,
            "agile should have caught up with the shift"
        );
    }

    // --- anomaly models fitted apart from the window they judge --------------------------

    fn bands(outputs: &[OutputResult]) -> (BTreeMap<i64, f64>, BTreeMap<i64, ninjapaws_anomaly::Band>) {
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].lines.len(), 1);
        let line = &outputs[0].lines[0];
        (line.data.points.clone(), line.extra.band.clone().unwrap_or_default())
    }

    /// How far each point lies from the middle of its band.
    fn off_centre(outputs: &[OutputResult]) -> Vec<f64> {
        let (points, band) = bands(outputs);
        points.iter().filter_map(|(t, v)| band.get(t).map(|r| (v - (r.lower + r.upper) / 2.0).abs())).collect()
    }

    #[tokio::test]
    async fn robust_fitted_apart_draws_the_band_evaluate_draws() {
        let plan = hour_ten("anomalies(a, 'robust', 3, seasonality='hourly')");
        let fake = Fake(seasonal_with_shift(37_200_000));
        let models = fit(&fake, &tenant(), &plan).await.unwrap();
        assert!(models[&0].contains_key(&Vec::<String>::new()));
        let fitted_here = evaluate(&fake, &tenant(), &plan, None).await.unwrap();
        let fitted_apart = evaluate(&fake, &tenant(), &plan, Some(&models)).await.unwrap();
        assert_eq!(bands(&fitted_here).1, bands(&fitted_apart).1);
    }

    #[tokio::test]
    async fn models_fitted_at_one_window_judge_a_later_one_in_phase() {
        for algorithm in ["robust", "agile"] {
            let formula = format!("anomalies(a, '{algorithm}', 3, seasonality='hourly')");
            let fake = Fake(seasonal_with_shift(i64::MAX));
            let models = fit(&fake, &tenant(), &hour_ten(&formula)).await.unwrap();
            // Half an hour later, 45 minutes: not a whole season, so a profile out of phase would
            // miss the season of amplitude 10 by up to 14.
            let later = plan_at(60_000, 39_600_000, 42_300_000, &formula);
            for given in [None, Some(&models)] {
                let distances = off_centre(&evaluate(&fake, &tenant(), &later, given).await.unwrap());
                assert_eq!(distances.len(), 45, "{algorithm}");
                assert!(max_of(&distances) <= 1.0, "{algorithm}: {}", max_of(&distances));
            }
        }
    }

    #[tokio::test]
    async fn a_window_that_ends_off_the_models_step_is_expected_in_phase_to_its_last_point() {
        for algorithm in ["robust", "agile"] {
            let formula = format!("anomalies(a, '{algorithm}', 3, seasonality='hourly')");
            let fake = Fake(seasonal_with_shift(i64::MAX));
            // Points every 15 s, the window ending half a minute into a model bucket.
            let plan = plan_at(15_000, 36_030_000, 39_630_000, &formula);
            let models = fit(&fake, &tenant(), &plan).await.unwrap();
            for given in [None, Some(&models)] {
                let distances = off_centre(&evaluate(&fake, &tenant(), &plan, given).await.unwrap());
                assert_eq!(distances.len(), 240, "{algorithm}");
                assert!(max_of(&distances) <= 1.0, "{algorithm}: {}", max_of(&distances));
            }
        }
    }

    #[tokio::test]
    async fn agile_with_a_model_reads_a_few_seasons_not_the_whole_history() {
        let reads = Mutex::new(Vec::new());
        let data = seasonal_with_shift(i64::MAX);
        let fake = Fake(|sql: &Sql| {
            reads.lock().unwrap().push(int64_param(sql, "to") - int64_param(sql, "from"));
            data(sql)
        });
        let plan = hour_ten("anomalies(a, 'agile', 3, seasonality='hourly')");
        let models = fit(&fake, &tenant(), &plan).await.unwrap();
        let fit_read = reads.lock().unwrap().drain(..).max().unwrap();
        evaluate(&fake, &tenant(), &plan, Some(&models)).await.unwrap();
        let evaluate_read = reads.lock().unwrap().iter().copied().max().unwrap();
        // Six seasons to fit; three and the window to run.
        let minutes = |m: i64| m * 60_000;
        assert_eq!(fit_read - minutes(61), minutes(6 * 60));
        assert_eq!(evaluate_read - minutes(61), minutes(4 * 60));
    }

    #[tokio::test]
    async fn a_group_without_a_model_has_no_band() {
        let plan = hour_ten("anomalies(a, 'robust', 3, seasonality='hourly')");
        let outputs =
            evaluate(&Fake(seasonal_with_shift(i64::MAX)), &tenant(), &plan, Some(&HashMap::new())).await.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].lines.len(), 1);
        let line = &outputs[0].lines[0];
        assert!(!line.data.points.is_empty());
        assert!(line.extra.band.is_none());
    }

    // --- scalar --------------------------------------------------------------------------

    fn scalar_body(queries: &str, formulas: &str) -> String {
        format!(
            r#"{{"data":{{"type":"scalar_request","attributes":{{"from":0,"to":100000,"queries":[{queries}],"formulas":[{formulas}]}}}}}}"#
        )
    }

    /// Rows every 20 s: host a at 1, 2, 3; host b at 10, 20, 30, for query x. Query y is
    /// ungrouped: 4, 4, 4.
    fn scalar_rows(sql: &Sql) -> Vec<Row> {
        let metric = sql.parameters.iter().find(|(n, _)| n.starts_with("metric")).unwrap();
        match &metric.1 {
            SqlValue::String(m) if m == "x" => [(0, 1.0), (20_000, 2.0), (40_000, 3.0)]
                .iter()
                .flat_map(|&(t, v)| [row(&["a"], t, v), row(&["b"], t, v * 10.0)])
                .collect(),
            _ => [0, 20_000, 40_000].iter().map(|&t| row(&[], t, 4.0)).collect(),
        }
    }

    /// The response's columns, as JSON values.
    async fn run_scalar_json(queries: &str, formulas: &str) -> Vec<serde_json::Value> {
        let request = scalar::read(&scalar_body(queries, formulas)).unwrap_or_else(|es| panic!("{}", es.join("\n")));
        let plan = plan::plan(DEFAULT_RETENTION, 86_400_000, &request.request)
            .unwrap_or_else(|es| panic!("{}", es.join("\n")));
        let response = run_scalar(&Fake(scalar_rows), &TenantId("t".into()), &plan, &request).await.unwrap();
        response.data.attributes.columns.iter().map(|c| serde_json::to_value(c).unwrap()).collect()
    }

    #[tokio::test]
    async fn one_value_per_group_a_group_column_per_key() {
        let columns = run_scalar_json(
            r#"{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"avg"}"#,
            "",
        )
        .await;
        assert!(columns.contains(&json!({"name":"host","type":"group","values":[["a"],["b"]]})), "{columns:?}");
        assert!(columns.contains(&json!({"name":"x","type":"number","values":[2.0,20.0],"meta":{}})), "{columns:?}");
    }

    #[tokio::test]
    async fn each_aggregator() {
        async fn value(agg: &str) -> f64 {
            let query = format!(
                r#"{{"data_source":"metrics","name":"y","query":"avg:y{{*}}.fill(null)","aggregator":"{agg}"}}"#
            );
            let columns = run_scalar_json(&query, "").await;
            columns[0]["values"][0].as_f64().unwrap()
        }
        assert_eq!(value("avg").await, 4.0);
        assert_eq!(value("mean").await, 4.0);
        assert_eq!(value("sum").await, 12.0);
        assert_eq!(value("last").await, 4.0);
        assert!((value("l2norm").await - 48f64.sqrt()).abs() < 1e-9);
        // A 100 s window takes a 2 s step (Datadog's table): three points of 4, each 2 s wide.
        assert_eq!(value("area").await, 12.0 * 2.0);
    }

    #[tokio::test]
    async fn formulas_run_on_the_reduced_values() {
        // avg(x) / avg(y): host a 2 / 4, host b 20 / 4.
        let columns = run_scalar_json(
            r#"{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"avg"},{"data_source":"metrics","name":"y","query":"avg:y{*}.fill(null)","aggregator":"avg"}"#,
            r#"{"formula":"x / y * 100"}"#,
        )
        .await;
        assert!(
            columns.contains(&json!({"name":"x / y * 100","type":"number","values":[50.0,500.0],"meta":{}})),
            "{columns:?}"
        );
    }

    #[tokio::test]
    async fn a_limit_orders_and_trims_the_rows() {
        let columns = run_scalar_json(
            r#"{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"max"}"#,
            r#"{"formula":"x","limit":{"count":1,"order":"desc"}}"#,
        )
        .await;
        assert!(columns.contains(&json!({"name":"host","type":"group","values":[["b"]]})), "{columns:?}");
        assert!(columns.contains(&json!({"name":"x","type":"number","values":[30.0],"meta":{}})), "{columns:?}");
    }

    #[tokio::test]
    async fn a_limit_on_one_formula_limits_the_rows_of_all() {
        let columns = run_scalar_json(
            r#"{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"max"}"#,
            r#"{"formula":"x","limit":{"count":1,"order":"desc"}},{"formula":"x * 2"}"#,
        )
        .await;
        assert!(columns.contains(&json!({"name":"host","type":"group","values":[["b"]]})), "{columns:?}");
        assert!(columns.contains(&json!({"name":"x * 2","type":"number","values":[60.0],"meta":{}})), "{columns:?}");
    }
}
