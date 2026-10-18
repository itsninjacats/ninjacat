/// A metric monitor's query:
///
///   threshold  avg(last_5m):<expr> > 90
///   change     change(avg(last_5m),last_5m):<expr> > 10
///   anomaly    avg(last_4h):anomalies(<q>, 'agile', 2, alert_window='last_15m', …) >= 1
///   forecast   max(next_1w):forecast(<q>, 'linear', 1, history='1w') >= 90
///   outlier    avg(last_30m):outliers(<q> by {host}, 'dbscan', 7) > 0
///
/// The syntax is read first and each method's rules after, so a refusal says
/// what is wrong rather than where a parser stopped.
module NinjaCat.Api.Engine.Monitor.Parse

open System
open System.Text.RegularExpressions
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor.Definition

// --- syntax ----------------------------------------------------------------------------

type private Head = { Name: string; Args: HeadArg list }

and private HeadArg =
    | InnerHead of Head
    | Plain of string

type private Syntax =
    { Head: Head
      Expression: Expr<MetricQuery>
      Comparator: Comparator
      Threshold: float }

/// In a module of its own because FParsec's `Ok` and `Error` would hide
/// Result's in the rest of the file.
module private Grammar =
    open FParsec

    type private P<'T> = Parser<'T, unit>

    let private ws: P<unit> = spaces

    let private word: P<string> = many1Satisfy (fun c -> isLetter c || isDigit c || c = '_') <?> "a name"

    let private headOf (argument: P<HeadArg>) : P<Head> =
        parse {
            let! name = word
            do! ws
            let! args = between (pchar '(' >>. ws) (pchar ')') (sepBy (argument .>> ws) (pchar ',' >>. ws))
            return { Name = name; Args = args }
        }

    let private head: P<Head> =
        // attempt: `last_5m` reads as a name too; with no `(` after it, rewind
        // and read it as a word.
        let nested = attempt (headOf (word |>> Plain)) |>> InnerHead
        headOf (nested <|> (word |>> Plain))

    let private comparator: P<Comparator> =
        choice
            [ pstring ">=" >>% Ge
              pstring "<=" >>% Le
              pstring "==" >>% Eq
              pstring "!=" >>% Ne
              pstring ">" >>% Gt
              pstring "<" >>% Lt ]
        <?> "a comparator: <, <=, >, >=, == or !="

    let private threshold: P<float> =
        let signed (minus: unit option, n: float) = if minus.IsSome then -n else n
        opt (skipChar '-') .>>. Parser.number |>> signed <?> "a threshold"

    let private syntax: P<Syntax> =
        parse {
            do! ws
            let! h = head
            do! ws
            do! skipChar ':'
            do! ws
            let! expression = Parser.expression
            do! ws
            let! c = comparator
            do! ws
            let! value = threshold
            do! ws
            do! eof

            return
                { Head = h
                  Expression = expression
                  Comparator = c
                  Threshold = value }
        }

    let read (text: string) : Result<Syntax, string> = run syntax text |> Parser.toResult

// --- windows ---------------------------------------------------------------------------

let private windowPattern = Regex(@"^(last|next)_(\d+)(mo|m|h|d|w)$")
let private agoPattern = Regex(@"^(\d+)(m|h|d)_ago$")

let private unitOf (text: string) : WindowUnit =
    match text with
    | "m" -> Minutes
    | "h" -> Hours
    | "d" -> Days
    | "w" -> Weeks
    | _ -> Months

// The cap keeps Window.Length inside TimeSpan; every window taken is far shorter.
let private countOf (text: string) : int option =
    match Int32.TryParse text with
    | true, n when n > 0 && n <= 100_000 -> Some n
    | _ -> None

let private readWindow (text: string) : (string * Window) option =
    let m = windowPattern.Match text

    if not m.Success then
        None
    else
        countOf m.Groups[2].Value
        |> Option.map (fun count -> m.Groups[1].Value, { Count = count; Unit = unitOf m.Groups[3].Value })

let private month = { Count = 1; Unit = Months }
let private longestHorizon = { Count = 3; Unit = Months }

/// A `last_…` window: `last_5m`, `last_4h`, `last_2d`, `last_1w`, `last_1mo`.
///
/// WARNING(undocumented): which spellings Datadog takes beyond its examples.
/// Any whole count of m, h, d or w is read, and mo only as `last_1mo`.
let lastWindow (text: string) : Result<Window, string> =
    if text.StartsWith "current_" then
        Error $"cumulative windows ('{text}') are not supported yet"
    else
        match readWindow text with
        | Some("last", w) when w.Unit = Months && w.Count <> 1 -> Error $"'{text}': the only month window is last_1mo"
        | Some("last", w) when w.Length > month.Length -> Error $"'{text}' is longer than last_1mo, the longest window"
        | Some("last", w) -> Ok w
        | Some(_, _) -> Error $"'{text}' is a forecast horizon; other monitors look back, e.g. last_5m"
        | None -> Error $"'{text}' is not a window; write it like last_5m, last_4h, last_2d, last_1w or last_1mo"

let private changeWindowError (text: string) =
    $"'{text}': a change monitor looks at most two days back (minutes up to 2880, hours up to 48, days up to 2)"

let private withinTwoDays (w: Window) = w.Length <= TimeSpan.FromDays 2.0

let private changeWindow (text: string) : Result<Window, string> =
    match lastWindow text with
    | Ok w when withinTwoDays w -> Ok w
    | Ok _ -> Error(changeWindowError text)
    | Error e -> Error e

/// `5m_ago`, `1h_ago`, `1d_ago`, or a window spelled `last_5m`.
///
/// WARNING(undocumented): Datadog documents 5, 10, 15, 30 minutes, 1, 2, 4
/// hours and 1 day. Any shift within two days is accepted, as for the window.
let private timeshift (text: string) : Result<Window, string> =
    let m = agoPattern.Match text

    if not m.Success then
        changeWindow text
    else
        match countOf m.Groups[1].Value |> Option.map (fun count -> { Count = count; Unit = unitOf m.Groups[2].Value }) with
        | Some shift when withinTwoDays shift -> Ok shift
        | _ -> Error(changeWindowError text)

/// `next_12h` to `next_3mo`.
let private horizon (text: string) : Result<Window, string> =
    match readWindow text with
    | Some("next", w) when w.Unit <> Minutes && w.Length >= TimeSpan.FromHours 12.0 && w.Length <= longestHorizon.Length -> Ok w
    | Some("next", _) -> Error $"'{text}': a forecast looks 12 hours to 3 months ahead, from next_12h to next_3mo"
    | _ -> Error $"'{text}': a forecast monitor's window is a horizon, e.g. max(next_1w)"

// --- the expression --------------------------------------------------------------------

/// Outermost first.
let rec functionNames (e: Expr<MetricQuery>) : string list =
    match e with
    | Leaf _
    | Number _ -> []
    | Neg inner -> functionNames inner
    | Binary(_, l, r) -> functionNames l @ functionNames r
    | Call(name, args) ->
        name
        :: (args
            |> List.collect (fun arg ->
                match arg with
                | Value v -> functionNames v
                | Lit _
                | Named _ -> []))

/// A count across series is one series, whatever went in.
let rec private groupKeys (e: Expr<MetricQuery>) : Set<string> =
    match e with
    | Leaf q -> set q.GroupBy
    | Number _ -> Set.empty
    | Neg inner -> groupKeys inner
    | Binary(_, l, r) -> Set.union (groupKeys l) (groupKeys r)
    | Call(("count_nonzero" | "count_nonzero_finite" | "count_not_null"), _) -> Set.empty
    | Call(_, args) ->
        args
        |> List.map (fun arg ->
            match arg with
            | Value v -> groupKeys v
            | Lit _
            | Named _ -> Set.empty)
        |> Set.unionMany

let private detectionFunctions = [ "anomalies"; "forecast"; "outliers" ]

/// "Not allowed inside" anomalies() and forecast(), per Datadog's docs.
let private refusedInModels = [ "cumsum"; "integral"; "outliers"; "piecewise_constant"; "robust_trend"; "trend_line" ]

let private problemOf (result: Result<'T, string>) : string option =
    match result with
    | Error e -> Some e
    | Ok _ -> None

// --- arguments only a monitor reads; the engine's readers take the rest ------------------

let private duplicates (fn: string) (named: (string * Literal) list) : string list =
    [ for name, count in named |> List.countBy fst do
          if count > 1 then
              $"{fn}() is given '{name}' more than once" ]

/// Seconds (`interval=60`), or a duration (`interval='60m'`, as Datadog's
/// forecast examples write it). Never longer than the longest horizon.
let private intervalArg (fn: string) (named: (string * Literal) list) : Result<TimeSpan option, string> =
    let fits (span: TimeSpan) =
        span >= TimeSpan.FromSeconds 1.0 && span <= longestHorizon.Length && span.Ticks % TimeSpan.TicksPerSecond = 0L

    let invalid = Error $"{fn}() interval is a whole number of seconds up to 93 days, e.g. interval=60"

    match Plan.namedArg "interval" named with
    | None -> Ok None
    | Some(Num seconds) when seconds >= 1.0 && seconds <= longestHorizon.Length.TotalSeconds && seconds = Math.Floor seconds ->
        Ok(Some(TimeSpan.FromSeconds seconds))
    | Some(Word text | Quoted text) ->
        match Duration.parse text with
        | Ok span when fits span -> Ok(Some span)
        | _ -> invalid
    | Some(Num _) -> invalid

let private timezoneArg (fn: string) (named: (string * Literal) list) : Result<string option, string> =
    match Plan.namedArg "timezone" named with
    | None -> Ok None
    | Some(Word zone | Quoted zone) ->
        try
            TimeZoneInfo.FindSystemTimeZoneById zone |> ignore
            Ok(Some zone)
        with _ ->
            Error $"{fn}() timezone '{zone}' is not a known IANA zone"
    | Some(Num _) -> Error $"{fn}() timezone is a zone name, e.g. timezone='Europe/Warsaw'"

// --- detection methods -----------------------------------------------------------------

let private anomaly (rest: Arg<string> list) : Result<AnomalySpec, string list> =
    let fn = "anomalies"
    let _, named = Plan.splitArgs rest

    let algorithm =
        match Plan.anomaliesArgs rest with
        | Ok(Plan.Basic, _) when (Plan.namedArg "seasonality" named).IsSome ->
            Error "anomalies() 'basic' has no seasonality; it compares each point with the ones just before it"
        | result -> result

    let direction =
        match Plan.namedArg "direction" named |> Option.map Plan.word with
        | None
        | Some(Some "both") -> Ok Both
        | Some(Some "above") -> Ok Above
        | Some(Some "below") -> Ok Below
        | Some _ -> Error "anomalies() direction must be 'both', 'above' or 'below'"

    // WARNING(undocumented): Datadog's anomaly queries always carry
    // alert_window; without it there is no window to judge, so it is required.
    let alertWindow =
        match Plan.namedArg "alert_window" named |> Option.map Plan.word with
        | None -> Error "an anomaly monitor needs alert_window, e.g. anomalies(query, 'agile', 2, alert_window='last_15m')"
        | Some(Some text) -> lastWindow text |> Result.mapError (fun e -> $"anomalies() alert_window {e}")
        | Some None -> Error "anomalies() alert_window is a window, e.g. alert_window='last_15m'"

    let interval = intervalArg fn named

    let countDefaultZero =
        match Plan.namedArg "count_default_zero" named |> Option.map Plan.word with
        | None
        | Some(Some "false") -> Ok false
        | Some(Some "true") -> Ok true
        | Some _ -> Error "anomalies() count_default_zero is 'true' or 'false'"

    let timezone = timezoneArg fn named

    match duplicates fn named, algorithm, direction, alertWindow, interval, countDefaultZero, timezone with
    | [], Ok(alg, bounds), Ok direction, Ok window, Ok interval, Ok countDefaultZero, Ok timezone ->
        Ok
            { Algorithm = alg
              Bounds = bounds
              Direction = direction
              AlertWindow = window
              Interval = interval
              CountDefaultZero = countDefaultZero
              Timezone = timezone }
    | duplicated, _, _, _, _, _, _ ->
        duplicated
        @ List.choose
            id
            [ problemOf algorithm
              problemOf direction
              problemOf alertWindow
              problemOf interval
              problemOf countDefaultZero
              problemOf timezone ]
        |> Error

let private forecast (rest: Arg<string> list) : Result<ForecastSpec, string list> =
    let fn = "forecast"
    let _, named = Plan.splitArgs rest
    let has name = (Plan.namedArg name named).IsSome

    let method =
        match Plan.forecastArgs rest with
        | Error e -> Error e
        | Ok(_, _, _, Some _) -> Error "forecast() takes no horizon in a monitor; the window sets it, e.g. max(next_1w)"
        | Ok(deviations, _, _, _) when deviations < 1.0 -> Error "forecast() deviations must be at least 1: forecast(query, 'linear', 1)"
        | Ok(_, Plan.LinearForecast _, _, _) when has "seasonality" -> Error "forecast() 'linear' takes no seasonality; it is for 'seasonal'"
        | Ok(_, Plan.SeasonalForecast _, _, _) when has "model" || has "history" ->
            Error "forecast() 'seasonal' takes no model or history; they are for 'linear'"
        | Ok(deviations, m, history, None) -> Ok(deviations, m, history)

    let interval = intervalArg fn named
    let timezone = timezoneArg fn named

    match duplicates fn named, method, interval, timezone with
    | [], Ok(deviations, m, history), Ok interval, Ok timezone ->
        Ok
            { Method = m
              Deviations = deviations
              Interval = interval
              History = history
              Timezone = timezone }
    | duplicated, _, _, _ ->
        duplicated
        @ List.choose id [ problemOf method; problemOf interval; problemOf timezone ]
        |> Error

let private outlier (rest: Arg<string> list) : Result<OutlierSpec, string list> =
    match Plan.splitArgs rest with
    | _, (name, _) :: _ -> Error [ $"outliers() has no argument '{name}'" ]
    | positional, [] ->
        // A query where a number belongs is not a literal; outlierArgs refuses it.
        let literals = if List.forall Option.isSome positional then List.choose id positional else []

        match Plan.outlierArgs literals with
        | Ok(algorithm, tolerance) -> Ok { Algorithm = algorithm; Tolerance = tolerance }
        | Error e -> Error [ e ]

// --- the whole query -------------------------------------------------------------------

let private aggregator (name: string) : Result<TimeAggregator, string> =
    match name with
    | "avg" -> Ok Avg
    | "sum" -> Ok Sum
    | "min" -> Ok Min
    | "max" -> Ok Max
    | "percentile" -> Error "the percentile time aggregation is not supported yet"
    | other -> Error $"unknown time aggregation '{other}'; use avg, sum, min or max"

let private readHead (h: Head) : Result<string * string * (ChangeMeasure * string) option, string> =
    let measureOf name = if name = "change" then Absolute else Percent

    match h.Name, h.Args with
    | ("change" | "pct_change"), [ InnerHead { Name = agg; Args = [ Plain window ] }; Plain shift ] ->
        Ok(agg, window, Some(measureOf h.Name, shift))
    | ("change" | "pct_change"), _ ->
        Error $"{h.Name}() takes a time aggregation and a timeshift: {h.Name}(avg(last_5m),5m_ago)"
    | agg, [ Plain window ] -> Ok(agg, window, None)
    | agg, _ -> Error $"the query starts with a time aggregation and a window, e.g. avg(last_5m), not {agg}(…)"

/// The arguments come back as the engine's formula holds them, for the engine's own readers.
let private outerFunction (e: Expr<MetricQuery>) : (string * Expr<MetricQuery> * Arg<string> list) option =
    match e, snd (Engine.split e) with
    | Call(name, Value inner :: _), Call(_, Value _ :: rest) when List.contains name detectionFunctions -> Some(name, inner, rest)
    | _ -> None

let private build (s: Syntax) (agg: TimeAggregator) (window: Window) (detection: Detection) (measured: Expr<MetricQuery>) =
    { Aggregator = agg
      Window = window
      Detection = detection
      Expression = s.Expression
      Measured = measured
      GroupBy = groupKeys measured |> Set.toList
      AsCount = fst (Engine.split measured) |> List.exists (fun p -> p.Query.Modifiers |> List.exists (fun m -> m.Name = "as_count"))
      Comparator = s.Comparator
      Threshold = s.Threshold }

let private interpret (s: Syntax) (aggName: string) (windowText: string) (change: (ChangeMeasure * string) option) : Result<MonitorQuery, string list> =
    match aggregator aggName, change, outerFunction s.Expression with
    | Error e, _, _ -> Error [ e ]
    | Ok _, Some _, Some(name, _, _) -> Error [ $"{name}() cannot be combined with change() or pct_change()" ]
    | Ok agg, Some(measure, shiftText), None ->
        match changeWindow windowText, timeshift shiftText with
        | Ok window, Ok shift -> Ok(build s agg window (Change { Measure = measure; Shift = shift }) s.Expression)
        | window, shift -> Error(List.choose id [ problemOf window; problemOf shift ])
    | Ok agg, None, None ->
        match lastWindow windowText with
        | Ok window -> Ok(build s agg window Threshold s.Expression)
        | Error e -> Error [ e ]
    | Ok agg, None, Some(name, inner, rest) ->
        let window = if name = "forecast" then horizon windowText else lastWindow windowText

        let detection =
            match name with
            | "anomalies" -> anomaly rest |> Result.map Anomaly
            | "forecast" -> forecast rest |> Result.map Forecast
            | _ -> outlier rest |> Result.map Outlier

        match window, detection with
        | Ok window, Ok detection -> Ok(build s agg window detection inner)
        | Error e, Ok _ -> Error [ e ]
        | window, Error problems -> Error(Option.toList (problemOf window) @ problems)

let private modelProblems (model: string) (q: MonitorQuery) (inside: string list) : string list =
    [ for name in refusedInModels |> List.filter (fun n -> List.contains n inside) |> List.distinct do
          $"{name}() is not allowed inside {model}()"

      if q.AsCount then
          $"{model}() monitors do not take .as_count() queries" ]

let private queryProblems (q: MonitorQuery) : string list =
    let inside = functionNames q.Measured

    let byMethod =
        match q.Detection with
        | Anomaly _ -> modelProblems "anomalies" q inside
        | Forecast _ -> modelProblems "forecast" q inside
        | Outlier _ ->
            // WARNING(undocumented): Datadog is silent on as_count in outlier
            // monitors; refused, as for anomalies and forecasts.
            [ if q.AsCount then
                  "outlier monitors do not take .as_count() queries"
              if q.GroupBy.IsEmpty then
                  "an outlier monitor compares groups, so its query needs a by {…}, e.g. outliers(avg:system.cpu.user{*} by {host}, 'dbscan', 3)" ]
        | Threshold
        | Change _ ->
            [ if q.AsCount && q.Aggregator <> Sum then
                  "with .as_count() the time aggregation must be sum: avg, min and max cannot be used with .as_count()" ]

    // outliers() inside a model is already refused by name.
    let refusedByModel =
        match q.Detection with
        | Anomaly _
        | Forecast _ -> refusedInModels
        | Threshold
        | Change _
        | Outlier _ -> []

    let nested =
        [ for name in detectionFunctions do
              if List.contains name inside && not (List.contains name refusedByModel) then
                  $"{name}() must be the outermost function of the monitor's query" ]

    let comparison =
        match q.Detection, q.Comparator, q.Aggregator with
        | (Threshold | Change _), _, _ -> []
        // WARNING(undocumented): Datadog's anomaly queries all end in `>=`; the
        // other comparators are refused.
        | Anomaly _, Ge, _ when q.Threshold > 0.0 && q.Threshold <= 1.0 -> []
        | Anomaly _, Ge, _ -> [ $"an anomaly monitor's threshold is the fraction of anomalous points, above 0 and at most 1; got {q.Threshold}" ]
        | Anomaly _, _, _ -> [ "an anomaly monitor compares with >=, e.g. anomalies(…) >= 1" ]
        | Forecast _, Ge, Max
        | Forecast _, Le, Min -> []
        | Forecast _, _, _ -> [ "a forecast monitor is max(next_…):forecast(…) >= threshold, or min(next_…):forecast(…) <= threshold" ]
        | Outlier _, Gt, _ when q.Threshold = 0.0 -> []
        | Outlier _, _, _ -> [ "an outlier monitor ends in > 0" ]

    nested @ byMethod @ comparison

/// Whether the inner query plans, and how the options fit it, is Validate's.
let query (text: string) : Result<MonitorQuery, string list> =
    match Grammar.read text with
    | Error message -> Error [ message ]
    | Ok s ->
        match readHead s.Head with
        | Error e -> Error [ e ]
        | Ok(aggName, windowText, change) ->
            match interpret s aggName windowText change with
            | Error problems -> Error problems
            | Ok q ->
                match queryProblems q with
                | [] -> Ok q
                | problems -> Error problems
