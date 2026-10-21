//! What the panel asks of the telemetry tables: metric names, tag keys and values, one metric's
//! series, log search and its facets.
//!
//! Everything here is pure: a request becomes `Sql` or, for series, a request to the query
//! engine, and the server runs it. A caller's text is never put into the SQL itself.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeDelta};
use regex::Regex;

use crate::api::v2::timeseries::{ParsedQuery, ParsedTimeseriesRequest};
use crate::duration;
use crate::metric_query::ast::{MetricQuery, TagFilter};
use crate::metric_query::filter;
use crate::metrics;
use crate::query::engine::FormulaResult;
use crate::query::series::NOT_APPLICABLE;
use crate::sql::{Sql, SqlParams, SqlValue, TenantId};

// --- reading the request ---------------------------------------------------------

/// `tag=env:prod&tag=env:staging&tag=service:api` as one filter: the values of one key are OR'd,
/// different keys are AND'd. Only the first colon splits, so a value keeps its own colons.
pub fn parse_tag_filters(raw: &[String]) -> Result<TagFilter, String> {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for tag in raw {
        match tag.find(':') {
            Some(i) if i > 0 && i < tag.len() - 1 => pairs.push((&tag[..i], &tag[i + 1..])),
            _ => return Err(format!("bad tag \"{tag}\": want key:value")),
        }
    }
    // Keys in the order they first appear.
    let mut keys: Vec<&str> = Vec::new();
    for (k, _) in &pairs {
        if !keys.contains(k) {
            keys.push(k);
        }
    }
    let of_key = |key: &str| {
        let mut values: Vec<String> = pairs.iter().filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()).collect();
        match values.len() {
            1 => TagFilter::Tag(key.to_string(), values.remove(0)),
            _ => TagFilter::In(key.to_string(), values),
        }
    };
    match keys.as_slice() {
        [] => Ok(TagFilter::All),
        [key] => Ok(of_key(key)),
        keys => Ok(TagFilter::And(keys.iter().map(|k| of_key(k)).collect())),
    }
}

/// A point in time from a query parameter, in unix milliseconds: empty (the fallback), "now", an
/// offset from now ("-6h"), or RFC 3339. A dashboard wants "the last six hours", not six hours
/// before the day it was saved. Milliseconds, so that a range is ordered before it is clamped to a
/// date.
pub fn parse_time(raw: &str, fallback: i64, now: i64) -> Result<i64, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        Ok(fallback)
    } else if raw == "now" {
        Ok(now)
    } else if raw.starts_with('-') || raw.starts_with('+') {
        duration::parse(raw).map(|offset| now + offset.num_milliseconds())
    } else {
        rfc3339(raw)
            .ok_or_else(|| format!("cannot parse \"{raw}\" as a time: want RFC 3339, \"now\" or an offset like -1h"))
    }
}

/// What .NET's `DateTimeOffset.TryParseExact` took with the formats `yyyy-MM-dd'T'HH:mm:ssK` and
/// `yyyy-MM-dd'T'HH:mm:ss.FFFFFFFK` and AssumeUniversal: up to seven fraction digits, `Z`, an
/// offset or no zone at all, which is UTC.
static RFC3339: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.([0-9]{0,7}))?(Z|([+-])(?:([0-9]{1,2}):([0-9]{2})|([0-9]{2})([0-9]{2})))?$",
    )
    .unwrap()
});

fn rfc3339(raw: &str) -> Option<i64> {
    let c = RFC3339.captures(raw)?;
    let number = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
    let date = NaiveDate::from_ymd_opt(c[1].parse().ok()?, number(2)?, number(3)?)?;
    let fraction = c.get(7).map_or("", |m| m.as_str());
    let ticks: u32 = format!("{fraction:0<7}").parse().ok()?;
    let local = date.and_hms_nano_opt(number(4)?, number(5)?, number(6)?, ticks * 100)?;
    let (hours, minutes) = match (number(10).or(number(12)), number(11).or(number(13))) {
        (Some(h), Some(m)) => (h, m),
        _ => (0, 0),
    };
    let sign = if c.get(9).is_some_and(|s| s.as_str() == "-") { -1 } else { 1 };
    let offset_minutes = sign * i64::from(hours * 60 + minutes);
    if date.year() == 0 || minutes >= 60 || offset_minutes.abs() > 14 * 60 {
        return None;
    }
    let utc = local - TimeDelta::minutes(offset_minutes);
    let earliest = NaiveDate::from_ymd_opt(1, 1, 1)?.and_hms_opt(0, 0, 0)?;
    let latest: NaiveDateTime = NaiveDate::from_ymd_opt(9999, 12, 31)?.and_hms_nano_opt(23, 59, 59, 999_999_900)?;
    if utc < earliest || utc > latest {
        return None;
    }
    Some(utc.and_utc().timestamp_millis())
}

/// The words of a search text as ClickHouse's hasToken wants them: runs of ASCII letters and
/// digits.
pub fn message_tokens(text: &str) -> Vec<String> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new("[0-9a-zA-Z]+").unwrap());
    TOKEN.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

// --- SQL ------------------------------------------------------------------------

fn tenant_of(tenant: &TenantId) -> SqlValue {
    SqlValue::String(tenant.0.clone())
}

fn bounded_limit(asked: i32, fallback: i32) -> SqlValue {
    SqlValue::Int64(i64::from(if asked <= 0 || asked > 1000 { fallback } else { asked }))
}

/// The hours of `metric_tag_keys` a range touches; the hour holding `from` counts in full.
fn hours(bound: &mut SqlParams, from: i64, until: i64) -> String {
    let from = bound.add(SqlValue::Int64(from));
    let until = bound.add(SqlValue::Int64(until));
    format!(" AND hour >= toStartOfHour(fromUnixTimestamp64Milli({from})) AND hour < fromUnixTimestamp64Milli({until})")
}

fn between(bound: &mut SqlParams, from: i64, until: i64) -> String {
    let from = bound.add(SqlValue::Int64(from));
    let until = bound.add(SqlValue::Int64(until));
    format!(" AND timestamp >= fromUnixTimestamp64Milli({from}) AND timestamp < fromUnixTimestamp64Milli({until})")
}

/// Metric names seen in `[from, until)`, optionally those containing `search`.
pub fn list_metrics(tenant: &TenantId, from: i64, until: i64, search: &str, limit: i32) -> Sql {
    let mut bound = SqlParams::new();
    let tenant = bound.add(tenant_of(tenant));
    let where_ = format!("tenant_id = {tenant}{}", hours(&mut bound, from, until));
    let search_filter = if search.is_empty() {
        String::new()
    } else {
        format!(" AND positionCaseInsensitive(metric, {}) > 0", bound.add(SqlValue::String(search.to_string())))
    };
    let limit = bound.add(bounded_limit(limit, 500));
    Sql {
        text: format!(
            "SELECT DISTINCT metric FROM metric_tag_keys WHERE {where_}{search_filter} ORDER BY metric LIMIT {limit}"
        ),
        parameters: bound.into_parameters(),
    }
}

/// The first 500 service check names seen in `[from, until)`.
pub fn list_checks(tenant: &TenantId, from: i64, until: i64) -> Sql {
    let mut bound = SqlParams::new();
    let tenant = bound.add(tenant_of(tenant));
    let where_ = format!("tenant_id = {tenant}{}", between(&mut bound, from, until));
    Sql {
        text: format!("SELECT DISTINCT check_name FROM check_runs WHERE {where_} ORDER BY check_name LIMIT 500"),
        parameters: bound.into_parameters(),
    }
}

/// Tag keys seen in `[from, until)`, optionally on one metric. The key '' only lists a metric
/// without tags.
pub fn list_tag_keys(tenant: &TenantId, from: i64, until: i64, metric: &str) -> Sql {
    let mut bound = SqlParams::new();
    let tenant = bound.add(tenant_of(tenant));
    let where_ = format!("tenant_id = {tenant}{}", hours(&mut bound, from, until));
    let metric_filter = metric_filter(&mut bound, metric);
    Sql {
        text: format!(
            "SELECT DISTINCT key FROM metric_tag_keys WHERE {where_}{metric_filter} AND key != '' ORDER BY key LIMIT 1000"
        ),
        parameters: bound.into_parameters(),
    }
}

fn metric_filter(bound: &mut SqlParams, metric: &str) -> String {
    if metric.is_empty() {
        String::new()
    } else {
        format!(" AND metric = {}", bound.add(SqlValue::String(metric.to_string())))
    }
}

/// Values of one tag key seen in `[from, until)`, optionally on one metric and containing
/// `search`.
#[allow(clippy::too_many_arguments)]
pub fn list_tag_values(
    tenant: &TenantId,
    from: i64,
    until: i64,
    metric: &str,
    key: &str,
    search: &str,
    limit: i32,
) -> Sql {
    let mut bound = SqlParams::new();
    let column = if metrics::LIFTED_TAGS.contains(&key) {
        key.to_string()
    } else {
        format!("tags[{}]", bound.add(SqlValue::String(key.to_string())))
    };
    let tenant_filter = format!("tenant_id = {}", bound.add(tenant_of(tenant)));
    let metric_filter = metric_filter(&mut bound, metric);
    let from_sql = format!("fromUnixTimestamp64Milli({})", bound.add(SqlValue::Int64(from)));
    let until_sql = format!("fromUnixTimestamp64Milli({})", bound.add(SqlValue::Int64(until)));
    let values = format!(
        "SELECT arrayJoin({column}) AS v FROM metrics WHERE {tenant_filter}{metric_filter} AND {}",
        metrics::raw_window(&from_sql, &until_sql)
    );
    let search_filter = if search.is_empty() {
        String::new()
    } else {
        format!(" WHERE positionCaseInsensitive(v, {}) > 0", bound.add(SqlValue::String(search.to_string())))
    };
    let limit = bound.add(bounded_limit(limit, 200));
    Sql {
        text: format!("SELECT DISTINCT v FROM ({values}){search_filter} ORDER BY v LIMIT {limit}"),
        parameters: bound.into_parameters(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeriesQuery {
    pub metric: String,
    pub hosts: Vec<String>,
    /// Unix milliseconds.
    pub from: i64,
    pub to: i64,
    /// The bucket width asked for; None leaves it to the engine.
    pub step: Option<TimeDelta>,
    /// avg, sum, min or max; "" is avg.
    pub aggregation: String,
    pub tags: TagFilter,
    /// Tag keys to split series by; empty splits by host.
    pub group_by: Vec<String>,
}

/// One metric over a time range as the query engine takes it: the request `avg:metric{tags} by
/// {keys}` would parse into. So the panel's series are aggregated like every other query: over
/// time within a series first, then across the series of a group.
pub fn series_request(q: &SeriesQuery) -> ParsedTimeseriesRequest {
    let filter = match (q.hosts.as_slice(), &q.tags) {
        ([], tags) => tags.clone(),
        (hosts, TagFilter::All) => TagFilter::In("host".to_string(), hosts.to_vec()),
        (hosts, tags) => TagFilter::And(vec![TagFilter::In("host".to_string(), hosts.to_vec()), tags.clone()]),
    };
    let group_by = if q.group_by.is_empty() {
        vec!["host".to_string()]
    } else {
        let mut keys = q.group_by.clone();
        keys.sort();
        keys.dedup();
        keys
    };
    let query = MetricQuery {
        space_agg: Some(if q.aggregation.is_empty() { "avg".to_string() } else { q.aggregation.to_lowercase() }),
        metric: q.metric.clone(),
        filter,
        group_by,
        modifiers: vec![],
    };
    ParsedTimeseriesRequest {
        from: q.from,
        to: q.to,
        interval: q.step,
        queries: vec![ParsedQuery { name: Some("q".to_string()), query }],
        formulas: vec![],
    }
}

/// One series as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// The host, or `key:value, key:value` of its group.
    pub name: String,
    /// Set when the query was not grouped: then a series is a host.
    pub host: Option<String>,
    pub tags: BTreeMap<String, String>,
    /// (unix milliseconds, value).
    pub points: Vec<(i64, f64)>,
}

/// The engine's answer to `series_request(q)` as the panel's series. A series without one of the
/// group's tags has Datadog's "N/A" for it.
pub fn series_of(q: &SeriesQuery, results: &[FormulaResult]) -> Vec<Series> {
    let mut all = Vec::new();
    for result in results {
        for line in &result.series {
            let pairs: Vec<(String, String)> = line
                .tags
                .iter()
                .map(|tag| (tag.key.clone(), tag.value.clone().unwrap_or_else(|| NOT_APPLICABLE.to_string())))
                .collect();
            let (name, host) = match (q.group_by.as_slice(), pairs.as_slice()) {
                ([], [(_, host)]) => (host.clone(), Some(host.clone())),
                _ => (pairs.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(", "), None),
            };
            all.push(Series {
                name,
                host,
                tags: pairs.into_iter().collect(),
                points: line.points.iter().map(|p| (p.time, p.value)).collect(),
            });
        }
    }
    all
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogSearch {
    pub tenant: TenantId,
    /// Free text looked for in the message.
    pub text: String,
    pub service: String,
    pub host: String,
    pub status: String,
    pub tags: TagFilter,
    pub from: i64,
    pub to: i64,
    pub limit: i32,
}

fn logs_where(bound: &mut SqlParams, q: &LogSearch) -> String {
    let tenant = bound.add(tenant_of(&q.tenant));
    let scope = format!(" WHERE tenant_id = {tenant}{}", between(bound, q.from, q.to));

    let mut equals = |column: &str, value: &str| {
        if value.is_empty() {
            String::new()
        } else {
            format!(" AND {column} = {}", bound.add(SqlValue::String(value.to_string())))
        }
    };
    let facets = equals("service", &q.service) + &equals("host", &q.host) + &equals("status", &q.status);

    // hasToken lets the token index skip granules; position then confirms the whole phrase, which
    // the index cannot.
    let text = match q.text.trim() {
        "" => String::new(),
        phrase => {
            let mut tokens = String::new();
            for token in message_tokens(phrase) {
                tokens += &format!(" AND hasToken(message, {})", bound.add(SqlValue::String(token)));
            }
            tokens + &format!(" AND position(message, {}) > 0", bound.add(SqlValue::String(phrase.to_string())))
        }
    };

    let tags =
        if q.tags == TagFilter::All { String::new() } else { format!(" AND ({})", filter::sql(&[], bound, &q.tags)) };
    scope + &facets + &text + &tags
}

/// Two queries: the newest matching logs, and how many match in all.
pub fn search_logs(q: &LogSearch) -> (Sql, Sql) {
    let limit: i64 = if q.limit <= 0 {
        200
    } else if q.limit > 1000 {
        1000
    } else {
        i64::from(q.limit)
    };

    let mut rows_bound = SqlParams::new();
    let rows_where = logs_where(&mut rows_bound, q);
    let limit = rows_bound.add(SqlValue::Int64(limit));
    let mut count_bound = SqlParams::new();
    let count_where = logs_where(&mut count_bound, q);

    (
        Sql {
            text: format!(
                "SELECT toUnixTimestamp64Milli(timestamp), host, service, source, status, message, tags FROM logs{rows_where} ORDER BY timestamp DESC LIMIT {limit}"
            ),
            parameters: rows_bound.into_parameters(),
        },
        Sql { text: format!("SELECT count() FROM logs{count_where}"), parameters: count_bound.into_parameters() },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFacet {
    Service,
    Host,
    Status,
}

/// The fifty most common values of one facet in `[from, until)`, with counts.
pub fn log_facet(tenant: &TenantId, from: i64, until: i64, facet: LogFacet) -> Sql {
    let mut bound = SqlParams::new();
    let column = match facet {
        LogFacet::Service => "service",
        LogFacet::Host => "host",
        LogFacet::Status => "status",
    };
    let tenant = bound.add(tenant_of(tenant));
    let window = between(&mut bound, from, until);
    Sql {
        text: format!(
            "SELECT {column} AS v, count() AS c FROM logs WHERE tenant_id = {tenant}{window} GROUP BY v ORDER BY c DESC, v LIMIT 50"
        ),
        parameters: bound.into_parameters(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration;
    use crate::query::compile::Row;
    use crate::query::engine;
    use crate::query::execute::{Executor, QueryError};
    use crate::query::plan::DEFAULT_RETENTION;

    fn tenant() -> TenantId {
        TenantId("default".into())
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap().and_utc().timestamp_millis()
    }

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;

    fn noon() -> i64 {
        at(2026, 9, 21, 12, 0, 0)
    }

    fn values(sql: &Sql) -> Vec<SqlValue> {
        sql.parameters.iter().map(|(_, v)| v.clone()).collect()
    }

    fn string(s: &str) -> SqlValue {
        SqlValue::String(s.into())
    }

    fn tags(raw: &[&str]) -> Result<TagFilter, String> {
        parse_tag_filters(&raw.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    }

    // --- tag filters --------------------------------------------------------------------

    #[test]
    fn tags_of_one_key_are_one_condition_keys_in_order_of_first_appearance() {
        assert_eq!(tags(&[]), Ok(TagFilter::All));
        assert_eq!(tags(&["env:prod"]), Ok(TagFilter::Tag("env".into(), "prod".into())));
        assert_eq!(
            tags(&["env:prod", "kube_service:a", "kube_service:b"]),
            Ok(TagFilter::And(vec![
                TagFilter::Tag("env".into(), "prod".into()),
                TagFilter::In("kube_service".into(), vec!["a".into(), "b".into()])
            ]))
        );
    }

    #[test]
    fn a_tag_value_keeps_its_colons() {
        assert_eq!(tags(&["image:nginx:1.25"]), Ok(TagFilter::Tag("image".into(), "nginx:1.25".into())));
    }

    #[test]
    fn a_tag_without_key_or_value_is_refused() {
        for tag in ["envprod", ":prod", "env:"] {
            assert!(tags(&[tag]).is_err(), "{tag}");
        }
    }

    // --- times and durations ------------------------------------------------------------------

    #[test]
    fn times_are_relative_absolute_or_the_fallback() {
        let fallback = noon() - HOUR;
        let parse = |raw: &str| parse_time(raw, fallback, noon()).unwrap();
        assert_eq!(parse(""), fallback);
        assert_eq!(parse("now"), noon());
        assert_eq!(parse("-6h"), noon() - 6 * HOUR);
        assert_eq!(parse("-15m"), noon() - 15 * MINUTE);
        assert_eq!(parse("-7d"), noon() - 7 * DAY);
        assert_eq!(parse("-2w"), noon() - 14 * DAY);
        assert_eq!(parse("-1.5d"), noon() - 36 * HOUR);
        assert_eq!(parse("2026-09-21T09:30:00Z"), at(2026, 9, 21, 9, 30, 0));
        assert_eq!(parse("-1h30m"), noon() - 90 * MINUTE);
    }

    #[test]
    fn rfc_3339_as_the_two_dotnet_formats_read_it() {
        let parse = |raw: &str| parse_time(raw, 0, 0);
        let half_past_nine = at(2026, 9, 21, 9, 30, 0);
        assert_eq!(parse("2026-09-21T09:30:00"), Ok(half_past_nine));
        assert_eq!(parse("2026-09-21T09:30:00.1234567Z"), Ok(half_past_nine + 123));
        assert_eq!(parse("2026-09-21T09:30:00.Z"), Ok(half_past_nine));
        assert_eq!(parse("2026-09-21T11:30:00+02:00"), Ok(half_past_nine));
        assert_eq!(parse("2026-09-21T11:30:00+0200"), Ok(half_past_nine));
        assert_eq!(parse("2026-09-21T04:00:00-5:30"), Ok(half_past_nine));
        assert_eq!(parse(" 2026-09-21T09:30:00Z "), Ok(half_past_nine));
        for bad in [
            "2026-09-21T09:30:00.12345678Z",
            "2026-09-21 09:30:00Z",
            "2026-09-21t09:30:00Z",
            "2026-09-21T09:30:00z",
            "2026-9-21T09:30:00Z",
            "2026-09-21T09:30Z",
            "2026-09-21T09:30:00+15:00",
            "2026-09-21T09:30:00+02",
            "2026-09-21T09:30:00+230",
            "2026-02-30T09:30:00Z",
            "2026-09-21T24:00:00Z",
            "0000-01-01T00:00:00Z",
            "0001-01-01T00:00:00+01:00",
        ] {
            assert_eq!(
                parse(bad),
                Err(format!("cannot parse \"{bad}\" as a time: want RFC 3339, \"now\" or an offset like -1h")),
                "{bad}"
            );
        }
        assert_eq!(parse("0001-01-01T00:00:00Z"), Ok(-62_135_596_800_000));
    }

    #[test]
    fn an_offset_past_what_a_date_can_hold_is_still_a_time() {
        assert_eq!(parse_time("-5000000d", noon(), noon()), Ok(noon() - 5_000_000 * DAY));
        assert_eq!(parse_time("+5000000d", noon(), noon()), Ok(noon() + 5_000_000 * DAY));
    }

    #[test]
    fn a_time_that_is_none_of_those_is_refused() {
        for raw in ["yesterday", "-3x", "-"] {
            assert!(parse_time(raw, noon(), noon()).is_err(), "{raw}");
        }
    }

    #[test]
    fn a_duration_is_a_sign_and_number_and_unit_pairs_from_nanoseconds_to_weeks() {
        assert_eq!(duration::parse("15s"), Ok(TimeDelta::seconds(15)));
        assert_eq!(duration::parse("-1h30m"), Ok(TimeDelta::minutes(-90)));
        assert_eq!(duration::parse("300ms"), Ok(TimeDelta::milliseconds(300)));
        assert_eq!(duration::parse("7d"), Ok(TimeDelta::days(7)));
        assert_eq!(duration::parse("+2w"), Ok(TimeDelta::days(14)));
        assert_eq!(duration::parse("1.5d"), Ok(TimeDelta::hours(36)));
        for bad in ["", "15", "s", "3x", "1h 30m", "-"] {
            assert!(duration::parse(bad).is_err(), "{bad}");
        }
    }

    // --- series ------------------------------------------------------------------------------

    fn cpu() -> SeriesQuery {
        SeriesQuery {
            metric: "system.cpu.user".into(),
            hosts: vec![],
            from: noon() - MINUTE,
            to: noon(),
            step: Some(TimeDelta::seconds(20)),
            aggregation: String::new(),
            tags: TagFilter::All,
            group_by: vec![],
        }
    }

    #[test]
    fn a_series_query_is_the_engines_own_request_by_host_unless_grouped_avg_unless_told() {
        let request = series_request(&cpu());
        let plain = &request.queries[0].query;
        assert_eq!(plain.space_agg.as_deref(), Some("avg"));
        assert_eq!(plain.group_by, vec!["host".to_string()]);
        assert_eq!(plain.filter, TagFilter::All);
        assert!(request.formulas.is_empty());
        assert_eq!(request.interval, cpu().step);

        let narrowed = series_request(&SeriesQuery {
            hosts: vec!["a".into(), "b".into()],
            tags: TagFilter::Tag("env".into(), "prod".into()),
            aggregation: "MAX".into(),
            group_by: vec!["service".into(), "env".into(), "env".into()],
            ..cpu()
        });
        let narrowed = &narrowed.queries[0].query;
        assert_eq!(narrowed.space_agg.as_deref(), Some("max"));
        assert_eq!(narrowed.group_by, vec!["env".to_string(), "service".to_string()]);
        assert_eq!(
            narrowed.filter,
            TagFilter::And(vec![
                TagFilter::In("host".into(), vec!["a".into(), "b".into()]),
                TagFilter::Tag("env".into(), "prod".into())
            ])
        );
        assert_eq!(
            series_request(&SeriesQuery { hosts: vec!["a".into()], ..cpu() }).queries[0].query.filter,
            TagFilter::In("host".into(), vec!["a".into()])
        );
    }

    /// Two series, each with one point per bucket; with two group keys the second series lacks
    /// the second tag.
    struct TwoSeries;

    impl Executor for TwoSeries {
        async fn rows(&self, sql: Sql) -> Result<Vec<Row>, String> {
            let groups = sql.text.matches("AS g").count();
            let row = |group: Vec<Option<&str>>, t: i64, v: f64| Row {
                groups: group.into_iter().map(|g| g.map(String::from)).collect(),
                series_id: None,
                bucket_ms: t,
                value: v,
            };
            let start = noon() - MINUTE;
            let mut rows = Vec::new();
            for i in 0..3 {
                let t = start + i * 20_000;
                rows.push(row(
                    if groups == 1 { vec![Some("a")] } else { vec![Some("prod"), Some("web")] },
                    t,
                    i as f64,
                ));
                rows.push(row(
                    if groups == 1 { vec![Some("b")] } else { vec![Some("prod"), None] },
                    t,
                    i as f64 * 10.0,
                ));
            }
            Ok(rows)
        }
    }

    async fn run(q: &SeriesQuery) -> Result<Vec<FormulaResult>, QueryError> {
        engine::parsed(&TwoSeries, &tenant(), DEFAULT_RETENTION, noon(), &["q".to_string()], &series_request(q)).await
    }

    async fn series_for(q: &SeriesQuery) -> Vec<Series> {
        match run(q).await {
            Ok(results) => series_of(q, &results),
            Err(e) => panic!("{e:?}"),
        }
    }

    #[tokio::test]
    async fn ungrouped_series_are_hosts_grouped_ones_are_named_by_their_tags_na_where_one_is_missing() {
        let hosts = series_for(&cpu()).await;
        assert_eq!(hosts.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(hosts.iter().map(|s| s.host.as_deref()).collect::<Vec<_>>(), [Some("a"), Some("b")]);
        assert_eq!(hosts[0].tags, BTreeMap::from([("host".to_string(), "a".to_string())]));
        assert_eq!(hosts[0].points, vec![(noon() - 60_000, 0.0), (noon() - 40_000, 1.0), (noon() - 20_000, 2.0)]);

        let grouped = series_for(&SeriesQuery { group_by: vec!["service".into(), "env".into()], ..cpu() }).await;
        let mut names: Vec<&str> = grouped.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        names.reverse();
        assert_eq!(names, ["env:prod, service:web", "env:prod, service:N/A"]);
        assert!(grouped.iter().all(|s| s.host.is_none()));
    }

    #[tokio::test]
    async fn an_aggregation_the_engine_does_not_have_is_refused_not_replaced() {
        match run(&SeriesQuery { aggregation: "median".into(), ..cpu() }).await {
            Err(QueryError::Invalid(problems)) => assert!(problems.join("; ").contains("median"), "{problems:?}"),
            other => panic!("median was accepted: {other:?}"),
        }
    }

    // --- SQL ---------------------------------------------------------------------------------

    fn logs() -> LogSearch {
        LogSearch {
            tenant: tenant(),
            text: String::new(),
            service: String::new(),
            host: String::new(),
            status: String::new(),
            tags: TagFilter::All,
            from: noon() - HOUR,
            to: noon(),
            limit: 0,
        }
    }

    #[test]
    fn a_tag_filter_on_logs_is_a_membership_test_on_the_map_several_values_has_any() {
        let (rows, _) = search_logs(&LogSearch {
            tags: TagFilter::And(vec![
                TagFilter::Tag("team".into(), "core".into()),
                TagFilter::In("zone".into(), vec!["a".into(), "b".into()]),
            ]),
            ..logs()
        });
        assert!(
            rows.text.contains(
                "AND ((has(tags[{k3:String}], {p4:String})) AND (hasAny(tags[{k5:String}], {p6:Array(String)})))"
            ),
            "{}",
            rows.text
        );
        assert!(values(&rows).contains(&string("team")));
        assert!(values(&rows).contains(&SqlValue::StringArray(vec!["a".into(), "b".into()])));

        let (unfiltered, _) = search_logs(&logs());
        assert!(!unfiltered.text[unfiltered.text.find(" WHERE").unwrap()..].contains("tags["));
    }

    #[test]
    fn the_callers_text_is_bound_never_part_of_the_sql() {
        let evil = "env'] , 1); DROP TABLE logs; --";
        let (rows, count) = search_logs(&LogSearch {
            tags: TagFilter::Tag(evil.into(), "x'y".into()),
            service: evil.into(),
            text: evil.into(),
            ..logs()
        });
        for sql in [rows, count] {
            assert!(!sql.text.contains("DROP"));
            assert!(!sql.text.replace("toUnixTimestamp64Milli", "").contains('\''));
            assert!(values(&sql).contains(&string(evil)));
            assert!(values(&sql).contains(&string("x'y")));
        }
    }

    #[test]
    fn message_tokens_are_runs_of_letters_and_digits() {
        assert_eq!(message_tokens("connection refused"), ["connection", "refused"]);
        assert_eq!(message_tokens("GET /api/v2/logs?x=1"), ["GET", "api", "v2", "logs", "x", "1"]);
        assert_eq!(message_tokens("err_code=500"), ["err", "code", "500"]);
        assert!(message_tokens("...").is_empty());
    }

    #[test]
    fn log_search_scopes_by_tenant_first_then_tokens_then_the_phrase() {
        let search = LogSearch {
            tenant: tenant(),
            text: "connection refused".into(),
            service: "nginx".into(),
            host: String::new(),
            status: String::new(),
            tags: TagFilter::Tag("k\"ey'".into(), "v".into()),
            from: noon() - HOUR,
            to: noon(),
            limit: 5000,
        };
        let (rows, count) = search_logs(&search);
        let where_ = &rows.text[rows.text.find(" WHERE").unwrap()..];
        assert!(where_.starts_with(" WHERE tenant_id = {p0:String}"));
        assert_eq!(where_.matches("hasToken(message, ").count(), 2);
        assert!(where_.find("hasToken").unwrap() < where_.find("position(message").unwrap());
        assert!(!rows.text.replace("toUnixTimestamp64Milli", "").contains('\''));
        assert!(!rows.text.contains('"'));
        assert_eq!(
            values(&rows),
            vec![
                string("default"),
                SqlValue::Int64(search.from),
                SqlValue::Int64(search.to),
                string("nginx"),
                string("connection"),
                string("refused"),
                string("connection refused"),
                string("k\"ey'"),
                string("v"),
                // The limit is capped.
                SqlValue::Int64(1000),
            ]
        );
        assert!(count.text.starts_with("SELECT count() FROM logs WHERE tenant_id = {p0:String}"));
        assert_eq!(count.parameters.len(), 9);
    }

    #[test]
    fn lookups_bound_their_limits() {
        let hour_ago = noon() - HOUR;
        assert!(values(&list_metrics(&tenant(), hour_ago, noon(), "", 0)).contains(&SqlValue::Int64(500)));
        assert!(values(&list_metrics(&tenant(), hour_ago, noon(), "cpu", 5000)).contains(&SqlValue::Int64(500)));
        assert!(values(&list_metrics(&tenant(), hour_ago, noon(), "cpu", 50)).contains(&SqlValue::Int64(50)));
        let search = list_metrics(&tenant(), hour_ago, noon(), "cpu", 50);
        assert!(search.text.contains("positionCaseInsensitive(metric, {p"));
        assert!(values(&search).contains(&string("cpu")));
        assert!(
            values(&list_tag_values(&tenant(), hour_ago, noon(), "m", "env", "", 0)).contains(&SqlValue::Int64(200))
        );
        assert!(
            log_facet(&tenant(), noon(), noon(), LogFacet::Status)
                .text
                .contains("GROUP BY v ORDER BY c DESC, v LIMIT 50")
        );
    }

    #[test]
    fn metric_names_and_tag_keys_come_from_metric_tag_keys_by_the_hour() {
        let hour_ago = noon() - HOUR;
        for sql in [list_metrics(&tenant(), hour_ago, noon(), "", 0), list_tag_keys(&tenant(), hour_ago, noon(), "m")] {
            assert!(sql.text.contains("FROM metric_tag_keys WHERE tenant_id = {p"));
            assert!(sql.text.contains("hour >= toStartOfHour(fromUnixTimestamp64Milli({p"));
            assert!(sql.text.contains("hour < fromUnixTimestamp64Milli({p"));
            assert!(values(&sql).contains(&SqlValue::Int64(hour_ago)));
            assert!(values(&sql).contains(&SqlValue::Int64(noon())));
        }
        // '' only marks a metric without tags.
        assert!(list_tag_keys(&tenant(), hour_ago, noon(), "").text.contains("key != ''"));
    }

    #[test]
    fn tag_values_read_a_lifted_tags_column_any_other_from_the_map_within_the_window() {
        let hour_ago = noon() - HOUR;
        let env = list_tag_values(&tenant(), hour_ago, noon(), "m", "env", "", 0);
        assert!(env.text.contains("arrayJoin(env) AS v FROM metrics"));
        let zone = list_tag_values(&tenant(), hour_ago, noon(), "m", "zone", "", 0);
        assert!(zone.text.contains("arrayJoin(tags[{p"));
        assert!(values(&zone).contains(&string("zone")));
        for sql in [env, zone] {
            assert!(
                sql.text.contains(
                    "toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(fromUnixTimestamp64Milli({p"
                )
            );
            assert!(sql.text.contains("timestamp < fromUnixTimestamp64Milli({p"));
        }
    }

    #[test]
    fn a_lookups_key_is_bound_never_inlined() {
        let sql = list_tag_values(&tenant(), noon() - HOUR, noon(), "m", "x') OR 1 --", "", 0);
        assert!(!sql.text.contains("OR 1"));
        assert!(values(&sql).contains(&string("x') OR 1 --")));
    }

    #[test]
    fn the_whole_texts() {
        let sql = list_tag_values(&tenant(), 1, 2, "m", "zone", "we", 7);
        assert_eq!(
            sql.text,
            "SELECT DISTINCT v FROM (SELECT arrayJoin(tags[{p0:String}]) AS v FROM metrics WHERE tenant_id = {p1:String} AND metric = {p2:String} AND timestamp >= fromUnixTimestamp64Milli({p3:Int64}) AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(fromUnixTimestamp64Milli({p3:Int64})) AND timestamp < fromUnixTimestamp64Milli({p4:Int64}) AND toStartOfFifteenMinutes(timestamp) < fromUnixTimestamp64Milli({p4:Int64})) WHERE positionCaseInsensitive(v, {p5:String}) > 0 ORDER BY v LIMIT {p6:Int64}"
        );
        assert_eq!(
            list_checks(&tenant(), 1, 2).text,
            "SELECT DISTINCT check_name FROM check_runs WHERE tenant_id = {p0:String} AND timestamp >= fromUnixTimestamp64Milli({p1:Int64}) AND timestamp < fromUnixTimestamp64Milli({p2:Int64}) ORDER BY check_name LIMIT 500"
        );
        assert_eq!(
            log_facet(&tenant(), 1, 2, LogFacet::Host).text,
            "SELECT host AS v, count() AS c FROM logs WHERE tenant_id = {p0:String} AND timestamp >= fromUnixTimestamp64Milli({p1:Int64}) AND timestamp < fromUnixTimestamp64Milli({p2:Int64}) GROUP BY v ORDER BY c DESC, v LIMIT 50"
        );
        let (rows, count) = search_logs(&LogSearch { host: "web".into(), ..logs() });
        assert!(
            rows.text.ends_with(" AND host = {p3:String} ORDER BY timestamp DESC LIMIT {p4:Int64}"),
            "{}",
            rows.text
        );
        assert!(count.text.ends_with(" AND host = {p3:String}"));
    }
}
