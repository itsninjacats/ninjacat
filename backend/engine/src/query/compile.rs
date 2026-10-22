//! Compiles one `QueryPlan` into SQL against `metrics`, or against its one-minute rollup
//! `metrics_1m` when the step is whole minutes.
//!
//! Time first, then space, as Datadog orders them (docs: dashboards/guide/query-to-the-graph): an
//! inner query makes one row per series and bucket with the time aggregator, an outer one combines
//! the series of a group with the space aggregator. Doing it in one pass would average raw points
//! across hosts, which weighs a host that reports twice as often twice as much: not what Datadog
//! shows.
//!
//! Every value that came from the request is a bound parameter; the SQL text holds only
//! identifiers this file chose.

use crate::metric_query::filter;
use crate::metrics::{LIFTED_TAGS, raw_window};
use crate::query::plan::{Aggregation, Fill, Plan, QueryPlan, Table, TimeAggregation, earliest_offset_ms, table_for};
use crate::sql::{Sql, SqlParams, SqlValue, TenantId};

/// One row of a compiled query: `g0 … gN-1` (Nullable(String), one per `by` key, NULL where the
/// series has no such tag), `bucket_ms` (Int64, the bucket's start), `value` (Float64), and
/// `series_id` (UInt64) only when the plan fills gaps: then each row is one series' bucket,
/// uncombined.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// None: the series has no value for that `by` key, Datadog's N/A group.
    pub groups: Vec<Option<String>>,
    /// Set when the query returns series one by one (see `compile`), for the engine to fill
    /// and combine them; None when ClickHouse already combined them.
    pub series_id: Option<u64>,
    pub bucket_ms: i64,
    pub value: f64,
}

fn aggregate(agg: Aggregation) -> &'static str {
    match agg {
        Aggregation::Avg => "avg",
        Aggregation::Sum => "sum",
        Aggregation::Min => "min",
        Aggregation::Max => "max",
        Aggregation::Count => "count",
    }
}

/// The inner, per-series aggregate. `step` is the bucket width placeholder.
///
/// One series has one metric type, so `any(metric_type)` over its rows in a bucket is that type:
/// the formula can follow it without a separate lookup. UNSPECIFIED and anything unknown are
/// treated as GAUGE.
///
/// A RATE point is events per second over the metric's `interval`, so its event count is
/// value × interval.
///
/// WARNING(undocumented): a RATE row with interval 0. Such rows exist, and would count as no
/// events at all; the interval is floored at 1 second.
fn time_aggregate_sql(step: &str, agg: TimeAggregation) -> String {
    match agg {
        TimeAggregation::Plain(Aggregation::Count) => "toFloat64(count())".to_string(),
        TimeAggregation::Plain(agg) => format!("{}(value)", aggregate(agg)),
        TimeAggregation::AsCount => {
            "multiIf(any(metric_type) = 'RATE', sum(value * greatest(interval, 1)), ".to_string()
                + "any(metric_type) = 'COUNT', sum(value), avg(value))"
        }
        // For RATE, the bucket is floored to the metric's interval: a 5 s bucket over a metric
        // sent every 10 s would otherwise double it.
        TimeAggregation::AsRate => {
            format!(
                "multiIf(any(metric_type) = 'RATE', sum(value * greatest(interval, 1)) / greatest({step}, max(interval)), "
            ) + &format!("any(metric_type) = 'COUNT', sum(value) / {step}, avg(value))")
        }
    }
}

/// The same over the rollup, whose rows are plain counts, sums, minima and maxima per minute,
/// read without FINAL: rows of one minute not yet merged simply add up. An average is the summed
/// sums over the summed counts, never an average of averages. `metric_type` and `interval` are
/// kept per minute as `any` and `max`, so the per-series reads above give the same answer here.
fn rollup_aggregate_sql(step: &str, agg: TimeAggregation) -> String {
    match agg {
        TimeAggregation::Plain(Aggregation::Count) => "toFloat64(sum(points))".to_string(),
        TimeAggregation::Plain(Aggregation::Avg) => "sum(value_sum) / sum(points)".to_string(),
        TimeAggregation::Plain(agg) => format!("{0}(value_{0})", aggregate(agg)),
        TimeAggregation::AsCount => {
            "multiIf(any(metric_type) = 'RATE', sum(value_events), ".to_string()
                + "any(metric_type) = 'COUNT', sum(value_sum), sum(value_sum) / sum(points))"
        }
        TimeAggregation::AsRate => {
            format!("multiIf(any(metric_type) = 'RATE', sum(value_events) / greatest({step}, max(interval)), ")
                + &format!("any(metric_type) = 'COUNT', sum(value_sum) / {step}, sum(value_sum) / sum(points))")
        }
    }
}

/// One expression per `by` key. A tag holds a list of values, and a series with `role:api` and
/// `role:web` belongs to both groups: arrayJoin gives it a row in each.
///
/// A series without the tag is not dropped: Datadog shows it in an N/A group (that is what
/// exclude_null() exists to remove; docs: dashboards/functions/exclusion). An empty list would
/// make arrayJoin yield no row at all, so it becomes [NULL] first.
///
/// `columns` are the tags the table keeps as array columns of their own.
pub fn group_sql(columns: &[&str], p: &mut SqlParams, key: &str) -> String {
    match key {
        // WARNING(undocumented): a series with an empty host. It joins the N/A group, as a
        // missing tag does.
        "host" => "nullIf(toString(host), '')".to_string(),
        k if columns.contains(&k) => format!("arrayJoin(if(empty({k}), [NULL], CAST({k}, 'Array(Nullable(String))')))"),
        k => {
            let k = p.add_as("k", SqlValue::String(k.to_string()));
            format!("arrayJoin(if(empty(tags[{k}]), [NULL], CAST(tags[{k}], 'Array(Nullable(String))')))")
        }
    }
}

/// The first bucket a query's series keep: `from`, less its lookback.
pub fn keep_from_ms(plan: &Plan, query: &QueryPlan) -> i64 {
    plan.from - query.lookback as i64 * query.step.num_milliseconds()
}

pub fn compile(tenant: &TenantId, plan: &Plan, query: &QueryPlan) -> Sql {
    let table = table_for(query.step);
    let mut p = SqlParams::new();

    // The rollup's groups are read from the union's `tags`, which has no lifted columns.
    let columns: &[&str] = match table {
        Table::Points => &LIFTED_TAGS,
        Table::Rollup => &[],
    };

    let groups: Vec<(String, String)> =
        query.group_by.iter().enumerate().map(|(i, k)| (format!("g{i}"), group_sql(columns, &mut p, k))).collect();
    let group_select: String = groups.iter().map(|(alias, expr)| format!("{expr} AS {alias}, ")).collect();
    let group_list: String = groups.iter().map(|(alias, _)| format!("{alias}, ")).collect();

    let tenant_p = p.add_as("tenant", SqlValue::String(tenant.0.clone()));
    let metric_p = p.add_as("metric", SqlValue::String(query.metric.clone()));
    let from_p = p.add_as("from", SqlValue::Int64(plan.from - earliest_offset_ms(query)));
    let to_p = p.add_as("to", SqlValue::Int64(plan.to));
    let step_p = p.add_as("step", SqlValue::Int64(query.step.num_seconds()));
    let from = format!("fromUnixTimestamp64Milli({from_p})");
    let until = format!("fromUnixTimestamp64Milli({to_p})");

    let raw_filter = filter::sql(&LIFTED_TAGS, &mut p, &query.filter);

    let (source, time, time_agg) = match table {
        Table::Points => {
            let raw = format!(
                "metrics
    WHERE tenant_id = {tenant_p}
      AND metric = {metric_p}
      AND {}
      AND ({raw_filter})",
                raw_window(&from, &until)
            );
            (raw, "timestamp", time_aggregate_sql(&step_p, query.time_agg))
        }
        // A rollup row is a whole minute, so it holds points after an unaligned `to`: the
        // minutes before the one holding `to` come from the rollup, and that minute's points
        // before `to` from the raw table, in the same columns. Past the raw retention that last
        // part minute is gone. The bucket holding an unaligned `from` is partial either way, and
        // the series drop it (`keep_from_ms`). The rollup has no lifted columns, so its own rows
        // are filtered on the map.
        Table::Rollup => {
            let map_filter = filter::sql(&[], &mut p, &query.filter);
            let rollup = format!(
                "(
        SELECT series_id, host, tags, minute, metric_type, interval,
               points, value_min, value_max, value_sum, value_events
        FROM metrics_1m
        WHERE tenant_id = {tenant_p}
          AND metric = {metric_p}
          AND minute >= {from}
          AND minute < toStartOfMinute({until})
          AND ({map_filter})
        UNION ALL
        SELECT series_id, host, any(tags), toStartOfMinute(timestamp) AS tail_minute, any(metric_type), max(interval),
               count(), min(value), max(value), sum(value), sum(value * greatest(interval, 1))
        FROM metrics
        WHERE tenant_id = {tenant_p}
          AND metric = {metric_p}
          AND {}
          AND ({raw_filter})
        GROUP BY series_id, host, tail_minute
    )",
                raw_window(&format!("greatest({from}, toStartOfMinute({until}))"), &until)
            );
            (rollup, "minute", rollup_aggregate_sql(&step_p, query.time_agg))
        }
    };

    // Filling happens per series, before series are combined: so with a fill ClickHouse stops
    // at the inner query, and `series::combine` does the rest.
    let text = if query.fill != Fill::NoFill {
        format!(
            "SELECT {group_list}toInt64(toUnixTimestamp(bucket)) * 1000 AS bucket_ms, v AS value, series_id
FROM (
    SELECT series_id, {group_select}toStartOfInterval({time}, toIntervalSecond({step_p})) AS bucket, {time_agg} AS v
    FROM {source}
    GROUP BY series_id, {group_list}bucket
)
ORDER BY {group_list}series_id, bucket"
        )
    } else {
        format!(
            "SELECT {group_list}toInt64(toUnixTimestamp(bucket)) * 1000 AS bucket_ms, {}(v) AS value
FROM (
    SELECT series_id, {group_select}toStartOfInterval({time}, toIntervalSecond({step_p})) AS bucket, {time_agg} AS v
    FROM {source}
    GROUP BY series_id, {group_list}bucket
)
GROUP BY {group_list}bucket
ORDER BY {group_list}bucket",
            aggregate(query.space_agg)
        )
    };

    Sql { text, parameters: p.into_parameters() }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::metric_query::ast::TagFilter;
    use crate::query::plan::{DEFAULT_RETENTION, FillMethod};

    fn plan() -> Plan {
        Plan {
            from: 1_711_977_600_000,
            to: 1_711_981_200_000,
            step: TimeDelta::seconds(20),
            queries: vec![],
            outputs: vec![],
            retention: DEFAULT_RETENTION,
        }
    }

    fn query(filter: TagFilter, group_by: &[&str]) -> QueryPlan {
        QueryPlan {
            metric: "system.cpu.user".into(),
            filter,
            group_by: group_by.iter().map(|k| k.to_string()).collect(),
            space_agg: Aggregation::Sum,
            time_agg: TimeAggregation::Plain(Aggregation::Avg),
            step: TimeDelta::seconds(20),
            lookback: 0,
            fill: Fill::NoFill,
        }
    }

    fn acme() -> TenantId {
        TenantId("acme".into())
    }

    fn compiled(filter: TagFilter, group_by: &[&str]) -> Sql {
        compile(&acme(), &plan(), &query(filter, group_by))
    }

    fn tag(k: &str, v: &str) -> TagFilter {
        TagFilter::Tag(k.into(), v.into())
    }

    /// The value bound to the placeholder that follows `before` in the SQL text.
    fn bound_after(before: &str, sql: &Sql) -> SqlValue {
        let i = sql.text.find(before).unwrap_or_else(|| panic!("'{before}' not in:\n{}", sql.text));
        let start = i + before.len() + 1;
        let name = &sql.text[start..start + sql.text[start..].find(':').unwrap()];
        sql.parameters.iter().find(|(n, _)| n == name).unwrap().1.clone()
    }

    fn string(s: &str) -> SqlValue {
        SqlValue::String(s.into())
    }

    #[test]
    fn time_first_then_space() {
        let sql = compiled(TagFilter::All, &[]);
        // Inner query: per series, the time aggregator; outer: across series, the space one.
        assert!(sql.text.contains("SELECT series_id, toStartOfInterval(timestamp, toIntervalSecond({step"));
        assert!(sql.text.contains("avg(value) AS v"));
        assert!(sql.text.contains("sum(v) AS value"));
        assert!(sql.text.contains("GROUP BY series_id, bucket"));
    }

    #[test]
    fn request_values_are_bound_never_inlined() {
        let sql = compiled(tag("env", "prod'; DROP TABLE metrics; --"), &[]);
        assert!(!sql.text.contains("DROP"));
        assert!(!sql.text.contains("acme"));
        assert!(!sql.text.contains("system.cpu.user"));
        assert_eq!(bound_after("tenant_id = ", &sql), string("acme"));
        assert_eq!(bound_after("metric = ", &sql), string("system.cpu.user"));
        assert_eq!(bound_after("fromUnixTimestamp64Milli(", &sql), SqlValue::Int64(1_711_977_600_000));
        assert_eq!(bound_after("toIntervalSecond(", &sql), SqlValue::Int64(20));
    }

    #[test]
    fn host_and_the_lifted_tags_are_columns_on_the_raw_points_other_tags_are_in_the_map() {
        assert!(compiled(tag("host", "web-1"), &[]).text.contains("host = {p"));
        let env = compiled(tag("env", "prod"), &[]);
        assert!(env.text.contains("has(env, {p"));
        assert_eq!(bound_after("has(env, ", &env), string("prod"));
        assert!(!env.text.contains("tags["));
        let role = compiled(tag("role", "api"), &[]);
        assert!(role.text.contains("has(tags[{k"));
        assert_eq!(bound_after("has(tags[", &role), string("role"));
    }

    #[test]
    fn wildcards_become_like_with_likes_own_wildcards_escaped() {
        let sql = compiled(tag("kube_namespace", "prod_*"), &[]);
        assert!(sql.text.contains("arrayExists(x -> x LIKE {p"));
        assert_eq!(bound_after("x LIKE ", &sql), string("prod\\_%"));
    }

    #[test]
    fn a_key_less_tag_asks_for_the_key() {
        assert!(compiled(TagFilter::Bare("canary".into()), &[]).text.contains("mapContains(tags, {k"));
        // Also for a lifted one: `{env}` asks whether the key exists, not whether it has an empty
        // value.
        assert!(compiled(TagFilter::Bare("env".into()), &[]).text.contains("mapContains(tags, {k"));
    }

    #[test]
    fn boolean_structure_is_kept() {
        let filter = TagFilter::And(vec![
            tag("env", "prod"),
            TagFilter::Not(Box::new(TagFilter::Or(vec![
                tag("host", "a"),
                TagFilter::In("zone".into(), vec!["x".into(), "y".into()]),
            ]))),
        ]);
        let sql = compiled(filter, &[]);
        assert!(sql.text.contains(") AND (NOT ((host = {p"), "{}", sql.text);
        assert!(sql.text.contains(") OR (hasAny(tags[{k"), "{}", sql.text);
    }

    #[test]
    fn group_by_host_as_a_column_tags_through_array_join_missing_as_null() {
        let sql = compiled(TagFilter::All, &["host", "role"]);
        assert!(sql.text.contains("nullIf(toString(host), '') AS g0"));
        assert!(sql.text.contains("arrayJoin(if(empty(tags[{k"));
        assert!(sql.text.contains("[NULL], CAST(tags[{k"));
        assert!(sql.text.contains("GROUP BY g0, g1, bucket"));
        assert!(sql.text.contains("GROUP BY series_id, g0, g1, bucket"));
    }

    #[test]
    fn group_by_a_lifted_tag_reads_its_column_on_the_raw_points_the_map_on_the_rollup() {
        let raw = compiled(TagFilter::All, &["kube_namespace"]);
        assert!(raw.text.contains(
            "arrayJoin(if(empty(kube_namespace), [NULL], CAST(kube_namespace, 'Array(Nullable(String))'))) AS g0"
        ));
        let rollup = compile(
            &acme(),
            &plan(),
            &QueryPlan { step: TimeDelta::minutes(1), ..query(TagFilter::All, &["kube_namespace"]) },
        );
        assert!(rollup.text.contains("arrayJoin(if(empty(tags[{k"));
        assert_eq!(bound_after("empty(tags[", &rollup), string("kube_namespace"));
    }

    // --- time aggregation ----------------------------------------------------------------

    fn with_time(agg: TimeAggregation, step_seconds: i64) -> Sql {
        compile(
            &acme(),
            &plan(),
            &QueryPlan { time_agg: agg, step: TimeDelta::seconds(step_seconds), ..query(TagFilter::All, &[]) },
        )
    }

    #[test]
    fn the_querys_own_step_is_bound() {
        assert_eq!(
            bound_after("toIntervalSecond(", &with_time(TimeAggregation::Plain(Aggregation::Sum), 60)),
            SqlValue::Int64(60)
        );
    }

    #[test]
    fn rollup_methods() {
        assert!(with_time(TimeAggregation::Plain(Aggregation::Sum), 20).text.contains("sum(value) AS v"));
        assert!(with_time(TimeAggregation::Plain(Aggregation::Count), 20).text.contains("toFloat64(count()) AS v"));
    }

    #[test]
    fn as_count_follows_the_metric_type() {
        let sql = with_time(TimeAggregation::AsCount, 20).text;
        assert!(sql.contains("any(metric_type) = 'RATE', sum(value * greatest(interval, 1))"));
        assert!(sql.contains("any(metric_type) = 'COUNT', sum(value)"));
        assert!(sql.contains(", avg(value)) AS v")); // GAUGE: unaffected
    }

    #[test]
    fn as_rate_divides_by_the_bucket_floored_to_a_rates_interval() {
        let sql = with_time(TimeAggregation::AsRate, 20);
        assert!(sql.text.contains("/ greatest({step"));
        assert!(sql.text.contains("}, max(interval))"));
        assert!(sql.text.contains("any(metric_type) = 'COUNT', sum(value) / {step"));
    }

    #[test]
    fn wildcards_inside_in_are_patterns_the_rest_stay_exact() {
        let sql = compiled(TagFilter::In("container_name".into(), vec!["ninjacat*".into(), "postgres".into()]), &[]);
        assert!(sql.text.contains("arrayExists(x -> x LIKE {p"));
        assert!(sql.text.contains(") OR (hasAny(tags[{k"));
        assert_eq!(bound_after("x LIKE ", &sql), string("ninjacat%"));
        assert_eq!(bound_after("], ", &sql), SqlValue::StringArray(vec!["postgres".into()]));
    }

    #[test]
    fn host_in_with_a_wildcard() {
        let sql = compiled(TagFilter::Not(Box::new(TagFilter::In("host".into(), vec!["web-*".into()]))), &[]);
        assert!(sql.text.contains("NOT ((host LIKE {p"));
    }

    #[test]
    fn lookback_reads_earlier_one_bucket_more() {
        let q = QueryPlan { lookback: 3, ..query(TagFilter::All, &[]) };
        let sql = compile(&acme(), &plan(), &q);
        // from − (3 + 1) × 20 s
        assert_eq!(bound_after("fromUnixTimestamp64Milli(", &sql), SqlValue::Int64(1_711_977_600_000 - 80_000));
        assert_eq!(keep_from_ms(&plan(), &q), 1_711_977_600_000 - 60_000);
    }

    #[test]
    fn with_a_fill_series_come_back_one_by_one() {
        let q = QueryPlan {
            fill: Fill::Within { method: FillMethod::Linear, limit_seconds: 300 },
            ..query(TagFilter::All, &["host"])
        };
        let sql = compile(&acme(), &plan(), &q);
        assert!(sql.text.contains("v AS value, series_id"));
        assert!(sql.text.contains("ORDER BY g0, series_id, bucket"));
        assert!(!sql.text.contains("sum(v)"));
    }

    // --- the rollup ----------------------------------------------------------------------

    #[test]
    fn a_step_of_whole_minutes_reads_the_rollup_a_finer_one_the_raw_points() {
        assert!(with_time(TimeAggregation::Plain(Aggregation::Avg), 20).text.contains("FROM metrics\n"));
        let rollup = with_time(TimeAggregation::Plain(Aggregation::Avg), 60).text;
        assert!(rollup.contains("FROM metrics_1m\n"));
        assert!(rollup.contains("sum(value_sum) / sum(points) AS v"));
        assert!(!rollup.contains("Merge("));
    }

    #[test]
    fn every_read_of_the_raw_points_states_the_15_minute_bucket() {
        let raw = compiled(TagFilter::All, &[]).text;
        assert_eq!(raw.matches("toStartOfFifteenMinutes(timestamp) >= ").count(), 1);
        assert!(
            raw.contains(
                "toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(fromUnixTimestamp64Milli({from"
            )
        );
        assert!(raw.contains("toStartOfFifteenMinutes(timestamp) < fromUnixTimestamp64Milli({to"));
        // The rollup's own part has no such column; its tail from the raw points does.
        let rollup = with_time(TimeAggregation::Plain(Aggregation::Avg), 60).text;
        assert_eq!(rollup.matches("toStartOfFifteenMinutes(timestamp) >= ").count(), 1);
        assert!(rollup.contains("toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(greatest("));
    }

    #[test]
    fn the_rollup_filters_by_the_map_its_tail_from_the_raw_points_by_the_lifted_column() {
        let sql =
            compile(&acme(), &plan(), &QueryPlan { step: TimeDelta::minutes(1), ..query(tag("env", "prod"), &[]) });
        let union = sql.text.find("UNION ALL").unwrap();
        let (rollup_part, tail) = sql.text.split_at(union);
        assert!(rollup_part.contains("has(tags[{k"));
        assert!(!rollup_part.contains("has(env, "));
        assert!(tail.contains("has(env, {p"));
        assert!(!tail[tail.find("WHERE").unwrap()..].contains("tags["));
    }
}
