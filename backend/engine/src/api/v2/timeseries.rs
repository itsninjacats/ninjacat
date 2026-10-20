//! `POST /api/v2/query/timeseries`: the request, from JSON to parsed trees. (F#:
//! `Api/V2/Timeseries.fs`.)
//!
//! Two steps, kept apart so each has one kind of error:
//!
//! ```text
//! decode   JSON → wire types                the deserializer; "missing field 'from'"
//! parse    wire → ParsedTimeseriesRequest   "queries[0]: expecting '}' at column 23"
//! ```
//!
//! Errors inside query and formula strings are all reported, not just the first, so a client
//! fixing a request does not go round once per mistake.
//!
//! The `Parsed…` names are ours on purpose: they are past validation, and must not be mistaken
//! for a schema in Datadog's spec.

use chrono::TimeDelta;

use crate::api::v2::wire::{
    FormulaLimit, MetricsTimeseriesQuery, QueryFormula, TimeseriesFormulaQueryRequest, TimeseriesFormulaRequestAttributes,
};
use crate::metric_query::ast::{Formula, MetricQuery};
use crate::metric_query::parser::{parse_formula, parse_metric_query};
use crate::unix_time;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// `limit` on a formula: keep the top or bottom N series. `{order: desc}` without a count was seen
/// in real dashboards, hence the option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedLimit {
    pub count: Option<i32>,
    pub order: SortOrder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedQuery {
    pub name: Option<String>,
    pub query: MetricQuery,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedFormula {
    pub formula: Formula,
    pub limit: Option<ParsedLimit>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedTimeseriesRequest {
    /// Unix milliseconds.
    pub from: i64,
    pub to: i64,
    pub interval: Option<TimeDelta>,
    pub queries: Vec<ParsedQuery>,
    /// Empty means no formulas were sent, and each query is shown as is.
    pub formulas: Vec<ParsedFormula>,
}

/// What the deserializer lets through but the spec requires.
fn required(kind: Option<&str>, a: &TimeseriesFormulaRequestAttributes) -> Vec<String> {
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
        // Only a metrics query has `query`; the others (logs: `search`, …) are refused by
        // data_source in parse, with a better message.
        if q.data_source.as_deref() == Some("metrics") && q.query.is_none() {
            missing.push("missing field 'query' in data.attributes.queries[]".to_string());
        }
    }
    for f in a.formulas.iter().flatten() {
        if f.formula.is_none() {
            missing.push("missing field 'formula' in data.attributes.formulas[]".to_string());
        }
    }
    distinct(missing)
}

/// The errors in the order first seen, each once.
pub(crate) fn distinct(errors: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    for e in errors {
        if !seen.contains(&e) {
            seen.push(e);
        }
    }
    seen
}

/// A body that is not JSON, or holds a value of the wrong type, is reported in the
/// deserializer's own words, and it stops at the first. Errors inside query and formula strings
/// are all collected (`parse`).
pub fn decode(json: &str) -> Result<TimeseriesFormulaRequestAttributes, Vec<String>> {
    let body: Option<TimeseriesFormulaQueryRequest> =
        serde_json::from_str(json).map_err(|e| vec![format!("invalid request body: {e}")])?;
    let Some(data) = body.and_then(|b| b.data) else {
        return Err(vec!["missing field 'data.attributes' in the body".to_string()]);
    };
    let Some(attributes) = data.attributes else {
        return Err(vec!["missing field 'data.attributes' in the body".to_string()]);
    };
    match required(data.kind.as_deref(), &attributes) {
        errors if !errors.is_empty() => Err(errors),
        _ if data.kind.as_deref() == Some("timeseries_request") => Ok(attributes),
        // Datadog's own wording, as recorded in its client's tests.
        _ => Err(vec!["API input validation failed: Invalid type. Expected \"timeseries_request\".".to_string()]),
    }
}

fn parse_query(i: usize, q: &MetricsTimeseriesQuery, errors: &mut Vec<String>) -> Option<ParsedQuery> {
    let label = match &q.name {
        Some(name) => format!("queries[{i}] ({name})"),
        None => format!("queries[{i}]"),
    };
    match q.data_source.as_deref().unwrap_or_default() {
        "metrics" => match parse_metric_query(q.query.as_deref().unwrap_or_default()) {
            Ok(query) => Some(ParsedQuery { name: q.name.clone(), query }),
            Err(e) => {
                errors.push(format!("{label}: {e}"));
                None
            }
        },
        // The spec's other sources (logs, spans, rum, …) need the search language, which comes
        // later.
        other => {
            errors.push(format!("{label}: data_source '{other}' is not supported yet"));
            None
        }
    }
}

fn parse_limit(label: &str, l: &FormulaLimit, errors: &mut Vec<String>) -> Option<ParsedLimit> {
    match l.order.as_deref() {
        None | Some("desc") => Some(ParsedLimit { count: l.count, order: SortOrder::Desc }),
        Some("asc") => Some(ParsedLimit { count: l.count, order: SortOrder::Asc }),
        Some(other) => {
            errors.push(format!("{label}.limit.order must be 'asc' or 'desc', got '{other}'"));
            None
        }
    }
}

fn parse_formula_at(i: usize, f: &QueryFormula, errors: &mut Vec<String>) -> Option<ParsedFormula> {
    let label = format!("formulas[{i}]");
    let limit = f.limit.as_ref().and_then(|l| parse_limit(&label, l, errors));
    match parse_formula(f.formula.as_deref().unwrap_or_default()) {
        Ok(formula) => Some(ParsedFormula { formula, limit }),
        Err(e) => {
            errors.push(format!("{label}: {e}"));
            None
        }
    }
}

/// `from` and `to` are there after `decode`; a caller that builds the attributes itself is told
/// in decode's words when they are not.
pub fn parse(raw: &TimeseriesFormulaRequestAttributes) -> Result<ParsedTimeseriesRequest, Vec<String>> {
    let mut errors = Vec::new();
    let sent_queries = raw.queries.as_deref().unwrap_or_default();

    if raw.from.is_none() {
        errors.push("missing field 'from' in data.attributes".to_string());
    }
    if raw.to.is_none() {
        errors.push("missing field 'to' in data.attributes".to_string());
    }
    if let (Some(from), Some(to)) = (raw.from, raw.to)
        && from >= to
    {
        errors.push("data.attributes.from must be earlier than data.attributes.to".to_string());
    }
    if sent_queries.is_empty() {
        errors.push("data.attributes.queries must not be empty".to_string());
    }
    if raw.interval.is_some_and(|i| i <= 0) {
        errors.push("data.attributes.interval must be positive".to_string());
    }

    let queries: Vec<ParsedQuery> =
        sent_queries.iter().enumerate().filter_map(|(i, q)| parse_query(i, q, &mut errors)).collect();
    let formulas: Vec<ParsedFormula> = raw
        .formulas
        .iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, f)| parse_formula_at(i, f, &mut errors))
        .collect();

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ParsedTimeseriesRequest {
        from: unix_time::of_millis(raw.from.unwrap_or_default()),
        to: unix_time::of_millis(raw.to.unwrap_or_default()),
        interval: raw.interval.map(|i| unix_time::span_of_millis(i as f64)),
        queries,
        formulas,
    })
}

/// Both steps; the handler needs nothing else.
pub fn read(json: &str) -> Result<ParsedTimeseriesRequest, Vec<String>> {
    decode(json).and_then(|raw| parse(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_query::ast::Expr;

    fn errors_of(json: &str) -> Vec<String> {
        match read(json) {
            Ok(r) => panic!("expected errors, got {r:?}"),
            Err(es) => es,
        }
    }

    fn ok(json: &str) -> ParsedTimeseriesRequest {
        read(json).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    /// Builds a request body; each test overrides only what it is about.
    fn body(attrs: &str) -> String {
        format!(r#"{{"data":{{"type":"timeseries_request","attributes":{{{attrs}}}}}}}"#)
    }

    const WINDOW: &str = r#""from":1711977601000,"to":1711981201000"#;

    #[test]
    fn the_recorded_datadog_request() {
        // datadog-api-client-go, Scenario_Timeseries_cross_product_query_returns_OK_response.
        let r = ok(
            r#"{"data":{"attributes":{"formulas":[{"formula":"a","limit":{"count":10,"order":"desc"}}],"from":1711977601000,"interval":5000,"queries":[{"data_source":"metrics","name":"a","query":"avg:datadog.estimated_usage.metrics.custom{*}"}],"to":1711981201000},"type":"timeseries_request"}}"#,
        );
        assert_eq!(r.from, 1711977601000);
        assert_eq!(r.interval, Some(TimeDelta::seconds(5)));
        let [q] = r.queries.as_slice() else { panic!("{:?}", r.queries) };
        assert_eq!(q.name.as_deref(), Some("a"));
        assert_eq!(q.query.metric, "datadog.estimated_usage.metrics.custom");
        let [f] = r.formulas.as_slice() else { panic!("{:?}", r.formulas) };
        assert_eq!(f.formula, Expr::Leaf("a".to_string()));
        assert_eq!(f.limit, Some(ParsedLimit { count: Some(10), order: SortOrder::Desc }));
    }

    #[test]
    fn formulas_and_interval_are_optional() {
        let r = ok(&body(&format!(r#"{WINDOW},"queries":[{{"data_source":"metrics","query":"avg:x{{*}}"}}]"#)));
        assert!(r.formulas.is_empty());
        assert_eq!(r.interval, None);
        assert_eq!(r.queries.len(), 1);
        assert_eq!(r.queries[0].name, None);
    }

    #[test]
    fn wrong_type_uses_datadogs_message() {
        // Recorded 400 from Datadog, for a fixture with the typo "timeseries_rquest".
        let json = body(&format!(r#"{WINDOW},"queries":[]"#)).replace("timeseries_request", "timeseries_rquest");
        assert!(
            errors_of(&json)
                .contains(&"API input validation failed: Invalid type. Expected \"timeseries_request\".".to_string())
        );
    }

    #[test]
    fn missing_fields_name_the_json_location() {
        assert_eq!(errors_of(&body(r#" "to":2,"queries":[] "#)), ["missing field 'from' in data.attributes"]);
        assert_eq!(
            errors_of(&body(r#" "from":1,"to":2,"queries":[{"query":"x{*}"}] "#)),
            ["missing field 'data_source' in data.attributes.queries[]"]
        );
    }

    #[test]
    fn explicit_nulls_read_as_absent() {
        let r = ok(&body(
            r#" "from":1,"to":2,"interval":null,"queries":[{"data_source":"metrics","name":null,"query":"x{*}"}],"formulas":[{"formula":"a","limit":null}] "#,
        ));
        assert_eq!(r.interval, None);
        assert_eq!(r.queries[0].name, None);
        assert_eq!(r.formulas[0].limit, None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        // Dashboards send semantic_mode alongside queries.
        let r = ok(&body(r#" "from":1,"to":2,"queries":[{"data_source":"metrics","query":"x{*}","semantic_mode":"native"}] "#));
        assert_eq!(r.queries.len(), 1);
    }

    #[test]
    fn syntax_errors_name_the_query_and_the_column() {
        let es = errors_of(&body(&format!(
            r#"{WINDOW},"queries":[{{"data_source":"metrics","name":"a","query":"avg:x{{env:prod"}}]"#
        )));
        let [e] = es.as_slice() else { panic!("{es:?}") };
        assert!(e.starts_with("queries[0] (a): Error in Ln: 1 Col: 15"), "{e}");
        assert!(!e.contains('\n'));
    }

    #[test]
    fn every_bad_query_and_formula_is_reported() {
        let es = errors_of(&body(&format!(
            r#"{WINDOW},"queries":[{{"data_source":"metrics","query":"x"}},{{"data_source":"metrics","query":"y{{*}}"}}],"formulas":[{{"formula":"a +"}}]"#
        )));
        assert_eq!(es.len(), 2, "{es:?}");
        assert!(es[0].starts_with("queries[0]:"));
        assert!(es[1].starts_with("formulas[0]:"));
    }

    #[test]
    fn other_data_sources_are_refused_for_now() {
        let es = errors_of(&body(&format!(r#"{WINDOW},"queries":[{{"data_source":"logs","query":"service:web"}}]"#)));
        assert_eq!(es, ["queries[0]: data_source 'logs' is not supported yet"]);
    }

    #[test]
    fn a_logs_query_is_refused_by_its_data_source_not_by_its_fields() {
        // Logs queries carry `search`, not `query` (spec: EventsTimeseriesQuery).
        let es = errors_of(&body(&format!(
            r#"{WINDOW},"queries":[{{"data_source":"logs","name":"a","search":{{"query":"service:web"}}}}]"#
        )));
        assert_eq!(es, ["queries[0] (a): data_source 'logs' is not supported yet"]);
    }

    #[test]
    fn from_must_be_before_to() {
        let es = errors_of(&body(r#" "from":2000,"to":1000,"queries":[{"data_source":"metrics","query":"x{*}"}] "#));
        assert!(es.contains(&"data.attributes.from must be earlier than data.attributes.to".to_string()));
    }

    #[test]
    fn a_time_past_what_is_stored_is_kept_at_the_limit_not_refused() {
        let end_of_2105 = 4_291_747_200_000;
        for (from, to) in [
            (253402300800000_i64, 253402300800001_i64), // year 10000
            (-62135596800001, -62135596800000),         // before year 1
            (i64::MIN, i64::MAX),
        ] {
            let r = ok(&body(&format!(
                r#" "from":{from},"to":{to},"queries":[{{"data_source":"metrics","query":"x{{*}}"}}] "#
            )));
            for t in [r.from, r.to] {
                assert!((0..=end_of_2105).contains(&t), "{t}");
            }
        }
    }

    /// serde_json's words, where System.Text.Json said "expected JSON array" and "could not be
    /// converted to System.Int64".
    #[test]
    fn wrong_json_types_are_reported_in_the_deserializers_own_words() {
        let es = errors_of(&body(r#" "from":1,"to":2,"queries":{} "#));
        let [not_a_list] = es.as_slice() else { panic!("{es:?}") };
        assert!(not_a_list.starts_with("invalid request body: "));
        assert!(not_a_list.contains("expected a sequence"), "{not_a_list}");

        let es = errors_of(&body(r#" "from":"yesterday","to":1000,"queries":[] "#));
        let [not_a_number] = es.as_slice() else { panic!("{es:?}") };
        assert!(not_a_number.starts_with("invalid request body: "));
        assert!(not_a_number.contains("expected i64"), "{not_a_number}");
    }

    #[test]
    fn queries_is_required() {
        assert_eq!(errors_of(&body(r#" "from":1,"to":2 "#)), ["missing field 'queries' in data.attributes"]);
    }

    #[test]
    fn not_json_at_all() {
        let es = errors_of("hello");
        let [e] = es.as_slice() else { panic!("{es:?}") };
        assert!(e.starts_with("invalid request body:"));
    }
}
