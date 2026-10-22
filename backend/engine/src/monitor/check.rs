//! A service check monitor,
//! `"postgres.can_connect".over("env:prod").by("host").last(3).count_by_status()`: read, and
//! evaluated from `check_runs`.

use chrono::TimeDelta;
use winnow::combinator::{alt, delimited, preceded, repeat, separated, terminated};
use winnow::error::{StrContext, StrContextValue};
use winnow::token::take_while;
use winnow::{ModalResult, Parser};

use crate::metric_query::ast::TagFilter;
use crate::metric_query::parser::{quoted, ws};
use crate::metric_query::{filter, parser};
use crate::monitor::definition::{CheckDefinition, CheckQuery};
use crate::monitor::evaluate::{Evaluation, GroupResult, Period, Reading, Streak};
use crate::monitor::options;
use crate::query::compile::{Row, group_sql};
use crate::query::execute::Executor;
use crate::query::series::grouped;
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
    let (name, calls) = parser::run(text, check_query).map_err(|e| vec![e])?;
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

/// How far back a tick reads: 8 runs of a check every 15 minutes, last(100) of one every 72 s, or
/// the no-data timeframe when longer, so a check run less often has its runs read.
fn lookback(c: &CheckDefinition) -> TimeDelta {
    TimeDelta::hours(2).max(c.options.no_data_timeframe.unwrap_or(TimeDelta::zero()))
}

/// WARNING(undocumented): evaluation_delay moves the read back, with no minimum: a run is stored
/// or not, and one stored late still counts at the next tick.
pub fn period(c: &CheckDefinition, at: i64) -> Period {
    let until = at - c.options.evaluation_delay.num_milliseconds();
    Period { from: until - lookback(c).num_milliseconds(), to: until }
}

/// The newest runs of each series, a host and tag set, in `Row`'s columns: `value` is the run's
/// status, `bucket_ms` its time. N of its known runs and N of its UNKNOWN ones, two at least, as
/// the live rule needs the gap between its newest two runs. A run stored twice is one run.
pub fn sql(tenant: &TenantId, q: &CheckQuery, p: &Period) -> Sql {
    let mut bound = SqlParams::new();

    let groups = match q.group_by.as_slice() {
        [star] if star == "*" => format!(
            "arrayStringConcat(arraySort(arrayConcat(if(host = '', [], [concat('host:', toString(host))]), {TAG_PAIRS})), ',') AS g0, "
        ),
        keys => keys.iter().enumerate().map(|(i, k)| format!("{} AS g{i}, ", group_sql(&[], &mut bound, k))).collect(),
    };
    let condition = condition(&mut bound, tenant, q, p.from, p.to);

    Sql {
        text: format!(
            "SELECT {groups}toInt64(toUnixTimestamp(timestamp)) * 1000 AS bucket_ms, toFloat64(CAST(status, 'Int8')) AS value, series_id
FROM (
    SELECT DISTINCT timestamp, host, tags, status, cityHash64(host, tags) AS series_id
    FROM check_runs
    WHERE {condition}
    ORDER BY timestamp DESC
    LIMIT {} BY series_id, status = 'UNKNOWN'
)",
            q.last.max(2)
        ),
        parameters: bound.into_parameters(),
    }
}

// --- judging ---------------------------------------------------------------------------------

const STATUS_OK: i32 = 0;
const STATUS_WARNING: i32 = 1;
const STATUS_CRITICAL: i32 = 2;
const STATUS_UNKNOWN: i32 = 3;

fn leading(status: i32, among: &[i32]) -> i32 {
    among.iter().take_while(|s| **s == status).count() as i32
}

/// WARNING(undocumented): a series counts while `until − newest ≤ 2 × gap + 1 min`, so a renamed
/// host or an old tag set stops counting; one with a single run in the window counts while it is
/// there, as a check run once a day does.
fn streak_of(last: i32, p: &Period, runs: &[&Row]) -> Option<(Streak, i64)> {
    let mut runs = runs.to_vec();
    runs.sort_by_key(|r| std::cmp::Reverse(r.bucket_ms));
    runs.dedup_by_key(|r| r.bucket_ms);
    let newest = runs.first()?.bucket_ms;

    let live = match runs.as_slice() {
        [_, previous, ..] => p.to - newest <= 2 * (newest - previous.bucket_ms) + 60_000,
        _ => true,
    };
    if !live {
        return None;
    }

    let last = usize::try_from(last).unwrap_or(0);
    let statuses: Vec<i32> = runs.iter().map(|r| r.value as i32).collect();
    let known: Vec<i32> = statuses.iter().copied().filter(|s| *s != STATUS_UNKNOWN).take(last).collect();

    Some((
        Streak {
            latest: statuses[0],
            critical: leading(STATUS_CRITICAL, &known),
            warning: leading(STATUS_WARNING, &known),
            ok: leading(STATUS_OK, &known),
            unknown: leading(STATUS_UNKNOWN, &statuses),
            ok_since_unknown: leading(STATUS_OK, &statuses[..last.min(statuses.len())]),
        },
        newest,
    ))
}

/// The rows of `sql` as an evaluation: one group per combination of the `by` values, or per series
/// for by("*"), each with its live series. A group with none is left out, so it goes silent.
pub fn evaluation(q: &CheckQuery, p: &Period, max_groups: usize, rows: &[Row]) -> Result<Evaluation, String> {
    let pairs_of = |values: &[Option<String>]| -> Vec<String> {
        let mut pairs: Vec<String> = match (q.group_by.as_slice(), values) {
            ([star], [Some(series)]) if star == "*" => {
                series.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()
            }
            (keys, values) => {
                keys.iter().zip(values).map(|(k, v)| format!("{k}:{}", v.as_deref().unwrap_or_default())).collect()
            }
        };
        pairs.sort();
        pairs
    };

    let complete: Vec<&Row> = rows.iter().filter(|r| r.groups.iter().all(Option::is_some)).collect();
    let groups = grouped(complete, |r| pairs_of(&r.groups));

    if groups.len() > max_groups {
        return Err(format!(
            "too_many_groups: the query yields {} groups, at most {max_groups} are evaluated",
            groups.len()
        ));
    }

    let results = groups
        .into_iter()
        .filter_map(|(pairs, members)| {
            let live: Vec<(Streak, i64)> = grouped(members, |r| r.series_id)
                .into_iter()
                .filter_map(|(_, runs)| streak_of(q.last, p, &runs))
                .collect();
            let newest = live.iter().map(|(_, newest)| *newest).max()?;
            Some(GroupResult {
                key: if pairs.is_empty() { "*".to_string() } else { pairs.join(",") },
                tags: pairs,
                reading: Reading::Streaks(live.into_iter().map(|(streak, _)| streak).collect()),
                recovery: None,
                last_point: Some(newest),
            })
        })
        .collect();

    Ok(Evaluation { period: *p, groups: results })
}

pub async fn evaluate<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    max_groups: usize,
    c: &CheckDefinition,
    at: i64,
) -> Result<Evaluation, String> {
    let p = period(c, at);
    let rows = execute.rows(sql(tenant, &c.query, &p)).await?;
    evaluation(&c.query, &p, max_groups, &rows)
}

fn severity(status: i32) -> i32 {
    match status {
        STATUS_CRITICAL => 3,
        STATUS_WARNING => 2,
        STATUS_UNKNOWN => 1,
        _ => 0,
    }
}

/// The worst newest status, as the group's value: CRITICAL, then WARNING, UNKNOWN and OK.
pub fn worst(streaks: &[Streak]) -> f64 {
    let worst = streaks.iter().map(|s| s.latest).reduce(|a, b| if severity(b) > severity(a) { b } else { a });
    f64::from(worst.unwrap_or(STATUS_OK))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::monitor::definition::Monitor;
    use crate::monitor::{state, validate};
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

    // --- the SQL -----------------------------------------------------------------------------

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;

    fn at() -> i64 {
        Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap().timestamp_millis()
    }

    fn acme() -> TenantId {
        TenantId("acme".to_string())
    }

    #[test]
    fn the_sql_binds_every_value_and_reads_the_newest_runs_of_each_series() {
        let c = defined(
            r#""pg.can_connect".over("env:prod").by("kube_namespace","host").last(3).count_by_status()"#,
            r#"{"evaluation_delay":60}"#,
        );
        let p = period(&c, at());
        assert_eq!(p, Period { from: at() - 121 * MINUTE, to: at() - MINUTE });

        let sql = sql(&acme(), &c.query, &p);
        let values: Vec<&SqlValue> = sql.parameters.iter().map(|(_, v)| v).collect();
        for v in [
            SqlValue::String("acme".into()),
            SqlValue::String("pg.can_connect".into()),
            SqlValue::Int64(p.from),
            SqlValue::Int64(p.to),
            SqlValue::String("env".into()),
            SqlValue::String("prod".into()),
        ] {
            assert!(values.contains(&&v), "{v:?}");
        }

        assert!(!sql.text.contains("acme"));
        assert!(sql.text.contains("SELECT DISTINCT timestamp"));
        assert!(sql.text.contains("LIMIT 3 BY series_id, status = 'UNKNOWN'"));
        let groups = regex::Regex::new(r"nullIf\(toString\(host\), ''\) AS g0, arrayJoin.* AS g1, toInt64").unwrap();
        assert!(groups.is_match(&sql.text), "{}", sql.text);
        let one = CheckQuery { last: 1, ..c.query.clone() };
        assert!(super::sql(&acme(), &one, &p).text.contains("LIMIT 2 BY series_id, status = 'UNKNOWN'"));
    }

    #[test]
    fn a_longer_no_data_timeframe_reads_further_back() {
        let c = defined(CONSUL, r#"{"notify_no_data":true,"no_data_timeframe":1440}"#);
        assert_eq!(period(&c, at()).from, at() - 24 * HOUR);
        assert_eq!(period(&defined(CONSUL, r#"{"no_data_timeframe":30}"#), at()).from, at() - 2 * HOUR);
    }

    #[test]
    fn by_star_groups_by_series_its_host_and_tags() {
        let sql = sql(&acme(), &parsed(r#""x".by("*").last(2).count_by_status()"#), &Period { from: at() - 2 * HOUR, to: at() });
        assert!(sql.text.contains("concat('host:', toString(host))"));
        assert!(sql.text.contains(" AS g0, toInt64"));
    }

    /// The text these queries have always run as (recorded on 2026-10-18).
    #[test]
    fn the_sql_is_what_it_was() {
        let by_host = defined(r#""pg.can_connect".over("env:prod").by("kube_namespace","host").last(3).count_by_status()"#, "");
        let per_series = defined(r#""x".by("*").last(2).count_by_status()"#, "");
        assert_eq!(sql(&acme(), &by_host.query, &period(&by_host, at())).text, BY_HOST_SQL);
        let per_series_sql = sql(&acme(), &per_series.query, &period(&per_series, at()));
        assert_eq!(per_series_sql.text, PER_SERIES_SQL);
        let names: Vec<&str> = per_series_sql.parameters.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["tenant0", "check1", "from2", "to3"]);
    }

    const BY_HOST_SQL: &str = r#"SELECT nullIf(toString(host), '') AS g0, arrayJoin(if(empty(tags[{k0:String}]), [NULL], CAST(tags[{k0:String}], 'Array(Nullable(String))'))) AS g1, toInt64(toUnixTimestamp(timestamp)) * 1000 AS bucket_ms, toFloat64(CAST(status, 'Int8')) AS value, series_id
FROM (
    SELECT DISTINCT timestamp, host, tags, status, cityHash64(host, tags) AS series_id
    FROM check_runs
    WHERE tenant_id = {tenant1:String} AND check_name = {check2:String}
      AND timestamp >= fromUnixTimestamp64Milli({from3:Int64})
      AND timestamp < fromUnixTimestamp64Milli({to4:Int64})
      AND (has(tags[{k5:String}], {p6:String}))
    ORDER BY timestamp DESC
    LIMIT 3 BY series_id, status = 'UNKNOWN'
)"#;

    const PER_SERIES_SQL: &str = r#"SELECT arrayStringConcat(arraySort(arrayConcat(if(host = '', [], [concat('host:', toString(host))]), arrayFlatten(arrayMap((k, vs) -> arrayMap(v -> if(v = '', toString(k), concat(toString(k), ':', toString(v))), vs), mapKeys(tags), mapValues(tags))))), ',') AS g0, toInt64(toUnixTimestamp(timestamp)) * 1000 AS bucket_ms, toFloat64(CAST(status, 'Int8')) AS value, series_id
FROM (
    SELECT DISTINCT timestamp, host, tags, status, cityHash64(host, tags) AS series_id
    FROM check_runs
    WHERE tenant_id = {tenant0:String} AND check_name = {check1:String}
      AND timestamp >= fromUnixTimestamp64Milli({from2:Int64})
      AND timestamp < fromUnixTimestamp64Milli({to3:Int64})
      AND (1)
    ORDER BY timestamp DESC
    LIMIT 2 BY series_id, status = 'UNKNOWN'
)"#;

    // --- the evaluation ----------------------------------------------------------------------

    const OK: i32 = 0;
    const WARNING: i32 = 1;
    const CRITICAL: i32 = 2;
    const UNKNOWN: i32 = 3;

    /// Runs of one series, newest first, `every` seconds apart, the newest `age` seconds before
    /// `to`.
    fn runs_of(series: u64, groups: &[Option<&str>], age: f64, every: f64, statuses: &[i32]) -> Vec<Row> {
        let p = period(&defined(CONSUL, ""), at());
        statuses
            .iter()
            .enumerate()
            .map(|(i, status)| Row {
                groups: groups.iter().map(|g| g.map(str::to_string)).collect(),
                series_id: Some(series),
                bucket_ms: p.to - ((age + every * i as f64) * 1000.0) as i64,
                value: f64::from(*status),
            })
            .collect()
    }

    fn evaluate_rows(query: &str, rows: &[Row]) -> Evaluation {
        let c = defined(query, "");
        evaluation(&c.query, &period(&c, at()), 100, rows).unwrap_or_else(|e| panic!("{e}"))
    }

    const BY_HOST: &str = r#""x".over("*").by("host").last(5).count_by_status()"#;
    const ONE_GROUP: &str = r#""x".over("*").last(5).count_by_status()"#;

    fn streaks(e: &Evaluation) -> Vec<(String, Vec<Streak>)> {
        e.groups
            .iter()
            .map(|g| match &g.reading {
                Reading::Streaks(s) => (g.key.clone(), s.clone()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn only_streak(e: &Evaluation) -> Streak {
        let (_, s) = streaks(e).remove(0);
        assert_eq!(s.len(), 1, "{s:?}");
        s[0]
    }

    fn streak(latest: i32, critical: i32, warning: i32, ok: i32, unknown: i32, ok_since_unknown: i32) -> Streak {
        Streak { latest, critical, warning, ok, unknown, ok_since_unknown }
    }

    #[test]
    fn a_streak_counts_the_newest_runs_in_a_row_within_the_last_n() {
        let e = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], 10.0, 15.0, &[CRITICAL, CRITICAL, WARNING, OK, OK, OK]));
        assert_eq!(streaks(&e), vec![("host:a".to_string(), vec![streak(CRITICAL, 2, 0, 0, 0, 0)])]);

        let healthy = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], 10.0, 15.0, &[OK; 7]));
        assert_eq!(only_streak(&healthy), streak(OK, 0, 0, 5, 0, 5));
    }

    #[test]
    fn unknown_runs_take_no_place_among_the_last_n_and_the_newest_in_a_row_are_counted() {
        let e = evaluate_rows(
            BY_HOST,
            &runs_of(1, &[Some("a")], 10.0, 15.0, &[UNKNOWN, CRITICAL, UNKNOWN, CRITICAL, OK, CRITICAL, CRITICAL]),
        );
        assert_eq!(only_streak(&e), streak(UNKNOWN, 2, 0, 0, 1, 0));

        // ntp: four UNKNOWN in a row do not push the CRITICAL runs out.
        let ntp = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], 10.0, 15.0, &[UNKNOWN, UNKNOWN, UNKNOWN, UNKNOWN, CRITICAL, CRITICAL]));
        assert_eq!(only_streak(&ntp), streak(UNKNOWN, 2, 0, 0, 4, 0));

        // OK runs before an UNKNOWN one count for ok, not for ok_since_unknown.
        let back = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], 10.0, 15.0, &[OK, UNKNOWN, UNKNOWN, OK, OK]));
        assert_eq!(only_streak(&back), streak(OK, 0, 0, 3, 0, 1));

        // All UNKNOWN: no streak of a known status.
        let silent = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], 10.0, 15.0, &[UNKNOWN, UNKNOWN]));
        assert_eq!(only_streak(&silent), streak(UNKNOWN, 0, 0, 0, 2, 0));
    }

    #[test]
    fn critical_runs_between_unknown_ones_alert() {
        let c = defined(r#""x".over("*").last(3).count_by_status()"#, r#"{"thresholds":{"critical":2}}"#);
        let p = period(&c, at());
        let rows = runs_of(1, &[], 10.0, 15.0, &[UNKNOWN, CRITICAL, UNKNOWN, CRITICAL, UNKNOWN, CRITICAL]);

        let e = evaluation(&c.query, &p, 100, &rows).unwrap_or_else(|e| panic!("{e}"));
        let (groups, transitions) = state::step(&Monitor::ServiceCheck(c), &Default::default(), &e, at());
        assert_eq!(groups["*"].state, Some(state::Status::Alert));
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].reason, state::Reason::New);
    }

    #[test]
    fn a_series_counts_while_its_newest_run_is_at_most_twice_its_gap_and_a_minute_old_or_is_its_only_one() {
        for (age, every, count, live) in
            [(90.0, 15.0, 2, true), (91.0, 15.0, 2, false), (1800.0, 900.0, 2, true), (1861.0, 900.0, 2, false), (3600.0, 15.0, 1, true)]
        {
            let e = evaluate_rows(BY_HOST, &runs_of(1, &[Some("a")], age, every, &vec![CRITICAL; count]));
            assert_eq!(!e.groups.is_empty(), live, "{age} {every} {count}");
        }
    }

    #[test]
    fn a_node_whose_tags_changed_the_old_series_drops_out() {
        let mut rows = runs_of(1, &[Some("a")], 600.0, 15.0, &[CRITICAL, CRITICAL, CRITICAL]);
        rows.extend(runs_of(2, &[Some("a")], 5.0, 15.0, &[OK, OK]));
        let e = evaluate_rows(BY_HOST, &rows);
        assert_eq!(streaks(&e), vec![("host:a".to_string(), vec![streak(OK, 0, 0, 2, 0, 2)])]);
    }

    #[test]
    fn runs_without_a_by_key_are_n_a_and_dropped() {
        let mut rows = runs_of(1, &[None], 5.0, 15.0, &[CRITICAL]);
        rows.extend(runs_of(2, &[Some("b")], 5.0, 15.0, &[OK]));
        let e = evaluate_rows(BY_HOST, &rows);
        assert_eq!(e.groups.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["host:b"]);
        assert_eq!(e.groups[0].tags, ["host:b"]);
    }

    #[test]
    fn no_by_is_one_group_judging_each_series() {
        let mut rows = runs_of(1, &[], 5.0, 15.0, &[CRITICAL]);
        rows.extend(runs_of(2, &[], 5.0, 15.0, &[OK]));
        let e = evaluate_rows(ONE_GROUP, &rows);
        assert_eq!(e.groups.len(), 1);
        assert_eq!((e.groups[0].key.as_str(), e.groups[0].tags.clone()), ("*", vec![]));
        assert_eq!(streaks(&e)[0].1.len(), 2);
    }

    #[test]
    fn by_star_is_a_group_per_series() {
        let per_series = r#""x".over("*").by("*").last(5).count_by_status()"#;
        let mut rows = runs_of(1, &[Some("host:a,role:db")], 5.0, 15.0, &[CRITICAL]);
        rows.extend(runs_of(2, &[Some("")], 5.0, 15.0, &[OK]));
        let e = evaluate_rows(per_series, &rows);
        let mut groups: Vec<(String, Vec<String>)> = e.groups.iter().map(|g| (g.key.clone(), g.tags.clone())).collect();
        groups.sort();
        assert_eq!(
            groups,
            vec![("*".to_string(), vec![]), ("host:a,role:db".to_string(), vec!["host:a".to_string(), "role:db".to_string()])]
        );
    }

    #[test]
    fn a_group_with_only_dead_series_is_left_out() {
        let mut rows = runs_of(1, &[Some("a")], 3600.0, 15.0, &[CRITICAL, CRITICAL]);
        rows.extend(runs_of(2, &[Some("b")], 5.0, 15.0, &[OK]));
        let e = evaluate_rows(BY_HOST, &rows);
        assert_eq!(e.groups.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["host:b"]);
    }

    #[test]
    fn too_many_groups_is_an_error() {
        let c = defined(BY_HOST, "");
        let rows: Vec<Row> = (1..=3).flat_map(|i| runs_of(i, &[Some(&format!("h{i}"))], 5.0, 15.0, &[OK])).collect();
        match evaluation(&c.query, &period(&c, at()), 2, &rows) {
            Err(e) => assert!(e.starts_with("too_many_groups: the query yields 3 groups, at most 2"), "{e}"),
            Ok(e) => panic!("{e:?}"),
        }
    }

    #[test]
    fn a_groups_last_point_is_its_newest_run_one_run_per_second_counts_once() {
        let mut rows = runs_of(1, &[Some("a")], 20.0, 15.0, &[CRITICAL, CRITICAL]);
        rows.extend(runs_of(2, &[Some("a")], 5.0, 15.0, &[OK, OK]));
        rows.extend(runs_of(1, &[Some("a")], 20.0, 15.0, &[CRITICAL]));
        let e = evaluate_rows(BY_HOST, &rows);
        let p = period(&defined(BY_HOST, ""), at());
        assert_eq!(e.groups[0].last_point, Some(p.to - 5_000));
        let critical = streaks(&e)[0].1.iter().find(|s| s.latest == CRITICAL).copied().unwrap();
        assert_eq!(critical.critical, 2);
    }

    #[test]
    fn the_worst_newest_status_is_the_groups_value() {
        let latest = |l: i32| streak(l, 0, 0, 0, 0, 0);
        assert_eq!(worst(&[latest(OK), latest(CRITICAL), latest(WARNING)]), 2.0);
        assert_eq!(worst(&[latest(UNKNOWN), latest(WARNING)]), 1.0);
        assert_eq!(worst(&[latest(OK), latest(UNKNOWN)]), 3.0);
        assert_eq!(worst(&[latest(OK)]), 0.0);
    }

    #[tokio::test]
    async fn evaluate_reads_the_runs_through_the_executor() {
        let c = defined(BY_HOST, "");
        let rows = runs_of(1, &[Some("a")], 10.0, 15.0, &[CRITICAL, OK]);
        let asked = std::sync::Mutex::new(vec![]);
        let fake = crate::query::execute::Fake(|sql: &Sql| {
            asked.lock().unwrap().push(sql.text.clone());
            rows.clone()
        });
        let e = evaluate(&fake, &acme(), 100, &c, at()).await.unwrap();
        assert_eq!(asked.into_inner().unwrap(), [super::sql(&acme(), &c.query, &period(&c, at())).text]);
        assert_eq!(only_streak(&e), streak(CRITICAL, 1, 0, 0, 0, 0));
    }
}
