/// A monitor's options, from Datadog's options object as stored.
///
/// The options evaluation acts on are read and range-checked here. Every other
/// option is accepted and named in NotActedOn, so a Datadog definition loads
/// unchanged and the panel can say what is ignored; only the two that would
/// change what is evaluated are refused. How the options fit the query is
/// Validate's.
module NinjaCat.Api.Engine.Monitor.Options

open System
open System.Text.Json
open NinjaCat.Api.Engine.Monitor.Definition

let defaults: Options =
    { Thresholds =
        { Critical = None
          Warning = None
          CriticalRecovery = None
          WarningRecovery = None }
      TriggerWindow = None
      RecoveryWindow = None
      EvaluationDelay = TimeSpan.Zero
      NewGroupDelay = TimeSpan.FromSeconds 60.0
      RequireFullWindow = false
      NotifyNoData = false
      NoDataTimeframe = None
      OnMissingData = None
      Timeout = None
      NotActedOn = [] }

let private actedOn =
    [ "thresholds"
      "threshold_windows"
      "evaluation_delay"
      "new_group_delay"
      "new_host_delay"
      "require_full_window"
      "notify_no_data"
      "no_data_timeframe"
      "on_missing_data"
      "timeout_h" ]

let private refused =
    Map
        [ "scheduling_options", "options.scheduling_options: custom schedules and cumulative windows are not supported yet"
          "variables", "options.variables: formula and function monitors over other data sources are not supported yet" ]

/// A property, with null the same as absent.
let private property (name: string) (e: JsonElement) : JsonElement option =
    match e.TryGetProperty name with
    | true, value when value.ValueKind <> JsonValueKind.Null -> Some value
    | _ -> None

let private number (path: string) (e: JsonElement) : Result<float, string> =
    match e.ValueKind with
    | JsonValueKind.Number ->
        match e.GetDouble() with
        | n when Double.IsFinite n -> Ok n
        | _ -> Error $"{path} is too large a number"
    | _ -> Error $"{path} must be a number"

let private wholeNumber (path: string) (low: int64) (high: int64) (e: JsonElement) : Result<int64, string> =
    match number path e with
    | Ok n when n = Math.Floor n && n >= float low && n <= float high -> Ok(int64 n)
    | Ok _ -> Error $"{path} must be a whole number from {low} to {high}"
    | Error e -> Error e

let private minutes (path: string) (e: JsonElement) : Result<int64, string> =
    match number path e with
    | Ok n when n = Math.Floor n && n >= 1.0 && n <= float Int32.MaxValue -> Ok(int64 n)
    | Ok _ -> Error $"{path} must be a whole number of minutes, at least 1"
    | Error e -> Error e

let private boolean (path: string) (e: JsonElement) : Result<bool, string> =
    match e.ValueKind with
    | JsonValueKind.True -> Ok true
    | JsonValueKind.False -> Ok false
    | _ -> Error $"{path} must be true or false"

let private missingData (path: string) (e: JsonElement) : Result<MissingData, string> =
    let text = if e.ValueKind = JsonValueKind.String then e.GetString() else ""

    match text with
    | "default" -> Ok MissingData.Default
    | "show_no_data" -> Ok MissingData.ShowNoData
    | "show_and_notify_no_data" -> Ok MissingData.ShowAndNotifyNoData
    | "resolve" -> Ok MissingData.Resolve
    | _ -> Error $"{path} must be one of default, show_no_data, show_and_notify_no_data, resolve"

let private window (path: string) (e: JsonElement) : Result<Window, string> =
    match e.ValueKind with
    | JsonValueKind.String -> Parse.lastWindow (e.GetString()) |> Result.mapError (fun problem -> $"{path}: {problem}")
    | _ -> Error $"{path} must be a window like \"last_15m\""

/// Reads `name` of `parent` with `reader`; absent is Ok None.
let private optional
    (parent: string)
    (name: string)
    (reader: string -> JsonElement -> Result<'T, string>)
    (e: JsonElement)
    : Result<'T option, string> =
    match property name e with
    | None -> Ok None
    | Some value -> reader $"{parent}.{name}" value |> Result.map Some

let private valueOf (result: Result<'T option, string>) : 'T option =
    match result with
    | Ok value -> value
    | Error _ -> None

let private unknownKeys (path: string) (known: string list) (e: JsonElement) : string list =
    [ for p in e.EnumerateObject() do
          if p.Value.ValueKind <> JsonValueKind.Null && not (List.contains p.Name known) then
              $"{path} has no '{p.Name}'" ]

let private thresholds (path: string) (e: JsonElement) : Result<Thresholds, string list> =
    if e.ValueKind <> JsonValueKind.Object then
        Error [ $"{path} must be an object" ]
    else
        let critical = optional path "critical" number e
        let warning = optional path "warning" number e
        let criticalRecovery = optional path "critical_recovery" number e
        let warningRecovery = optional path "warning_recovery" number e

        let problems =
            unknownKeys path [ "critical"; "warning"; "critical_recovery"; "warning_recovery" ] e
            @ List.choose id
                [ Parse.problemOf critical
                  Parse.problemOf warning
                  Parse.problemOf criticalRecovery
                  Parse.problemOf warningRecovery ]

        match problems with
        | [] ->
            Ok
                { Critical = valueOf critical
                  Warning = valueOf warning
                  CriticalRecovery = valueOf criticalRecovery
                  WarningRecovery = valueOf warningRecovery }
        | problems -> Error problems

let private thresholdWindows (path: string) (e: JsonElement) : Result<Window option * Window option, string list> =
    if e.ValueKind <> JsonValueKind.Object then
        Error [ $"{path} must be an object" ]
    else
        let trigger = optional path "trigger_window" window e
        let recovery = optional path "recovery_window" window e

        match unknownKeys path [ "trigger_window"; "recovery_window" ] e @ List.choose id [ Parse.problemOf trigger; Parse.problemOf recovery ] with
        | [] -> Ok(valueOf trigger, valueOf recovery)
        | problems -> Error problems

let private seconds (n: int64) = TimeSpan.FromSeconds(float n)

let private ofObject (root: JsonElement) : Result<Options, string list> =
    let path = "options"
    let present =
        [ for p in root.EnumerateObject() do
              if p.Value.ValueKind <> JsonValueKind.Null then
                  p.Name ]

    let keyProblems = present |> List.choose refused.TryFind

    let thresholds =
        match property "thresholds" root with
        | None -> Ok defaults.Thresholds
        | Some e -> thresholds $"{path}.thresholds" e

    let windows =
        match property "threshold_windows" root with
        | None -> Ok(None, None)
        | Some e -> thresholdWindows $"{path}.threshold_windows" e

    let evaluationDelay = optional path "evaluation_delay" (fun p -> wholeNumber p 0L 86400L) root
    // WARNING(undocumented): Datadog documents no cap; ours is a day.
    let newGroupDelay = optional path "new_group_delay" (fun p -> wholeNumber p 0L 86400L) root
    let newHostDelay = optional path "new_host_delay" (fun p -> wholeNumber p 0L 86400L) root
    let requireFullWindow = optional path "require_full_window" boolean root
    let notifyNoData = optional path "notify_no_data" boolean root
    // Minutes, as Datadog counts it. The longest is the retention's, which Validate checks.
    let noDataTimeframe = optional path "no_data_timeframe" minutes root
    let onMissingData = optional path "on_missing_data" missingData root
    let timeoutH = optional path "timeout_h" (fun p -> wholeNumber p 0L 24L) root

    // WARNING(undocumented): on_missing_data replaces the older two, so both
    // together are refused; an explicit notify_no_data false is accepted, as
    // a definition read back from Datadog may carry it.
    let replaced =
        match valueOf onMissingData, valueOf notifyNoData, valueOf noDataTimeframe with
        | Some _, Some true, _
        | Some _, _, Some _ -> [ "options.on_missing_data replaces notify_no_data and no_data_timeframe; set one or the other" ]
        | _ -> []

    let nested (result: Result<'T, string list>) =
        match result with
        | Error problems -> problems
        | Ok _ -> []

    let problems =
        keyProblems
        @ nested thresholds
        @ nested windows
        @ List.choose
            id
            [ Parse.problemOf evaluationDelay
              Parse.problemOf newGroupDelay
              Parse.problemOf newHostDelay
              Parse.problemOf requireFullWindow
              Parse.problemOf notifyNoData
              Parse.problemOf noDataTimeframe
              Parse.problemOf onMissingData
              Parse.problemOf timeoutH ]
        @ replaced

    match problems, thresholds, windows with
    | [], Ok thresholds, Ok(trigger, recovery) ->
        // new_host_delay is the deprecated name; new_group_delay wins.
        let newGroupDelay =
            match valueOf newGroupDelay, valueOf newHostDelay with
            | Some s, _
            | None, Some s -> seconds s
            | None, None -> defaults.NewGroupDelay

        Ok
            { Thresholds = thresholds
              TriggerWindow = trigger
              RecoveryWindow = recovery
              EvaluationDelay = valueOf evaluationDelay |> Option.map seconds |> Option.defaultValue TimeSpan.Zero
              NewGroupDelay = newGroupDelay
              RequireFullWindow = valueOf requireFullWindow |> Option.defaultValue false
              NotifyNoData = valueOf notifyNoData |> Option.defaultValue false
              NoDataTimeframe = valueOf noDataTimeframe |> Option.map (fun m -> TimeSpan.FromMinutes(float m))
              OnMissingData = valueOf onMissingData
              Timeout =
                match valueOf timeoutH with
                | None
                | Some 0L -> None
                | Some h -> Some(TimeSpan.FromHours(float h))
              NotActedOn = present |> List.filter (fun name -> not (List.contains name actedOn)) }
    | problems, _, _ -> Error problems

/// Datadog's options object. Absent or null is every default.
let ofElement (root: JsonElement) : Result<Options, string list> =
    match root.ValueKind with
    | JsonValueKind.Undefined
    | JsonValueKind.Null -> Ok defaults
    | JsonValueKind.Object -> ofObject root
    | _ -> Error [ "options must be an object" ]

/// The options as JSON text, as Postgres and the panel hold them.
let parse (json: string) : Result<Options, string list> =
    if String.IsNullOrWhiteSpace json then
        Ok defaults
    else
        try
            use document = JsonDocument.Parse json
            ofElement document.RootElement
        with :? JsonException as e ->
            Error [ $"options are not valid JSON: {e.Message}" ]
