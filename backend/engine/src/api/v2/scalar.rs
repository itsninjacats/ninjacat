//! `POST /api/v2/query/scalar`: the request, from JSON to a parsed timeseries request plus each
//! query's aggregator.
//!
//! A scalar request is a timeseries request whose every query also says how its window becomes
//! one value. Everything else (the query language, formulas, functions) is the timeseries
//! engine's, reused as it is.

use crate::api::v2::timeseries::{self, ParsedTimeseriesRequest, distinct};
use crate::api::v2::wire::{
    MetricsScalarQuery, MetricsTimeseriesQuery, ScalarFormulaQueryRequest, ScalarFormulaRequestAttributes,
    TimeseriesFormulaRequestAttributes,
};

/// `MetricsAggregator`: how one series' points in the window become one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarAggregator {
    Avg,
    Min,
    Max,
    Sum,
    /// The latest point's value.
    Last,
    /// sqrt of the sum of squares.
    L2norm,
    /// Sum of value × bucket width in seconds.
    Area,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedScalarRequest {
    pub request: ParsedTimeseriesRequest,
    /// Parallel to `request.queries`.
    pub aggregators: Vec<ScalarAggregator>,
    /// A name per output column: the formula as written, or the query's name when there are no
    /// formulas.
    ///
    /// WARNING(undocumented): what Datadog names a formula's column. Its one recorded response
    /// names the column of formula "a" "a"; we use the formula text for every formula.
    pub column_names: Vec<String>,
}

/// "mean" is listed beside "avg" in the spec's enum, and means the same.
///
/// WARNING(undocumented): "percentile". It applies to distribution metrics (`p95:`), which are
/// not read yet; it is refused by name.
fn aggregator(i: usize, name: &str) -> Result<ScalarAggregator, String> {
    match name {
        "avg" | "mean" => Ok(ScalarAggregator::Avg),
        "min" => Ok(ScalarAggregator::Min),
        "max" => Ok(ScalarAggregator::Max),
        "sum" => Ok(ScalarAggregator::Sum),
        "last" => Ok(ScalarAggregator::Last),
        "l2norm" => Ok(ScalarAggregator::L2norm),
        "area" => Ok(ScalarAggregator::Area),
        "percentile" => Err(format!("queries[{i}]: aggregator 'percentile' is not supported yet")),
        other => Err(format!(
            "queries[{i}]: aggregator must be one of avg, min, max, sum, last, percentile, mean, l2norm, area, got '{other}'"
        )),
    }
}

fn is_metrics(q: &MetricsScalarQuery) -> bool {
    q.data_source.as_deref() == Some("metrics")
}

fn required(kind: Option<&str>, a: &ScalarFormulaRequestAttributes) -> Vec<String> {
    let mut missing = Vec::new();
    if kind.is_none() {
        missing.push("missing field 'type' in data".to_string());
    }
    if a.from.is_none() {
        missing.push("missing field 'from' in data.attributes".to_string());
    }
    if a.to.is_none() {
        missing.push("missing field 'to' in data.attributes".to_string());
    }
    if a.queries.is_none() {
        missing.push("missing field 'queries' in data.attributes".to_string());
    }
    for q in a.queries.iter().flatten() {
        if q.data_source.is_none() {
            missing.push("missing field 'data_source' in data.attributes.queries[]".to_string());
        }
        if is_metrics(q) && q.query.is_none() {
            missing.push("missing field 'query' in data.attributes.queries[]".to_string());
        }
        if is_metrics(q) && q.aggregator.is_none() {
            missing.push("missing field 'aggregator' in data.attributes.queries[]".to_string());
        }
    }
    distinct(missing)
}

pub fn decode(json: &str) -> Result<ScalarFormulaRequestAttributes, Vec<String>> {
    let body: Option<ScalarFormulaQueryRequest> =
        serde_json::from_str(json).map_err(|e| vec![format!("invalid request body: {e}")])?;
    let Some(data) = body.and_then(|b| b.data) else {
        return Err(vec!["missing field 'data.attributes' in the body".to_string()]);
    };
    let Some(attributes) = data.attributes else {
        return Err(vec!["missing field 'data.attributes' in the body".to_string()]);
    };
    match required(data.kind.as_deref(), &attributes) {
        errors if !errors.is_empty() => Err(errors),
        _ if data.kind.as_deref() == Some("scalar_request") => Ok(attributes),
        _ => Err(vec!["API input validation failed: Invalid type. Expected \"scalar_request\".".to_string()]),
    }
}

pub fn parse(raw: &ScalarFormulaRequestAttributes) -> Result<ParsedScalarRequest, Vec<String>> {
    // The same attributes a timeseries request has, without an interval: the step falls back to
    // Datadog's table for the window.
    let queries = raw.queries.as_deref().unwrap_or_default();
    let as_timeseries = TimeseriesFormulaRequestAttributes {
        from: raw.from,
        to: raw.to,
        interval: None,
        queries: Some(
            queries
                .iter()
                .map(|q| MetricsTimeseriesQuery { data_source: q.data_source.clone(), name: q.name.clone(), query: q.query.clone() })
                .collect(),
        ),
        formulas: raw.formulas.clone(),
    };

    let aggregators: Vec<Result<ScalarAggregator, String>> = queries
        .iter()
        .enumerate()
        .map(|(i, q)| if is_metrics(q) { aggregator(i, q.aggregator.as_deref().unwrap_or_default()) } else { Ok(ScalarAggregator::Avg) })
        .collect();
    let aggregator_errors: Vec<String> = aggregators.iter().filter_map(|a| a.as_ref().err().cloned()).collect();

    match (timeseries::parse(&as_timeseries), aggregator_errors.is_empty()) {
        (Ok(request), true) => {
            let column_names = match raw.formulas.as_deref() {
                Some(fs) if !fs.is_empty() => fs.iter().map(|f| f.formula.clone().unwrap_or_default()).collect(),
                _ => queries
                    .iter()
                    .enumerate()
                    .map(|(i, q)| q.name.clone().unwrap_or_else(|| format!("query{}", i + 1)))
                    .collect(),
            };
            Ok(ParsedScalarRequest {
                request,
                aggregators: aggregators.into_iter().filter_map(Result::ok).collect(),
                column_names,
            })
        }
        (Ok(_), false) => Err(aggregator_errors),
        (Err(mut errors), _) => {
            errors.extend(aggregator_errors);
            Err(errors)
        }
    }
}

pub fn read(json: &str) -> Result<ParsedScalarRequest, Vec<String>> {
    decode(json).and_then(|raw| parse(&raw))
}

/// The request-reading tests; the ones that run queries are `query::execute`'s.
#[cfg(test)]
mod tests {
    use super::*;

    fn body(queries: &str, formulas: &str) -> String {
        format!(
            r#"{{"data":{{"type":"scalar_request","attributes":{{"from":0,"to":100000,"queries":[{queries}],"formulas":[{formulas}]}}}}}}"#
        )
    }

    fn errors_of(json: &str) -> Vec<String> {
        match read(json) {
            Ok(r) => panic!("expected errors, got {r:?}"),
            Err(es) => es,
        }
    }

    #[test]
    fn the_recorded_datadog_request() {
        // datadog-api-client-go, Scenario_Scalar_cross_product_query_returns_OK_response.
        let r = read(
            r#"{"data":{"attributes":{"formulas":[{"formula":"a","limit":{"count":10,"order":"desc"}}],"from":1711391280000,"queries":[{"aggregator":"avg","data_source":"metrics","name":"a","query":"avg:system.cpu.user{*}"}],"to":1711394880000},"type":"scalar_request"}}"#,
        )
        .unwrap_or_else(|es| panic!("{}", es.join("\n")));
        assert_eq!(r.aggregators, [ScalarAggregator::Avg]);
        assert_eq!(r.column_names, ["a"]);
    }

    #[test]
    fn aggregator_is_required_and_checked() {
        assert!(
            errors_of(&body(r#"{"data_source":"metrics","query":"x{*}"}"#, ""))
                .contains(&"missing field 'aggregator' in data.attributes.queries[]".to_string())
        );
        assert!(
            errors_of(&body(r#"{"data_source":"metrics","query":"x{*}","aggregator":"percentile"}"#, ""))
                .contains(&"queries[0]: aggregator 'percentile' is not supported yet".to_string())
        );
        let es = errors_of(&body(r#"{"data_source":"metrics","query":"x{*}","aggregator":"median"}"#, ""));
        assert!(es[0].starts_with("queries[0]: aggregator must be one of"), "{es:?}");
    }

    #[test]
    fn wrong_type_uses_datadogs_wording() {
        let json = body(r#"{"data_source":"metrics","query":"x{*}","aggregator":"avg"}"#, "")
            .replace("scalar_request", "timeseries_request");
        assert!(
            errors_of(&json).contains(&"API input validation failed: Invalid type. Expected \"scalar_request\".".to_string())
        );
    }
}
