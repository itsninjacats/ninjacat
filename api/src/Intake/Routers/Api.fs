/// api.<site> / app.<site> — the main forwarder.
///
///   agent config: dd_url / DD_DD_URL
///
/// The one intake carrying several unrelated signals, because it predates
/// Datadog's split into per-product intakes. Where the rows go:
///
///   /api/v1/series, /api/v2/series        metrics
///   /api/beta/sketches                    sketches + agent_batch_metadata
///   /api/v1/distribution_points           sketches
///   /api/v1/check_run                     check_runs
///   /api/v1/events                        events
///   /intake/  events variant              events
///             agent_checks variant        agent_checks + external_host_tags
///             gohai/systemStats variant   hosts
///             resources alone (legacy V5) raw_payloads
///   /api/v1/metadata                      agent_metadata
///   /api/v2/intake-key                    delegated_auth_requests
///   /api/v2/profiles/symbols/query        symbol_queries
///   Private Action Runner                 runner_enrollments, runner_dequeues,
///                                         runner_task_updates, runner_heartbeats,
///                                         action_connections
///   GET /api/v1/query, GET /api/v2/validate, the runner's health-check:
///                                         nothing; they are questions, not data
///
/// The rule for every handler: what has a column is stored, what cannot be
/// decoded goes to raw_payloads.
module NinjaCat.Api.Intake.Routers.Api

open System
open System.Buffers.Binary
open System.Globalization
open System.Security.Cryptography
open System.Text
open System.Text.Json
open System.Threading.Tasks
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open Datadog.Agentpayload
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.ApiPayloads
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The reply the agent expects after a batch of metrics. It reads the body:
/// an empty object would be an API error to it.
let private ackSeries: EndpointHandler = setStatusCode 202 >=> json {| errors = List.empty<string> |}

let private statusOk: EndpointHandler = json {| status = "ok" |}

/// The keys of a map for a note: sorted, the first eight, then how many more.
let private keyList (values: Map<string, 'a>) : string =
    let keys = values |> Map.toList |> List.map fst
    let limit = 8

    if keys.IsEmpty then "(none)"
    elif keys.Length <= limit then String.Join(", ", keys)
    else String.Join(", ", List.take limit keys) + $" ... {keys.Length - limit} more"

/// For an `extra` column: every value JSON-encoded, so one column takes a
/// string, a number and an object alike, and a string that looks like a
/// number stays a string.
let private extraEncoded (skip: string list) (values: Map<string, JsonElement>) : Map<string, string> =
    values |> Map.filter (fun name _ -> not (List.contains name skip)) |> Json.memberTexts Json.compact

/// The same for an envelope read one level down: the values byte for byte.
let private extraRaw (skip: string list) (values: Map<string, JsonElement>) : Map<string, string> =
    values |> Map.filter (fun name _ -> not (List.contains name skip)) |> Json.memberTexts _.GetRawText()

/// Folds a nested object's extras into the outer ones under a prefix, so two
/// levels of one document share a column without colliding.
let private mergeExtra (outer: Map<string, string>) (prefix: string) (nested: Map<string, string>) : Map<string, string> =
    nested |> Map.fold (fun merged name value -> merged.Add(prefix + name, value)) outer

/// A float timestamp as whole seconds: the fraction dropped, and NaN or
/// anything an int64 cannot hold becomes the minimum, which Time.wireSeconds
/// then reads as "not supplied".
let private wholeSeconds (value: float) : int64 =
    if Double.IsNaN value || value >= 9.2233720368547758e18 || value < -9.2233720368547758e18 then
        Int64.MinValue
    else
        int64 value

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// /api/v2/series: protobuf from the agent, JSON from the public API's
/// clients, both the same shape.
///
/// The host is a resource of type "host"; every other resource type (the
/// agent writes one more, "device") goes into the row's resources map, so a
/// device series stays distinguishable from a host one.
let handleSeriesV2 (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        ackSeries ctx
    else
        let decoded =
            if Body.isJson (Ctx.header ctx "Content-Type") body then
                parseSeriesV2Json body
            else
                parseSeriesV2Protobuf body

        match decoded with
        | Error error ->
            Raw.store ctx "series" "decode_error" error body
        | Ok payload ->
            let points = ResizeArray<MetricPoint>()

            for series in payload.Series do
                let host =
                    series.Resources
                    |> Seq.tryFind (fun resource -> resource.Type = "host")
                    |> Option.map _.Name
                    |> Option.defaultValue ""

                let resources =
                    series.Resources
                    |> Seq.filter (fun resource -> resource.Type <> "host")
                    |> Seq.map (fun resource -> resource.Type, resource.Name)
                    |> Map.ofSeq

                let origin =
                    if isNull series.Metadata || isNull series.Metadata.Origin then
                        Origin()
                    else
                        series.Metadata.Origin

                let tags = Tags.toMultiMap series.Tags

                for point in series.Points do
                    points.Add
                        { Metrics.point tenant (Time.wireSeconds point.Timestamp) series.Metric point.Value with
                            Host = host
                            MetricType = Wire.metricTypeName (int series.Type)
                            SourceType = series.SourceTypeName
                            Unit = series.Unit
                            Interval = uint32 series.Interval
                            Tags = tags
                            OriginProduct = origin.OriginProduct
                            OriginCategory = origin.OriginCategory
                            OriginService = origin.OriginService
                            Resources = resources }

            Ctx.write ctx Metrics.table (points.ToArray())

        ackSeries ctx

/// The v1 wire's type word; an unknown word stays UNSPECIFIED rather than
/// being guessed at.
let normalizeTypeWord (word: string) : string =
    match word.ToLowerInvariant() with
    | "count" -> "COUNT"
    | "rate" -> "RATE"
    | "gauge" -> "GAUGE"
    | _ -> "UNSPECIFIED"

/// /api/v1/series: the older public API, JSON only. The agent's v1 encoder
/// also writes device, source_type_name and unit, which the API does not
/// have: the last two have columns, device goes into the resources map under
/// the name v2 uses for it, anything else the API does not have into `extra`.
let handleSeriesV1 (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        ackSeries ctx
    else
        match parseSeriesV1 body with
        | Error error ->
            Raw.store ctx "series" "decode_error" error body
        | Ok payload ->
            if not payload.Additional.IsEmpty then
                // A key beside `series` belongs to the batch, not to a point:
                // copying it onto every row would multiply it. The body is
                // kept instead, once.
                Raw.store
                    ctx
                    "series"
                    "unexpected_shape"
                    ("v1 payload carried undeclared top-level keys: " + keyList payload.Additional)
                    body

            for _, item, why in payload.Rejected do
                Raw.store ctx "series" "unexpected_shape" why (Json.rawBytes item)

            let points = ResizeArray<MetricPoint>()

            for i in 0 .. payload.Series.Length - 1 do
                let series = payload.Series[i]
                let at = payload.Positions[i]

                let interval =
                    match series.Interval with
                    | Some seconds -> uint32 seconds
                    | None -> 0u

                let resources =
                    match Json.lenientString (series.Additional.TryFind "device") with
                    | "" -> Map.empty
                    | device -> Map.ofList [ "device", device ]

                let extra = extraEncoded [ "device"; "source_type_name"; "unit" ] series.Additional
                let tags = Tags.toMultiMap series.Tags

                for j in 0 .. series.Points.Length - 1 do
                    let point = series.Points[j]

                    match point.Pair |> Array.truncate 2 with
                    | [| Some timestamp; Some value |] ->
                        points.Add
                            { Metrics.point tenant (Time.wireSeconds (wholeSeconds timestamp)) series.Metric value with
                                Host = series.Host
                                MetricType = normalizeTypeWord series.Type
                                SourceType = Json.lenientString (series.Additional.TryFind "source_type_name")
                                Unit = Json.lenientString (series.Additional.TryFind "unit")
                                Interval = interval
                                Tags = tags
                                Resources = resources
                                Extra = extra }
                    | _ ->
                        // The series is stored through its other
                        // points; the pair is the smallest thing
                        // worth keeping.
                        Raw.store
                            ctx
                            "series"
                            "unexpected_shape"
                            $"series[{at}].points[{j}]: expected a [timestamp, value] pair of numbers"
                            (Json.rawBytes point.Raw)

            Ctx.write ctx Metrics.table (points.ToArray())

        ackSeries ctx

/// Who sent a batch, as opposed to what was in it: agent version, timezone,
/// the agent's clock and both of its IPs. The api_key in the same block is a
/// credential and is not carried over. An all-empty block is not stored.
let private storeBatchMetadata (ctx: HttpContext) (intake: string) (sender: CommonMetadata) : unit =
    let tenant = Ctx.tenant ctx

    let empty =
        sender.AgentVersion = ""
        && sender.Timezone = ""
        && sender.CurrentEpoch = 0.0
        && sender.InternalIp = ""
        && sender.PublicIp = ""

    if not empty then
        let row: AgentBatchMetadataRow =
            { TenantID = tenant
              ReceivedAt = DateTime.UtcNow
              Intake = intake
              AgentVersion = sender.AgentVersion
              Timezone = sender.Timezone
              CurrentEpoch = sender.CurrentEpoch
              InternalIP = sender.InternalIp
              PublicIP = sender.PublicIp }

        Ctx.write ctx AgentBatchMetadata.table [| row |]

/// /api/beta/sketches: DDSketch histograms the agent has already built. One
/// row per dogsketch, not per metric: each is its own point in time.
///
/// A sketch may also carry `distributions`, the pre-DDSketch encoding (raw
/// values and ranks). The current agent never writes it and it cannot become
/// a bucket row without inventing numbers, so such a sketch is kept raw,
/// whole.
let handleSketches (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        ackSeries ctx
    else
        let decoded =
            if Body.isJson (Ctx.header ctx "Content-Type") body then
                parseSketchesJson body
            else
                parseSketchesProtobuf body

        match decoded with
        | Error error ->
            Raw.store ctx "sketches" "decode_error" error body
        | Ok payload ->
            let rows = ResizeArray<SketchRow>()

            for sketch in payload.Sketches do
                if sketch.Distributions.Count > 0 then
                    Raw.store
                        ctx
                        "sketches"
                        "no_schema"
                        $"legacy pre-DDSketch Distribution for {sketch.Metric} has no row shape"
                        (Encoding.UTF8.GetBytes(ProtoJson.message sketch))

                let origin =
                    if isNull sketch.Metadata || isNull sketch.Metadata.Origin then
                        Origin()
                    else
                        sketch.Metadata.Origin

                let tags = Tags.toMultiMap sketch.Tags

                for dogsketch in sketch.Dogsketches do
                    rows.Add
                        { TenantID = tenant
                          Timestamp = Time.wireSeconds dogsketch.Ts
                          Metric = sketch.Metric
                          Host = sketch.Host
                          Tags = tags
                          Count = uint64 dogsketch.Cnt
                          Min = dogsketch.Min
                          Max = dogsketch.Max
                          Avg = dogsketch.Avg
                          Sum = dogsketch.Sum
                          BucketKeys = Array.ofSeq dogsketch.K
                          BucketCounts = Array.ofSeq dogsketch.N
                          OriginProduct = origin.OriginProduct
                          OriginCategory = origin.OriginCategory
                          OriginService = origin.OriginService
                          Resources = Map.empty
                          Extra = Map.empty }

            Ctx.write ctx Sketches.table (rows.ToArray())
            storeBatchMetadata ctx "sketches" (if isNull payload.Metadata then CommonMetadata() else payload.Metadata)

        ackSeries ctx

/// A distribution point's time and values. The pair is heterogeneous: of its
/// elements the last timestamp and the last value list count. No values, no
/// point.
let distributionPoint (items: DistributionItem[]) : (DateTime * float[]) option =
    let mutable timestamp = 0.0
    let mutable values = [||]

    for item in items do
        match item with
        | Timestamp seconds -> timestamp <- seconds
        | Values list -> values <- list
        | Absent
        | Unfit -> ()

    if values.Length = 0 then None else Some(Time.wireSeconds (wholeSeconds timestamp), values)

/// /api/v1/distribution_points. The agent never calls this — it sends
/// finished sketches. Clients send RAW VALUES here, so the sketch is built on
/// our side, with the agent's own bucketing so both sources merge by key.
///
/// The raw values are not kept: a distribution exists to be merged with the
/// agent's sketches, which arrive already bucketed. What does not fit is kept
/// whole: a body with no series list, an element that is not a series, a
/// pair that could not be split.
let handleDistributionPoints (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        ackSeries ctx
    else
        match parseDistributionPoints body with
        | Error error ->
            Raw.store ctx "distribution_points" "decode_error" error body
        | Ok payload ->
            if not payload.Additional.IsEmpty then
                Raw.store
                    ctx
                    "distribution_points"
                    "unexpected_shape"
                    ("payload carried undeclared top-level keys: " + keyList payload.Additional)
                    body

            for _, item, why in payload.Rejected do
                Raw.store ctx "distribution_points" "unexpected_shape" why (Json.rawBytes item)

            let rows = ResizeArray<SketchRow>()

            for i in 0 .. payload.Series.Length - 1 do
                let series = payload.Series[i]
                let at = payload.Positions[i]

                if series.Type <> "" && series.Type <> "distribution" then
                    // Noted, and the points still become rows: a typo in
                    // that one word must not cost a host its distribution.
                    Raw.store
                        ctx
                        "distribution_points"
                        "unexpected_shape"
                        $"series[{at}].type: expected \"distribution\", got \"{series.Type}\""
                        (Json.rawBytes series.Raw)

                let extra = extraEncoded [] series.Additional
                let tags = Tags.toMultiMap series.Tags

                for j in 0 .. series.Points.Length - 1 do
                    let point = series.Points[j]
                    let unfit = point.Items |> Array.exists (fun item -> item = Unfit)

                    match distributionPoint point.Items with
                    | Some(timestamp, values) when not unfit ->
                        let sketch = DDSketch.build values
                        let stats = sketch.Stats

                        rows.Add
                            { TenantID = tenant
                              Timestamp = timestamp
                              Metric = series.Metric
                              Host = series.Host
                              Tags = tags
                              Count = uint64 stats.Count
                              Min = stats.Min
                              Max = stats.Max
                              Avg = stats.Avg
                              Sum = stats.Sum
                              BucketKeys = sketch.Keys
                              BucketCounts = sketch.Counts
                              OriginProduct = 0u
                              OriginCategory = 0u
                              OriginService = 0u
                              Resources = Map.empty
                              Extra = extra }
                    | _ ->
                        Raw.store
                            ctx
                            "distribution_points"
                            "unexpected_shape"
                            $"series[{at}].points[{j}]: expected a [timestamp, [values]] pair"
                            (Json.rawBytes point.Raw)

            Ctx.write ctx Sketches.table (rows.ToArray())

        ackSeries ctx

// ---------------------------------------------------------------------------
// Service checks
// ---------------------------------------------------------------------------

let statusName (status: int) : string =
    match status with
    | 0 -> "OK"
    | 1 -> "WARNING"
    | 2 -> "CRITICAL"
    | _ -> "UNKNOWN"

/// /api/v1/check_run. A check answers "is this thing working", not "how much
/// of it is there"; monitors of the "service is down" kind are built on these.
let handleCheckRun (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        ackSeries ctx
    else
        match parseCheckRuns body with
        | Error error ->
            Raw.store ctx "check_run" "decode_error" error body
        | Ok checks ->
            for _, item, why in checks.Rejected do
                Raw.store ctx "check_run" "unexpected_shape" why (Json.rawBytes item)

            let rows =
                [| for run in checks.Runs ->
                       { TenantID = tenant
                         Timestamp = Time.wireSeconds run.Timestamp
                         CheckName = run.Check
                         Host = run.HostName
                         Status = statusName run.Status
                         Message = run.Message
                         Tags = Tags.toMultiMap run.Tags
                         Extra = extraEncoded [] run.Additional } |]

            Ctx.write ctx Checks.table rows

        ackSeries ctx

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Anything outside Datadog's vocabulary is coerced rather than rejected:
/// dropping an event because someone invented an alert type would lose the
/// one thing we were asked to remember. What the sender wrote is kept beside
/// the coerced value.
let coerceAlertType (raw: string) : string =
    let word = raw.ToLowerInvariant()
    if eventAlertTypes.Contains word then word else "info"

let coercePriority (raw: string) : string =
    let word = raw.ToLowerInvariant()
    if eventPriorities.Contains word then word else "normal"

/// A new event id, and the numeric form the API hands back to the caller:
/// the id's first eight bytes, shifted to stay inside an int64.
let private newEventId () : Guid * uint64 =
    let id = Guid.NewGuid()
    id, BinaryPrimitives.ReadUInt64BigEndian(ReadOnlySpan(id.ToByteArray true, 0, 8)) >>> 1

/// POST /api/v1/events. Unlike every other endpoint this one MUST return a
/// body: the client reads the reply into EventCreateResponse and keeps the
/// id.
let handleEvents (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if Diagnose.isSweep ctx then
        (setStatusCode 202 >=> json {| status = "ok" |}) ctx
    else
        match parseEvents body with
        | Error error ->
            Raw.store ctx "events" "decode_error" error body
            (setStatusCode 400 >=> json {| status = "error"; error = error |}) ctx
        | Ok [] -> (setStatusCode 400 >=> json {| status = "error"; error = "no event with a title" |}) ctx
        | Ok events ->
            // Every event gets an id; the reply carries the first one's.
            let ids = events |> List.map (fun _ -> newEventId ())
            let firstId = snd ids.Head

            let rows =
                [| for event, (id, number) in List.zip events ids ->
                       { TenantID = tenant
                         Timestamp = Time.wireSeconds event.DateHappened
                         EventID = id
                         EventIDNum = number
                         Title = event.Title
                         Text = event.Text
                         Host = event.Host
                         AlertType = coerceAlertType event.AlertType
                         Priority = coercePriority (defaultArg event.Priority "")
                         AggregationKey = event.AggregationKey
                         SourceTypeName = event.SourceTypeName
                         DeviceName = event.DeviceName
                         Tags = Tags.toMultiMap (defaultArg event.Tags [||])
                         EventType = ""
                         RelatedEventID = event.RelatedEventId
                         AlertTypeRaw = event.AlertType
                         PriorityRaw = defaultArg event.Priority ""
                         Extra = extraEncoded [] event.Additional } |]

            Ctx.write ctx Events.table rows

            let first = events.Head

            let reply =
                {| status = "ok"
                   event =
                    {| id = firstId
                       id_str = string firstId
                       title = first.Title
                       text = first.Text
                       date_happened = DateTimeOffset(Time.wireSeconds first.DateHappened).ToUnixTimeSeconds()
                       host = first.Host
                       alert_type = coerceAlertType first.AlertType
                       priority = coercePriority (defaultArg first.Priority "")
                       tags = Option.toObj first.Tags
                       source_type_name = first.SourceTypeName
                       device_name = first.DeviceName
                       url = "" |} |}

            (setStatusCode 202 >=> json reply) ctx

// ---------------------------------------------------------------------------
// /intake/: the Agent v5 endpoint
// ---------------------------------------------------------------------------

/// One agent event (pkg/metrics/event.Event) as the shared events row. The
/// agent's names differ from the public API's for the same three things:
/// msg_title, msg_text and timestamp against title, text and date_happened.
///
/// `source` is the key the agent grouped the event under and stands in when
/// the event does not repeat it; likewise the payload's host stands in for an
/// event with none of its own.
let intakeEventRow (tenant: string) (source: string) (payloadHost: string) (event: JsonElement) : EventRow =
    let fields = Json.members event
    let id, number = newEventId ()
    let alertRaw = Json.lenientString (fields.TryFind "alert_type")
    let priorityRaw = Json.lenientString (fields.TryFind "priority")

    let tags =
        match fields.TryFind "tags" with
        | Some list when list.ValueKind = JsonValueKind.Array -> list.EnumerateArray() |> Seq.choose Json.stringOf |> List.ofSeq
        | _ -> []

    { TenantID = tenant
      Timestamp = Time.wireSeconds (Json.lenientInt64 (fields.TryFind "timestamp"))
      EventID = id
      EventIDNum = number
      Title = Json.lenientString (fields.TryFind "msg_title")
      Text = Json.lenientString (fields.TryFind "msg_text")
      Host = Text.firstNonEmpty [ Json.lenientString (fields.TryFind "host"); payloadHost ]
      AlertType = coerceAlertType alertRaw
      Priority = coercePriority priorityRaw
      AggregationKey = Json.lenientString (fields.TryFind "aggregation_key")
      SourceTypeName = Text.firstNonEmpty [ Json.lenientString (fields.TryFind "source_type_name"); source ]
      DeviceName = ""
      Tags = Tags.toMultiMap tags
      EventType = Json.lenientString (fields.TryFind "event_type")
      RelatedEventID = None
      AlertTypeRaw = alertRaw
      PriorityRaw = priorityRaw
      Extra =
        extraEncoded
            [ "msg_title"; "msg_text"; "timestamp"; "host"; "alert_type"; "priority"; "aggregation_key"
              "source_type_name"; "event_type"; "tags" ]
            fields }

/// `events` as source → its events, or what does not fit that shape: an
/// object of lists of objects. A null event is an empty one.
let private eventsBySource (events: JsonElement) : Result<Map<string, JsonElement list>, string> =
    match events.ValueKind with
    | JsonValueKind.Null -> Ok Map.empty
    | JsonValueKind.Object ->
        let problem (source: string) (list: JsonElement) : string option =
            match list.ValueKind with
            | JsonValueKind.Null -> None
            | JsonValueKind.Array ->
                list.EnumerateArray()
                |> Seq.indexed
                |> Seq.tryFind (fun (_, event) -> event.ValueKind <> JsonValueKind.Object && event.ValueKind <> JsonValueKind.Null)
                |> Option.map (fun (i, event) -> $"events.{source}[{i}]: expected an object, got {Json.kind event}")
            | _ -> Some $"events.{source}: expected a list, got {Json.kind list}"

        match events.EnumerateObject() |> Seq.tryPick (fun source -> problem source.Name source.Value) with
        | Some error -> Error error
        | None ->
            Json.members events
            |> Map.map (fun _ list -> if list.ValueKind = JsonValueKind.Array then List.ofSeq (list.EnumerateArray()) else [])
            |> Ok
    | _ -> Error $"events: expected an object, got {Json.kind events}"

/// The events variant: {"events": {<source>: [...]}, "internalHostname"}.
/// The rows go to the same table as /api/v1/events.
let private intakeEvents (ctx: HttpContext) (envelope: Map<string, JsonElement>) : unit =
    let tenant = Ctx.tenant ctx

    let events = envelope["events"]

    match eventsBySource events with
    | Error error ->
        Raw.store ctx "intake" "unexpected_shape" error (Json.rawBytes events)
    | Ok bySource ->
        let host = Json.lenientString (envelope.TryFind "internalHostname")

        // By source in sorted order, so two replays of one payload give
        // the same rows.
        let rows =
            [| for pair in bySource do
                   for event in pair.Value do
                       intakeEventRow tenant pair.Key host event |]

        Ctx.write ctx Events.table rows

/// One positional agent_checks entry: 0 check name, 1 source type, 2 instance
/// id, 3 status, 4 message. A shorter array is normal, which is why Status is
/// optional: a missing position 3 must not read as 0, which means OK.
/// Anything after position 4 is kept as a JSON array, so the positions
/// survive.
let agentCheckRow
    (tenant: string)
    (at: DateTime)
    (host: string)
    (version: string)
    (uuid: string)
    (meta: string)
    (positional: JsonElement[])
    : AgentCheckRow =
    let text (i: int) : string =
        if i < positional.Length then Json.lenientString (Some positional[i]) else ""

    { TenantID = tenant
      ReceivedAt = at
      Hostname = host
      AgentVersion = version
      UUID = uuid
      CheckName = text 0
      SourceType = text 1
      InstanceID = text 2
      Status = if positional.Length > 3 then Json.int64Of positional[3] else None
      Message = text 4
      PositionalExtra = if positional.Length > 5 then Json.compactArray (Array.skip 5 positional) else ""
      Meta = meta }

/// The agent_checks variant (the V5 collector's payload): check statuses as
/// positional arrays, and external_host_tags, [[hostname, {source: [tags]}]]
/// — tags for hosts the agent is NOT running on, which is why they get a
/// table of their own.
let private intakeAgentChecks (ctx: HttpContext) (envelope: Map<string, JsonElement>) : unit =
    let tenant = Ctx.tenant ctx

    let checks = Json.lenientItems (envelope.TryFind "agent_checks")

    let externalTags =
        Json.lenientItems (envelope.TryFind "external_host_tags")
        |> List.mapi (fun i entry ->
            if entry.ValueKind = JsonValueKind.Array && entry.GetArrayLength() = 2 then
                Some(Json.lenientString (Some entry[0]), Json.lenientStringLists (Some entry[1]))
            else
                Raw.store
                    ctx
                    "intake"
                    "unexpected_shape"
                    $"external_host_tags[{i}] is not a [hostname, tags] pair"
                    (Json.rawBytes entry)

                None)

    let now = DateTime.UtcNow
    let host = Json.lenientString (envelope.TryFind "internalHostname")
    let version = Json.lenientString (envelope.TryFind "agentVersion")
    let uuid = Json.lenientString (envelope.TryFind "uuid")
    let meta = Json.rawOrEmpty (envelope.TryFind "meta")
    let checkRows = ResizeArray<AgentCheckRow>()

    checks
    |> List.iteri (fun i check ->
        if check.ValueKind = JsonValueKind.Array then
            checkRows.Add(agentCheckRow tenant now host version uuid meta (Array.ofSeq (check.EnumerateArray())))
        else
            Raw.store ctx "intake" "unexpected_shape" $"agent_checks[{i}] is not a positional array" (Json.rawBytes check))

    Ctx.write ctx AgentChecks.table (checkRows.ToArray())

    // One row per SOURCE: a host carries several at once, and flattening
    // them would lose which of them said what.
    let tagRows: ExternalHostTagsRow[] =
        [| for entry in externalTags do
               match entry with
               | None -> ()
               | Some(describedHost, bySource) ->
                   for pair in bySource do
                       { TenantID = tenant
                         ReceivedAt = now
                         Host = describedHost
                         Source = pair.Key
                         Tags = Tags.toMultiMap pair.Value } |]

    Ctx.write ctx ExternalHostTags.table tagRows

/// The envelope keys hostRow reads by name; the rest go to intake_extra.
/// apiKey is in the list so that a credential cannot reach a column by way of
/// a catch-all.
let private hostKnownKeys =
    [ "apiKey"; "internalHostname"; "agentVersion"; "os"; "uuid"; "agent-flavor"; "python"; "gohai"; "host-tags"
      "systemStats"; "meta"; "network"; "logs"; "install-method"; "proxy-info"; "otlp"; "container-meta"; "fips_mode"
      "fips_proxy_enabled"; "resources" ]

/// The gohai sections with columns of their own. gohai.network is not one of
/// them: the envelope's own `network` (the host's addresses) holds the column
/// of that name, and gohai's interface inventory keeps its name in
/// gohai_extra.
let private gohaiKnownSections = [ "platform"; "cpu"; "memory"; "filesystem" ]

/// The hosts row of an /intake/ host envelope.
///
///   internalHostname, agentVersion, os   columns
///   gohai (a STRING holding JSON)        platform, cpu, memory: columns;
///                                        filesystem: its JSON; the rest: gohai_extra
///   host-tags                            every source → host_tags; the
///                                        "system" source also fills tags
///   meta, network, logs, otlp, resources their JSON text, as sent
///   systemStats, install-method, proxy-info, container-meta
///                                        flat maps, values JSON-encoded
///   apiKey                               the real key: never stored
///   anything else                        intake_extra
let hostRow
    (tenant: string)
    (hostname: string)
    (agentVersion: string)
    (osName: string)
    (seenAt: DateTime)
    (envelope: Map<string, JsonElement>)
    (gohai: Map<string, JsonElement>)
    (hostTags: Map<string, string[]>)
    : HostRow =
    // gohai reports every value as a string, even numbers. A section that is
    // absent or shaped differently is left empty rather than failing the row.
    let section (name: string) : Map<string, string> =
        gohai.TryFind name |> Option.bind Json.stringMapOf |> Option.defaultValue Map.empty

    let objectMap (name: string) : Map<string, string> =
        extraRaw [] (Json.lenientMembers (envelope.TryFind name))

    { TenantID = tenant
      Host = hostname
      SeenAt = seenAt
      AgentVersion = agentVersion
      OS = osName
      Platform = section "platform"
      CPU = section "cpu"
      Memory = section "memory"
      Tags = Tags.toMultiMap (hostTags.TryFind "system" |> Option.defaultValue [||])
      UUID = Json.lenientString (envelope.TryFind "uuid")
      AgentFlavor = Json.lenientString (envelope.TryFind "agent-flavor")
      PythonVersion = Json.lenientString (envelope.TryFind "python")
      Meta = Json.rawOrEmpty (envelope.TryFind "meta")
      Network = Json.rawOrEmpty (envelope.TryFind "network")
      Filesystem = Json.rawOrEmpty (gohai.TryFind "filesystem")
      Logs = Json.rawOrEmpty (envelope.TryFind "logs")
      OTLP = Json.rawOrEmpty (envelope.TryFind "otlp")
      SystemStats = objectMap "systemStats"
      InstallMethod = objectMap "install-method"
      ProxyInfo = objectMap "proxy-info"
      ContainerMeta = objectMap "container-meta"
      FIPSMode = Json.flag (envelope.TryFind "fips_mode")
      FIPSProxyEnabled = Json.flag (envelope.TryFind "fips_proxy_enabled")
      HostTags = hostTags
      GohaiExtra = extraRaw gohaiKnownSections gohai
      IntakeExtra = extraRaw hostKnownKeys envelope
      Resources = Json.rawOrEmpty (envelope.TryFind "resources") }

/// The host metadata variant. The payload is loose and shifts between agent
/// versions, so fields are read by name.
let private intakeHost (ctx: HttpContext) (envelope: Map<string, JsonElement>) : unit =
    let tenant = Ctx.tenant ctx

    let hostname = Json.lenientString (envelope.TryFind "internalHostname")

    if hostname <> "" then
        let gohai =
            match Json.lenientString (envelope.TryFind "gohai") with
            | "" -> Map.empty
            | inner ->
                match Json.tryParse (Encoding.UTF8.GetBytes inner) with
                | Ok sections -> Json.members sections
                | Error _ -> Map.empty

        let row =
            hostRow
                tenant
                hostname
                (Json.lenientString (envelope.TryFind "agentVersion"))
                (Json.lenientString (envelope.TryFind "os"))
                DateTime.UtcNow
                envelope
                gohai
                (Json.lenientStringLists (envelope.TryFind "host-tags"))

        Ctx.write ctx Hosts.table [| row |]

/// One row of a resources snapshot, as gohai writes it:
/// [usernames, cpu %, memory %, vms, rss, name, number of pids]
/// (pkg/gohai/processes). None when it is not that.
let private processGroup (tenant: string) (host: string) (at: DateTime) (fields: JsonElement) : ProcessGroupRow option =
    if fields.ValueKind <> JsonValueKind.Array || fields.GetArrayLength() <> 7 then
        None
    else
        let field (i: int) = fields[i]

        match Json.stringOf (field 0), Json.floatOf (field 1), Json.floatOf (field 2), Json.stringOf (field 5) with
        | Some usernames, Some cpu, Some mem, Some name ->
            let unsigned (i: int) : uint64 =
                match (field i).TryGetUInt64() with
                | true, n -> n
                | false, _ -> 0UL

            Some
                { TenantID = tenant
                  Timestamp = at
                  Host = host
                  Name = name
                  // The agent joins them with commas.
                  Usernames = (if usernames = "" then [||] else usernames.Split ',')
                  ProcessCount = uint32 (unsigned 6)
                  CPUPct = cpu
                  MemPct = mem
                  VMS = unsigned 3
                  RSS = unsigned 4 }
        | _ -> None

/// {"meta":{"host":…},"processes":{"snaps":[[unix seconds, [row, …]], …]}}:
/// the processes of the host grouped by name, sent with the host metadata.
/// Whatever of it is not that shape keeps the body raw, once.
let private intakeResources (body: byte[]) (ctx: HttpContext) (resources: JsonElement) : unit =
    let tenant = Ctx.tenant ctx

    let host =
        Json.field "meta" resources |> Option.bind (Json.field "host") |> Option.bind Json.stringOf |> Option.defaultValue ""

    let snaps =
        match Json.field "processes" resources |> Option.bind (Json.field "snaps") with
        | Some list when list.ValueKind = JsonValueKind.Array -> Some(List.ofSeq (list.EnumerateArray()))
        | _ -> None

    match snaps with
    | None -> Raw.store ctx "intake" "unexpected_shape" "resources without processes.snaps" body
    | Some snaps ->
        let rows = ResizeArray<ProcessGroupRow>()
        let mutable unread = 0

        for snap in snaps do
            let groups =
                if snap.ValueKind = JsonValueKind.Array && snap.GetArrayLength() = 2 && snap[1].ValueKind = JsonValueKind.Array then
                    Json.int64Of snap[0] |> Option.map (fun seconds -> Time.fromUnixSeconds seconds, snap[1])
                else
                    None

            match groups with
            | None -> unread <- unread + 1
            | Some(at, list) ->
                for fields in list.EnumerateArray() do
                    match processGroup tenant host at fields with
                    | Some row -> rows.Add row
                    | None -> unread <- unread + 1

        if unread > 0 then
            Raw.store ctx "intake" "unexpected_shape" $"resources: {unread} snapshots or rows are not the resources check's shape" body

        Ctx.write ctx ProcessGroups.table (rows.ToArray())

/// Four producers share /intake/ and nothing but the path; they are told
/// apart by their top-level keys, in this order:
///
///   events                 agent events, the shutdown event among them
///   agent_checks           the V5 collector: check statuses, external host tags
///   gohai or systemStats   host metadata
///   resources alone        the resources check's process groups
let private decodeIntake (body: byte[]) (ctx: HttpContext) : unit =

    let undecodable (error: string) =
        Raw.store ctx "intake" "decode_error" error body

    match Json.tryParse body with
    | Error error -> undecodable error
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        undecodable $"expected an object, got {Json.kind root}"
    | Ok root ->
        let envelope = Json.members root

        if envelope.ContainsKey "events" then
            intakeEvents ctx envelope
        elif envelope.ContainsKey "agent_checks" then
            intakeAgentChecks ctx envelope
        elif envelope.ContainsKey "gohai" || envelope.ContainsKey "systemStats" then
            intakeHost ctx envelope
        elif envelope.ContainsKey "resources" && envelope.Count = 1 then
            intakeResources body ctx envelope["resources"]
        else
            Raw.store ctx "intake" "unexpected_shape" ("no known /intake/ variant key, keys: " + keyList envelope) body

let handleIntake (body: byte[]) (ctx: HttpContext) : Task =
    if not (Diagnose.isSweep ctx) then
        decodeIntake body ctx

    statusOk ctx

// ---------------------------------------------------------------------------
// /api/v1/metadata
// ---------------------------------------------------------------------------

/// The variant keys of /api/v1/metadata, one per producer (comp/metadata/*).
let private metadataVariants =
    [ "agent_metadata"; "host_metadata"; "check_metadata"; "logs_metadata"; "files_metadata"
      "system_probe_metadata"; "security_agent_metadata"; "signing_metadata"; "host_system_info_metadata"
      "host_gpu_metadata"; "ha_agent_metadata"; "datadog_cluster_agent_metadata"; "clustercheck_metadata" ]

let private metadataEnvelopeKeys = [ "hostname"; "clustername"; "cluster_id"; "timestamp"; "uuid" ]

let private storeMetadata (body: byte[]) (ctx: HttpContext) : unit =
    let tenant = Ctx.tenant ctx

    let undecodable (error: string) =
        Raw.store ctx "metadata" "decode_error" error body

    match Json.tryParse body with
    | Error error -> undecodable error
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        undecodable $"expected an object, got {Json.kind root}"
    | Ok root ->
        let envelope = Json.members root
        let found = metadataVariants |> List.filter envelope.ContainsKey

        if found.IsEmpty then
            // A producer we have never seen, or a variant key renamed
            // between releases.
            Raw.store
                ctx
                "metadata"
                "no_schema"
                ("no known /api/v1/metadata variant key, keys: " + keyList envelope)
                body
        else
            let timestamp = Json.lenientInt64 (Json.field "timestamp" root)
            let now = DateTime.UtcNow
            // Every key that is neither the shared envelope nor a variant of
            // this request — clustercheck_status and its sibling among them.
            let extra = extraRaw (metadataEnvelopeKeys @ found) envelope

            // One row per variant key present: `variant` stays a real filter.
            let rows: AgentMetadataRow[] =
                [| for variant in found ->
                       { TenantID = tenant
                         ReceivedAt = now
                         Variant = variant
                         Hostname = Json.lenientString (Json.field "hostname" root)
                         ClusterName = Json.lenientString (Json.field "clustername" root)
                         ClusterID = Json.lenientString (Json.field "cluster_id" root)
                         Timestamp = if timestamp > 0L then Some(Time.fromUnixSeconds timestamp) else None
                         UUID = Json.lenientString (Json.field "uuid" root)
                         Payload = Json.rawOrEmpty (envelope.TryFind variant)
                         EnvelopeExtra = extra } |]

            Ctx.write ctx AgentMetadata.table rows

/// /api/v1/metadata: the inventory payloads. Each is an envelope of hostname,
/// timestamp and uuid plus ONE variant key; the cluster-agent's carry
/// clustername and cluster_id instead of hostname. None of the variants has
/// a published type, so each is stored as its JSON under its key.
let handleMetadata (body: byte[]) (ctx: HttpContext) : Task =
    if not (Diagnose.isSweep ctx) then
        storeMetadata body ctx

    statusOk ctx

// ---------------------------------------------------------------------------
// Endpoints the agent reads from: each needs a real answer, not a 202
// ---------------------------------------------------------------------------

/// GET /api/v2/validate — the trace-agent asks which org its key belongs to
/// (pkg/trace/api/opm.go) and wants {"data":{"id":"<org UUID>"}}. It hashes
/// the id into the Org Propagation Marker that tracers attach to the trace
/// context they pass on, so the id must be the same on every call: here it
/// is derived from the tenant. Two installations that both use the tenant
/// "default" therefore share a marker. After four failures the agent leaves
/// the marker unset for the life of the process.
let handleOpmValidate (_: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    let digest = SHA256.HashData(Encoding.UTF8.GetBytes("ninjacat-org:" + tenant))
    json {| data = {| id = Guid(digest.AsSpan(0, 16)).ToString() |} |} ctx

/// The SHA-256 of a delegated-auth proof, hex. The proof never leaves here.
let private proofFingerprint (proof: string) : string =
    if proof = "" then
        ""
    else
        Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes proof)).ToLowerInvariant()

/// POST /api/v2/intake-key: the delegated-auth exchange. The agent sends an
/// empty body with `Authorization: Delegated <proof>` and reads
/// {"data":{"attributes":{"api_key":"…"}}}; an empty api_key is an error on
/// its side. The key that admitted the request is what it gets back,
/// whether it came in the header or in the query. The proof IS the
/// credential, so only its fingerprint is stored.
let handleIntakeKey (_: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    let authorization = Ctx.header ctx "Authorization"

    let scheme, proof =
        match authorization.IndexOf ' ' with
        | -1 -> authorization, ""
        | i -> authorization.Substring(0, i), authorization.Substring(i + 1)

    // The key the request was admitted with, as the authentication left it.
    match ctx.User.FindFirst "key_id" with
    | null -> ()
    | keyId ->
        let row: DelegatedAuthRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              Scheme = scheme
              ProofFingerprint = proofFingerprint proof
              APIKeyID = keyId.Value }

        Ctx.write ctx DelegatedAuth.table [| row |]

    let admittedWith = Text.firstNonEmpty [ Ctx.header ctx "Dd-Api-Key"; Ctx.query ctx "api_key" ]
    json {| data = {| attributes = {| api_key = admittedWith |} |} |} ctx

/// GET /api/v1/query, for the cluster-agent's External Metrics Provider.
/// Nothing is queryable here yet, so the answer is a well-formed EMPTY
/// result: it marks each metric invalid on the agent's side, which is the
/// truth, where a 202 would count as an API error.
let handleQuery (_: byte[]) (ctx: HttpContext) : Task =
    json
        {| status = "ok"
           res_type = "time_series"
           query = Ctx.query ctx "query"
           series = Array.empty<string> |}
        ctx

/// A single-resource JSON:API document, {"data":{"type","id","attributes"}},
/// read no further than the envelope: the concrete types belong to the agent.
type private JsonApiResource =
    { Id: string
      Attributes: Map<string, JsonElement> }

    member resource.Text(name: string) : string =
        Json.lenientString (resource.Attributes.TryFind name)

/// None when the body is not a JSON:API document.
let private tryJsonApi (body: byte[]) : JsonApiResource option =
    match Json.tryParse body with
    | Ok root when root.ValueKind = JsonValueKind.Object ->
        match (Json.members root).TryFind "data" with
        | Some data when data.ValueKind = JsonValueKind.Object || data.ValueKind = JsonValueKind.Null ->
            let fields = Json.members data

            Some
                { Id = Json.lenientString (fields.TryFind "id")
                  Attributes = Json.lenientMembers (fields.TryFind "attributes") }
        | _ -> None
    | _ -> None

/// The request's JSON:API resource. A body that is not one is kept raw, with
/// `note`, and gives None.
let private decodeJsonApi (body: byte[]) (ctx: HttpContext) (intake: string) (note: string) : JsonApiResource option =

    match tryJsonApi body with
    | Some resource -> Some resource
    | None ->
        Raw.store ctx intake "decode_error" note body
        None

/// POST /api/v2/profiles/symbols/query, from the host profiler's symbol
/// uploader. The reply lists the build ids already held, and the uploader
/// sends only what is missing; it reads the reply as a JSON:API array, so
/// anything else stops all uploads. We hold nothing, so the array is empty.
/// The question is kept: these build ids are the profiled binaries of the
/// fleet.
let handleSymbolsQuery (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "symbols" "not a JSON:API document" with
    | None -> ()
    | Some resource ->
        let row: SymbolQueryRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              Arch = resource.Text "arch"
              BuildIDs = Json.lenientStrings (resource.Attributes.TryFind "buildIds") |> Option.defaultValue [||]
              Resource = Text.utf8 body }

        Ctx.write ctx SymbolQueries.table [| row |]

    json {| data = List.empty<string> |} ctx

// ---------------------------------------------------------------------------
// Private Action Runner
// ---------------------------------------------------------------------------
//
// A pull loop steered entirely by our answers: enrollment hands out the
// identity every later request is signed with, dequeue hands out work,
// health-check hands out timing. Its request types are internal to the agent,
// so decoding stops at the JSON:API envelope and the attribute keys.

/// The numeric org_id the runner insists on, derived from the tenant
/// (FNV-64a): stable across restarts without a table, never zero, inside the
/// int64 the runner parses it into.
let tenantOrgId (tenant: string) : int64 =
    int64 (Fnv.hash64 (Encoding.UTF8.GetBytes tenant) >>> 1) ||| 1L

/// POST /api/unstable/on_prem_runners and its api_key_only variant.
///
/// The runner builds urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id>
/// from the reply, splits it on ":" and signs every later request with the
/// pair, so runner_id must not contain a colon. The status must be exactly
/// 200 (anything else is retried forever) and the reply's type must be
/// "createRunnerResponse".
let handleRunnerEnroll (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "par" "enroll: not a JSON:API document" with
    | None -> setStatusCode 400 ctx
    | Some resource ->
        let modes = Json.lenientStrings (resource.Attributes.TryFind "runner_modes")
        let runnerId = Guid.NewGuid().ToString()
        let orgId = tenantOrgId tenant

        let row: RunnerEnrollmentRow =
            { TenantID = tenant
              RunnerID = runnerId
              EnrolledAt = DateTime.UtcNow
              Name = resource.Text "runner_name"
              Modes = defaultArg modes [||]
              Host = resource.Text "runner_host"
              PublicKeyPEM = resource.Text "public_key_pem"
              AgentHostname = resource.Text "agent_hostname"
              OrchClusterID = resource.Text "orch_cluster_id"
              AgentFlavor = resource.Text "agent_flavor"
              OrgID = orgId
              Attributes = Json.compactObject resource.Attributes }

        Ctx.write ctx RunnerEnrollments.table [| row |]

        let reply =
                {| data =
                    {| ``type`` = "createRunnerResponse"
                       id = runnerId
                       attributes =
                        {| runner_id = runnerId
                           org_id = orgId
                           runner_modes = Option.toObj modes
                           agent_hostname = resource.Text "agent_hostname"
                           orch_cluster_id = resource.Text "orch_cluster_id"
                           agent_flavor = resource.Text "agent_flavor" |} |} |}

        // JSON:API's own media type, which Oxpecker's `json` would replace.
        (setContentType "application/vnd.api+json" >=> bytes (JsonSerializer.SerializeToUtf8Bytes reply)) ctx

/// The runner's poll for work. An EMPTY 200 means "no task" and is the normal
/// idle answer: a body would be parsed as a task, and any other status is an
/// error on the runner's side. Every poll is also the only liveness signal a
/// runner with no tasks produces.
let handleRunnerDequeue (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "par" "dequeue: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        // Both timestamps stay strings: their format is undocumented and
        // an unparseable value must not become 1970.
        let row: RunnerDequeueRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              RunnerStartedAt = resource.Text "runner_started_at"
              LastTaskReceivedAt = resource.Text "last_task_received_at"
              Version = Ctx.header ctx "X-Datadog-OnPrem-Version"
              Modes = Ctx.header ctx "X-Datadog-OnPrem-Modes" }

        Ctx.write ctx RunnerDequeues.table [| row |]

    setStatusCode 200 ctx
/// publish-task-update: the outcome of a task. The document id is
/// "succeed_task" or "fail_task"; `payload.outputs` is the action's own
/// result, arbitrary JSON of any size. The runner accepts any status.
let handleRunnerTaskUpdate (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "par" "task-update: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        let payload = Json.lenientMembers (resource.Attributes.TryFind "payload")

        // A numeric enum on the runner's side, and a succeeded task sends
        // none: absent must not become a 0 that reads as an error code.
        let errorCode =
            match payload.TryFind "error_code" with
            | Some code when code.ValueKind = JsonValueKind.Null -> Some 0L
            | Some code -> Json.int64Of code
            | None -> None

        let row: RunnerTaskUpdateRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              Outcome = resource.Id
              TaskID = resource.Text "task_id"
              ActionFQN = resource.Text "action_fqn"
              JobID = resource.Text "job_id"
              Client = Json.rawOrEmpty (resource.Attributes.TryFind "client")
              Branch = Json.lenientString (payload.TryFind "branch")
              Outputs = Json.rawOrEmpty (payload.TryFind "outputs")
              ErrorCode = errorCode
              ErrorDetails = Json.lenientString (payload.TryFind "error_details")
              APIError = Json.lenientString (payload.TryFind "api_error")
              Extra =
                mergeExtra
                    (extraRaw [ "task_id"; "client"; "action_fqn"; "job_id"; "payload" ] resource.Attributes)
                    "payload."
                    (extraRaw [ "branch"; "outputs"; "error_code"; "error_details"; "api_error" ] payload) }

        Ctx.write ctx RunnerTaskUpdates.table [| row |]

    accepted ctx
/// The per-task heartbeat. It must be answered with exactly 200: a 404 tells
/// the runner the job is gone and it stops heartbeating. The rows answer
/// "when did this job stop making progress" — a job that hangs never sends a
/// task update.
let handleRunnerHeartbeat (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "par" "heartbeat: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        let row: RunnerHeartbeatRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              TaskID = resource.Text "task_id"
              ActionFQN = resource.Text "action_fqn"
              JobID = resource.Text "job_id"
              Client = Json.rawOrEmpty (resource.Attributes.TryFind "client") }

        Ctx.write ctx RunnerHeartbeats.table [| row |]

    json {||} ctx
/// GET runner/health-check. The runner ignores the body and reads
/// X-Server-Time; with no X-Retry-After-Ms it polls at its default interval.
let handleRunnerHealthCheck (_: byte[]) (ctx: HttpContext) : Task =
    let now = DateTime.UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture)
    (setHttpHeader "X-Server-Time" now >=> json {||}) ctx

/// POST /api/v2/actions/connections: a connection a runner may use to reach a
/// third-party system. SENSITIVE: `integration.credentials` holds secrets and
/// is stored verbatim, because a connection without them cannot be used. It
/// must never be logged.
let handleActionConnections (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match decodeJsonApi body ctx "par" "connections: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        let integration = Json.lenientMembers (resource.Attributes.TryFind "integration")
        let tags = Json.lenientStrings (resource.Attributes.TryFind "tags") |> Option.defaultValue [||]

        let row: ActionConnectionRow =
            { TenantID = tenant
              At = DateTime.UtcNow
              Name = resource.Text "name"
              RunnerID = resource.Text "runner_id"
              Tags = Tags.toMultiMap tags
              IntegrationType = Json.lenientString (integration.TryFind "type")
              Credentials = Json.rawOrEmpty (integration.TryFind "credentials")
              Extra =
                mergeExtra
                    (extraRaw [ "name"; "runner_id"; "tags"; "integration" ] resource.Attributes)
                    "integration."
                    (extraRaw [ "type"; "credentials" ] integration) }

        Ctx.write ctx ActionConnections.table [| row |]

    accepted ctx