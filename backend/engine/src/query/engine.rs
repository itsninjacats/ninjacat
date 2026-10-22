//! The query engine as Rust code calls it: the panel, monitors, anything that is not an HTTP
//! client.
//!
//! A request goes in already parsed (trees, not strings); results come back as plain records,
//! with no JSON shape, parallel lists or "key:value" strings to take apart. The HTTP endpoints
//! run the same engine and turn its results into Datadog's response shape; this module turns
//! them into these. `split` turns an expression with its queries inline, as Datadog's v1 API and
//! its monitors write them, into the named queries and the formula a request is made of.

use ninjapaws_anomaly::Band;

use crate::api::v2::timeseries::{ParsedQuery, ParsedTimeseriesRequest};
use crate::metric_query::ast::{Arg, Expr, Formula, MetricQuery};
use crate::query::execute::{self, Executor, QueryError};
use crate::query::plan::{self, Retention};
use crate::query::series::{NOT_APPLICABLE, OutputResult};
use crate::sql::TenantId;

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

/// Runs an already parsed request, for code that parses once and runs many times, as a monitor
/// evaluating the same query every minute does. Problems with the request come back as
/// `QueryError::Invalid`, each one named, as the HTTP endpoint reports them.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::v2::timeseries::read;
    use crate::metric_query::ast::{BinaryOp, Literal};
    use crate::metric_query::parser;
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

    /// The query `cpu`, over 0 to 60 s in 20 s buckets, with these formulas.
    fn request(query: &str, formulas: &[&str]) -> ParsedTimeseriesRequest {
        let formulas: Vec<String> = formulas.iter().map(|f| format!(r#"{{"formula":"{f}"}}"#)).collect();
        let body = format!(
            r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":0,"to":60000,"interval":20000,"queries":[{{"data_source":"metrics","name":"cpu","query":"{query}"}}],"formulas":[{}]}}}}}}"#,
            formulas.join(",")
        );
        read(&body).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    async fn run(request: ParsedTimeseriesRequest, labels: &[&str]) -> Vec<FormulaResult> {
        let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
        parsed(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, &labels, &request).await.unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn values(s: &TimeSeries) -> Vec<f64> {
        s.points.iter().map(|p| p.value).collect()
    }

    #[tokio::test]
    async fn a_query_in_series_with_tags_and_points_out() {
        let result = run(request("avg:cpu{*} by {host}.fill(null)", &[]), &["cpu"]).await;

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
        let formulas = ["cpu * 2", "top(cpu, 5, 'max', 'desc')"];
        let result = run(request("avg:cpu{*} by {host}.fill(null)", &formulas), &formulas).await;

        assert_eq!(result.iter().map(|r| r.formula.as_str()).collect::<Vec<_>>(), formulas);
        assert_eq!(values(&result[0].series[0]), [20.0, 40.0, 60.0]);
    }

    #[tokio::test]
    async fn a_formula_naming_no_query_is_invalid() {
        let labels = ["nope".to_string()];
        let result =
            parsed(&Fake(rows), &t(), DEFAULT_RETENTION, NOW, &labels, &request("avg:cpu{*}", &["nope"])).await;
        match result {
            Err(QueryError::Invalid(es)) => assert!(!es.is_empty()),
            other => panic!("expected errors, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_database_failure_is_failed() {
        struct Broken;
        impl Executor for Broken {
            fn rows(&self, _: Sql) -> impl Future<Output = Result<Vec<Row>, String>> + Send {
                std::future::ready(Err("Code: 60. Unknown table".to_string()))
            }
        }
        let labels = ["cpu".to_string()];
        let result = parsed(&Broken, &t(), DEFAULT_RETENTION, NOW, &labels, &request("avg:cpu{*}", &[])).await;
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
