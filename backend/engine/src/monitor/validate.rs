//! A monitor definition checked whole; the query process and the monitor service both run it.
//! (F#: `Monitor/Validate.fs`.)

use chrono::TimeDelta;
use ninjapaws_anomaly::Forecast;
use serde_json::Value;

use crate::api::v2::timeseries::{ParsedFormula, ParsedTimeseriesRequest};
use crate::monitor::check;
use crate::monitor::definition::{
    AnomalySpec, Comparator, Definition, Detection, MissingData, Monitor, MonitorQuery, Options, Thresholds,
};
use crate::monitor::options;
use crate::monitor::parse;
use crate::query::engine;
use crate::query::plan::{self, Retention};

/// Whether threshold `a` is reached after `b` on the way to alerting.
fn stricter(c: Comparator, a: f64, b: f64) -> bool {
    match c {
        Comparator::Gt | Comparator::Ge => a > b,
        Comparator::Lt | Comparator::Le => a < b,
        Comparator::Eq | Comparator::Ne => false,
    }
}

/// Where the less strict side of a threshold is.
fn lax_side(c: Comparator) -> &'static str {
    match c {
        Comparator::Gt | Comparator::Ge => "below",
        Comparator::Lt | Comparator::Le | Comparator::Eq | Comparator::Ne => "above",
    }
}

fn warning_problems(q: &MonitorQuery, t: &Thresholds) -> Vec<String> {
    let critical = q.threshold;
    match (q.comparator, t.warning) {
        // WARNING(undocumented): "less strict" has no direction for == and !=; the warning only
        // has to differ from critical.
        (Comparator::Eq | Comparator::Ne, Some(w)) if w == critical => {
            vec![format!("options.thresholds.warning ({w}) must differ from critical ({critical})")]
        }
        (c @ (Comparator::Gt | Comparator::Ge | Comparator::Lt | Comparator::Le), Some(w)) if !stricter(c, critical, w) => {
            vec![format!(
                "options.thresholds.warning ({w}) must be less strict than critical ({critical}), so {} it",
                lax_side(c)
            )]
        }
        _ => vec![],
    }
}

/// WARNING(undocumented): Datadog does not say whether a recovery threshold may equal the one it
/// recovers from. Ours must sit strictly on the recovery side; equal would be the same as having
/// none.
fn recovery_problems(q: &MonitorQuery, t: &Thresholds) -> Vec<String> {
    let c = q.comparator;
    let recovery = |name: &str, value: Option<f64>, from: &str, threshold: f64| match value {
        Some(r) if !stricter(c, threshold, r) => vec![format!(
            "options.thresholds.{name} ({r}) must be {} {from} ({threshold}), on the side the monitor recovers to",
            lax_side(c)
        )],
        _ => vec![],
    };

    match (c, t.critical_recovery, t.warning_recovery) {
        (Comparator::Eq | Comparator::Ne, None, None) => vec![],
        (Comparator::Eq | Comparator::Ne, _, _) => vec![format!(
            "recovery thresholds do not apply to {}: the monitor recovers when the condition stops holding",
            c.text()
        )],
        _ => {
            let mut problems = recovery("critical_recovery", t.critical_recovery, "critical", q.threshold);
            match (t.warning_recovery, t.warning) {
                (Some(_), None) => problems.push("options.thresholds.warning_recovery needs a warning threshold".to_string()),
                (wr, Some(w)) => problems.extend(recovery("warning_recovery", wr, "warning", w)),
                (None, None) => {}
            }
            problems
        }
    }
}

fn threshold_problems(q: &MonitorQuery, t: &Thresholds) -> Vec<String> {
    let mut problems = match t.critical {
        Some(x) if x != q.threshold => vec![format!("options.thresholds.critical ({x}) must equal the query's threshold ({})", q.threshold)],
        _ => vec![],
    };

    match (&q.detection, t.warning, t.critical_recovery, t.warning_recovery) {
        (Detection::Outlier(_), None, None, None) => {}
        // WARNING(undocumented): an outlier's value is 1 or 0, so there is nothing between alerting
        // and not for a warning or a recovery to mark.
        (Detection::Outlier(_), _, _, _) => problems.push("an outlier monitor takes no warning or recovery thresholds".to_string()),
        _ => {
            problems.extend(warning_problems(q, t));
            problems.extend(recovery_problems(q, t));
        }
    }
    problems
}

fn check_only_thresholds(t: &Thresholds) -> Vec<String> {
    let mut problems = vec![];
    if t.ok.is_some() {
        problems.push("options.thresholds.ok is for service check monitors".to_string());
    }
    if t.unknown.is_some() {
        problems.push("options.thresholds.unknown is for service check monitors".to_string());
    }
    problems
}

/// .NET's "0.##": at most two decimals, without trailing zeros.
fn at_most_two_decimals(x: f64) -> String {
    let text = format!("{x:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_string() } else { text.to_string() }
}

fn total_seconds(span: TimeDelta) -> f64 {
    span.num_seconds() as f64 + f64::from(span.subsec_nanos()) / 1e9
}

fn minutes(span: TimeDelta) -> String {
    format!("{} minutes", at_most_two_decimals(total_seconds(span) / 60.0))
}

fn days(span: TimeDelta) -> String {
    format!("{} days", at_most_two_decimals(total_seconds(span) / 86_400.0))
}

/// A span times a factor, rounded to .NET's 100 ns ticks as `TimeSpan * float` does.
fn scaled(span: TimeDelta, factor: f64) -> TimeDelta {
    let ticks = (span.num_nanoseconds().unwrap_or(i64::MAX) as f64 / 100.0 * factor).round();
    TimeDelta::nanoseconds((ticks as i64).saturating_mul(100))
}

fn anomaly_problems(q: &MonitorQuery, a: &AnomalySpec, o: &Options) -> Vec<String> {
    let trigger = o.trigger_window.unwrap_or(a.alert_window);
    // WARNING(undocumented): without a recovery_window, the trigger window.
    let recovery = o.recovery_window.unwrap_or(trigger);
    let c = q.threshold;
    let r = o.thresholds.critical_recovery.unwrap_or(0.0);
    // WARNING(undocumented): Datadog documents the bound for a recovery threshold of 0, T·(1−c);
    // generalised to T·(1−c)/(1−r). Only for r < c ≤ 1, where it is finite.
    let lowest = || scaled(trigger.length(), (1.0 - c) / (1.0 - r));

    let mut problems = vec![];
    if trigger.length() != a.alert_window.length() {
        problems.push(format!(
            "options.threshold_windows.trigger_window (last_{}) must equal anomalies() alert_window (last_{})",
            trigger.text(),
            a.alert_window.text()
        ));
    }
    if q.window.length() < a.alert_window.length() {
        problems.push(format!(
            "the monitor's window (last_{}) must be at least alert_window (last_{})",
            q.window.text(),
            a.alert_window.text()
        ));
    }
    if let Some(i) = a.interval
        && i > a.alert_window.length() / 5
    {
        problems.push(format!(
            "anomalies() interval ({} s) must be at most a fifth of alert_window (last_{}), so the window holds at least 5 buckets",
            total_seconds(i),
            a.alert_window.text()
        ));
    }
    if recovery.length() > trigger.length() {
        problems.push(format!(
            "options.threshold_windows.recovery_window (last_{}) must not be longer than the trigger window (last_{})",
            recovery.text(),
            trigger.text()
        ));
    } else if r < c && recovery.length() <= lowest() {
        problems.push(format!(
            "options.threshold_windows.recovery_window (last_{}) must be longer than {}, or the monitor could recover while still anomalous",
            recovery.text(),
            minutes(lowest())
        ));
    }
    if let Some(w) = o.thresholds.warning
        && w <= 0.0
    {
        problems.push("options.thresholds.warning is a fraction of anomalous points, above 0".to_string());
    }
    for (name, value) in [("critical_recovery", o.thresholds.critical_recovery), ("warning_recovery", o.thresholds.warning_recovery)] {
        if let Some(v) = value
            && v < 0.0
        {
            problems.push(format!("options.thresholds.{name} is a fraction of anomalous points, 0 or more"));
        }
    }
    problems
}

/// What the metrics tables must still hold for the monitor to be evaluated.
fn retention_problems(retention: Retention, q: &MonitorQuery, o: &Options) -> Vec<String> {
    let kept = format!("metrics are kept {}", days(retention.rollup));

    let reach = match &q.detection {
        Detection::Change(c) => q.window.length() + c.shift.length(),
        Detection::Forecast(f) => f.history.unwrap_or(TimeDelta::zero()),
        Detection::Threshold | Detection::Anomaly(_) | Detection::Outlier(_) => q.window.length(),
    };

    let mut problems = vec![];
    if reach > retention.rollup {
        problems.push(format!("the monitor reads {} back; {kept}", days(reach)));
    }
    if let Detection::Forecast(f) = &q.detection
        && let Forecast::Seasonal(s) = f.method
        && plan::season_length(s) * 2 > retention.rollup
    {
        problems.push(format!("a seasonal forecast needs two seasons of history ({}); {kept}", days(plan::season_length(s) * 2)));
    }
    if let Some(timeframe) = o.no_data_timeframe
        && timeframe > retention.rollup
    {
        problems.push(format!("options.no_data_timeframe ({}) is longer than {kept}", minutes(timeframe)));
    }
    problems
}

fn window_problems(q: &MonitorQuery, o: &Options) -> Vec<String> {
    match (&q.detection, o.trigger_window, o.recovery_window) {
        (Detection::Anomaly(_) | Detection::Forecast(_), _, _) | (_, None, None) => vec![],
        _ => vec!["options.threshold_windows is for anomaly monitors".to_string()],
    }
}

/// Datadog accepts only `default` beside default_zero(), which already reads missing points as 0.
fn missing_data_problems(q: &MonitorQuery, o: &Options) -> Vec<String> {
    match o.on_missing_data {
        None | Some(MissingData::Default) => vec![],
        Some(_) if parse::function_names(&q.expression).iter().any(|n| n == "default_zero") => {
            vec!["options.on_missing_data must be default when the query has default_zero()".to_string()]
        }
        Some(_) => vec![],
    }
}

/// Whether the engine can plan the query, as the evaluation will run it.
fn plan_problems(retention: Retention, now: i64, q: &MonitorQuery) -> Vec<String> {
    let (queries, formula) = engine::split(&q.expression);

    let (lookback, interval) = match &q.detection {
        Detection::Forecast(f) => (f.history.unwrap_or(q.window.length()), f.interval),
        Detection::Anomaly(a) => (q.window.length(), a.interval),
        Detection::Threshold | Detection::Change(_) | Detection::Outlier(_) => (q.window.length(), None),
    };

    // retention_problems already refuses a lookback this long, and a far longer one would not fit
    // before `now`.
    if lookback > retention.rollup {
        return vec![];
    }
    let request = ParsedTimeseriesRequest {
        from: now - lookback.num_milliseconds(),
        to: now,
        interval,
        queries,
        formulas: vec![ParsedFormula { formula, limit: None }],
    };
    match plan::plan(retention, now, &request) {
        Ok(_) => vec![],
        Err(problems) => problems,
    }
}

fn problems_of<T>(result: &Result<T, Vec<String>>) -> Vec<String> {
    match result {
        Err(problems) => problems.clone(),
        Ok(_) => vec![],
    }
}

/// `now` is the evaluation's time, in unix milliseconds; the engine never reads the clock.
pub fn definition(retention: Retention, now: i64, query: &str, options: &str) -> Result<Definition, Vec<String>> {
    let (q, o) = match (parse::query(query), options::parse(options)) {
        (Ok(q), Ok(o)) => (q, o),
        (q, o) => {
            let mut problems = problems_of(&q);
            problems.extend(problems_of(&o));
            return Err(problems);
        }
    };

    let mut problems = threshold_problems(&q, &o.thresholds);
    problems.extend(check_only_thresholds(&o.thresholds));
    problems.extend(window_problems(&q, &o));
    problems.extend(missing_data_problems(&q, &o));
    if let Detection::Anomaly(a) = &q.detection {
        problems.extend(anomaly_problems(&q, a, &o));
    }
    problems.extend(retention_problems(retention, &q, &o));
    problems.extend(plan_problems(retention, now, &q));

    if !problems.is_empty() {
        return Err(problems);
    }

    let mut options = o;
    if matches!(q.detection, Detection::Forecast(_)) && (options.trigger_window.is_some() || options.recovery_window.is_some()) {
        options.not_acted_on.push("threshold_windows".to_string());
    }
    options.thresholds.critical = Some(q.threshold);
    Ok(Definition { query: q, options })
}

/// A metric monitor or a service check monitor, by its type.
pub fn by_type(retention: Retention, now: i64, monitor_type: &str, query: &str, options: &str) -> Result<Monitor, Vec<String>> {
    match monitor_type {
        // Datadog still accepts the older name.
        "query alert" | "metric alert" => definition(retention, now, query, options).map(Monitor::Metric),
        "service check" => check::definition(query, options).map(Monitor::ServiceCheck),
        other => Err(vec![format!(
            "monitor type '{other}' is not supported; a metric monitor is a 'query alert', a check monitor a 'service check'"
        )]),
    }
}

/// A monitor as Datadog's API takes it, as JSON; fields other than type, query and options are
/// the panel's.
pub fn monitor(retention: Retention, now: i64, json: &str) -> Result<Monitor, Vec<String>> {
    let root = match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(root)) => root,
        Ok(_) => return Err(vec!["a monitor must be a JSON object".to_string()]),
        Err(e) => return Err(vec![format!("the monitor is not valid JSON: {e}")]),
    };
    let text = |name: &str| match root.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(vec![format!("{name} must be a string")]),
    };
    // Written back out for options::parse, as the F# passed on the property's raw text.
    let options = root.get("options").map(Value::to_string).unwrap_or_default();

    match (text("type"), text("query")) {
        (Ok(monitor_type), Ok(query)) => by_type(retention, now, &monitor_type, &query, &options),
        (monitor_type, query) => {
            let mut problems = problems_of(&monitor_type);
            problems.extend(problems_of(&query));
            Err(problems)
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::monitor::definition::{Window, WindowUnit};
    use crate::query::plan::DEFAULT_RETENTION;

    fn now() -> i64 {
        Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap().timestamp_millis()
    }

    fn validate_with(retention: Retention, query: &str, options: &str) -> Result<Definition, Vec<String>> {
        definition(retention, now(), query, options)
    }

    fn valid(query: &str, options: &str) -> Definition {
        match validate_with(DEFAULT_RETENTION, query, options) {
            Ok(d) => d,
            Err(problems) => panic!("refused:\n{}", problems.join("\n")),
        }
    }

    fn problems<T: std::fmt::Debug>(result: Result<T, Vec<String>>) -> Vec<String> {
        match result {
            Ok(d) => panic!("expected problems, got {d:?}"),
            Err(problems) => problems,
        }
    }

    fn mentions(fragment: &str, problems: &[String]) {
        assert!(problems.iter().any(|p| p.contains(fragment)), "no problem mentions '{fragment}':\n{}", problems.join("\n"));
    }

    fn refused(query: &str, options: &str, fragment: &str) {
        mentions(fragment, &problems(validate_with(DEFAULT_RETENTION, query, options)));
    }

    const ABOVE_90: &str = "avg(last_5m):avg:system.cpu.user{*} by {host} > 90";

    // --- options -----------------------------------------------------------------------------

    #[test]
    fn options_as_datadog_exports_them() {
        let d = valid(
            ABOVE_90,
            r#"{"thresholds":{"critical":90,"warning":80,"critical_recovery":70,"warning_recovery":60},
                "notify_no_data":true,"no_data_timeframe":10,"evaluation_delay":60,"new_host_delay":300,
                "require_full_window":false,"timeout_h":2,"include_tags":true,"notify_audit":false,
                "renotify_interval":0,"silenced":{},"locked":false,"escalation_message":null}"#,
        );
        let o = d.options;
        assert_eq!(o.thresholds.warning, Some(80.0));
        assert_eq!(o.thresholds.critical_recovery, Some(70.0));
        assert_eq!(o.thresholds.warning_recovery, Some(60.0));
        assert!(o.notify_no_data);
        assert_eq!(o.no_data_timeframe, Some(TimeDelta::minutes(10)));
        assert_eq!(o.on_missing_data, None);
        assert_eq!(o.evaluation_delay, TimeDelta::seconds(60));
        assert_eq!(o.new_group_delay, TimeDelta::seconds(300));
        assert_eq!(o.timeout, Some(TimeDelta::hours(2)));
        assert_eq!(o.not_acted_on, vec!["include_tags", "notify_audit", "renotify_interval", "silenced", "locked"]);
    }

    #[test]
    fn no_options_the_defaults_critical_from_the_query() {
        for options in ["", "null", "{}"] {
            let o = valid(ABOVE_90, options).options;
            assert_eq!(o.thresholds.critical, Some(90.0));
            assert_eq!(o.new_group_delay, TimeDelta::seconds(60));
            assert_eq!(o.evaluation_delay, TimeDelta::zero());
            assert!(!o.require_full_window);
            assert!(!o.notify_no_data);
            assert_eq!(o.no_data_timeframe, None);
            assert_eq!(o.timeout, None);
            assert!(o.not_acted_on.is_empty());
        }
    }

    #[test]
    fn options_this_version_does_not_act_on_are_accepted_and_named() {
        // Other products' options, and a misspelling, which the panel then shows as ignored.
        let o = valid(
            ABOVE_90,
            r#"{"groupby_simple_monitor":false,"enable_logs_sample":true,"notify_nodata":true,"min_failure_duration":0}"#,
        )
        .options;
        assert_eq!(o.not_acted_on, vec!["groupby_simple_monitor", "enable_logs_sample", "notify_nodata", "min_failure_duration"]);
    }

    #[test]
    fn require_full_window_is_read() {
        assert!(valid(ABOVE_90, r#"{"require_full_window":true}"#).options.require_full_window);
    }

    #[test]
    fn new_group_delay_wins_over_the_deprecated_new_host_delay() {
        let o = valid(ABOVE_90, r#"{"new_group_delay":120,"new_host_delay":300}"#).options;
        assert_eq!(o.new_group_delay, TimeDelta::seconds(120));
    }

    #[test]
    fn timeout_h_0_is_no_timeout() {
        assert_eq!(valid(ABOVE_90, r#"{"timeout_h":0}"#).options.timeout, None);
    }

    #[test]
    fn options_refused() {
        for (options, fragment) in [
            (r#"{"variables":[{"data_source":"metrics","name":"query1","query":"avg:x{*}"}]}"#, "options.variables"),
            (r#"{"new_group_delay":86401}"#, "new_group_delay must be a whole number from 0 to 86400"),
            (r#"{"new_group_delay":-1}"#, "new_group_delay must be a whole number from 0 to 86400"),
            (r#"{"threshold_windows":{"trigger_window":"last_99999999d"}}"#, "trigger_window: 'last_99999999d' is not a window"),
            (r#"{"scheduling_options":{"evaluation_window":{"day_starts":"04:00"}}}"#, "custom schedules"),
            (r#"{"evaluation_delay":90000}"#, "evaluation_delay must be a whole number from 0 to 86400"),
            (r#"{"evaluation_delay":1.5}"#, "evaluation_delay must be a whole number"),
            (r#"{"timeout_h":25}"#, "timeout_h must be a whole number from 0 to 24"),
            (r#"{"no_data_timeframe":0}"#, "no_data_timeframe must be a whole number of minutes, at least 1"),
            (r#"{"notify_no_data":"yes"}"#, "notify_no_data must be true or false"),
            (r#"{"thresholds":{"critical":90,"ok":1}}"#, "options.thresholds.ok is for service check monitors"),
            (r#"{"thresholds":{"critical":"90"}}"#, "thresholds.critical must be a number"),
            (r#"{"thresholds":[]}"#, "thresholds must be an object"),
            ("[1]", "options must be an object"),
            (r#"{"thresholds":"#, "not valid JSON"),
        ] {
            refused(ABOVE_90, options, fragment);
        }
    }

    /// serde_json refuses a number out of f64's range as a whole, where System.Text.Json read it
    /// as infinity, which the F# refused as "options.thresholds.critical is too large a number".
    #[test]
    fn a_number_too_large_is_refused() {
        refused(ABOVE_90, r#"{"thresholds":{"critical":1e999}}"#, "options are not valid JSON: number out of range");
    }

    #[test]
    fn on_missing_data_is_read() {
        for (rule, expected) in [
            ("default", MissingData::Default),
            ("show_no_data", MissingData::ShowNoData),
            ("show_and_notify_no_data", MissingData::ShowAndNotifyNoData),
            ("resolve", MissingData::Resolve),
        ] {
            let o = valid(ABOVE_90, &format!(r#"{{"on_missing_data":"{rule}","notify_no_data":false}}"#)).options;
            assert_eq!(o.on_missing_data, Some(expected));
            assert!(o.not_acted_on.is_empty());
        }
    }

    #[test]
    fn on_missing_data_refused() {
        for (options, fragment) in [
            (r#"{"on_missing_data":"notify"}"#, "on_missing_data must be one of default, show_no_data"),
            (r#"{"on_missing_data":true}"#, "on_missing_data must be one of"),
            (r#"{"on_missing_data":"resolve","notify_no_data":true}"#, "on_missing_data replaces notify_no_data and no_data_timeframe"),
            (r#"{"on_missing_data":"default","no_data_timeframe":10}"#, "on_missing_data replaces notify_no_data and no_data_timeframe"),
        ] {
            refused(ABOVE_90, options, fragment);
        }
    }

    #[test]
    fn with_default_zero_only_the_default_on_missing_data() {
        let query = "sum(last_5m):default_zero(sum:requests{*} by {host}) < 10";
        for rule in ["show_no_data", "show_and_notify_no_data", "resolve"] {
            refused(
                query,
                &format!(r#"{{"on_missing_data":"{rule}"}}"#),
                "on_missing_data must be default when the query has default_zero()",
            );
        }
        assert_eq!(valid(query, r#"{"on_missing_data":"default"}"#).options.on_missing_data, Some(MissingData::Default));
    }

    #[test]
    fn several_problems_are_reported_together() {
        let problems = problems(validate_with(DEFAULT_RETENTION, ABOVE_90, r#"{"timeout_h":25,"evaluation_delay":-1}"#));
        mentions("timeout_h", &problems);
        mentions("evaluation_delay", &problems);
    }

    // --- the type ----------------------------------------------------------------------------

    #[test]
    fn metric_alert_is_the_older_name() {
        assert!(by_type(DEFAULT_RETENTION, now(), "metric alert", ABOVE_90, "").is_ok());
    }

    #[test]
    fn other_monitor_types_are_refused() {
        mentions("'log alert' is not supported", &problems(by_type(DEFAULT_RETENTION, now(), "log alert", ABOVE_90, "")));
    }

    // --- thresholds --------------------------------------------------------------------------

    #[test]
    fn critical_must_equal_the_querys() {
        refused(ABOVE_90, r#"{"thresholds":{"critical":95}}"#, "must equal the query's threshold (90)");
    }

    #[test]
    fn warning_less_strict_than_critical() {
        for (comparator, critical, warning, ok, fragment) in [
            (">", 90.0, 80.0, true, "less strict than critical"),
            (">", 90.0, 90.0, false, "less strict than critical"),
            (">", 90.0, 95.0, false, "less strict than critical"),
            ("<", 10.0, 20.0, true, "so above it"),
            ("<", 10.0, 10.0, false, "so above it"),
            ("<", 10.0, 5.0, false, "so above it"),
        ] {
            let query = format!("avg(last_5m):avg:x{{*}} by {{host}} {comparator} {critical}");
            let result = validate_with(DEFAULT_RETENTION, &query, &format!(r#"{{"thresholds":{{"warning":{warning}}}}}"#));
            if ok {
                assert!(result.is_ok(), "{query}: {result:?}");
            } else {
                mentions(fragment, &problems(result));
            }
        }
    }

    #[test]
    fn critical_recovery_strictly_on_the_recovery_side() {
        for (comparator, recovery, ok) in
            [(">", 89.0, true), (">", 90.0, false), (">=", 90.0, false), ("<", 91.0, true), ("<", 90.0, false), ("<=", 85.0, false)]
        {
            let query = format!("avg(last_5m):avg:x{{*}} {comparator} 90");
            let result = validate_with(DEFAULT_RETENTION, &query, &format!(r#"{{"thresholds":{{"critical_recovery":{recovery}}}}}"#));
            if ok {
                assert!(result.is_ok(), "{query}: {result:?}");
            } else {
                mentions("on the side the monitor recovers to", &problems(result));
            }
        }
    }

    #[test]
    fn warning_recovery_is_checked_against_warning() {
        refused(ABOVE_90, r#"{"thresholds":{"warning":80,"warning_recovery":85}}"#, "warning_recovery (85) must be below warning (80)");
    }

    #[test]
    fn warning_recovery_needs_a_warning() {
        refused(ABOVE_90, r#"{"thresholds":{"warning_recovery":70}}"#, "needs a warning threshold");
    }

    #[test]
    fn no_recovery_for_equality() {
        for comparator in ["==", "!="] {
            refused(&format!("avg(last_5m):avg:x{{*}} {comparator} 1"), r#"{"thresholds":{"critical_recovery":0}}"#, "do not apply");
        }
    }

    #[test]
    fn an_equality_warning_must_differ() {
        refused("avg(last_5m):avg:x{*} == 1", r#"{"thresholds":{"warning":1}}"#, "must differ from critical");
        assert!(validate_with(DEFAULT_RETENTION, "avg(last_5m):avg:x{*} == 1", r#"{"thresholds":{"warning":2}}"#).is_ok());
    }

    #[test]
    fn threshold_windows_are_for_anomalies() {
        refused(ABOVE_90, r#"{"threshold_windows":{"trigger_window":"last_5m"}}"#, "for anomaly monitors");
    }

    #[test]
    fn outliers_take_no_warning() {
        refused("avg(last_30m):outliers(avg:x{*} by {host}, 'dbscan', 3) > 0", r#"{"thresholds":{"warning":0}}"#, "no warning or recovery");
    }

    // --- anomaly -----------------------------------------------------------------------------

    fn anomaly(outer: &str, alert: &str, interval: i32, threshold: &str) -> String {
        format!(
            "avg({outer}):anomalies(avg:system.cpu.user{{*}} by {{host}}, 'agile', 2, direction='above', alert_window='{alert}', interval={interval}) >= {threshold}"
        )
    }

    fn windows(trigger: &str, recovery: &str, thresholds: &str) -> String {
        format!(r#"{{"thresholds":{{{thresholds}}},"threshold_windows":{{"trigger_window":"{trigger}","recovery_window":"{recovery}"}}}}"#)
    }

    #[test]
    fn anomaly_as_datadog_writes_it() {
        let d = valid(&anomaly("last_4h", "last_15m", 60, "1"), &windows("last_15m", "last_15m", r#""critical":1,"critical_recovery":0"#));
        assert_eq!(d.options.trigger_window, Some(Window { count: 15, unit: WindowUnit::Minutes }));
    }

    #[test]
    fn anomaly_without_threshold_windows_takes_alert_window() {
        valid(&anomaly("last_4h", "last_15m", 60, "1"), "");
    }

    #[test]
    fn the_trigger_window_is_alert_window() {
        refused(
            &anomaly("last_4h", "last_15m", 60, "1"),
            &windows("last_30m", "last_15m", r#""critical":1"#),
            "must equal anomalies() alert_window",
        );
    }

    #[test]
    fn the_window_holds_alert_window() {
        refused(&anomaly("last_10m", "last_15m", 60, "1"), "", "must be at least alert_window");
    }

    #[test]
    fn the_interval_is_at_most_a_fifth_of_alert_window() {
        valid(&anomaly("last_4h", "last_15m", 180, "1"), "");
        refused(&anomaly("last_4h", "last_15m", 181, "1"), "", "at most a fifth of alert_window");
    }

    #[test]
    fn the_recovery_window_is_not_longer_than_the_trigger_window() {
        refused(
            &anomaly("last_4h", "last_15m", 60, "1"),
            &windows("last_15m", "last_30m", r#""critical":1"#),
            "must not be longer than the trigger window",
        );
    }

    /// Datadog's documented case: T = 15 m, c = 0.8, no recovery threshold, so the recovery window
    /// must exceed 15 · (1 − 0.8) = 3 minutes. With a recovery threshold r = 0.5:
    /// 15 · (1 − 0.8) / (1 − 0.5) = 6 minutes.
    #[test]
    fn the_recovery_windows_lower_bound() {
        for (recovery, recovery_threshold, ok, bound) in [
            ("last_3m", "", false, "3 minutes"),
            ("last_4m", "", true, "3 minutes"),
            ("last_6m", r#","critical_recovery":0.5"#, false, "6 minutes"),
            ("last_7m", r#","critical_recovery":0.5"#, true, "6 minutes"),
        ] {
            let result = validate_with(
                DEFAULT_RETENTION,
                &anomaly("last_4h", "last_15m", 60, "0.8"),
                &windows("last_15m", recovery, &format!(r#""critical":0.8{recovery_threshold}"#)),
            );
            if ok {
                assert!(result.is_ok(), "{recovery}: {result:?}");
            } else {
                mentions(&format!("must be longer than {bound}"), &problems(result));
            }
        }
    }

    /// A recovery threshold of 1 is a problem, not a crash.
    #[test]
    fn anomaly_recovery_below_critical() {
        for options in [
            r#"{"thresholds":{"critical":0.5,"critical_recovery":1},"threshold_windows":{"trigger_window":"last_15m","recovery_window":"last_15m"}}"#,
            r#"{"thresholds":{"critical_recovery":0.5}}"#,
        ] {
            refused(&anomaly("last_4h", "last_15m", 60, "0.5"), options, "on the side the monitor recovers to");
        }
    }

    // --- forecast ----------------------------------------------------------------------------

    #[test]
    fn a_forecasts_threshold_windows_are_stored_not_acted_on() {
        let d = valid(
            "max(next_1w):forecast(avg:system.load.1{*}, 'linear', 1, interval='60m', history='1w', model='default') >= 3",
            r#"{"thresholds":{"critical":3,"critical_recovery":2},"threshold_windows":{"trigger_window":"last_1h","recovery_window":"last_1h"}}"#,
        );
        assert!(d.options.not_acted_on.contains(&"threshold_windows".to_string()));
    }

    // --- retention ---------------------------------------------------------------------------

    #[test]
    fn beyond_the_rollups_retention() {
        for (rollup_days, query, options, fragment) in [
            (7, "avg(last_2w):avg:x{*} > 1", "", "metrics are kept 7 days"),
            (3, "change(avg(last_2d),2d_ago):avg:x{*} > 1", "", "reads 4 days back"),
            (7, "max(next_1w):forecast(avg:x{*}, 'linear', 1, history='2w') >= 3", "", "reads 14 days back"),
            (400, "max(next_1w):forecast(avg:x{*}, 'linear', 1, history='999999w') >= 3", "", "metrics are kept 400 days"),
            (7, "max(next_1w):forecast(avg:x{*}, 'seasonal', 1) >= 3", "", "two seasons"),
            (7, "avg(last_5m):avg:x{*} > 1", r#"{"no_data_timeframe":20160}"#, "no_data_timeframe"),
        ] {
            let retention = Retention { raw: TimeDelta::days(2), rollup: TimeDelta::days(rollup_days) };
            mentions(fragment, &problems(validate_with(retention, query, options)));
        }
    }

    // --- the engine plans the query ----------------------------------------------------------

    #[test]
    fn what_the_engine_cannot_plan() {
        for (query, fragment) in [
            ("avg(last_5m):nosuchfn(avg:x{*}) > 1", "'nosuchfn' is not supported yet"),
            ("avg(last_5m):avg:x{*}.rollup() > 1", ".rollup() needs a method"),
            ("avg(last_5m):avg:a{*} by {host} / avg:b{*} by {env} > 1", "grouped by the same tags"),
        ] {
            refused(query, "", fragment);
        }
    }

    #[test]
    fn every_type_plans() {
        for query in [
            ABOVE_90.to_string(),
            "change(avg(last_5m),5m_ago):avg:x{*} by {host} > 10".to_string(),
            "pct_change(sum(last_1h),1d_ago):sum:x{*}.as_count() < -50".to_string(),
            anomaly("last_4h", "last_15m", 60, "1"),
            "max(next_1w):forecast(avg:x{*}, 'seasonal', 1, seasonality='daily') >= 3".to_string(),
            "avg(last_30m):outliers(avg:x{*} by {host}, 'scaledmad', 3, 20) > 0".to_string(),
        ] {
            valid(&query, "");
        }
    }

    // --- a monitor as JSON -------------------------------------------------------------------

    fn monitor_json(json: &str) -> Result<Monitor, Vec<String>> {
        monitor(DEFAULT_RETENTION, now(), json)
    }

    #[test]
    fn a_monitors_json_is_checked_by_its_type_query_and_options() {
        let json = r#"{"name":"cpu","type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} by {host} > 90",
            "message":"@ops","tags":["team:a"],"options":{"thresholds":{"warning":80},"renotify_interval":60}}"#;
        match monitor_json(json) {
            Ok(m) => {
                assert_eq!(m.options().thresholds.critical, Some(90.0));
                assert!(m.options().not_acted_on.contains(&"renotify_interval".to_string()));
            }
            Err(problems) => panic!("{}", problems.join("\n")),
        }
        assert!(monitor_json(r#"{"type":"metric alert","query":"avg(last_5m):avg:x{*} > 1","options":null}"#).is_ok());
    }

    #[test]
    fn json_that_is_no_monitor_is_a_problem() {
        mentions("not valid JSON", &problems(monitor_json("{")));
        mentions("must be a JSON object", &problems(monitor_json("[]")));

        let both = problems(monitor_json(r#"{"type":1}"#));
        mentions("type must be a string", &both);
        mentions("query must be a string", &both);

        mentions(
            "must equal the query's threshold",
            &problems(monitor_json(r#"{"type":"query alert","query":"avg(last_5m):avg:x{*} > 1","options":{"thresholds":{"critical":2}}}"#)),
        );
    }

    #[test]
    fn two_decimals_at_most() {
        assert_eq!(at_most_two_decimals(3.0), "3");
        assert_eq!(at_most_two_decimals(2.9999999999999996), "3");
        assert_eq!(at_most_two_decimals(0.5), "0.5");
        assert_eq!(at_most_two_decimals(1.256), "1.26");
    }
}
