//! Metric queries that need no parser: catalog lookups.

use crate::sql::{Sql, SqlValue, TenantId, param};

/// The tags `metrics` also keeps as columns of their own, each the whole list of the tag's values.
/// The rollup has none of them.
pub const LIFTED_TAGS: [&str; 5] = ["env", "service", "kube_namespace", "kube_deployment", "pod_name"];

/// Points of `metrics` from a SQL expression on. The table is sorted by the 15-minute bucket
/// before the series, and ClickHouse does not derive the bucket from a condition on `timestamp`:
/// without the bucket condition a query reads every granule of its metric.
pub fn raw_since(from: &str) -> String {
    format!("timestamp >= {from} AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes({from})")
}

/// Points of `metrics` between two SQL expressions, the bucket stated as in `raw_since`.
pub fn raw_window(from: &str, until: &str) -> String {
    raw_since(from) + &format!(" AND timestamp < {until} AND toStartOfFifteenMinutes(timestamp) < {until}")
}

/// Datadog's `GET /api/v1/metrics`: metrics reported since `from`, optionally by one host.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveMetricsRequest {
    pub tenant: TenantId,
    /// Unix milliseconds.
    pub from: i64,
    pub host: Option<String>,
}

pub fn active_metrics(req: &ActiveMetricsRequest) -> Sql {
    let tenant = SqlValue::String(req.tenant.0.clone());
    let from = SqlValue::Int64(req.from.div_euclid(1000));
    let from_sql = format!("fromUnixTimestamp({})", param("from", &from));
    let mut text = format!(
        "SELECT DISTINCT metric\nFROM metrics\nWHERE tenant_id = {}\n  AND {}",
        param("tenant", &tenant),
        raw_since(&from_sql)
    );
    let mut parameters = vec![("tenant".to_string(), tenant), ("from".to_string(), from)];
    if let Some(host) = &req.host {
        let host = SqlValue::String(host.clone());
        text += &format!("\n  AND host = {}", param("host", &host));
        parameters.push(("host".to_string(), host));
    }
    // Raw `metrics` rather than `metrics_1m`: the rollup is per minute, so a `from` inside the
    // last minute would miss metrics that just appeared.
    Sql { text: text + "\nORDER BY metric", parameters }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(host: Option<&str>) -> ActiveMetricsRequest {
        ActiveMetricsRequest { tenant: TenantId("acme".into()), from: 1_700_000_000_000, host: host.map(String::from) }
    }

    #[test]
    fn tenant_and_from_are_bound_never_inlined() {
        let sql = active_metrics(&req(None));
        assert!(sql.text.contains("tenant_id = {tenant:String}"));
        assert!(sql.text.contains("fromUnixTimestamp({from:Int64})"));
        // The key has the 15-minute bucket after the metric: this is what prunes.
        assert!(sql.text.contains(
            "toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(fromUnixTimestamp({from:Int64}))"
        ));
        assert!(!sql.text.contains("acme"));
        assert_eq!(
            sql.parameters,
            vec![
                ("tenant".to_string(), SqlValue::String("acme".into())),
                ("from".to_string(), SqlValue::Int64(1_700_000_000))
            ]
        );
    }

    #[test]
    fn host_filter_appears_only_when_a_host_is_given() {
        assert!(!active_metrics(&req(None)).text.contains("host"));
        let sql = active_metrics(&req(Some("web-1")));
        assert!(sql.text.contains("AND host = {host:String}"));
        assert!(sql.parameters.contains(&("host".to_string(), SqlValue::String("web-1".into()))));
    }

    #[test]
    fn every_placeholder_has_a_parameter_and_vice_versa() {
        let sql = active_metrics(&req(Some("web-1")));
        let placeholders: std::collections::BTreeSet<String> =
            regex::Regex::new(r"\{(\w+):").unwrap().captures_iter(&sql.text).map(|c| c[1].to_string()).collect();
        let names: std::collections::BTreeSet<String> = sql.parameters.iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(placeholders, names);
    }

    #[test]
    fn the_whole_text() {
        assert_eq!(
            active_metrics(&req(Some("web-1"))).text,
            "SELECT DISTINCT metric\nFROM metrics\nWHERE tenant_id = {tenant:String}\n  AND timestamp >= fromUnixTimestamp({from:Int64}) AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(fromUnixTimestamp({from:Int64}))\n  AND host = {host:String}\nORDER BY metric"
        );
        assert_eq!(
            raw_window("a", "b"),
            "timestamp >= a AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(a) AND timestamp < b AND toStartOfFifteenMinutes(timestamp) < b"
        );
    }
}
