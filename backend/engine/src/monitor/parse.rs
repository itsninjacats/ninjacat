//! A metric monitor's query.
//!
//! ```text
//! threshold  avg(last_5m):<expr> > 90
//! change     change(avg(last_5m),last_5m):<expr> > 10
//! anomaly    avg(last_4h):anomalies(<q>, 'agile', 2, alert_window='last_15m', …) >= 1
//! forecast   max(next_1w):forecast(<q>, 'linear', 1, history='1w') >= 90
//! outlier    avg(last_30m):outliers(<q> by {host}, 'dbscan', 7) > 0
//! ```
//!
//! The syntax is read first and each method's rules after, so a refusal says what is wrong rather
//! than where a parser stopped.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use chrono::TimeDelta;
use ninjapaws_anomaly::{Anomalies, Forecast};
use regex::Regex;
use winnow::combinator::{alt, delimited, opt, separated, terminated};
use winnow::error::{ContextError, ErrMode, StrContext, StrContextValue};
use winnow::token::take_while;
use winnow::{ModalResult, Parser};

use crate::duration;
use crate::metric_query::ast::{Arg, Expr, Literal, MetricQuery};
use crate::metric_query::parser::{self, ws};
use crate::monitor::definition::{
    AnomalySpec, ChangeMeasure, ChangeSpec, Comparator, Detection, Direction, ForecastSpec, MonitorQuery, OutlierSpec,
    TimeAggregator, Window, WindowUnit,
};
use crate::query::engine;
use crate::query::plan;

// --- syntax ----------------------------------------------------------------------------------

struct Head {
    name: String,
    args: Vec<HeadArg>,
}

enum HeadArg {
    Inner(Head),
    Plain(String),
}

struct Syntax {
    head: Head,
    expression: Expr<MetricQuery>,
    comparator: Comparator,
    threshold: f64,
}

fn expected(description: &'static str) -> StrContext {
    StrContext::Expected(StrContextValue::Description(description))
}

fn word(input: &mut &str) -> ModalResult<String> {
    take_while(1.., |c: char| c.is_alphabetic() || c.is_ascii_digit() || c == '_')
        .map(str::to_string)
        .context(expected("a name"))
        .parse_next(input)
}

fn head_of<'i>(
    argument: impl Parser<&'i str, HeadArg, ErrMode<ContextError>>,
) -> impl Parser<&'i str, Head, ErrMode<ContextError>> {
    let args = delimited(('(', ws), separated(0.., terminated(argument, ws), (',', ws)), ')');
    (word, ws, args).map(|(name, _, args)| Head { name, args })
}

fn inner_head(input: &mut &str) -> ModalResult<Head> {
    head_of(word.map(HeadArg::Plain)).parse_next(input)
}

/// `last_5m` reads as a name too; with no `(` after it, it is read as a word.
fn head(input: &mut &str) -> ModalResult<Head> {
    head_of(alt((inner_head.map(HeadArg::Inner), word.map(HeadArg::Plain)))).parse_next(input)
}

fn comparator(input: &mut &str) -> ModalResult<Comparator> {
    alt((
        ">=".value(Comparator::Ge),
        "<=".value(Comparator::Le),
        "==".value(Comparator::Eq),
        "!=".value(Comparator::Ne),
        ">".value(Comparator::Gt),
        "<".value(Comparator::Lt),
    ))
    .context(expected("a comparator: <, <=, >, >=, == or !="))
    .parse_next(input)
}

fn threshold(input: &mut &str) -> ModalResult<f64> {
    (opt('-'), parser::number)
        .map(|(minus, n)| if minus.is_some() { -n } else { n })
        .context(expected("a threshold"))
        .parse_next(input)
}

fn syntax(input: &mut &str) -> ModalResult<Syntax> {
    let colon = ':'.context(StrContext::Expected(StrContextValue::CharLiteral(':')));
    (ws, head, ws, colon, ws, parser::expression, ws, comparator, ws, threshold, ws)
        .map(|(_, head, _, _, _, expression, _, comparator, _, threshold, _)| Syntax {
            head,
            expression,
            comparator,
            threshold,
        })
        .parse_next(input)
}

fn read_syntax(text: &str) -> Result<Syntax, String> {
    parser::run(text, syntax)
}

// --- windows ---------------------------------------------------------------------------------

// A final newline is taken, as it always has been.
static WINDOW_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(last|next)_(\d+)(mo|m|h|d|w)\n?$").unwrap());
static AGO_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)(m|h|d)_ago\n?$").unwrap());

fn unit_of(text: &str) -> WindowUnit {
    match text {
        "m" => WindowUnit::Minutes,
        "h" => WindowUnit::Hours,
        "d" => WindowUnit::Days,
        "w" => WindowUnit::Weeks,
        _ => WindowUnit::Months,
    }
}

/// The cap keeps a window's length far inside what a span holds; every window taken is far
/// shorter.
fn count_of(text: &str) -> Option<i32> {
    text.parse::<i32>().ok().filter(|n| *n > 0 && *n <= 100_000)
}

fn read_window(text: &str) -> Option<(String, Window)> {
    let m = WINDOW_PATTERN.captures(text)?;
    let count = count_of(&m[2])?;
    Some((m[1].to_string(), Window { count, unit: unit_of(&m[3]) }))
}

const MONTH: Window = Window { count: 1, unit: WindowUnit::Months };
const LONGEST_HORIZON: Window = Window { count: 3, unit: WindowUnit::Months };

/// A `last_…` window: `last_5m`, `last_4h`, `last_2d`, `last_1w`, `last_1mo`.
///
/// WARNING(undocumented): which spellings Datadog takes beyond its examples. Any whole count of
/// m, h, d or w is read, and mo only as `last_1mo`.
pub fn last_window(text: &str) -> Result<Window, String> {
    if text.starts_with("current_") {
        return Err(format!("cumulative windows ('{text}') are not supported yet"));
    }
    match read_window(text) {
        Some((direction, w)) if direction == "last" && w.unit == WindowUnit::Months && w.count != 1 => {
            Err(format!("'{text}': the only month window is last_1mo"))
        }
        Some((direction, w)) if direction == "last" && w.length() > MONTH.length() => {
            Err(format!("'{text}' is longer than last_1mo, the longest window"))
        }
        Some((direction, w)) if direction == "last" => Ok(w),
        Some(_) => Err(format!("'{text}' is a forecast horizon; other monitors look back, e.g. last_5m")),
        None => Err(format!("'{text}' is not a window; write it like last_5m, last_4h, last_2d, last_1w or last_1mo")),
    }
}

fn change_window_error(text: &str) -> String {
    format!("'{text}': a change monitor looks at most two days back (minutes up to 2880, hours up to 48, days up to 2)")
}

fn within_two_days(w: &Window) -> bool {
    w.length() <= TimeDelta::days(2)
}

fn change_window(text: &str) -> Result<Window, String> {
    match last_window(text) {
        Ok(w) if within_two_days(&w) => Ok(w),
        Ok(_) => Err(change_window_error(text)),
        Err(e) => Err(e),
    }
}

/// `5m_ago`, `1h_ago`, `1d_ago`, or a window spelled `last_5m`.
///
/// WARNING(undocumented): Datadog documents 5, 10, 15, 30 minutes, 1, 2, 4 hours and 1 day. Any
/// shift within two days is accepted, as for the window.
fn timeshift(text: &str) -> Result<Window, String> {
    let Some(m) = AGO_PATTERN.captures(text) else {
        return change_window(text);
    };
    match count_of(&m[1]).map(|count| Window { count, unit: unit_of(&m[2]) }) {
        Some(shift) if within_two_days(&shift) => Ok(shift),
        _ => Err(change_window_error(text)),
    }
}

/// `next_12h` to `next_3mo`.
fn horizon(text: &str) -> Result<Window, String> {
    match read_window(text) {
        Some((direction, w))
            if direction == "next"
                && w.unit != WindowUnit::Minutes
                && w.length() >= TimeDelta::hours(12)
                && w.length() <= LONGEST_HORIZON.length() =>
        {
            Ok(w)
        }
        Some((direction, _)) if direction == "next" => {
            Err(format!("'{text}': a forecast looks 12 hours to 3 months ahead, from next_12h to next_3mo"))
        }
        _ => Err(format!("'{text}': a forecast monitor's window is a horizon, e.g. max(next_1w)")),
    }
}

// --- the expression --------------------------------------------------------------------------

/// The functions an expression calls, outermost first.
pub fn function_names(e: &Expr<MetricQuery>) -> Vec<String> {
    match e {
        Expr::Leaf(_) | Expr::Number(_) => vec![],
        Expr::Neg(inner) => function_names(inner),
        Expr::Binary(_, l, r) => {
            let mut names = function_names(l);
            names.extend(function_names(r));
            names
        }
        Expr::Call(name, args) => {
            let mut names = vec![name.clone()];
            for arg in args {
                if let Arg::Value(v) = arg {
                    names.extend(function_names(v));
                }
            }
            names
        }
    }
}

/// A count across series is one series, whatever went in.
fn group_keys(e: &Expr<MetricQuery>) -> BTreeSet<String> {
    match e {
        Expr::Leaf(q) => q.group_by.iter().cloned().collect(),
        Expr::Number(_) => BTreeSet::new(),
        Expr::Neg(inner) => group_keys(inner),
        Expr::Binary(_, l, r) => group_keys(l).union(&group_keys(r)).cloned().collect(),
        Expr::Call(name, _) if matches!(name.as_str(), "count_nonzero" | "count_nonzero_finite" | "count_not_null") => {
            BTreeSet::new()
        }
        Expr::Call(_, args) => args
            .iter()
            .flat_map(|arg| match arg {
                Arg::Value(v) => group_keys(v),
                Arg::Lit(_) | Arg::Named(_, _) => BTreeSet::new(),
            })
            .collect(),
    }
}

const DETECTION_FUNCTIONS: [&str; 3] = ["anomalies", "forecast", "outliers"];

/// "Not allowed inside" anomalies() and forecast(), per Datadog's docs.
const REFUSED_IN_MODELS: [&str; 6] = ["cumsum", "integral", "outliers", "piecewise_constant", "robust_trend", "trend_line"];

fn problem_of<T>(result: &Result<T, String>) -> Option<String> {
    result.as_ref().err().cloned()
}

// --- arguments only a monitor reads; the engine's readers take the rest ------------------------

fn duplicates(function: &str, named: &[(String, Literal)]) -> Vec<String> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for (name, _) in named {
        match seen.iter_mut().find(|(n, _)| n == name) {
            Some((_, count)) => *count += 1,
            None => seen.push((name, 1)),
        }
    }
    seen.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(name, _)| format!("{function}() is given '{name}' more than once"))
        .collect()
}

/// Seconds (`interval=60`), or a duration (`interval='60m'`, as Datadog's forecast examples write
/// it). Never longer than the longest horizon.
fn interval_arg(function: &str, named: &[(String, Literal)]) -> Result<Option<TimeDelta>, String> {
    let longest = LONGEST_HORIZON.length();
    let fits = |span: TimeDelta| span >= TimeDelta::seconds(1) && span <= longest && span.subsec_nanos() == 0;
    let invalid = || format!("{function}() interval is a whole number of seconds up to 93 days, e.g. interval=60");

    match plan::named_arg("interval", named) {
        None => Ok(None),
        Some(Literal::Num(seconds)) if *seconds >= 1.0 && *seconds <= longest.num_seconds() as f64 && *seconds == seconds.floor() => {
            Ok(Some(TimeDelta::seconds(*seconds as i64)))
        }
        Some(Literal::Word(text) | Literal::Quoted(text)) => match duration::parse(text) {
            Ok(span) if fits(span) => Ok(Some(span)),
            _ => Err(invalid()),
        },
        Some(Literal::Num(_)) => Err(invalid()),
    }
}

fn timezone_arg(function: &str, named: &[(String, Literal)]) -> Result<Option<String>, String> {
    match plan::named_arg("timezone", named) {
        None => Ok(None),
        Some(Literal::Word(zone) | Literal::Quoted(zone)) => match zone.parse::<chrono_tz::Tz>() {
            Ok(_) => Ok(Some(zone.clone())),
            Err(_) => Err(format!("{function}() timezone '{zone}' is not a known IANA zone")),
        },
        Some(Literal::Num(_)) => Err(format!("{function}() timezone is a zone name, e.g. timezone='Europe/Warsaw'")),
    }
}

// --- detection methods -----------------------------------------------------------------------

fn anomaly(rest: &[Arg<String>]) -> Result<AnomalySpec, Vec<String>> {
    let function = "anomalies";
    let (_, named) = plan::split_args(rest);
    let word_of = |name: &str| plan::named_arg(name, &named).map(|l| plan::word(l).map(str::to_string));

    let algorithm = match plan::anomalies_args(rest) {
        Ok((Anomalies::Basic, _)) if plan::named_arg("seasonality", &named).is_some() => {
            Err("anomalies() 'basic' has no seasonality; it compares each point with the ones just before it".to_string())
        }
        result => result,
    };

    let direction = match word_of("direction") {
        None => Ok(Direction::Both),
        Some(Some(w)) if w == "both" => Ok(Direction::Both),
        Some(Some(w)) if w == "above" => Ok(Direction::Above),
        Some(Some(w)) if w == "below" => Ok(Direction::Below),
        Some(_) => Err("anomalies() direction must be 'both', 'above' or 'below'".to_string()),
    };

    // WARNING(undocumented): Datadog's anomaly queries always carry alert_window; without it there
    // is no window to judge, so it is required.
    let alert_window = match word_of("alert_window") {
        None => Err("an anomaly monitor needs alert_window, e.g. anomalies(query, 'agile', 2, alert_window='last_15m')".to_string()),
        Some(Some(text)) => last_window(&text).map_err(|e| format!("anomalies() alert_window {e}")),
        Some(None) => Err("anomalies() alert_window is a window, e.g. alert_window='last_15m'".to_string()),
    };

    let interval = interval_arg(function, &named);

    let count_default_zero = match word_of("count_default_zero") {
        None => Ok(false),
        Some(Some(w)) if w == "false" => Ok(false),
        Some(Some(w)) if w == "true" => Ok(true),
        Some(_) => Err("anomalies() count_default_zero is 'true' or 'false'".to_string()),
    };

    let timezone = timezone_arg(function, &named);

    let duplicated = duplicates(function, &named);
    match (&algorithm, &direction, &alert_window, &interval, &count_default_zero, &timezone) {
        (Ok((algorithm, bounds)), Ok(direction), Ok(alert_window), Ok(interval), Ok(count_default_zero), Ok(timezone))
            if duplicated.is_empty() =>
        {
            Ok(AnomalySpec {
                algorithm: *algorithm,
                bounds: *bounds,
                direction: *direction,
                alert_window: *alert_window,
                interval: *interval,
                count_default_zero: *count_default_zero,
                timezone: timezone.clone(),
            })
        }
        _ => {
            let mut problems = duplicated;
            problems.extend(
                [
                    problem_of(&algorithm),
                    problem_of(&direction),
                    problem_of(&alert_window),
                    problem_of(&interval),
                    problem_of(&count_default_zero),
                    problem_of(&timezone),
                ]
                .into_iter()
                .flatten(),
            );
            Err(problems)
        }
    }
}

fn forecast(rest: &[Arg<String>]) -> Result<ForecastSpec, Vec<String>> {
    let function = "forecast";
    let (_, named) = plan::split_args(rest);
    let has = |name: &str| plan::named_arg(name, &named).is_some();

    let method = match plan::forecast_args(rest) {
        Err(e) => Err(e),
        Ok((_, _, _, Some(_))) => Err("forecast() takes no horizon in a monitor; the window sets it, e.g. max(next_1w)".to_string()),
        Ok((deviations, _, _, _)) if deviations < 1.0 => {
            Err("forecast() deviations must be at least 1: forecast(query, 'linear', 1)".to_string())
        }
        Ok((_, Forecast::Linear(_), _, _)) if has("seasonality") => {
            Err("forecast() 'linear' takes no seasonality; it is for 'seasonal'".to_string())
        }
        Ok((_, Forecast::Seasonal(_), _, _)) if has("model") || has("history") => {
            Err("forecast() 'seasonal' takes no model or history; they are for 'linear'".to_string())
        }
        Ok((deviations, method, history, None)) => Ok((deviations, method, history)),
    };

    let interval = interval_arg(function, &named);
    let timezone = timezone_arg(function, &named);

    let duplicated = duplicates(function, &named);
    match (&method, &interval, &timezone) {
        (Ok((deviations, method, history)), Ok(interval), Ok(timezone)) if duplicated.is_empty() => Ok(ForecastSpec {
            method: *method,
            deviations: *deviations,
            interval: *interval,
            history: *history,
            timezone: timezone.clone(),
        }),
        _ => {
            let mut problems = duplicated;
            problems.extend([problem_of(&method), problem_of(&interval), problem_of(&timezone)].into_iter().flatten());
            Err(problems)
        }
    }
}

fn outlier(rest: &[Arg<String>]) -> Result<OutlierSpec, Vec<String>> {
    let (positional, named) = plan::split_args(rest);
    if let Some((name, _)) = named.first() {
        return Err(vec![format!("outliers() has no argument '{name}'")]);
    }
    // A query where a number belongs is not a literal; outlier_args refuses it.
    let literals: Vec<Literal> =
        if positional.iter().all(Option::is_some) { positional.into_iter().flatten().collect() } else { vec![] };

    match plan::outlier_args(&literals) {
        Ok((algorithm, tolerance)) => Ok(OutlierSpec { algorithm, tolerance }),
        Err(e) => Err(vec![e]),
    }
}

// --- the whole query -------------------------------------------------------------------------

fn aggregator(name: &str) -> Result<TimeAggregator, String> {
    match name {
        "avg" => Ok(TimeAggregator::Avg),
        "sum" => Ok(TimeAggregator::Sum),
        "min" => Ok(TimeAggregator::Min),
        "max" => Ok(TimeAggregator::Max),
        "percentile" => Err("the percentile time aggregation is not supported yet".to_string()),
        other => Err(format!("unknown time aggregation '{other}'; use avg, sum, min or max")),
    }
}

type HeadParts<'a> = (&'a str, &'a str, Option<(ChangeMeasure, &'a str)>);

fn read_head(h: &Head) -> Result<HeadParts<'_>, String> {
    let is_change = h.name == "change" || h.name == "pct_change";
    let measure = if h.name == "change" { ChangeMeasure::Absolute } else { ChangeMeasure::Percent };

    match h.args.as_slice() {
        [HeadArg::Inner(inner), HeadArg::Plain(shift)] if is_change => match inner.args.as_slice() {
            [HeadArg::Plain(window)] => Ok((&inner.name, window, Some((measure, shift)))),
            _ => Err(format!("{0}() takes a time aggregation and a timeshift: {0}(avg(last_5m),5m_ago)", h.name)),
        },
        _ if is_change => Err(format!("{0}() takes a time aggregation and a timeshift: {0}(avg(last_5m),5m_ago)", h.name)),
        [HeadArg::Plain(window)] => Ok((&h.name, window, None)),
        _ => Err(format!(
            "the query starts with a time aggregation and a window, e.g. avg(last_5m), not {}(…)",
            h.name
        )),
    }
}

/// The arguments come back as the engine's formula holds them, for the engine's own readers.
fn outer_function(e: &Expr<MetricQuery>) -> Option<(String, Expr<MetricQuery>, Vec<Arg<String>>)> {
    let Expr::Call(name, args) = e else {
        return None;
    };
    let Some(Arg::Value(inner)) = args.first() else {
        return None;
    };
    if !DETECTION_FUNCTIONS.contains(&name.as_str()) {
        return None;
    }
    match engine::split(e).1 {
        Expr::Call(_, formula_args) => match formula_args.split_first() {
            Some((Arg::Value(_), rest)) => Some((name.clone(), inner.clone(), rest.to_vec())),
            _ => None,
        },
        _ => None,
    }
}

fn build(s: &Syntax, aggregator: TimeAggregator, window: Window, detection: Detection, measured: Expr<MetricQuery>) -> MonitorQuery {
    let as_count = engine::split(&measured).0.iter().any(|p| p.query.modifiers.iter().any(|m| m.name == "as_count"));
    MonitorQuery {
        aggregator,
        window,
        detection,
        expression: s.expression.clone(),
        group_by: group_keys(&measured).into_iter().collect(),
        measured,
        as_count,
        comparator: s.comparator,
        threshold: s.threshold,
    }
}

fn interpret(s: &Syntax, aggregator_name: &str, window_text: &str, change: Option<(ChangeMeasure, &str)>) -> Result<MonitorQuery, Vec<String>> {
    let aggregator = aggregator(aggregator_name).map_err(|e| vec![e])?;
    match (change, outer_function(&s.expression)) {
        (Some(_), Some((name, _, _))) => Err(vec![format!("{name}() cannot be combined with change() or pct_change()")]),
        (Some((measure, shift_text)), None) => match (change_window(window_text), timeshift(shift_text)) {
            (Ok(window), Ok(shift)) => {
                Ok(build(s, aggregator, window, Detection::Change(ChangeSpec { measure, shift }), s.expression.clone()))
            }
            (window, shift) => Err([problem_of(&window), problem_of(&shift)].into_iter().flatten().collect()),
        },
        (None, None) => match last_window(window_text) {
            Ok(window) => Ok(build(s, aggregator, window, Detection::Threshold, s.expression.clone())),
            Err(e) => Err(vec![e]),
        },
        (None, Some((name, inner, rest))) => {
            let window = if name == "forecast" { horizon(window_text) } else { last_window(window_text) };
            let detection = match name.as_str() {
                "anomalies" => anomaly(&rest).map(Detection::Anomaly),
                "forecast" => forecast(&rest).map(Detection::Forecast),
                _ => outlier(&rest).map(Detection::Outlier),
            };
            match (window, detection) {
                (Ok(window), Ok(detection)) => Ok(build(s, aggregator, window, detection, inner)),
                (Err(e), Ok(_)) => Err(vec![e]),
                (window, Err(problems)) => Err(problem_of(&window).into_iter().chain(problems).collect()),
            }
        }
    }
}

fn model_problems(model: &str, q: &MonitorQuery, inside: &[String]) -> Vec<String> {
    let mut problems: Vec<String> = REFUSED_IN_MODELS
        .iter()
        .filter(|name| inside.iter().any(|i| i == *name))
        .map(|name| format!("{name}() is not allowed inside {model}()"))
        .collect();
    if q.as_count {
        problems.push(format!("{model}() monitors do not take .as_count() queries"));
    }
    problems
}

fn query_problems(q: &MonitorQuery) -> Vec<String> {
    let inside = function_names(&q.measured);

    let by_method = match &q.detection {
        Detection::Anomaly(_) => model_problems("anomalies", q, &inside),
        Detection::Forecast(_) => model_problems("forecast", q, &inside),
        Detection::Outlier(_) => {
            let mut problems = vec![];
            // WARNING(undocumented): Datadog is silent on as_count in outlier monitors; refused, as
            // for anomalies and forecasts.
            if q.as_count {
                problems.push("outlier monitors do not take .as_count() queries".to_string());
            }
            if q.group_by.is_empty() {
                problems.push(
                    "an outlier monitor compares groups, so its query needs a by {…}, e.g. outliers(avg:system.cpu.user{*} by {host}, 'dbscan', 3)"
                        .to_string(),
                );
            }
            problems
        }
        Detection::Threshold | Detection::Change(_) => {
            if q.as_count && q.aggregator != TimeAggregator::Sum {
                vec![
                    "with .as_count() the time aggregation must be sum: avg, min and max cannot be used with .as_count()"
                        .to_string(),
                ]
            } else {
                vec![]
            }
        }
    };

    // outliers() inside a model is already refused by name.
    let refused_by_model: &[&str] = match &q.detection {
        Detection::Anomaly(_) | Detection::Forecast(_) => &REFUSED_IN_MODELS,
        Detection::Threshold | Detection::Change(_) | Detection::Outlier(_) => &[],
    };

    let nested = DETECTION_FUNCTIONS
        .iter()
        .filter(|name| inside.iter().any(|i| i == *name) && !refused_by_model.contains(name))
        .map(|name| format!("{name}() must be the outermost function of the monitor's query"));

    let comparison: Vec<String> = match (&q.detection, q.comparator, q.aggregator) {
        (Detection::Threshold | Detection::Change(_), _, _) => vec![],
        // WARNING(undocumented): Datadog's anomaly queries all end in `>=`; the other comparators
        // are refused.
        (Detection::Anomaly(_), Comparator::Ge, _) if q.threshold > 0.0 && q.threshold <= 1.0 => vec![],
        (Detection::Anomaly(_), Comparator::Ge, _) => vec![format!(
            "an anomaly monitor's threshold is the fraction of anomalous points, above 0 and at most 1; got {}",
            q.threshold
        )],
        (Detection::Anomaly(_), _, _) => vec!["an anomaly monitor compares with >=, e.g. anomalies(…) >= 1".to_string()],
        (Detection::Forecast(_), Comparator::Ge, TimeAggregator::Max) | (Detection::Forecast(_), Comparator::Le, TimeAggregator::Min) => {
            vec![]
        }
        (Detection::Forecast(_), _, _) => vec![
            "a forecast monitor is max(next_…):forecast(…) >= threshold, or min(next_…):forecast(…) <= threshold".to_string(),
        ],
        (Detection::Outlier(_), Comparator::Gt, _) if q.threshold == 0.0 => vec![],
        (Detection::Outlier(_), _, _) => vec!["an outlier monitor ends in > 0".to_string()],
    };

    nested.chain(by_method).chain(comparison).collect()
}

/// Whether the inner query plans, and how the options fit it, is `validate`'s.
pub fn query(text: &str) -> Result<MonitorQuery, Vec<String>> {
    let s = read_syntax(text).map_err(|message| vec![message])?;
    let (aggregator_name, window_text, change) = read_head(&s.head).map_err(|e| vec![e])?;
    let q = interpret(&s, aggregator_name, window_text, change)?;
    match query_problems(&q) {
        problems if problems.is_empty() => Ok(q),
        problems => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use ninjapaws_anomaly::{LinearModel, Outliers, Seasonality};

    use super::*;
    use crate::metric_query::ast::TagFilter;

    fn parsed(text: &str) -> MonitorQuery {
        match query(text) {
            Ok(q) => q,
            Err(problems) => panic!("'{text}' was refused:\n{}", problems.join("\n")),
        }
    }

    fn refused(text: &str, fragment: &str) {
        match query(text) {
            Ok(q) => panic!("'{text}' should be refused, got {q:?}"),
            Err(problems) => assert!(
                problems.iter().any(|p| p.contains(fragment)),
                "'{text}': no problem mentions '{fragment}':\n{}",
                problems.join("\n")
            ),
        }
    }

    fn leaf(agg: &str, metric: &str, filter: TagFilter, groups: &[&str]) -> Expr<MetricQuery> {
        Expr::Leaf(MetricQuery {
            space_agg: Some(agg.to_string()),
            metric: metric.to_string(),
            filter,
            group_by: groups.iter().map(|g| g.to_string()).collect(),
            modifiers: vec![],
        })
    }

    fn tag(k: &str, v: &str) -> TagFilter {
        TagFilter::Tag(k.to_string(), v.to_string())
    }

    fn minutes(count: i32) -> Window {
        Window { count, unit: WindowUnit::Minutes }
    }

    // --- threshold ---------------------------------------------------------------------------

    #[test]
    fn threshold_datadogs_create_a_monitor_example() {
        let q = parsed("avg(last_5m):sum:system.net.bytes_rcvd{host:host0} > 100");
        assert_eq!(q.aggregator, TimeAggregator::Avg);
        assert_eq!(q.window, minutes(5));
        assert_eq!(q.detection, Detection::Threshold);
        assert_eq!(q.expression, leaf("sum", "system.net.bytes_rcvd", tag("host", "host0"), &[]));
        assert_eq!(q.measured, q.expression);
        assert!(q.group_by.is_empty());
        assert_eq!(q.comparator, Comparator::Gt);
        assert_eq!(q.threshold, 100.0);
    }

    #[test]
    fn by_makes_a_multi_alert_its_keys_sorted() {
        let q = parsed("avg(last_1h):avg:system.cpu.user{env:prod} by {host,env} >= 90.5");
        assert_eq!(q.group_by, vec!["env", "host"]);
        assert_eq!(q.window, Window { count: 1, unit: WindowUnit::Hours });
        assert_eq!(q.comparator, Comparator::Ge);
        assert_eq!(q.threshold, 90.5);
    }

    #[test]
    fn a_formula_of_as_count_queries_with_sum() {
        // Datadog's APM error-rate monitor.
        let q = parsed(
            "sum(last_5m):sum:trace.http.request.errors{service:web}.as_count() / sum:trace.http.request.hits{service:web}.as_count() > 0.05",
        );
        assert!(q.as_count);
        assert_eq!(q.aggregator, TimeAggregator::Sum);
    }

    #[test]
    fn no_spaces_a_negative_threshold() {
        let q = parsed("min(last_1d):avg:x{*}<-5");
        assert_eq!(q.comparator, Comparator::Lt);
        assert_eq!(q.threshold, -5.0);
    }

    #[test]
    fn functions_and_filters_the_engine_takes() {
        let q = parsed("max(last_15m):abs(diff(avg:x{env:prod AND NOT host:a} by {host})) != 0");
        assert_eq!(q.group_by, vec!["host"]);
        assert_eq!(q.comparator, Comparator::Ne);
    }

    #[test]
    fn count_functions_leave_one_group() {
        assert!(parsed("avg(last_5m):count_nonzero(avg:x{*} by {host}) < 3").group_by.is_empty());
    }

    #[test]
    fn windows_up_to_a_month() {
        for window in ["last_1mo", "last_31d", "last_4w", "last_44640m"] {
            parsed(&format!("avg({window}):avg:x{{*}} > 1"));
        }
    }

    #[test]
    fn windows_refused() {
        for (window, fragment) in [
            ("last_5w", "longer than last_1mo"),
            ("last_32d", "longer than last_1mo"),
            ("last_2mo", "only month window"),
            ("current_1d", "cumulative windows"),
            ("next_1w", "forecast horizon"),
            ("5m", "is not a window"),
            ("last_0m", "is not a window"),
        ] {
            refused(&format!("avg({window}):avg:x{{*}} > 1"), fragment);
        }
    }

    #[test]
    fn as_count_takes_only_sum() {
        for agg in ["avg", "min", "max"] {
            refused(&format!("{agg}(last_5m):sum:requests{{*}}.as_count() > 1"), "must be sum");
        }
    }

    #[test]
    fn other_refusals() {
        for (text, fragment) in [
            ("percentile(last_5m):p99:request.latency{*} > 1", "percentile"),
            ("last(last_5m):avg:x{*} > 1", "unknown time aggregation 'last'"),
            ("avg(last_5m):avg:x{*}", "comparator"),
            ("avg(last_5m):outliers(avg:x{*} by {host}, 'dbscan', 3) * 2 > 0", "outermost"),
            (
                "change(avg(last_5m),5m_ago):anomalies(avg:x{*}, 'basic', 2, alert_window='last_5m') >= 1",
                "cannot be combined",
            ),
        ] {
            refused(text, fragment);
        }
    }

    // --- change ------------------------------------------------------------------------------

    #[test]
    fn change_over_the_window_before() {
        let q = parsed("change(avg(last_5m),last_5m):avg:system.load.1{*} by {host} > 10");
        assert_eq!(q.detection, Detection::Change(ChangeSpec { measure: ChangeMeasure::Absolute, shift: minutes(5) }));
        assert_eq!(q.window, minutes(5));
        assert_eq!(q.aggregator, TimeAggregator::Avg);
        assert_eq!(q.group_by, vec!["host"]);
    }

    #[test]
    fn pct_change_with_an_ago_shift() {
        let q = parsed("pct_change(avg(last_5m),1h_ago):avg:system.load.1{*} < -50");
        assert_eq!(
            q.detection,
            Detection::Change(ChangeSpec { measure: ChangeMeasure::Percent, shift: Window { count: 1, unit: WindowUnit::Hours } })
        );
        assert_eq!(q.threshold, -50.0);
    }

    #[test]
    fn documented_shifts() {
        for shift in ["5m_ago", "30m_ago", "4h_ago", "1d_ago", "last_1h"] {
            parsed(&format!("change(sum(last_1h),{shift}):sum:x{{*}}.as_count() > 1"));
        }
    }

    #[test]
    fn change_refusals() {
        for (head, fragment) in [
            ("change(avg(last_3d),5m_ago)", "two days"),
            ("change(avg(last_2881m),5m_ago)", "two days"),
            ("change(avg(last_1w),5m_ago)", "two days"),
            ("change(avg(last_5m),3d_ago)", "two days"),
            ("change(avg(last_5m),yesterday)", "is not a window"),
            ("change(avg(last_5m))", "takes a time aggregation and a timeshift"),
            ("pct_change(last_5m,5m_ago)", "takes a time aggregation and a timeshift"),
        ] {
            refused(&format!("{head}:avg:x{{*}} > 1"), fragment);
        }
    }

    // --- anomaly -----------------------------------------------------------------------------

    #[test]
    fn basic_anomaly_datadogs_api_example() {
        let q = parsed(
            "avg(last_1h):anomalies(avg:system.cpu.system{name:cassandra}, 'basic', 3, direction='above', alert_window='last_5m', interval=20, count_default_zero='true') >= 1",
        );
        assert_eq!(q.measured, leaf("avg", "system.cpu.system", tag("name", "cassandra"), &[]));
        assert_eq!(
            q.detection,
            Detection::Anomaly(AnomalySpec {
                algorithm: Anomalies::Basic,
                bounds: 3.0,
                direction: Direction::Above,
                alert_window: minutes(5),
                interval: Some(TimeDelta::seconds(20)),
                count_default_zero: true,
                timezone: None,
            })
        );
        assert_eq!(q.window, Window { count: 1, unit: WindowUnit::Hours });
    }

    #[test]
    fn agile_anomaly_with_every_argument_the_plans_example() {
        let q = parsed(
            "avg(last_4h):anomalies(avg:system.cpu.user{*} by {host}, 'agile', 2, direction='above', alert_window='last_15m', interval=60, count_default_zero='true', seasonality='weekly', timezone='Europe/Warsaw') >= 1",
        );
        match &q.detection {
            Detection::Anomaly(a) => {
                assert_eq!(a.algorithm, Anomalies::Agile(Seasonality::Weekly));
                assert_eq!(a.timezone.as_deref(), Some("Europe/Warsaw"));
                assert_eq!(a.alert_window, minutes(15));
            }
            other => panic!("expected an anomaly, got {other:?}"),
        }
        assert_eq!(q.group_by, vec!["host"]);
    }

    #[test]
    fn anomaly_defaults() {
        match parsed("avg(last_1h):anomalies(avg:x{*}, 'robust', 2, alert_window='last_10m') >= 0.5").detection {
            Detection::Anomaly(a) => {
                assert_eq!(a.algorithm, Anomalies::Robust(Seasonality::Weekly));
                assert_eq!(a.direction, Direction::Both);
                assert_eq!(a.interval, None);
                assert!(!a.count_default_zero);
            }
            other => panic!("expected an anomaly, got {other:?}"),
        }
    }

    #[test]
    fn anomaly_refusals() {
        for (expression, fragment) in [
            ("anomalies(avg:x{*}, 'adaptive', 2, alert_window='last_15m') >= 1", "legacy"),
            ("anomalies(avg:x{*}, 'agile', 2) >= 1", "needs alert_window"),
            ("anomalies(avg:x{*}, 'basic', 2, alert_window='last_15m', seasonality='daily') >= 1", "'basic' has no seasonality"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', seasonality='monthly') >= 1", "seasonality must be"),
            ("anomalies(avg:x{*}, 'agile', 0, alert_window='last_15m') >= 1", "bounds above 0"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', direction='up') >= 1", "direction must be"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', foo=1) >= 1", "has no argument 'foo'"),
            (
                "anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', direction='above', direction='below') >= 1",
                "more than once",
            ),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', interval=1.5) >= 1", "whole number of seconds"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', count_default_zero='yes') >= 1", "'true' or 'false'"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', timezone='Mars/Olympus') >= 1", "not a known IANA zone"),
            ("anomalies(sum:x{*}.as_count(), 'agile', 2, alert_window='last_15m') >= 1", "do not take .as_count()"),
            ("anomalies(cumsum(avg:x{*}), 'agile', 2, alert_window='last_15m') >= 1", "cumsum() is not allowed inside anomalies()"),
            (
                "anomalies(outliers(avg:x{*} by {host}, 'dbscan', 3), 'agile', 2, alert_window='last_15m') >= 1",
                "outliers() is not allowed inside anomalies()",
            ),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m') > 1", "compares with >="),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m') >= 2", "at most 1"),
            ("anomalies(avg:x{*}, 'agile', 2, alert_window='15m') >= 1", "alert_window '15m' is not a window"),
        ] {
            refused(&format!("avg(last_4h):{expression}"), fragment);
        }
    }

    // --- forecast ----------------------------------------------------------------------------

    #[test]
    fn linear_forecast_datadogs_api_example() {
        let q = parsed("max(next_1w):forecast(avg:system.load.1{*}, 'linear', 1, interval='60m', history='1w', model='default') >= 3");
        assert_eq!(q.aggregator, TimeAggregator::Max);
        assert_eq!(q.window, Window { count: 1, unit: WindowUnit::Weeks });
        assert_eq!(q.measured, leaf("avg", "system.load.1", TagFilter::All, &[]));
        assert_eq!(
            q.detection,
            Detection::Forecast(ForecastSpec {
                method: Forecast::Linear(LinearModel::Default),
                deviations: 1.0,
                interval: Some(TimeDelta::hours(1)),
                history: Some(TimeDelta::days(7)),
                timezone: None,
            })
        );
    }

    #[test]
    fn seasonal_forecast_falling_below() {
        let q = parsed("min(next_3mo):forecast(avg:system.disk.free{*} by {host}, 'seasonal', 2, interval=3600, seasonality='daily') <= 0");
        assert_eq!(q.window, Window { count: 3, unit: WindowUnit::Months });
        assert_eq!(q.group_by, vec!["host"]);
        match q.detection {
            Detection::Forecast(f) => assert_eq!(f.method, Forecast::Seasonal(Seasonality::Daily)),
            other => panic!("expected a forecast, got {other:?}"),
        }
    }

    #[test]
    fn forecast_refusals() {
        for (text, fragment) in [
            ("max(last_1w):forecast(avg:x{*}, 'linear', 1) >= 3", "is a horizon"),
            ("max(next_6h):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months"),
            ("max(next_720m):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months"),
            ("max(next_4mo):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months"),
            ("min(next_1w):forecast(avg:x{*}, 'linear', 1) >= 3", "max(next_…)"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1) > 3", "max(next_…)"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 0.5) >= 3", "at least 1"),
            ("max(next_1w):forecast(avg:x{*}, 'quadratic', 1) >= 3", "'linear' or 'seasonal'"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, seasonality='weekly') >= 3", "'linear' takes no seasonality"),
            ("max(next_1w):forecast(avg:x{*}, 'seasonal', 1, history='2w') >= 3", "'seasonal' takes no model or history"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, model='fast') >= 3", "model must be"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, history='a week') >= 3", "history must be a duration"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, horizon='1w') >= 3", "takes no horizon"),
            ("max(next_1w):forecast(sum:x{*}.as_count(), 'linear', 1) >= 3", "do not take .as_count()"),
            ("max(next_1w):forecast(trend_line(avg:x{*}), 'linear', 1) >= 3", "trend_line() is not allowed inside forecast()"),
        ] {
            refused(text, fragment);
        }
    }

    // --- outlier -----------------------------------------------------------------------------

    #[test]
    fn dbscan_outliers_datadogs_api_example() {
        let q = parsed("avg(last_30m):outliers(avg:system.cpu.user{role:es-events-data} by {host}, 'dbscan', 7) > 0");
        assert_eq!(q.detection, Detection::Outlier(OutlierSpec { algorithm: Outliers::Dbscan, tolerance: 7.0 }));
        assert_eq!(q.measured, leaf("avg", "system.cpu.user", tag("role", "es-events-data"), &["host"]));
        assert_eq!(q.group_by, vec!["host"]);
    }

    #[test]
    fn mad_outliers_take_a_percentage_the_name_in_any_case() {
        match parsed("avg(last_1h):outliers(avg:system.load.1{*} by {host}, 'MAD', 3, 20) > 0").detection {
            Detection::Outlier(o) => assert_eq!(o.algorithm, Outliers::Mad { percent: 20.0 }),
            other => panic!("expected an outlier, got {other:?}"),
        }
    }

    #[test]
    fn outlier_refusals() {
        for (expression, fragment) in [
            ("outliers(avg:x{*}, 'dbscan', 3) > 0", "needs a by"),
            ("outliers(avg:x{*} by {host}, 'dbscan', 3) > 1", "ends in > 0"),
            ("outliers(avg:x{*} by {host}, 'dbscan', 3) >= 0", "ends in > 0"),
            ("outliers(avg:x{*} by {host}, 'mad', 3) > 0", "needs a percentage"),
            ("outliers(avg:x{*} by {host}, 'kmeans', 3) > 0", "outliers() takes"),
            ("outliers(sum:x{*}.as_count() by {host}, 'dbscan', 3) > 0", "do not take .as_count()"),
        ] {
            refused(&format!("avg(last_30m):{expression}"), fragment);
        }
    }

    // --- numbers too large -------------------------------------------------------------------

    #[test]
    fn numbers_too_large_are_refused_not_thrown() {
        for (text, fragment) in [
            ("avg(last_99999999d):avg:x{*} > 90", "is not a window"),
            ("avg(last_5m):avg:x{*} > 1e999", "too large"),
            ("avg(last_5m):avg:x{*} * 1e999 > 1", "too large"),
            ("change(avg(last_5m),99999999d_ago):avg:x{*} > 1", "two days"),
            ("max(next_99999999w):forecast(avg:x{*}, 'linear', 1) >= 3", "is a horizon"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, history='999999999999w') >= 3", "history must be a duration"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, interval=1e300) >= 3", "up to 93 days"),
            ("max(next_1w):forecast(avg:x{*}, 'linear', 1, interval='99999999999w') >= 3", "up to 93 days"),
            ("avg(last_1h):anomalies(avg:a{*}, 'basic', 2, alert_window='last_5m', interval=1e12) >= 1", "up to 93 days"),
            ("avg(last_1h):anomalies(avg:a{*}, 'basic', 1e999, alert_window='last_5m') >= 1", "too large"),
            ("avg(last_30m):outliers(avg:x{*} by {host}, 'dbscan', 1e999) > 0", "too large"),
        ] {
            refused(text, fragment);
        }
    }
}
