//! A service check monitor,
//! `"postgres.can_connect".over("env:prod").by("host").last(3).count_by_status()`: read, and
//! evaluated from `check_runs`. (F#: `Monitor/Check.fs`.)

use chrono::TimeDelta;
use winnow::combinator::{alt, delimited, preceded, repeat, separated, terminated};
use winnow::error::{StrContext, StrContextValue};
use winnow::token::take_while;
use winnow::{ModalResult, Parser};

use crate::metric_query::ast::TagFilter;
use crate::metric_query::parser::{quoted, ws};
use crate::metric_query::{filter, parser};
use crate::monitor::definition::{CheckDefinition, CheckQuery};
use crate::monitor::evaluate::{Evaluation, Period, Streak};
use crate::monitor::options;
use crate::query::compile::Row;
use crate::query::execute::Executor;
use crate::sql::{Sql, SqlParams, SqlValue, TenantId};

enum CallArg {
    Item(String),
    Count(f64),
}

struct Call {
    name: String,
    args: Vec<CallArg>,
}

// --- grammar ---------------------------------------------------------------------------------

fn word(input: &mut &str) -> ModalResult<String> {
    take_while(1.., |c: char| c.is_alphabetic() || c.is_ascii_digit() || c == '_')
        .map(str::to_string)
        .context(StrContext::Expected(StrContextValue::Description("a name")))
        .parse_next(input)
}

fn call_arg(input: &mut &str) -> ModalResult<CallArg> {
    alt((quoted.map(CallArg::Item), parser::number.map(CallArg::Count))).parse_next(input)
}

fn call(input: &mut &str) -> ModalResult<Call> {
    let args = delimited(('(', ws), separated(0.., terminated(call_arg, ws), (',', ws)), ')');
    terminated(preceded(('.', ws), (terminated(word, ws), args)), ws)
        .map(|(name, args)| Call { name, args })
        .parse_next(input)
}

fn check_query(input: &mut &str) -> ModalResult<(String, Vec<Call>)> {
    (preceded(ws, terminated(quoted, ws)), repeat(0.., call)).parse_next(input)
}

fn read(text: &str) -> Result<(String, Vec<Call>), String> {
    parser::run(text, check_query)
}

// --- the query -------------------------------------------------------------------------------

fn problems_of<T>(result: &Result<T, Vec<String>>) -> Vec<String> {
    match result {
        Err(problems) => problems.clone(),
        Ok(_) => vec![],
    }
}

fn collect<T>(results: Vec<Result<T, String>>) -> Result<Vec<T>, Vec<String>> {
    let problems: Vec<String> = results.iter().filter_map(|r| r.as_ref().err().cloned()).collect();
    if problems.is_empty() { Ok(results.into_iter().flatten().collect()) } else { Err(problems) }
}

fn texts(c: &Call) -> Result<Vec<String>, Vec<String>> {
    let items: Vec<String> = c
        .args
        .iter()
        .filter_map(|a| match a {
            CallArg::Item(t) => Some(t.trim().to_string()),
            CallArg::Count(_) => None,
        })
        .collect();

    if items.len() != c.args.len() {
        Err(vec![format!("{}() takes quoted strings", c.name)])
    } else if items.is_empty() || items.iter().any(String::is_empty) {
        Err(vec![format!("{}() cannot be blank", c.name)])
    } else {
        Ok(items)
    }
}

fn tag_filter(call: &str, item: &str) -> Result<TagFilter, String> {
    parser::parse_tag_filter(&format!("{{{item}}}")).map_err(|e| format!("{call}(\"{item}\"): {e}"))
}

const KNOWN: [&str; 5] = ["over", "exclude", "by", "last", "count_by_status"];

fn shape_problems(name: &str, calls: &[Call]) -> Vec<String> {
    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    let mut problems = vec![];

    if name.trim().is_empty() {
        problems.push("the check name is empty".to_string());
    }
    if names.contains(&"pct_by_status") {
        problems.push("cluster alerts (pct_by_status) are not supported yet".to_string());
    } else {
        match calls.last() {
            Some(last) if last.name == "count_by_status" && last.args.is_empty() => {}
            _ => problems.push("a check monitor's query ends in .count_by_status()".to_string()),
        }
    }
    let mut distinct: Vec<&str> = vec![];
    for n in &names {
        if !distinct.contains(n) {
            distinct.push(n);
        }
    }
    for n in distinct {
        if !KNOWN.contains(&n) && n != "pct_by_status" {
            problems.push(format!(".{n}() is not part of a check query, which takes over, exclude, by, last and count_by_status"));
        } else if names.iter().filter(|other| **other == n).count() > 1 {
            problems.push(format!(".{n}() is given more than once"));
        }
    }
    problems
}

/// WARNING(undocumented): what over() takes. Each item is a tag filter, a comma is AND, and a `*`
/// among tags is dropped; exclude() items are ANDed on as NOT.
fn over_of(call: Option<&Call>) -> Result<Vec<TagFilter>, Vec<String>> {
    match call.map(texts) {
        None => Ok(vec![]),
        Some(Ok(items)) => collect(items.iter().filter(|i| *i != "*").map(|i| tag_filter("over", i)).collect()),
        Some(Err(e)) => Err(e),
    }
}

fn exclude_of(call: Option<&Call>) -> Result<Vec<TagFilter>, Vec<String>> {
    match call.map(texts) {
        None => Ok(vec![]),
        Some(Ok(items)) if items.iter().any(|i| i == "*") => Err(vec!["exclude() cannot take \"*\"".to_string()]),
        Some(Ok(items)) => {
            collect(items.iter().map(|i| tag_filter("exclude", i).map(|f| TagFilter::Not(Box::new(f)))).collect())
        }
        Some(Err(e)) => Err(e),
    }
}

/// WARNING(undocumented): by("*") is one group per series, as Datadog's own monitors name
/// {{host.name}} under it; no by() is one group. A `key:value` is refused rather than guessed at.
fn by_of(call: Option<&Call>) -> Result<Vec<String>, Vec<String>> {
    match call.map(texts) {
        None => Ok(vec![]),
        Some(Ok(items)) if items == ["*"] => Ok(items),
        Some(Ok(items)) if items.iter().any(|i| i == "*") => Err(vec!["by() takes \"*\" alone or tag keys".to_string()]),
        Some(Ok(items)) => {
            let bad: Vec<String> =
                items.iter().filter(|i| i.contains(':')).map(|i| format!("by() takes tag keys; put {i} in over()")).collect();
            if !bad.is_empty() {
                return Err(bad);
            }
            let mut keys = items;
            keys.sort();
            keys.dedup();
            Ok(keys)
        }
        Some(Err(e)) => Err(e),
    }
}

fn last_of(call: Option<&Call>) -> Result<i32, Vec<String>> {
    match call {
        Some(Call { args, .. }) => match args.as_slice() {
            [CallArg::Count(n)] if *n == n.floor() && *n >= 1.0 && *n <= 100.0 => Ok(*n as i32),
            _ => Err(vec!["last() takes a whole number of runs from 1 to 100".to_string()]),
        },
        None => Err(vec!["last(N) is required: the number of runs the thresholds count in".to_string()]),
    }
}

pub fn query(text: &str) -> Result<CheckQuery, Vec<String>> {
    let (name, calls) = read(text).map_err(|e| vec![e])?;
    let find = |n: &str| calls.iter().find(|c| c.name == n);

    let shape = shape_problems(&name, &calls);
    let over = over_of(find("over"));
    let exclude = exclude_of(find("exclude"));
    let by = by_of(find("by"));
    let last = last_of(find("last"));

    match (over, exclude, by, last) {
        (Ok(over), Ok(exclude), Ok(group_by), Ok(last)) if shape.is_empty() => Ok(CheckQuery {
            check: name,
            filter: parser::and_of(over.into_iter().chain(exclude).collect()),
            group_by,
            last,
        }),
        (over, exclude, by, last) => {
            let mut problems = shape;
            problems.extend(problems_of(&over));
            problems.extend(problems_of(&exclude));
            problems.extend(problems_of(&by));
            problems.extend(problems_of(&last));
            Err(problems)
        }
    }
}

/// No Data must fire before a silent group is forgotten (the monitor service's `State.forgetAfter`).
pub const LONGEST_NO_DATA: TimeDelta = TimeDelta::hours(48);

/// A count of consecutive runs, as a threshold of a check monitor is.
fn count_problems(name: &str, value: Option<f64>) -> Vec<String> {
    match value {
        Some(v) if v != v.floor() || !(1.0..=100.0).contains(&v) => {
            vec![format!("options.thresholds.{name} is a count of consecutive runs, a whole number from 1 to 100")]
        }
        _ => vec![],
    }
}

pub fn definition(text: &str, options: &str) -> Result<CheckDefinition, Vec<String>> {
    let (q, o) = match (query(text), options::parse(options)) {
        (Ok(q), Ok(o)) => (q, o),
        (q, o) => {
            let mut problems = problems_of(&q);
            problems.extend(problems_of(&o));
            return Err(problems);
        }
    };
    let t = o.thresholds;

    // WARNING(undocumented): a missing threshold is 1 run.
    let critical = t.critical.unwrap_or(1.0);
    let warning = t.warning.unwrap_or(1.0);
    let ok = t.ok.unwrap_or(1.0);
    let largest = [critical, warning, ok, t.unknown.unwrap_or(1.0)].into_iter().fold(f64::NEG_INFINITY, f64::max);

    let mut problems = count_problems("critical", t.critical);
    problems.extend(count_problems("warning", t.warning));
    problems.extend(count_problems("ok", t.ok));
    problems.extend(count_problems("unknown", t.unknown));
    if f64::from(q.last) < largest {
        problems.push(format!(
            "last({}) must be at least the largest threshold ({largest}), as the thresholds count among the last runs",
            q.last
        ));
    }
    if t.critical_recovery.is_some() || t.warning_recovery.is_some() {
        problems.push(
            "recovery thresholds are not available for check monitors; options.thresholds.ok counts the runs to resolve".to_string(),
        );
    }
    if o.trigger_window.is_some() || o.recovery_window.is_some() {
        problems.push("options.threshold_windows is for anomaly monitors".to_string());
    }
    if o.on_missing_data.is_some() {
        problems.push("options.on_missing_data is not available for check monitors; use notify_no_data and no_data_timeframe".to_string());
    }
    if let Some(timeframe) = o.no_data_timeframe
        && timeframe > LONGEST_NO_DATA
    {
        problems.push(format!(
            "options.no_data_timeframe ({} minutes) must be at most 2880, as a silent check group is forgotten after 48 hours",
            timeframe.num_minutes()
        ));
    }

    if !problems.is_empty() {
        return Err(problems);
    }

    let mut not_acted_on = o.not_acted_on.clone();
    if o.require_full_window {
        not_acted_on.push("require_full_window".to_string());
    }
    let mut options = o;
    options.thresholds.critical = Some(critical);
    options.thresholds.warning = Some(warning);
    options.thresholds.ok = Some(ok);
    options.not_acted_on = not_acted_on;
    Ok(CheckDefinition { query: q, options })
}

// --- reading the runs ------------------------------------------------------------------------

fn condition(bound: &mut SqlParams, tenant: &TenantId, q: &CheckQuery, from: i64, until: i64) -> String {
    let tenant = bound.add_as("tenant", SqlValue::String(tenant.0.clone()));
    let check = bound.add_as("check", SqlValue::String(q.check.clone()));
    let from = bound.add_as("from", SqlValue::Int64(from));
    let to = bound.add_as("to", SqlValue::Int64(until));
    let tags = filter::sql(&[], bound, &q.filter);
    format!(
        "tenant_id = {tenant} AND check_name = {check}
      AND timestamp >= fromUnixTimestamp64Milli({from})
      AND timestamp < fromUnixTimestamp64Milli({to})
      AND ({tags})"
    )
}

/// A run's tags as `k:v`, or `k` for a tag without a value.
const TAG_PAIRS: &str = "arrayFlatten(arrayMap((k, vs) -> arrayMap(v -> if(v = '', toString(k), concat(toString(k), ':', toString(v))), vs), mapKeys(tags), mapValues(tags)))";

/// The newest runs of the check in `[from, until)` (unix milliseconds), newest first: time, host,
/// status, message and tags as `k:v`.
pub fn runs(tenant: &TenantId, q: &CheckQuery, from: i64, until: i64) -> Sql {
    let mut bound = SqlParams::new();
    let condition = condition(&mut bound, tenant, q, from, until);
    Sql {
        text: format!(
            "SELECT toInt64(toUnixTimestamp(timestamp)) * 1000, host, toString(status), message, {TAG_PAIRS}
FROM check_runs
WHERE {condition}
ORDER BY timestamp DESC
LIMIT 100"
        ),
        parameters: bound.into_parameters(),
    }
}

/// WARNING(undocumented): evaluation_delay moves the read back, with no minimum: a run is stored
/// or not, and one stored late still counts at the next tick.
pub fn period(c: &CheckDefinition, at: i64) -> Period {
    let _ = (c, at);
    todo!()
}

/// The newest runs of each series, a host and tag set, in `Row`'s columns: `value` is the run's
/// status, `bucket_ms` its time. N of its known runs and N of its UNKNOWN ones, two at least, as
/// the live rule needs the gap between its newest two runs. A run stored twice is one run.
pub fn sql(tenant: &TenantId, q: &CheckQuery, p: &Period) -> Sql {
    let _ = (tenant, q, p);
    todo!()
}

/// The rows of `sql` as an evaluation: one group per combination of the `by` values, or per series
/// for by("*"), each with its live series. A group with none is left out, so it goes silent.
pub fn evaluation(q: &CheckQuery, p: &Period, max_groups: usize, rows: &[Row]) -> Result<Evaluation, String> {
    let _ = (q, p, max_groups, rows);
    todo!()
}

pub async fn evaluate<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    max_groups: usize,
    c: &CheckDefinition,
    at: i64,
) -> Result<Evaluation, String> {
    let _ = (execute, tenant, max_groups, c, at);
    todo!()
}

/// The worst newest status, as the group's value: CRITICAL, then WARNING, UNKNOWN and OK.
pub fn worst(streaks: &[Streak]) -> f64 {
    let _ = streaks;
    todo!()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::monitor::definition::Monitor;
    use crate::monitor::validate;
    use crate::query::plan::DEFAULT_RETENTION;

    fn or_fail<T>(result: Result<T, Vec<String>>) -> T {
        match result {
            Ok(v) => v,
            Err(problems) => panic!("{}", problems.join("\n")),
        }
    }

    fn parsed(text: &str) -> CheckQuery {
        or_fail(query(text))
    }

    fn mentions(fragment: &str, problems: &[String]) {
        assert!(problems.iter().any(|p| p.contains(fragment)), "no problem mentions '{fragment}':\n{}", problems.join("\n"));
    }

    fn problems<T: std::fmt::Debug>(result: Result<T, Vec<String>>) -> Vec<String> {
        match result {
            Ok(v) => panic!("expected problems, got {v:?}"),
            Err(problems) => problems,
        }
    }

    fn check(name: &str, filter: TagFilter, group_by: &[&str], last: i32) -> CheckQuery {
        CheckQuery { check: name.to_string(), filter, group_by: group_by.iter().map(|g| g.to_string()).collect(), last }
    }

    fn tag(k: &str, v: &str) -> TagFilter {
        TagFilter::Tag(k.to_string(), v.to_string())
    }

    fn not(f: TagFilter) -> TagFilter {
        TagFilter::Not(Box::new(f))
    }

    // --- the query ---------------------------------------------------------------------------

    #[test]
    fn queries_as_integrations_core_and_the_datadog_operator_write_them() {
        assert_eq!(parsed(r#""consul.check".over("*").by("host").last(3).count_by_status()"#), check("consul.check", TagFilter::All, &["host"], 3));
        assert_eq!(
            parsed(r#""helm.release_state".over("*").by("helm_namespace","helm_release","helm_storage","kube_cluster_name").last(6).count_by_status()"#),
            check("helm.release_state", TagFilter::All, &["helm_namespace", "helm_release", "helm_storage", "kube_cluster_name"], 6)
        );
        assert_eq!(
            parsed(r#""mysql.replication.replica_running".over("*").by("*").last(2).count_by_status()"#),
            check("mysql.replication.replica_running", TagFilter::All, &["*"], 2)
        );
        assert_eq!(
            parsed(r#""kubernetes.kubelet.check".over("*").by("host").last(2).count_by_status()"#),
            check("kubernetes.kubelet.check", TagFilter::All, &["host"], 2)
        );
    }

    #[test]
    fn calls_come_in_any_order_over_and_by_may_be_left_out() {
        assert_eq!(
            parsed(r#""c".over("env:prod", "role:db").last(3).by("host").count_by_status()"#),
            check("c", TagFilter::And(vec![tag("env", "prod"), tag("role", "db")]), &["host"], 3)
        );
        assert_eq!(parsed(r#""custom.check".by("host").last(2).count_by_status()"#), check("custom.check", TagFilter::All, &["host"], 2));
        assert_eq!(parsed(r#""ntp.in_sync".over("*").last(2).count_by_status()"#), check("ntp.in_sync", TagFilter::All, &[], 2));
    }

    #[test]
    fn exclude_is_anded_on_as_not() {
        let q = parsed(r#""datadog.agent.up".over("*").exclude("group:pgsynclog","environment:enikshay").by("host").last(2).count_by_status()"#);
        assert_eq!(q.filter, TagFilter::And(vec![not(tag("group", "pgsynclog")), not(tag("environment", "enikshay"))]));

        let scoped = parsed(r#""x".over("env:prod").exclude("role:db").last(1).count_by_status()"#);
        assert_eq!(scoped.filter, TagFilter::And(vec![tag("env", "prod"), not(tag("role", "db"))]));
    }

    #[test]
    fn over_takes_what_a_tag_filter_takes() {
        assert_eq!(parsed(r#""x".over("database").last(1).count_by_status()"#).filter, TagFilter::Bare("database".to_string()));

        let q = parsed(r#"  'Mixed.Case'  .over( 'role:web-*' , '!env:dev' ) .by( 'host' ) .last( 2 ) .count_by_status( )  "#);
        assert_eq!(q, check("Mixed.Case", TagFilter::And(vec![tag("role", "web-*"), not(tag("env", "dev"))]), &["host"], 2));

        assert_eq!(parsed(r#""x".over("*", "env:prod").last(1).count_by_status()"#).filter, tag("env", "prod"));
    }

    #[test]
    fn queries_that_are_refused_say_why() {
        for (text, fragment) in [
            (r#""x".over("*").by("environment:foo").last(2).count_by_status()"#, "by() takes tag keys; put environment:foo in over()"),
            (r#""x".over("*").by("host").last(2).pct_by_status()"#, "cluster alerts (pct_by_status) are not supported yet"),
            (r#""x".over("*").by("host").count_by_status()"#, "last(N) is required"),
            (r#""x".last(0).count_by_status()"#, "whole number of runs from 1 to 100"),
            (r#""x".last(101).count_by_status()"#, "whole number of runs from 1 to 100"),
            (r#""x".last(2.5).count_by_status()"#, "whole number of runs from 1 to 100"),
            (r#""x".by("host").by("role").last(2).count_by_status()"#, ".by() is given more than once"),
            (r#""x".foo().last(2).count_by_status()"#, ".foo() is not part of a check query"),
            (r#""x".last(2)"#, "ends in .count_by_status()"),
            (r#""".last(2).count_by_status()"#, "the check name is empty"),
            (r#""x".over().last(2).count_by_status()"#, "over() cannot be blank"),
            (r#""x".over(" ").last(2).count_by_status()"#, "over() cannot be blank"),
            (r#""x".exclude("*").last(2).count_by_status()"#, "exclude() cannot take \"*\""),
            (r#""x".by("*","host").last(2).count_by_status()"#, "by() takes \"*\" alone"),
        ] {
            mentions(fragment, &problems(query(text)));
        }
    }

    /// FParsec said "Expecting: end of input or '.'"; the metric parser's run names the column and
    /// what was expected in its own words.
    #[test]
    fn text_after_the_query_is_a_syntax_error() {
        let problems = problems(query(r#""x".last(2).count_by_status() > 1"#));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].to_lowercase().contains("expect"), "{}", problems[0]);
    }

    // --- the definition ----------------------------------------------------------------------

    const CONSUL: &str = r#""consul.check".over("*").by("host").last(3).count_by_status()"#;

    fn defined(query: &str, options: &str) -> CheckDefinition {
        or_fail(definition(query, options))
    }

    #[test]
    fn a_missing_threshold_is_one_run() {
        let t = defined(CONSUL, "").options.thresholds;
        assert_eq!((t.critical, t.warning, t.ok), (Some(1.0), Some(1.0), Some(1.0)));
    }

    #[test]
    fn thresholds_as_integrations_core_sets_them() {
        let t = defined(CONSUL, r#"{"thresholds":{"critical":2,"warning":2,"ok":1}}"#).options.thresholds;
        assert_eq!((t.critical, t.warning, t.ok), (Some(2.0), Some(2.0), Some(1.0)));

        // foundationdb: a warning above its critical.
        defined(r#""foundationdb.check".over("*").by("host").last(6).count_by_status()"#, r#"{"thresholds":{"critical":2,"warning":5,"ok":1}}"#);

        let helm = r#""helm.release_state".over("*").by("helm_release").last(6).count_by_status()"#;
        defined(helm, r#"{"thresholds":{"critical":5,"warning":1,"ok":1}}"#);
    }

    #[test]
    fn definitions_that_are_refused_say_why() {
        let two = r#""x".last(2).count_by_status()"#;
        for (options, fragment) in [
            (r#"{"thresholds":{"critical":3}}"#, "last(2) must be at least the largest threshold (3)"),
            (r#"{"thresholds":{"critical":"0.5"}}"#, "options.thresholds.critical must be a number"),
            (r#"{"thresholds":{"warning":1.5}}"#, "options.thresholds.warning is a count of consecutive runs"),
            (r#"{"thresholds":{"ok":0}}"#, "options.thresholds.ok is a count of consecutive runs"),
            (r#"{"thresholds":{"unknown":0}}"#, "options.thresholds.unknown is a count of consecutive runs"),
            (r#"{"thresholds":{"unknown":1.5}}"#, "options.thresholds.unknown is a count of consecutive runs"),
            (r#"{"thresholds":{"unknown":3}}"#, "last(2) must be at least the largest threshold (3)"),
            (r#"{"thresholds":{"critical":1,"critical_recovery":1}}"#, "recovery thresholds are not available"),
            (r#"{"threshold_windows":{"trigger_window":"last_15m"}}"#, "options.threshold_windows"),
            (r#"{"on_missing_data":"resolve"}"#, "options.on_missing_data is not available"),
            (r#"{"notify_no_data":true,"no_data_timeframe":2881}"#, "must be at most 2880"),
        ] {
            mentions(fragment, &problems(definition(two, options)));
        }
    }

    #[test]
    fn unknown_is_kept_require_full_window_is_accepted_and_not_acted_on() {
        let c = defined(CONSUL, r#"{"thresholds":{"unknown":2},"require_full_window":true,"renotify_interval":60}"#);
        assert_eq!(c.options.not_acted_on, vec!["renotify_interval", "require_full_window"]);
        assert_eq!(c.options.thresholds.unknown, Some(2.0));
        assert_eq!(defined(CONSUL, "").options.thresholds.unknown, None);
        assert_eq!(
            defined(CONSUL, r#"{"notify_no_data":true,"no_data_timeframe":2880}"#).options.no_data_timeframe,
            Some(TimeDelta::minutes(2880))
        );
    }

    #[test]
    fn the_type_picks_the_kind_of_monitor() {
        let now = Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap().timestamp_millis();
        let by_type = |t: &str, q: &str, o: &str| validate::by_type(DEFAULT_RETENTION, now, t, q, o);
        let metric = "avg(last_5m):avg:cpu{*} > 90";

        assert!(matches!(by_type("query alert", metric, ""), Ok(Monitor::Metric(_))));
        assert!(matches!(by_type("metric alert", metric, ""), Ok(Monitor::Metric(_))));
        assert!(matches!(by_type("service check", CONSUL, ""), Ok(Monitor::ServiceCheck(_))));
        mentions("'log alert' is not supported", &problems(by_type("log alert", metric, "")));
        mentions("options.thresholds.ok is for service check monitors", &problems(by_type("query alert", metric, r#"{"thresholds":{"ok":1}}"#)));
        problems(by_type("service check", metric, ""));
    }

    // --- the runs ----------------------------------------------------------------------------

    #[test]
    fn the_runs_bind_every_value() {
        let q = parsed(r#""pg.can_connect".over("env:prod").by("host").last(3).count_by_status()"#);
        let sql = runs(&TenantId("acme".to_string()), &q, 1_000, 2_000);
        let names: Vec<&str> = sql.parameters.iter().take(4).map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["tenant0", "check1", "from2", "to3"]);
        let values: Vec<&SqlValue> = sql.parameters.iter().map(|(_, v)| v).collect();
        for v in [
            SqlValue::String("acme".into()),
            SqlValue::String("pg.can_connect".into()),
            SqlValue::Int64(1_000),
            SqlValue::Int64(2_000),
        ] {
            assert!(values.contains(&&v), "{v:?}");
        }
        assert!(!sql.text.contains("acme"));
        assert!(sql.text.starts_with(&format!(
            "SELECT toInt64(toUnixTimestamp(timestamp)) * 1000, host, toString(status), message, {TAG_PAIRS}\nFROM check_runs\nWHERE tenant_id = {{tenant0:String}} AND check_name = {{check1:String}}\n      AND timestamp >= fromUnixTimestamp64Milli({{from2:Int64}})\n      AND timestamp < fromUnixTimestamp64Milli({{to3:Int64}})\n      AND ("
        )));
        assert!(sql.text.ends_with(")\nORDER BY timestamp DESC\nLIMIT 100"));
    }
}
