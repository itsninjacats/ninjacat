//! A monitor's options, from Datadog's options object. (F#: `Monitor/Options.fs`.)
//!
//! Every option not acted on is accepted and named in `not_acted_on`, so a Datadog definition
//! loads unchanged; only the two that would change what is evaluated are refused.

use chrono::TimeDelta;
use serde_json::{Map, Value};

use crate::monitor::definition::{MissingData, Options, Thresholds, Window};
use crate::monitor::parse;

pub fn defaults() -> Options {
    Options {
        thresholds: Thresholds::default(),
        trigger_window: None,
        recovery_window: None,
        evaluation_delay: TimeDelta::zero(),
        new_group_delay: TimeDelta::seconds(60),
        require_full_window: false,
        notify_no_data: false,
        no_data_timeframe: None,
        on_missing_data: None,
        timeout: None,
        not_acted_on: vec![],
    }
}

const ACTED_ON: [&str; 10] = [
    "thresholds",
    "threshold_windows",
    "evaluation_delay",
    "new_group_delay",
    "new_host_delay",
    "require_full_window",
    "notify_no_data",
    "no_data_timeframe",
    "on_missing_data",
    "timeout_h",
];

/// A property, with null the same as absent.
fn property<'a>(name: &str, e: &'a Map<String, Value>) -> Option<&'a Value> {
    e.get(name).filter(|value| !value.is_null())
}

fn number(path: &str, e: &Value) -> Result<f64, String> {
    match e {
        Value::Number(n) => match n.as_f64() {
            Some(n) if n.is_finite() => Ok(n),
            _ => Err(format!("{path} is too large a number")),
        },
        _ => Err(format!("{path} must be a number")),
    }
}

fn whole_number(path: &str, low: i64, high: i64, e: &Value) -> Result<i64, String> {
    match number(path, e) {
        Ok(n) if n == n.floor() && n >= low as f64 && n <= high as f64 => Ok(n as i64),
        Ok(_) => Err(format!("{path} must be a whole number from {low} to {high}")),
        Err(e) => Err(e),
    }
}

fn day_of_seconds(path: &str, e: &Value) -> Result<i64, String> {
    whole_number(path, 0, 86400, e)
}

fn minutes(path: &str, e: &Value) -> Result<i64, String> {
    match number(path, e) {
        Ok(n) if n == n.floor() && n >= 1.0 && n <= f64::from(i32::MAX) => Ok(n as i64),
        Ok(_) => Err(format!("{path} must be a whole number of minutes, at least 1")),
        Err(e) => Err(e),
    }
}

fn boolean(path: &str, e: &Value) -> Result<bool, String> {
    match e {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("{path} must be true or false")),
    }
}

fn missing_data(path: &str, e: &Value) -> Result<MissingData, String> {
    match e.as_str().unwrap_or("") {
        "default" => Ok(MissingData::Default),
        "show_no_data" => Ok(MissingData::ShowNoData),
        "show_and_notify_no_data" => Ok(MissingData::ShowAndNotifyNoData),
        "resolve" => Ok(MissingData::Resolve),
        _ => Err(format!("{path} must be one of default, show_no_data, show_and_notify_no_data, resolve")),
    }
}

fn window(path: &str, e: &Value) -> Result<Window, String> {
    match e {
        Value::String(text) => parse::last_window(text).map_err(|problem| format!("{path}: {problem}")),
        _ => Err(format!("{path} must be a window like \"last_15m\"")),
    }
}

fn unknown_keys(path: &str, known: &[&str], e: &Map<String, Value>) -> Vec<String> {
    e.iter()
        .filter(|(name, value)| !value.is_null() && !known.contains(&name.as_str()))
        .map(|(name, _)| format!("{path} has no '{name}'"))
        .collect()
}

/// A value with a problem reads as absent; the problem is noted.
fn read<T>(
    problems: &mut Vec<String>,
    parent: &str,
    name: &str,
    reader: fn(&str, &Value) -> Result<T, String>,
    e: &Map<String, Value>,
) -> Option<T> {
    let value = property(name, e)?;
    match reader(&format!("{parent}.{name}"), value) {
        Ok(v) => Some(v),
        Err(problem) => {
            problems.push(problem);
            None
        }
    }
}

fn object_at<'a>(
    problems: &mut Vec<String>,
    root: &'a Map<String, Value>,
    name: &str,
    known: &[&str],
) -> Option<&'a Map<String, Value>> {
    let path = format!("options.{name}");
    match property(name, root) {
        None => None,
        Some(Value::Object(e)) => {
            problems.extend(unknown_keys(&path, known, e));
            Some(e)
        }
        Some(_) => {
            problems.push(format!("{path} must be an object"));
            None
        }
    }
}

fn of_object(root: &Map<String, Value>) -> Result<Options, Vec<String>> {
    let mut problems = Vec::new();

    let present: Vec<&str> = root.iter().filter(|(_, value)| !value.is_null()).map(|(name, _)| name.as_str()).collect();

    for name in &present {
        match *name {
            "scheduling_options" => {
                problems.push("options.scheduling_options: custom schedules and cumulative windows are not supported yet".to_string())
            }
            "variables" => problems
                .push("options.variables: formula and function monitors over other data sources are not supported yet".to_string()),
            _ => {}
        }
    }

    let known_thresholds = ["critical", "warning", "critical_recovery", "warning_recovery", "ok", "unknown"];
    let thresholds = match object_at(&mut problems, root, "thresholds", &known_thresholds) {
        None => defaults().thresholds,
        Some(e) => {
            let p = "options.thresholds";
            Thresholds {
                critical: read(&mut problems, p, "critical", number, e),
                warning: read(&mut problems, p, "warning", number, e),
                critical_recovery: read(&mut problems, p, "critical_recovery", number, e),
                warning_recovery: read(&mut problems, p, "warning_recovery", number, e),
                ok: read(&mut problems, p, "ok", number, e),
                unknown: read(&mut problems, p, "unknown", number, e),
            }
        }
    };

    let windows = object_at(&mut problems, root, "threshold_windows", &["trigger_window", "recovery_window"]);
    let trigger_window = windows.and_then(|e| read(&mut problems, "options.threshold_windows", "trigger_window", window, e));
    let recovery_window = windows.and_then(|e| read(&mut problems, "options.threshold_windows", "recovery_window", window, e));

    let evaluation_delay = read(&mut problems, "options", "evaluation_delay", day_of_seconds, root);
    // WARNING(undocumented): Datadog documents no cap; ours is a day.
    let new_group_delay = read(&mut problems, "options", "new_group_delay", day_of_seconds, root);
    let new_host_delay = read(&mut problems, "options", "new_host_delay", day_of_seconds, root);
    let require_full_window = read(&mut problems, "options", "require_full_window", boolean, root);
    let notify_no_data = read(&mut problems, "options", "notify_no_data", boolean, root);
    // Minutes, as Datadog counts it. The longest is the retention's, which validate checks.
    let no_data_timeframe = read(&mut problems, "options", "no_data_timeframe", minutes, root);
    let on_missing_data = read(&mut problems, "options", "on_missing_data", missing_data, root);
    let timeout_h = read(&mut problems, "options", "timeout_h", |p, e| whole_number(p, 0, 24, e), root);

    // WARNING(undocumented): on_missing_data replaces the older two, so both together are refused;
    // an explicit notify_no_data false is accepted, as a definition read back from Datadog may
    // carry it.
    if on_missing_data.is_some() && (notify_no_data == Some(true) || no_data_timeframe.is_some()) {
        problems.push("options.on_missing_data replaces notify_no_data and no_data_timeframe; set one or the other".to_string());
    }

    if !problems.is_empty() {
        return Err(problems);
    }

    // new_host_delay is the deprecated name; new_group_delay wins.
    let new_group_delay = match new_group_delay.or(new_host_delay) {
        Some(s) => TimeDelta::seconds(s),
        None => defaults().new_group_delay,
    };

    Ok(Options {
        thresholds,
        trigger_window,
        recovery_window,
        evaluation_delay: evaluation_delay.map_or(TimeDelta::zero(), TimeDelta::seconds),
        new_group_delay,
        require_full_window: require_full_window.unwrap_or(false),
        notify_no_data: notify_no_data.unwrap_or(false),
        no_data_timeframe: no_data_timeframe.map(TimeDelta::minutes),
        on_missing_data,
        timeout: match timeout_h {
            None | Some(0) => None,
            Some(h) => Some(TimeDelta::hours(h)),
        },
        not_acted_on: present.into_iter().filter(|name| !ACTED_ON.contains(name)).map(str::to_string).collect(),
    })
}

/// The options as JSON text, as Postgres and the panel hold them. Absent or null is every default.
pub fn parse(json: &str) -> Result<Options, Vec<String>> {
    if json.trim().is_empty() {
        return Ok(defaults());
    }
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Null) => Ok(defaults()),
        Ok(Value::Object(root)) => of_object(&root),
        Ok(_) => Err(vec!["options must be an object".to_string()]),
        Err(e) => Err(vec![format!("options are not valid JSON: {e}")]),
    }
}
