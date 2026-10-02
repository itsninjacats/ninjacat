/// The wire formats of api.<site>.
///
///   /api/v1/series               the older public API's series, JSON
///   /api/v2/series               agent-payload's MetricPayload (protobuf, or JSON of the same shape)
///   /api/beta/sketches           agent-payload's SketchPayload (protobuf, or JSON of the same shape)
///   /api/v1/check_run            a list of service checks, JSON
///   /api/v1/events               an event or a list of them, JSON
///   /api/v1/distribution_points  series of raw values, JSON
///
/// The JSON ones are read item by item. An item that is not an object, has a
/// member of the wrong type or lacks one it must have is an Error saying
/// which; the handler keeps that item raw beside the rows of the others.
module NinjaCat.Api.Intake.Routers.ApiPayloads

open System
open System.Text.Json
open Google.Protobuf
open Datadog.Agentpayload
open NinjaCat.Api.Intake

let private parseProtobuf (name: string) (parser: MessageParser<'message>) (body: byte[]) : Result<'message, string> =
    try
        Ok(parser.ParseFrom body)
    with :? InvalidProtocolBufferException as e ->
        Error $"protobuf {name}: {e.Message}"

let parseSeriesV2Protobuf (body: byte[]) : Result<MetricPayload, string> =
    parseProtobuf "MetricPayload" MetricPayload.Parser body

/// What the public API's clients send: the same message as JSON.
let parseSeriesV2Json (body: byte[]) : Result<MetricPayload, string> =
    ProtoJson.tryParse<MetricPayload> body |> Result.mapError (fun e -> "JSON MetricPayload: " + e)

let parseSketchesProtobuf (body: byte[]) : Result<SketchPayload, string> =
    parseProtobuf "SketchPayload" SketchPayload.Parser body

let parseSketchesJson (body: byte[]) : Result<SketchPayload, string> =
    ProtoJson.tryParse<SketchPayload> body |> Result.mapError (fun e -> "JSON SketchPayload: " + e)

// ---------------------------------------------------------------------------
// The JSON bodies of the v1 API
// ---------------------------------------------------------------------------

/// Reads the item at `path` with `read`; Error is everything that did not
/// fit, by member.
let private readItem (path: string) (item: JsonElement) (read: JsonFields.Fields -> 'a) : Result<'a, string> =
    let bad = JsonFields.Mismatches()
    let fields = JsonFields.fields bad path item

    if bad.Count > 0 then
        Error bad[0]
    else
        let result = read fields
        if bad.Count > 0 then Error(String.Join("; ", bad)) else Ok result

/// The members of an item that have no field of their own.
let private additional (declared: string list) (item: JsonElement) : Map<string, JsonElement> =
    Json.members item |> Map.filter (fun name _ -> not (List.contains name declared))

/// The items of a list with their places, the null ones left out: a null
/// carries nothing.
let private placed (list: JsonElement seq) : (int * JsonElement) list =
    list |> Seq.indexed |> Seq.filter (fun (_, item) -> item.ValueKind <> JsonValueKind.Null) |> List.ofSeq

/// A v1 point as sent: a [timestamp, value] pair where either may be null.
type SeriesPoint =
    { Pair: float option[]
      Raw: JsonElement }

type SeriesV1 =
    { Host: string
      Interval: int64 option
      Metric: string
      Points: SeriesPoint[]
      Tags: string[]
      Type: string
      /// The members without a field here. The agent's v1 encoder sends
      /// three: device, source_type_name and unit.
      Additional: Map<string, JsonElement> }

let private seriesPointsOf (list: JsonElement) : SeriesPoint[] option =
    let pair (item: JsonElement) : SeriesPoint option =
        match item.ValueKind with
        | JsonValueKind.Null -> Some { Pair = [||]; Raw = item }
        | JsonValueKind.Array ->
            let numbers =
                [| for number in item.EnumerateArray() ->
                       if number.ValueKind = JsonValueKind.Null then Some None else Json.floatOf number |> Option.map Some |]

            if numbers |> Array.forall Option.isSome then
                Some { Pair = numbers |> Array.map Option.get; Raw = item }
            else
                None
        | _ -> None

    if list.ValueKind <> JsonValueKind.Array then
        None
    else
        let points = [| for item in list.EnumerateArray() -> pair item |]
        if points |> Array.forall Option.isSome then Some(points |> Array.map Option.get) else None

let decodeSeriesV1 (path: string) (item: JsonElement) : Result<SeriesV1, string> =
    readItem path item (fun fields ->
        fields.Require "metric"
        fields.Require "points"

        { Host = fields.String "host"
          Interval = fields.OptionalInt64 "interval"
          Metric = fields.String "metric"
          Points = fields.Read("points", "a list of [timestamp, value] pairs", [||], seriesPointsOf)
          Tags = fields.Strings "tags"
          Type = fields.String "type"
          Additional = additional [ "host"; "interval"; "metric"; "points"; "tags"; "type" ] item })

/// One element of a distribution point.
type DistributionItem =
    | Timestamp of float
    | Values of float[]
    /// null: neither.
    | Absent
    /// Anything else.
    | Unfit

type DistributionPoint =
    { Items: DistributionItem[]
      Raw: JsonElement }

type DistributionSeries =
    { Host: string
      Metric: string
      Points: DistributionPoint[]
      Tags: string[]
      /// "" when not sent. The only word the API has is "distribution".
      Type: string
      Additional: Map<string, JsonElement>
      /// The series as it arrived.
      Raw: JsonElement }

let private distributionItemOf (item: JsonElement) : DistributionItem =
    match item.ValueKind with
    | JsonValueKind.Null -> Absent
    | JsonValueKind.Number ->
        match Json.floatOf item with
        | Some seconds -> Timestamp seconds
        | None -> Unfit
    | _ ->
        match Json.floatsOf item with
        | Some values -> Values values
        | None -> Unfit

let private distributionPointsOf (list: JsonElement) : DistributionPoint[] option =
    let point (item: JsonElement) : DistributionPoint option =
        match item.ValueKind with
        | JsonValueKind.Null -> Some { Items = [||]; Raw = item }
        | JsonValueKind.Array -> Some { Items = [| for element in item.EnumerateArray() -> distributionItemOf element |]; Raw = item }
        | _ -> None

    if list.ValueKind <> JsonValueKind.Array then
        None
    else
        let points = [| for item in list.EnumerateArray() -> point item |]
        if points |> Array.forall Option.isSome then Some(points |> Array.map Option.get) else None

let decodeDistributionSeries (path: string) (item: JsonElement) : Result<DistributionSeries, string> =
    readItem path item (fun fields ->
        fields.Require "metric"
        fields.Require "points"

        { Host = fields.String "host"
          Metric = fields.String "metric"
          Points = fields.Read("points", "a list of [timestamp, [values]] pairs", [||], distributionPointsOf)
          Tags = fields.Strings "tags"
          Type = fields.String "type"
          Additional = additional [ "host"; "metric"; "points"; "tags"; "type" ] item
          Raw = item })

/// A body of the shape {"series": [...]}, and whatever else the sender put
/// beside the list.
type SeriesPayload<'series> =
    { Series: 'series[]
      /// Where each of Series stood in the list as sent.
      Positions: int[]
      /// The elements of the list that did not fit, each with its place and
      /// what was wrong with it.
      Rejected: (int * JsonElement * string) list
      Additional: Map<string, JsonElement> }

/// Item by item, so one element that does not fit does not cost the series
/// beside it.
let private parsePayload
    (label: string)
    (decode: string -> JsonElement -> Result<'series, string>)
    (body: byte[])
    : Result<SeriesPayload<'series>, string> =
    match Json.tryParse body with
    | Error e -> Error $"{label}: {e}"
    | Ok root ->
        let list =
            readItem "" root (fun fields ->
                fields.Require "series"
                fields.Items "series")

        match list with
        | Error e -> Error $"{label}: {e}"
        | Ok list ->
            let decoded = placed list |> List.map (fun (i, item) -> i, item, decode $"series[{i}]" item)

            let fitted =
                decoded
                |> List.choose (fun (i, _, result) ->
                    match result with
                    | Ok series -> Some(i, series)
                    | Error _ -> None)

            Ok
                { Series = fitted |> List.map snd |> Array.ofList
                  Positions = fitted |> List.map fst |> Array.ofList
                  Rejected =
                    decoded
                    |> List.choose (fun (i, item, result) ->
                        match result with
                        | Ok _ -> None
                        | Error why -> Some(i, item, why))
                  Additional = additional [ "series" ] root }

let parseSeriesV1 (body: byte[]) : Result<SeriesPayload<SeriesV1>, string> =
    parsePayload "JSON v1 series" decodeSeriesV1 body

let parseDistributionPoints (body: byte[]) : Result<SeriesPayload<DistributionSeries>, string> =
    parsePayload "JSON distribution_points" decodeDistributionSeries body

type ServiceCheck =
    { Check: string
      HostName: string
      Message: string
      Status: int
      Tags: string[]
      Timestamp: int64
      Additional: Map<string, JsonElement> }

type CheckRuns =
    { Runs: ServiceCheck list
      /// The items that did not fit, each with its place and what was wrong.
      Rejected: (int * JsonElement * string) list }

/// Decodes one check. Datadog's OpenAPI spec calls `tags` required; the
/// agent disagrees and sends `"tags": null` on checks without any
/// (containerd.health, the kubelet checks). With `supplyTags` a missing or
/// null list is an empty one.
///
/// A status outside 0..3 does not fit: it has no name, and an unreadable
/// check must never be stored as OK.
let decodeServiceCheck (supplyTags: bool) (path: string) (item: JsonElement) : Result<ServiceCheck, string> =
    readItem path item (fun fields ->
        fields.Require "check"
        fields.Require "host_name"
        fields.Require "status"

        if not supplyTags then
            fields.Require "tags"

        let status = fields.Int32 "status"

        if status < 0 || status > 3 then
            fields.Invalid("status", $"expected 0 to 3, got {status}")

        { Check = fields.String "check"
          HostName = fields.String "host_name"
          Message = fields.String "message"
          Status = status
          Tags = fields.Strings "tags"
          Timestamp = fields.Int64 "timestamp"
          Additional = additional [ "check"; "host_name"; "message"; "status"; "tags"; "timestamp" ] item })

/// Decodes a check_run batch item by item, so one odd check does not discard
/// the checks beside it. A body that is not a list is read as one check —
/// datadogpy posts a bare object — and there no tags are supplied.
let parseCheckRuns (body: byte[]) : Result<CheckRuns, string> =
    match Json.tryParse body with
    | Error e -> Error $"JSON check_run: {e}"
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok { Runs = []; Rejected = [] }
    | Ok root when root.ValueKind = JsonValueKind.Array ->
        let decoded =
            placed (root.EnumerateArray()) |> List.map (fun (i, item) -> i, item, decodeServiceCheck true $"[{i}]" item)

        Ok
            { Runs =
                decoded
                |> List.choose (fun (_, _, result) ->
                    match result with
                    | Ok run -> Some run
                    | Error _ -> None)
              Rejected =
                decoded
                |> List.choose (fun (i, item, result) ->
                    match result with
                    | Ok _ -> None
                    | Error why -> Some(i, item, why)) }
    | Ok root ->
        match decodeServiceCheck false "" root with
        | Ok run -> Ok { Runs = [ run ]; Rejected = [] }
        | Error e -> Error $"JSON check_run: {e}"

/// The alert types and priorities Datadog has.
let eventAlertTypes =
    set [ "error"; "warning"; "info"; "success"; "user_update"; "recommendation"; "snapshot" ]

let eventPriorities = set [ "normal"; "low" ]

type EventRequest =
    { AggregationKey: string
      /// "" when not sent.
      AlertType: string
      DateHappened: int64
      DeviceName: string
      Host: string
      Priority: string option
      RelatedEventId: int64 option
      SourceTypeName: string
      /// None when not sent: the reply then says null, not [].
      Tags: string[] option
      Text: string
      Title: string
      Additional: Map<string, JsonElement> }

let decodeEvent (path: string) (item: JsonElement) : Result<EventRequest, string> =
    readItem path item (fun fields ->
        fields.Require "text"
        fields.Require "title"

        { AggregationKey = fields.String "aggregation_key"
          AlertType = fields.String "alert_type"
          DateHappened = fields.Int64 "date_happened"
          DeviceName = fields.String "device_name"
          Host = fields.String "host"
          Priority = fields.OptionalString "priority"
          RelatedEventId = fields.OptionalInt64 "related_event_id"
          SourceTypeName = fields.String "source_type_name"
          Tags = fields.OptionalStrings "tags"
          Text = fields.String "text"
          Title = fields.String "title"
          Additional =
            additional
                [ "aggregation_key"; "alert_type"; "date_happened"; "device_name"; "host"; "priority"
                  "related_event_id"; "source_type_name"; "tags"; "text"; "title" ]
                item })

/// The events of a body: a list, or one bare object. One event that does not
/// fit refuses the whole body: the sender gets a 400 and has to know that
/// nothing of it was taken.
let parseEvents (body: byte[]) : Result<EventRequest list, string> =
    match Json.tryParse body with
    | Error e -> Error $"JSON events: {e}"
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok []
    | Ok root when root.ValueKind = JsonValueKind.Array ->
        let decoded = root.EnumerateArray() |> Seq.mapi (fun i item -> decodeEvent $"[{i}]" item) |> List.ofSeq

        let firstError =
            decoded
            |> List.tryPick (fun result ->
                match result with
                | Error e -> Some e
                | Ok _ -> None)

        match firstError with
        | Some e -> Error $"JSON events: {e}"
        | None ->
            Ok(
                decoded
                |> List.choose (fun result ->
                    match result with
                    | Ok event -> Some event
                    | Error _ -> None)
            )
    | Ok root ->
        match decodeEvent "" root with
        | Ok event -> Ok [ event ]
        | Error e -> Error $"JSON events: {e}"
