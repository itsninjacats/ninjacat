/// A monitor's options, from Datadog's options object. Every option not acted
/// on is accepted and named in NotActedOn, so a Datadog definition loads
/// unchanged; only the two that would change what is evaluated are refused.
module NinjaCat.Api.Engine.Monitor.Options

open System
open System.Text.Json
open NinjaCat.Api.Engine.Monitor.Definition

let defaults: Options =
    { Thresholds =
        { Critical = None
          Warning = None
          CriticalRecovery = None
          WarningRecovery = None
          Ok = None
          Unknown = None }
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

let private unknownKeys (path: string) (known: string list) (e: JsonElement) : string list =
    [ for p in e.EnumerateObject() do
          if p.Value.ValueKind <> JsonValueKind.Null && not (List.contains p.Name known) then
              $"{path} has no '{p.Name}'" ]

let private seconds (n: int64) = TimeSpan.FromSeconds(float n)

let private ofObject (root: JsonElement) : Result<Options, string list> =
    let problems = ResizeArray<string>()

    let present =
        [ for p in root.EnumerateObject() do
              if p.Value.ValueKind <> JsonValueKind.Null then
                  p.Name ]

    for name in present do
        match name with
        | "scheduling_options" -> problems.Add "options.scheduling_options: custom schedules and cumulative windows are not supported yet"
        | "variables" -> problems.Add "options.variables: formula and function monitors over other data sources are not supported yet"
        | _ -> ()

    // A value with a problem reads as absent; the problem is noted.
    let read (parent: string) (name: string) (reader: string -> JsonElement -> Result<'T, string>) (e: JsonElement) : 'T option =
        match property name e with
        | None -> None
        | Some value ->
            match reader $"{parent}.{name}" value with
            | Ok v -> Some v
            | Error problem ->
                problems.Add problem
                None

    let objectAt (name: string) (known: string list) : JsonElement option =
        let path = $"options.{name}"

        match property name root with
        | None -> None
        | Some e when e.ValueKind <> JsonValueKind.Object ->
            problems.Add $"{path} must be an object"
            None
        | Some e ->
            problems.AddRange(unknownKeys path known e)
            Some e

    let thresholds =
        match objectAt "thresholds" [ "critical"; "warning"; "critical_recovery"; "warning_recovery"; "ok"; "unknown" ] with
        | None -> defaults.Thresholds
        | Some e ->
            { Critical = read "options.thresholds" "critical" number e
              Warning = read "options.thresholds" "warning" number e
              CriticalRecovery = read "options.thresholds" "critical_recovery" number e
              WarningRecovery = read "options.thresholds" "warning_recovery" number e
              Ok = read "options.thresholds" "ok" number e
              Unknown = read "options.thresholds" "unknown" number e }

    let windows = objectAt "threshold_windows" [ "trigger_window"; "recovery_window" ]
    let triggerWindow = windows |> Option.bind (read "options.threshold_windows" "trigger_window" window)
    let recoveryWindow = windows |> Option.bind (read "options.threshold_windows" "recovery_window" window)

    let evaluationDelay = read "options" "evaluation_delay" (fun p -> wholeNumber p 0L 86400L) root
    // WARNING(undocumented): Datadog documents no cap; ours is a day.
    let newGroupDelay = read "options" "new_group_delay" (fun p -> wholeNumber p 0L 86400L) root
    let newHostDelay = read "options" "new_host_delay" (fun p -> wholeNumber p 0L 86400L) root
    let requireFullWindow = read "options" "require_full_window" boolean root
    let notifyNoData = read "options" "notify_no_data" boolean root
    // Minutes, as Datadog counts it. The longest is the retention's, which Validate checks.
    let noDataTimeframe = read "options" "no_data_timeframe" minutes root
    let onMissingData = read "options" "on_missing_data" missingData root
    let timeoutH = read "options" "timeout_h" (fun p -> wholeNumber p 0L 24L) root

    // WARNING(undocumented): on_missing_data replaces the older two, so both
    // together are refused; an explicit notify_no_data false is accepted, as
    // a definition read back from Datadog may carry it.
    match onMissingData, notifyNoData, noDataTimeframe with
    | Some _, Some true, _
    | Some _, _, Some _ -> problems.Add "options.on_missing_data replaces notify_no_data and no_data_timeframe; set one or the other"
    | _ -> ()

    if problems.Count > 0 then
        Error(List.ofSeq problems)
    else
        // new_host_delay is the deprecated name; new_group_delay wins.
        let newGroupDelay =
            match newGroupDelay, newHostDelay with
            | Some s, _
            | None, Some s -> seconds s
            | None, None -> defaults.NewGroupDelay

        Ok
            { Thresholds = thresholds
              TriggerWindow = triggerWindow
              RecoveryWindow = recoveryWindow
              EvaluationDelay = evaluationDelay |> Option.map seconds |> Option.defaultValue TimeSpan.Zero
              NewGroupDelay = newGroupDelay
              RequireFullWindow = requireFullWindow |> Option.defaultValue false
              NotifyNoData = notifyNoData |> Option.defaultValue false
              NoDataTimeframe = noDataTimeframe |> Option.map (fun m -> TimeSpan.FromMinutes(float m))
              OnMissingData = onMissingData
              Timeout =
                match timeoutH with
                | None
                | Some 0L -> None
                | Some h -> Some(TimeSpan.FromHours(float h))
              NotActedOn = present |> List.filter (fun name -> not (List.contains name actedOn)) }

/// The options as JSON text, as Postgres and the panel hold them. Absent or null is every default.
let parse (json: string) : Result<Options, string list> =
    if String.IsNullOrWhiteSpace json then
        Ok defaults
    else
        try
            use document = JsonDocument.Parse json

            match document.RootElement.ValueKind with
            | JsonValueKind.Null -> Ok defaults
            | JsonValueKind.Object -> ofObject document.RootElement
            | _ -> Error [ "options must be an object" ]
        with :? JsonException as e ->
            Error [ $"options are not valid JSON: {e.Message}" ]
