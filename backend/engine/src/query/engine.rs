//! The query engine as Rust code calls it: the panel, monitors, anything that is not an HTTP
//! client.
//!
//! Queries and formulas go in as the same strings Datadog takes; results come back as plain
//! records, with no JSON shape, parallel lists or "key:value" strings to take apart. The HTTP
//! endpoints run the same engine and turn its results into Datadog's response shape; this module
//! turns them into these.
//!
//! ```text
//! let request = TimeseriesRequest {
//!     from: now - 3_600_000, to: now, interval: None,
//!     queries: vec![Query { name: "cpu".into(), query: "avg:system.cpu.user{env:prod} by {host}".into() }],
//!     formulas: vec![],
//! };
//! match timeseries(&executor, &tenant, DEFAULT_RETENTION, now, &request).await {
//!     Ok(results) => for s in &results[0].series { println!("{:?}: {} points", s.tags, s.points.len()) },
//!     Err(QueryError::Invalid(problems)) => …,
//!     Err(QueryError::Failed(message)) => …,
//! }
//! ```

use chrono::TimeDelta;
use ninjapaws_anomaly::Band;

use crate::api::v2::scalar::ScalarAggregator;
use crate::api::v2::timeseries::{self, ParsedFormula, ParsedQuery, ParsedTimeseriesRequest};
use crate::api::v2::wire::{MetricsTimeseriesQuery, QueryFormula, TimeseriesFormulaRequestAttributes};
use crate::metric_query::ast::{Arg, Expr, Formula, MetricQuery};
use crate::metric_query::parser;
use crate::query::execute::{self, Executor, QueryError};
use crate::query::plan::{self, Retention};
use crate::query::series::{NOT_APPLICABLE, OutputResult};
use crate::sql::TenantId;

/// One query, as Datadog writes it: `avg:system.cpu.user{env:prod} by {host}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub name: String,
    pub query: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimeseriesRequest {
    /// Unix milliseconds.
    pub from: i64,
    pub to: i64,
    /// A hint for the bucket width; None picks it from the window, as Datadog does.
    pub interval: Option<TimeDelta>,
    pub queries: Vec<Query>,
    /// Formulas over the queries' names. Empty: each query is a result of its own.
    pub formulas: Vec<String>,
}

/// A query for a scalar: the window reduced to one value per group.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarQuery {
    pub name: String,
    pub query: String,
    pub aggregator: ScalarAggregator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScalarRequest {
    pub from: i64,
    pub to: i64,
    pub queries: Vec<ScalarQuery>,
    pub formulas: Vec<String>,
}

/// One `by` key of a series' group. Value None: the series has no such tag, Datadog's N/A group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Unix milliseconds.
    pub time: i64,
    pub value: f64,
    /// anomalies(): the expected range at this point, where known.
    pub expected: Option<Band>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForecastPoint {
    pub time: i64,
    pub value: f64,
    pub band: Band,
}

/// One group's series: its tags, and its points in time order. A bucket with no value is absent.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeSeries {
    pub tags: Vec<Tag>,
    pub points: Vec<Point>,
    /// forecast(): what comes after the window. Empty otherwise.
    pub forecast: Vec<ForecastPoint>,
}

/// One formula (or query, when there are no formulas) and its series.
#[derive(Clone, Debug, PartialEq)]
pub struct FormulaResult {
    pub formula: String,
    pub series: Vec<TimeSeries>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScalarValue {
    pub tags: Vec<Tag>,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScalarResult {
    pub formula: String,
    pub values: Vec<ScalarValue>,
}

/// A series' group tags, `k:v` as the engine keeps them, read into keys and values; `k:N/A` is a
/// key without a value.
pub fn tags(group_tags: &[String]) -> Vec<Tag> {
    group_tags
        .iter()
        .map(|t| match t.split_once(':') {
            None => Tag { key: t.clone(), value: None },
            Some((key, value)) => {
                Tag { key: key.to_string(), value: (value != NOT_APPLICABLE).then(|| value.to_string()) }
            }
        })
        .collect()
}

/// The names results are labelled with: the formulas, or else the queries.
fn labels(queries: Vec<String>, formulas: &[String]) -> Vec<String> {
    if formulas.is_empty() { queries } else { formulas.to_vec() }
}

/// The engine's own results, as records. `names` labels them, one per output.
pub fn results(names: &[String], outputs: Vec<OutputResult>) -> Vec<FormulaResult> {
    outputs
        .into_iter()
        .map(|output| FormulaResult {
            formula: names[output.query_index].clone(),
            series: output
                .lines
                .into_iter()
                .map(|s| TimeSeries {
                    tags: tags(&s.data.group_tags),
                    points: s
                        .data
                        .points
                        .iter()
                        .map(|(&time, &value)| Point {
                            time,
                            value,
                            expected: s.extra.band.as_ref().and_then(|b| b.get(&time).copied()),
                        })
                        .collect(),
                    forecast: s
                        .extra
                        .forecast
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| ForecastPoint { time: p.time_ms, value: p.value, band: p.range })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

/// The primitive under the others: an already parsed request (trees, not strings), for code that
/// parses once and runs many times, as a monitor evaluating the same query every minute does.
///
/// `labels` names the results, one per formula (or per query when there are no formulas). `now`
/// is when the request is made, in unix milliseconds: the tables' retention counts back from it,
/// and the engine never reads the clock.
pub async fn parsed<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    now: i64,
    labels: &[String],
    request: &ParsedTimeseriesRequest,
) -> Result<Vec<FormulaResult>, QueryError> {
    let plan = plan::plan(retention, now, request).map_err(QueryError::Invalid)?;
    let outputs = execute::evaluate(execute, tenant, &plan, None).await.map_err(QueryError::Failed)?;
    Ok(results(labels, outputs))
}

fn wire(
    from: i64,
    to: i64,
    interval: Option<TimeDelta>,
    queries: Vec<MetricsTimeseriesQuery>,
    formulas: &[String],
) -> TimeseriesFormulaRequestAttributes {
    TimeseriesFormulaRequestAttributes {
        from: Some(from),
        to: Some(to),
        interval: interval.map(|i| i.num_milliseconds()),
        queries: Some(queries),
        formulas: (!formulas.is_empty())
            .then(|| formulas.iter().map(|f| QueryFormula { formula: Some(f.clone()), limit: None }).collect()),
    }
}

fn metrics_query(name: &str, query: &str) -> MetricsTimeseriesQuery {
    MetricsTimeseriesQuery { data_source: Some("metrics".into()), name: Some(name.into()), query: Some(query.into()) }
}

/// Timeseries: every formula's series over the window.
///
/// Problems with the request (a query that does not parse, a function given the wrong arguments,
/// a formula naming no query) come back as `QueryError::Invalid`, each one named, exactly as the
/// HTTP endpoint reports them.
pub async fn timeseries<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    now: i64,
    request: &TimeseriesRequest,
) -> Result<Vec<FormulaResult>, QueryError> {
    let queries = request.queries.iter().map(|q| metrics_query(&q.name, &q.query)).collect();
    let raw = wire(request.from, request.to, request.interval, queries, &request.formulas);
    let parsed_request = timeseries::parse(&raw).map_err(QueryError::Invalid)?;
    let names = labels(request.queries.iter().map(|q| q.name.clone()).collect(), &request.formulas);
    parsed(execute, tenant, retention, now, &names, &parsed_request).await
}

/// Splits an expression with queries inline into named queries and the formula over their names:
/// each metric query becomes q1, q2, …, the same query twice being one.
pub fn split(expr: &Expr<MetricQuery>) -> (Vec<ParsedQuery>, Formula) {
    fn formula(e: &Expr<MetricQuery>, queries: &mut Vec<MetricQuery>) -> Formula {
        match e {
            Expr::Leaf(q) => {
                let i = match queries.iter().position(|known| known == q) {
                    Some(i) => i,
                    None => {
                        queries.push(q.clone());
                        queries.len() - 1
                    }
                };
                Expr::Leaf(format!("q{}", i + 1))
            }
            Expr::Number(n) => Expr::Number(*n),
            Expr::Neg(inner) => Expr::Neg(Box::new(formula(inner, queries))),
            Expr::Binary(op, l, r) => Expr::Binary(*op, Box::new(formula(l, queries)), Box::new(formula(r, queries))),
            Expr::Call(f, args) => Expr::Call(
                f.clone(),
                args.iter()
                    .map(|a| match a {
                        Arg::Value(v) => Arg::Value(formula(v, queries)),
                        Arg::Lit(l) => Arg::Lit(l.clone()),
                        Arg::Named(n, v) => Arg::Named(n.clone(), v.clone()),
                    })
                    .collect(),
            ),
        }
    }

    let mut queries = Vec::new();
    let f = formula(expr, &mut queries);
    let named = queries
        .into_iter()
        .enumerate()
        .map(|(i, query)| ParsedQuery { name: Some(format!("q{}", i + 1)), query })
        .collect();
    (named, f)
}

/// One expression with its queries inline, the way Datadog's v1 API and its monitors write them:
///
/// ```text
/// anomalies(avg:system.cpu.user{*} by {host}, 'basic', 3)
/// sum:requests.errors{*}.as_count() / sum:requests.hits{*}.as_count() * 100
/// ```
///
/// `split` turns it into named queries and a formula; from there it is the same engine as
/// `timeseries`.
#[allow(clippy::too_many_arguments)]
pub async fn expression<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    now: i64,
    from: i64,
    until: i64,
    text: &str,
) -> Result<Vec<FormulaResult>, QueryError> {
    let program = parser::parse_program(text).map_err(|e| QueryError::Invalid(vec![e]))?;
    let [expr] = program.as_slice() else {
        return Err(QueryError::Invalid(vec![format!(
            "expected one expression, got {} separated by commas",
            program.len()
        )]));
    };
    let (queries, formula) = split(expr);
    let request = ParsedTimeseriesRequest {
        from,
        to: until,
        interval: None,
        queries,
        formulas: vec![ParsedFormula { formula, limit: None }],
    };
    parsed(execute, tenant, retention, now, &[text.to_string()], &request).await
}

/// Scalar: every formula's one value per group, over the window.
pub async fn scalar<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    now: i64,
    request: &ScalarRequest,
) -> Result<Vec<ScalarResult>, QueryError> {
    let queries = request.queries.iter().map(|q| metrics_query(&q.name, &q.query)).collect();
    let raw = wire(request.from, request.to, None, queries, &request.formulas);
    let plan = timeseries::parse(&raw).and_then(|r| plan::plan(retention, now, &r)).map_err(QueryError::Invalid)?;
    let aggregators: Vec<ScalarAggregator> = request.queries.iter().map(|q| q.aggregator).collect();
    let outputs = execute::evaluate_scalar(execute, tenant, &plan, &aggregators).await?;
    let names = labels(request.queries.iter().map(|q| q.name.clone()).collect(), &request.formulas);

    Ok(names
        .into_iter()
        .zip(outputs)
        .map(|(formula, series)| ScalarResult {
            formula,
            // A group whose window held no points has no value.
            values: series
                .iter()
                .filter_map(|s| s.points.values().next().map(|&value| ScalarValue { tags: tags(&s.group_tags), value }))
                .collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_query::ast::{BinaryOp, Literal};
    use crate::query::compile::Row;
    use crate::query::execute::Fake;
    use crate::query::plan::DEFAULT_RETENTION;
    use crate::sql::Sql;

    const NOW: i64 = 86_400_000;

    /// Query `cpu`: host a at 10, 20, 30 and host b at 1, 2, 3, every 20 s.
    fn rows(sql: &Sql) -> Vec<Row> {
        let grouped = sql.text.matches("AS g").count() > 0;
        let row = |groups: Vec<Option<String>>, bucket_ms, value| Row { groups, series_id: None, bucket_ms, value };
        [(0, 1.0), (20_000, 2.0), (40_000, 3.0)]
            .into_iter()
            .flat_map(|(t, v)| {
                let a = row(if grouped { vec![Some("a".into())] } else { vec![] }, t, v * 10.0);
                let b = grouped.then(|| row(vec![Some("b".into())], t, v));
                std::iter::once(a).chain(b)
            })
            .collect()
    }

    fn t() -> TenantId {
        TenantId("t".into())
    }

    fn cpu(query: &str) -> Vec<Query> {
        vec![Query { name: "cpu".into(), query: query.into() }]
    }

    async fn run(request: TimeseriesRequest) -> Vec<FormulaResult> {
        timeseries(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, &request).await.unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn values(s: &TimeSeries) -> Vec<f64> {
        s.points.iter().map(|p| p.value).collect()
    }

    #[tokio::test]
    async fn a_query_in_series_with_tags_and_points_out() {
        let result = run(TimeseriesRequest {
            from: 0,
            to: 60_000,
            interval: Some(TimeDelta::seconds(20)),
            queries: cpu("avg:cpu{*} by {host}.fill(null)"),
            formulas: vec![],
        })
        .await;

        assert_eq!(result.len(), 1);
        let cpu = &result[0];
        assert_eq!(cpu.formula, "cpu");
        let host = |v: &str| vec![Tag { key: "host".into(), value: Some(v.into()) }];
        assert_eq!(cpu.series.iter().map(|s| s.tags.clone()).collect::<Vec<_>>(), [host("a"), host("b")]);
        let a = &cpu.series[0];
        assert_eq!(values(a), [10.0, 20.0, 30.0]);
        assert_eq!(a.points[1].time, 20_000);
        assert!(a.forecast.is_empty());
        assert!(a.points.iter().all(|p| p.expected.is_none()));
    }

    #[tokio::test]
    async fn formulas_label_their_results() {
        let result = run(TimeseriesRequest {
            from: 0,
            to: 60_000,
            interval: Some(TimeDelta::seconds(20)),
            queries: cpu("avg:cpu{*} by {host}.fill(null)"),
            formulas: vec!["cpu * 2".into(), "top(cpu, 5, 'max', 'desc')".into()],
        })
        .await;

        assert_eq!(
            result.iter().map(|r| r.formula.as_str()).collect::<Vec<_>>(),
            ["cpu * 2", "top(cpu, 5, 'max', 'desc')"]
        );
        assert_eq!(values(&result[0].series[0]), [20.0, 40.0, 60.0]);
    }

    #[tokio::test]
    async fn problems_come_back_named() {
        let request = TimeseriesRequest {
            from: 0,
            to: 60_000,
            interval: None,
            queries: cpu("avg:cpu{host:a"),
            formulas: vec!["nope".into()],
        };
        match timeseries(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, &request).await {
            Err(QueryError::Invalid(es)) => assert!(es.iter().any(|e| e.starts_with("queries[0] (cpu):")), "{es:?}"),
            other => panic!("expected errors, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_scalar_is_one_value_per_group() {
        let request = ScalarRequest {
            from: 0,
            to: 60_000,
            queries: vec![ScalarQuery {
                name: "cpu".into(),
                query: "avg:cpu{*} by {host}.fill(null)".into(),
                aggregator: ScalarAggregator::Max,
            }],
            formulas: vec![],
        };
        let result =
            scalar(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, &request).await.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(result.len(), 1);
        let values: Vec<(Option<String>, f64)> =
            result[0].values.iter().map(|v| (v.tags[0].value.clone(), v.value)).collect();
        assert_eq!(values, [(Some("a".into()), 30.0), (Some("b".into()), 3.0)]);
    }

    #[tokio::test]
    async fn a_database_failure_is_failed() {
        struct Broken;
        impl Executor for Broken {
            fn rows(&self, _: Sql) -> impl Future<Output = Result<Vec<Row>, String>> + Send {
                std::future::ready(Err("Code: 60. Unknown table".to_string()))
            }
        }
        let request =
            TimeseriesRequest { from: 0, to: 60_000, interval: None, queries: cpu("avg:cpu{*}"), formulas: vec![] };
        let result = timeseries(&Broken, &t(), DEFAULT_RETENTION, NOW, &request).await;
        assert_eq!(result, Err(QueryError::Failed("Code: 60. Unknown table".into())));
    }

    // --- one expression, queries inline --------------------------------------------------

    fn split_of(text: &str) -> (Vec<ParsedQuery>, Formula) {
        match parser::parse_program(text) {
            Ok(program) if program.len() == 1 => split(&program[0]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn split_names_each_query_and_keeps_the_formula() {
        let (queries, formula) = split_of("anomalies(avg:cpu{*} by {host}, 'basic', 3)");
        assert_eq!(queries.iter().map(|q| q.name.clone().unwrap()).collect::<Vec<_>>(), ["q1"]);
        assert_eq!(queries[0].query.metric, "cpu");
        assert_eq!(
            formula,
            Expr::Call(
                "anomalies".into(),
                vec![
                    Arg::Value(Expr::Leaf("q1".into())),
                    Arg::Lit(Literal::Quoted("basic".into())),
                    Arg::Value(Expr::Number(3.0))
                ]
            )
        );
    }

    #[test]
    fn the_same_query_twice_is_one_query() {
        let (queries, formula) = split_of("(sum:hits{*} - sum:errors{*}) / sum:hits{*}");
        assert_eq!(queries.iter().map(|q| q.query.metric.as_str()).collect::<Vec<_>>(), ["hits", "errors"]);
        let leaf = |n: &str| Box::new(Expr::Leaf(n.to_string()));
        assert_eq!(
            formula,
            Expr::Binary(BinaryOp::Div, Box::new(Expr::Binary(BinaryOp::Sub, leaf("q1"), leaf("q2"))), leaf("q1"))
        );
    }

    #[tokio::test]
    async fn an_expression_runs_like_a_formula() {
        let text = "avg:cpu{*} by {host}.fill(null) * 2";
        let result = expression(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, 0, 60_000, text)
            .await
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].formula, text);
        assert_eq!(values(&result[0].series[0]), [20.0, 40.0, 60.0]);
    }

    #[tokio::test]
    async fn expression_problems_are_named() {
        let two = expression(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, 0, 60_000, "avg:cpu{*}, avg:mem{*}").await;
        assert_eq!(two, Err(QueryError::Invalid(vec!["expected one expression, got 2 separated by commas".into()])));

        match expression(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, 0, 60_000, "ewma_4(avg:cpu{*})").await {
            Err(QueryError::Invalid(es)) => {
                assert!(es.contains(&"formulas[0]: function 'ewma_4' is not supported yet".to_string()), "{es:?}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tags_read_na_as_no_value() {
        let read = tags(&["host:a".into(), "role:N/A".into(), "bare".into(), "url:http://x".into()]);
        assert_eq!(
            read,
            [
                Tag { key: "host".into(), value: Some("a".into()) },
                Tag { key: "role".into(), value: None },
                Tag { key: "bare".into(), value: None },
                Tag { key: "url".into(), value: Some("http://x".into()) },
            ]
        );
    }
}
