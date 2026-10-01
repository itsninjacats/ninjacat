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
open Microsoft.Extensions.Logging
open Datadog.Agentpayload
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.ApiPayloads
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The reply the agent expects after a batch of metrics. It reads the body:
/// an empty object would be an API error to it.
let private ackSeries = Response.json 202 """{"errors":[]}"""

let private statusOk = Response.json 200 """{"status":"ok"}"""

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
    values
    |> Map.filter (fun name _ -> not (List.contains name skip))
    |> Map.map (fun _ value -> GoJson.compact value)

/// The same for an envelope read one level down: the values byte for byte.
let private extraRaw (skip: string list) (values: Map<string, JsonElement>) : Map<string, string> =
    values
    |> Map.filter (fun name _ -> not (List.contains name skip))
    |> Map.map (fun _ value -> value.GetRawText())

/// Folds a nested object's extras into the outer ones under a prefix, so two
/// levels of one document share a column without colliding.
let private mergeExtra (outer: Map<string, string>) (prefix: string) (nested: Map<string, string>) : Map<string, string> =
    nested |> Map.fold (fun merged name value -> merged.Add(prefix + name, value)) outer

/// The text of a member as it arrived; "" when the key is absent, which is
/// not the same statement as "{}".
let private rawText (name: string) (values: Map<string, JsonElement>) : string =
    match values.TryFind name with
    | Some value -> value.GetRawText()
    | None -> ""

/// Go's time holds any int64 of seconds; DateTime stops at the year 9999, so
/// a later value becomes the last second there is.
let private lastSecond = 253402300799L

let private unixSeconds (seconds: int64) : DateTime =
    Time.fromUnixSeconds (min seconds lastSecond)

let private wireSeconds (seconds: int64) : DateTime = Time.wireSeconds (min seconds lastSecond)

/// A float timestamp as whole seconds, the way Go's int64() converts on
/// amd64: the fraction dropped, and anything an int64 cannot hold becomes
/// the minimum — which wireSeconds then reads as "not supplied".
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
let handleSeriesV2 (r: Request) : Response =
    if Diagnose.isSweep r then
        ackSeries
    else
        let decoded =
            if Body.isJson (r.Header "Content-Type") r.Body then
                parseSeriesV2Json r.Body
            else
                parseSeriesV2Protobuf r.Body

        match decoded with
        | Error error ->
            r.Log.LogWarning("[v2/series] {Error}", error)
            Raw.store r "series" "decode_error" error r.Body
        | Ok payload ->
            if r.Tenant <> "" then
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
                            { Metrics.point r.Tenant (wireSeconds point.Timestamp) series.Metric point.Value with
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

                if r.Log.IsEnabled LogLevel.Debug then
                    // The agent also writes Origin field 3 (metric_type = 9,
                    // "do not index"), which the published proto reserves. A
                    // reserved field has no name to decode into, so it is
                    // counted, not stored.
                    let doNotIndex =
                        payload.Series
                        |> Seq.filter (fun series ->
                            not (isNull series.Metadata)
                            && not (isNull series.Metadata.Origin)
                            && hasUnknownFields Origin.Parser series.Metadata.Origin)
                        |> Seq.length

                    if doNotIndex > 0 then
                        r.Log.LogDebug("[v2/series] {Count} series flagged do-not-index by the agent (Origin field 3)", doNotIndex)

                Sink.write r.Sink Metrics.table (points.ToArray())

        ackSeries

/// The v1 wire's type word; an unknown word stays UNSPECIFIED rather than
/// being guessed at.
let normalizeTypeWord (word: string) : string =
    match word.ToLowerInvariant() with
    | "count" -> "COUNT"
    | "rate" -> "RATE"
    | "gauge" -> "GAUGE"
    | _ -> "UNSPECIFIED"

let private additionalString (name: string) (values: Map<string, JsonElement>) : string =
    GoJson.lenientString (values.TryFind name)

/// /api/v1/series: the older public API, JSON only. The agent's v1 encoder
/// also writes device, source_type_name and unit, which the model does not
/// declare: the last two have columns, device goes into the resources map
/// under the name v2 uses for it, anything else undeclared into `extra`.
let handleSeriesV1 (r: Request) : Response =
    if Diagnose.isSweep r then
        ackSeries
    else
        match parseSeriesV1 r.Body with
        | Error error ->
            r.Log.LogWarning("[v1/series] {Error}", error)
            Raw.store r "series" "decode_error" error r.Body
        | Ok payload ->
            if payload.Unparsed then
                r.Log.LogWarning("[v1/series] the payload did not fit datadogV1.MetricsPayload, kept raw")
                Raw.store r "series" "unexpected_shape" "v1 payload did not fit datadogV1.MetricsPayload" r.Body

            if not payload.Additional.IsEmpty then
                // A key beside `series` belongs to the batch, not to a point:
                // copying it onto every row would multiply it. The body is
                // kept instead, once.
                Raw.store
                    r
                    "series"
                    "unexpected_shape"
                    ("v1 payload carried undeclared top-level keys: " + keyList payload.Additional)
                    r.Body

            for at, item in payload.Rejected do
                r.Log.LogWarning("[v1/series] series #{Index} is not a datadogV1.Series, kept raw", at)
                Raw.store r "series" "unexpected_shape" $"v1 series #{at} is not a datadogV1.Series" (GoJson.rawBytes item)

            if r.Tenant <> "" then
                let points = ResizeArray<MetricPoint>()
                let mutable badPoints = 0

                for i in 0 .. payload.Series.Length - 1 do
                    let series = payload.Series[i]
                    let at = payload.Positions[i]

                    if series.Unparsed then
                        r.Log.LogWarning("[v1/series] series #{Index} did not fit datadogV1.Series, kept raw", at)
                        Raw.store r "series" "unexpected_shape" $"v1 series #{at} did not fit datadogV1.Series" (GoJson.rawBytes series.Raw)
                    else
                        let interval =
                            match series.Interval with
                            | Some seconds -> uint32 seconds
                            | None -> 0u

                        let resources =
                            match additionalString "device" series.Additional with
                            | "" -> Map.empty
                            | device -> Map.ofList [ "device", device ]

                        let extra = extraEncoded [ "device"; "source_type_name"; "unit" ] series.Additional
                        let tags = Tags.toMultiMap series.Tags

                        for j in 0 .. series.Points.Length - 1 do
                            let point = series.Points[j]

                            match point.Pair |> Array.truncate 2 with
                            | [| Some timestamp; Some value |] ->
                                points.Add
                                    { Metrics.point r.Tenant (wireSeconds (wholeSeconds timestamp)) series.Metric value with
                                        Host = series.Host
                                        MetricType = normalizeTypeWord series.Type
                                        SourceType = additionalString "source_type_name" series.Additional
                                        Unit = additionalString "unit" series.Additional
                                        Interval = interval
                                        Tags = tags
                                        Resources = resources
                                        Extra = extra }
                            | _ ->
                                // The series is stored through its other
                                // points; the pair is the smallest thing
                                // worth keeping.
                                badPoints <- badPoints + 1

                                Raw.store
                                    r
                                    "series"
                                    "unexpected_shape"
                                    $"v1 series #{at} ({series.Metric}) point #{j} has a nil timestamp or value"
                                    (GoJson.rawBytes point.Raw)

                if badPoints > 0 then
                    r.Log.LogWarning("[v1/series] {Count} points had a nil timestamp or value, kept raw", badPoints)

                Sink.write r.Sink Metrics.table (points.ToArray())

        ackSeries

/// Who sent a batch, as opposed to what was in it: agent version, timezone,
/// the agent's clock and both of its IPs. The api_key in the same block is a
/// credential and is not carried over. An all-empty block is not stored.
let private storeBatchMetadata (r: Request) (intake: string) (sender: CommonMetadata) : unit =
    let empty =
        sender.AgentVersion = ""
        && sender.Timezone = ""
        && sender.CurrentEpoch = 0.0
        && sender.InternalIp = ""
        && sender.PublicIp = ""

    if r.Tenant <> "" && not empty then
        let row: AgentBatchMetadataRow =
            { TenantID = r.Tenant
              ReceivedAt = DateTime.UtcNow
              Intake = intake
              AgentVersion = sender.AgentVersion
              Timezone = sender.Timezone
              CurrentEpoch = sender.CurrentEpoch
              InternalIP = sender.InternalIp
              PublicIP = sender.PublicIp }

        Sink.write r.Sink AgentBatchMetadata.table [| row |]

/// /api/beta/sketches: DDSketch histograms the agent has already built. One
/// row per dogsketch, not per metric: each is its own point in time.
///
/// A sketch may also carry `distributions`, the pre-DDSketch encoding (raw
/// values and ranks). The current agent never writes it and it cannot become
/// a bucket row without inventing numbers, so such a sketch is kept raw,
/// whole.
let handleSketches (r: Request) : Response =
    if Diagnose.isSweep r then
        ackSeries
    else
        let decoded =
            if Body.isJson (r.Header "Content-Type") r.Body then
                parseSketchesJson r.Body
            else
                parseSketchesProtobuf r.Body

        match decoded with
        | Error error ->
            r.Log.LogWarning("[sketches] {Error}", error)
            Raw.store r "sketches" "decode_error" error r.Body
        | Ok payload ->
            if r.Tenant <> "" then
                let rows = ResizeArray<SketchRow>()

                for sketch in payload.Sketches do
                    if sketch.Distributions.Count > 0 then
                        r.Log.LogWarning("[sketches] legacy Distribution for {Metric}, kept raw", sketch.Metric)

                        Raw.store
                            r
                            "sketches"
                            "no_schema"
                            $"legacy pre-DDSketch Distribution for {sketch.Metric} has no row shape"
                            (sketchJson sketch)

                    let origin =
                        if isNull sketch.Metadata || isNull sketch.Metadata.Origin then
                            Origin()
                        else
                            sketch.Metadata.Origin

                    let tags = Tags.toMultiMap sketch.Tags

                    for dogsketch in sketch.Dogsketches do
                        rows.Add
                            { TenantID = r.Tenant
                              Timestamp = wireSeconds dogsketch.Ts
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

                Sink.write r.Sink Sketches.table (rows.ToArray())
                storeBatchMetadata r "sketches" (if isNull payload.Metadata then CommonMetadata() else payload.Metadata)

        ackSeries

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

    if values.Length = 0 then None else Some(wireSeconds (wholeSeconds timestamp), values)

/// /api/v1/distribution_points. The agent never calls this — it sends
/// finished sketches. Clients send RAW VALUES here, so the sketch is built on
/// our side, with the agent's own bucketing so both sources merge by key.
///
/// The raw values are not kept: a distribution exists to be merged with the
/// agent's sketches, which arrive already bucketed. What the model could not
/// fit is kept whole: a body with no series list, a series that fit no
/// series, a pair that could not be split.
let handleDistributionPoints (r: Request) : Response =
    if Diagnose.isSweep r then
        ackSeries
    else
        match parseDistributionPoints r.Body with
        | Error error ->
            r.Log.LogWarning("[distribution_points] {Error}", error)
            Raw.store r "distribution_points" "decode_error" error r.Body
        | Ok payload ->
            if payload.Unparsed then
                r.Log.LogWarning("[distribution_points] the payload did not fit the model, kept raw")

                Raw.store
                    r
                    "distribution_points"
                    "unexpected_shape"
                    "payload did not fit datadogV1.DistributionPointsPayload"
                    r.Body

            if not payload.Additional.IsEmpty then
                Raw.store
                    r
                    "distribution_points"
                    "unexpected_shape"
                    ("payload carried undeclared top-level keys: " + keyList payload.Additional)
                    r.Body

            for at, item in payload.Rejected do
                r.Log.LogWarning("[distribution_points] series #{Index} is not a series, kept raw", at)

                Raw.store
                    r
                    "distribution_points"
                    "unexpected_shape"
                    $"series #{at} is not a datadogV1.DistributionPointsSeries"
                    (GoJson.rawBytes item)

            if r.Tenant <> "" then
                let rows = ResizeArray<SketchRow>()
                let mutable badPairs = 0

                for i in 0 .. payload.Series.Length - 1 do
                    let series = payload.Series[i]
                    let at = payload.Positions[i]

                    if series.Unparsed then
                        // When only the `type` word was wrong the points came
                        // through and still become rows: a typo there must
                        // not cost a host its distribution.
                        r.Log.LogWarning("[distribution_points] series #{Index} did not fit the model, kept raw", at)

                        Raw.store
                            r
                            "distribution_points"
                            "unexpected_shape"
                            $"series #{at} did not fit datadogV1.DistributionPointsSeries"
                            (GoJson.rawBytes series.Raw)

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
                                { TenantID = r.Tenant
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
                            badPairs <- badPairs + 1

                            Raw.store
                                r
                                "distribution_points"
                                "unexpected_shape"
                                $"series #{at} ({series.Metric}) point #{j} is not a [timestamp, [values]] pair"
                                (GoJson.rawBytes point.Raw)

                if badPairs > 0 then
                    r.Log.LogWarning("[distribution_points] {Count} points were not [timestamp, [values]] pairs, kept raw", badPairs)

                Sink.write r.Sink Sketches.table (rows.ToArray())

        ackSeries

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
let handleCheckRun (r: Request) : Response =
    if Diagnose.isSweep r then
        ackSeries
    else
        match parseCheckRuns r.Body with
        | Error error ->
            r.Log.LogWarning("[check_run] {Error}", error)
            Raw.store r "check_run" "decode_error" error r.Body
        | Ok checks ->
            if not checks.Undecodable.IsEmpty then
                r.Log.LogWarning("[check_run] {Count} checks could not be decoded, kept raw", checks.Undecodable.Length)

            checks.Undecodable
            |> List.iteri (fun i item ->
                Raw.store r "check_run" "unexpected_shape" $"check #{i} is not a datadogV1.ServiceCheck" (GoJson.rawBytes item))

            if r.Tenant <> "" then
                let rows = ResizeArray<CheckRunRow>()

                checks.Runs
                |> List.iteri (fun i run ->
                    if run.Unparsed then
                        r.Log.LogWarning("[check_run] check #{Index} did not fit datadogV1.ServiceCheck, kept raw", i)

                        Raw.store
                            r
                            "check_run"
                            "unexpected_shape"
                            $"check #{i} did not fit datadogV1.ServiceCheck"
                            (GoJson.rawBytes run.Raw)
                    else
                        rows.Add
                            { TenantID = r.Tenant
                              Timestamp = wireSeconds run.Timestamp
                              CheckName = run.Check
                              Host = run.HostName
                              Status = statusName run.Status
                              Message = run.Message
                              Tags = Tags.toMultiMap run.Tags
                              Extra = extraEncoded [] run.Additional })

                Sink.write r.Sink Checks.table (rows.ToArray())

        ackSeries

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
let handleEvents (r: Request) : Response =
    if Diagnose.isSweep r then
        Response.json 202 """{"status":"ok"}"""
    else
        match parseEvents r.Body with
        | Error error ->
            r.Log.LogWarning("[events] {Error}", error)
            Raw.store r "events" "decode_error" error r.Body
            Response.jsonOf 400 {| status = "error"; error = error |}
        | Ok [] -> Response.jsonOf 400 {| status = "error"; error = "no event with a title" |}
        | Ok events ->
            let rows = ResizeArray<EventRow>()
            // Every event gets an id, stored or not: the reply carries the
            // first one's.
            let ids = events |> List.map (fun _ -> newEventId ())
            let firstId = snd ids.Head

            List.zip events ids
            |> List.iteri (fun i (event, (id, number)) ->
                if event.Unparsed then
                    // Nothing decoded, or an alert_type or priority outside
                    // the enum. Coercing an event whose own words could not
                    // be read would put a guess in a column.
                    r.Log.LogWarning("[events] event #{Index} did not fit datadogV1.EventCreateRequest, kept raw", i)

                    Raw.store
                        r
                        "events"
                        "unexpected_shape"
                        $"event #{i} did not fit datadogV1.EventCreateRequest"
                        (GoJson.rawBytes event.Raw)
                else
                    rows.Add
                        { TenantID = r.Tenant
                          Timestamp = wireSeconds event.DateHappened
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
                          Extra = extraEncoded [] event.Additional })

            if r.Tenant <> "" then
                Sink.write r.Sink Events.table (rows.ToArray())

            let first = events.Head

            Response.jsonOf
                202
                {| status = "ok"
                   event =
                    {| id = firstId
                       id_str = string firstId
                       title = first.Title
                       text = first.Text
                       date_happened = DateTimeOffset(wireSeconds first.DateHappened).ToUnixTimeSeconds()
                       host = first.Host
                       alert_type = coerceAlertType first.AlertType
                       priority = coercePriority (defaultArg first.Priority "")
                       tags = Option.toObj first.Tags
                       source_type_name = first.SourceTypeName
                       device_name = first.DeviceName
                       url = "" |} |}

// ---------------------------------------------------------------------------
// /intake/: the Agent v5 endpoint
// ---------------------------------------------------------------------------

let private eventText (name: string) (fields: Map<string, JsonElement>) : string =
    GoJson.lenientString (fields.TryFind name)

/// One agent event (pkg/metrics/event.Event) as the shared events row. The
/// agent's names differ from the public API's for the same three things:
/// msg_title, msg_text and timestamp against title, text and date_happened.
///
/// `source` is the key the agent grouped the event under and stands in when
/// the event does not repeat it; likewise the payload's host stands in for an
/// event with none of its own.
let intakeEventRow (tenant: string) (source: string) (payloadHost: string) (event: JsonElement) : EventRow =
    let fields = GoJson.members event
    let id, number = newEventId ()
    let alertRaw = eventText "alert_type" fields
    let priorityRaw = eventText "priority" fields

    let tags =
        match fields.TryFind "tags" with
        | Some list when list.ValueKind = JsonValueKind.Array -> list.EnumerateArray() |> Seq.choose GoJson.asString |> List.ofSeq
        | _ -> []

    { TenantID = tenant
      Timestamp = wireSeconds (GoJson.lenientInt64 (fields.TryFind "timestamp"))
      EventID = id
      EventIDNum = number
      Title = eventText "msg_title" fields
      Text = eventText "msg_text" fields
      Host = Text.firstNonEmpty [ eventText "host" fields; payloadHost ]
      AlertType = coerceAlertType alertRaw
      Priority = coercePriority priorityRaw
      AggregationKey = eventText "aggregation_key" fields
      SourceTypeName = Text.firstNonEmpty [ eventText "source_type_name" fields; source ]
      DeviceName = ""
      Tags = Tags.toMultiMap tags
      EventType = eventText "event_type" fields
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
        // The texts are Go's own, path included, for the raw row's note.
        let problem (source: string) (list: JsonElement) : string option =
            match list.ValueKind with
            | JsonValueKind.Null -> None
            | JsonValueKind.Array ->
                list.EnumerateArray()
                |> Seq.indexed
                |> Seq.tryFind (fun (_, event) -> event.ValueKind <> JsonValueKind.Object && event.ValueKind <> JsonValueKind.Null)
                |> Option.map (fun (i, event) ->
                    $"json: cannot unmarshal {GoJson.kindName event} into .{source}.{i} of type map[string]interface {{}}")
            | _ ->
                Some
                    $"json: cannot unmarshal {GoJson.kindName list} into Go struct field .{source} of type []map[string]interface {{}}"

        match events.EnumerateObject() |> Seq.tryPick (fun source -> problem source.Name source.Value) with
        | Some error -> Error error
        | None ->
            GoJson.members events
            |> Map.map (fun _ list -> if list.ValueKind = JsonValueKind.Array then List.ofSeq (list.EnumerateArray()) else [])
            |> Ok
    | _ -> Error(GoJson.mismatch events "map[string][]map[string]interface {}")

/// The events variant: {"events": {<source>: [...]}, "internalHostname"}.
/// The rows go to the same table as /api/v1/events.
let private intakeEvents (r: Request) (envelope: Map<string, JsonElement>) : unit =
    let events = envelope["events"]

    match eventsBySource events with
    | Error error ->
        r.Log.LogWarning("[intake] events: {Error}", error)
        Raw.store r "intake" "unexpected_shape" ("events is not a source -> events map: " + error) (GoJson.rawBytes events)
    | Ok bySource ->
        if r.Tenant <> "" then
            let host = GoJson.lenientString (envelope.TryFind "internalHostname")

            // By source in sorted order, so two replays of one payload give
            // the same rows.
            let rows =
                [| for pair in bySource do
                       for event in pair.Value do
                           intakeEventRow r.Tenant pair.Key host event |]

            Sink.write r.Sink Events.table rows

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
        if i < positional.Length then GoJson.lenientString (Some positional[i]) else ""

    { TenantID = tenant
      ReceivedAt = at
      Hostname = host
      AgentVersion = version
      UUID = uuid
      CheckName = text 0
      SourceType = text 1
      InstanceID = text 2
      Status = if positional.Length > 3 then GoJson.asInt64 positional[3] else None
      Message = text 4
      PositionalExtra = if positional.Length > 5 then GoJson.compactArray (Array.skip 5 positional) else ""
      Meta = meta }

let private arrayItems (value: JsonElement option) : JsonElement list =
    match value with
    | Some list when list.ValueKind = JsonValueKind.Array -> List.ofSeq (list.EnumerateArray())
    | _ -> []

/// The agent_checks variant (the V5 collector's payload): check statuses as
/// positional arrays, and external_host_tags, [[hostname, {source: [tags]}]]
/// — tags for hosts the agent is NOT running on, which is why they get a
/// table of their own.
let private intakeAgentChecks (r: Request) (envelope: Map<string, JsonElement>) : unit =
    let checks = arrayItems (envelope.TryFind "agent_checks")

    let externalTags =
        arrayItems (envelope.TryFind "external_host_tags")
        |> List.mapi (fun i entry ->
            if entry.ValueKind = JsonValueKind.Array && entry.GetArrayLength() = 2 then
                Some(GoJson.lenientString (Some entry[0]), GoJson.lenientStringLists (Some entry[1]))
            else
                r.Log.LogWarning("[intake] external_host_tags[{Index}] is not a [hostname, tags] pair", i)

                Raw.store
                    r
                    "intake"
                    "unexpected_shape"
                    $"external_host_tags[{i}] is not a [hostname, tags] pair"
                    (GoJson.rawBytes entry)

                None)

    if r.Tenant <> "" then
        let now = DateTime.UtcNow
        let host = GoJson.lenientString (envelope.TryFind "internalHostname")
        let version = GoJson.lenientString (envelope.TryFind "agentVersion")
        let uuid = GoJson.lenientString (envelope.TryFind "uuid")
        let meta = rawText "meta" envelope
        let checkRows = ResizeArray<AgentCheckRow>()

        checks
        |> List.iteri (fun i check ->
            if check.ValueKind = JsonValueKind.Array then
                checkRows.Add(agentCheckRow r.Tenant now host version uuid meta (Array.ofSeq (check.EnumerateArray())))
            else
                r.Log.LogWarning("[intake] agent_checks[{Index}] is not an array", i)
                Raw.store r "intake" "unexpected_shape" $"agent_checks[{i}] is not a positional array" (GoJson.rawBytes check))

        Sink.write r.Sink AgentChecks.table (checkRows.ToArray())

        // One row per SOURCE: a host carries several at once, and flattening
        // them would lose which of them said what.
        let tagRows: ExternalHostTagsRow[] =
            [| for entry in externalTags do
                   match entry with
                   | None -> ()
                   | Some(describedHost, bySource) ->
                       for pair in bySource do
                           { TenantID = r.Tenant
                             ReceivedAt = now
                             Host = describedHost
                             Source = pair.Key
                             Tags = Tags.toMultiMap pair.Value } |]

        Sink.write r.Sink ExternalHostTags.table tagRows

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
        gohai.TryFind name |> Option.bind GoJson.asStringMap |> Option.defaultValue Map.empty

    // Absent stays None; null reads as false, as Go's decoder leaves it.
    let flag (name: string) : uint8 option =
        match envelope.TryFind name with
        | Some value when value.ValueKind = JsonValueKind.True -> Some 1uy
        | Some value when value.ValueKind = JsonValueKind.False || value.ValueKind = JsonValueKind.Null -> Some 0uy
        | _ -> None

    let objectMap (name: string) : Map<string, string> =
        extraRaw [] (GoJson.lenientMembers (envelope.TryFind name))

    { TenantID = tenant
      Host = hostname
      SeenAt = seenAt
      AgentVersion = agentVersion
      OS = osName
      Platform = section "platform"
      CPU = section "cpu"
      Memory = section "memory"
      Tags = Tags.toMultiMap (hostTags.TryFind "system" |> Option.defaultValue [||])
      UUID = GoJson.lenientString (envelope.TryFind "uuid")
      AgentFlavor = GoJson.lenientString (envelope.TryFind "agent-flavor")
      PythonVersion = GoJson.lenientString (envelope.TryFind "python")
      Meta = rawText "meta" envelope
      Network = rawText "network" envelope
      Filesystem = rawText "filesystem" gohai
      Logs = rawText "logs" envelope
      OTLP = rawText "otlp" envelope
      SystemStats = objectMap "systemStats"
      InstallMethod = objectMap "install-method"
      ProxyInfo = objectMap "proxy-info"
      ContainerMeta = objectMap "container-meta"
      FIPSMode = flag "fips_mode"
      FIPSProxyEnabled = flag "fips_proxy_enabled"
      HostTags = hostTags
      GohaiExtra = extraRaw gohaiKnownSections gohai
      IntakeExtra = extraRaw hostKnownKeys envelope
      Resources = rawText "resources" envelope }

/// The host metadata variant. The payload is loose and shifts between agent
/// versions, so fields are read by name.
let private intakeHost (r: Request) (envelope: Map<string, JsonElement>) : unit =
    let hostname = GoJson.lenientString (envelope.TryFind "internalHostname")

    if r.Tenant <> "" && hostname <> "" then
        let gohai =
            match GoJson.lenientString (envelope.TryFind "gohai") with
            | "" -> Map.empty
            | inner ->
                match GoJson.parse (Encoding.UTF8.GetBytes inner) with
                | Ok sections -> GoJson.members sections
                | Error _ -> Map.empty

        let row =
            hostRow
                r.Tenant
                hostname
                (GoJson.lenientString (envelope.TryFind "agentVersion"))
                (GoJson.lenientString (envelope.TryFind "os"))
                DateTime.UtcNow
                envelope
                gohai
                (GoJson.lenientStringLists (envelope.TryFind "host-tags"))

        Sink.write r.Sink Hosts.table [| row |]

/// One row of a resources snapshot, as gohai writes it:
/// [usernames, cpu %, memory %, vms, rss, name, number of pids]
/// (pkg/gohai/processes). None when it is not that.
let private processGroup (tenant: string) (host: string) (at: DateTime) (fields: JsonElement) : ProcessGroupRow option =
    if fields.ValueKind <> JsonValueKind.Array || fields.GetArrayLength() <> 7 then
        None
    else
        let field (i: int) = fields[i]

        match GoJson.asString (field 0), GoJson.asFloat (field 1), GoJson.asFloat (field 2), GoJson.asString (field 5) with
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
let private intakeResources (r: Request) (resources: JsonElement) : unit =
    let host =
        GoJson.field "meta" resources |> Option.bind (GoJson.field "host") |> Option.bind GoJson.asString |> Option.defaultValue ""

    let snaps =
        match GoJson.field "processes" resources |> Option.bind (GoJson.field "snaps") with
        | Some list when list.ValueKind = JsonValueKind.Array -> Some(List.ofSeq (list.EnumerateArray()))
        | _ -> None

    match snaps with
    | None -> Raw.store r "intake" "unexpected_shape" "resources without processes.snaps" r.Body
    | Some snaps ->
        let rows = ResizeArray<ProcessGroupRow>()
        let mutable unread = 0

        for snap in snaps do
            let groups =
                if snap.ValueKind = JsonValueKind.Array && snap.GetArrayLength() = 2 && snap[1].ValueKind = JsonValueKind.Array then
                    GoJson.asInt64 snap[0] |> Option.map (fun seconds -> Time.fromUnixSeconds seconds, snap[1])
                else
                    None

            match groups with
            | None -> unread <- unread + 1
            | Some(at, list) ->
                for fields in list.EnumerateArray() do
                    match processGroup r.Tenant host at fields with
                    | Some row -> rows.Add row
                    | None -> unread <- unread + 1

        if unread > 0 then
            r.Log.LogWarning("[intake] resources: {Count} snapshots or rows not read, kept raw", unread)
            Raw.store r "intake" "unexpected_shape" $"resources: {unread} snapshots or rows are not the resources check's shape" r.Body

        if r.Tenant <> "" then
            Sink.write r.Sink ProcessGroups.table (rows.ToArray())

/// Four producers share /intake/ and nothing but the path; they are told
/// apart by their top-level keys, in this order:
///
///   events                 agent events, the shutdown event among them
///   agent_checks           the V5 collector: check statuses, external host tags
///   gohai or systemStats   host metadata
///   resources alone        the resources check's process groups
let private decodeIntake (r: Request) : unit =
    let undecodable (error: string) =
        r.Log.LogWarning("[intake] JSON: {Error}", error)
        Raw.store r "intake" "decode_error" error r.Body

    match GoJson.parse r.Body with
    | Error error -> undecodable error
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        undecodable (GoJson.mismatch root "map[string]jsontext.Value")
    | Ok root ->
        let envelope = GoJson.members root

        if envelope.ContainsKey "events" then
            intakeEvents r envelope
        elif envelope.ContainsKey "agent_checks" then
            intakeAgentChecks r envelope
        elif envelope.ContainsKey "gohai" || envelope.ContainsKey "systemStats" then
            intakeHost r envelope
        elif envelope.ContainsKey "resources" && envelope.Count = 1 then
            intakeResources r envelope["resources"]
        else
            r.Log.LogWarning("[intake] unrecognised shape, keys: {Keys}", keyList envelope)
            Raw.store r "intake" "unexpected_shape" ("no known /intake/ variant key, keys: " + keyList envelope) r.Body

let handleIntake (r: Request) : Response =
    if not (Diagnose.isSweep r) then
        decodeIntake r

    statusOk

// ---------------------------------------------------------------------------
// /api/v1/metadata
// ---------------------------------------------------------------------------

/// The variant keys of /api/v1/metadata, one per producer (comp/metadata/*).
let private metadataVariants =
    [ "agent_metadata"; "host_metadata"; "check_metadata"; "logs_metadata"; "files_metadata"
      "system_probe_metadata"; "security_agent_metadata"; "signing_metadata"; "host_system_info_metadata"
      "host_gpu_metadata"; "ha_agent_metadata"; "datadog_cluster_agent_metadata"; "clustercheck_metadata" ]

let private metadataEnvelopeKeys = [ "hostname"; "clustername"; "cluster_id"; "timestamp"; "uuid" ]

let private storeMetadata (r: Request) : unit =
    let undecodable (error: string) =
        r.Log.LogWarning("[metadata] JSON: {Error}", error)
        Raw.store r "metadata" "decode_error" error r.Body

    match GoJson.parse r.Body with
    | Error error -> undecodable error
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        undecodable (GoJson.mismatch root "map[string]jsontext.Value")
    | Ok root ->
        let envelope = GoJson.members root
        let found = metadataVariants |> List.filter envelope.ContainsKey

        if found.IsEmpty then
            // A producer we have never seen, or a variant key renamed
            // between releases.
            r.Log.LogWarning("[metadata] no known variant key, keys: {Keys}", keyList envelope)

            Raw.store
                r
                "metadata"
                "no_schema"
                ("no known /api/v1/metadata variant key, keys: " + keyList envelope)
                r.Body
        elif r.Tenant <> "" then
            let timestamp = GoJson.lenientInt64 (GoJson.field "timestamp" root)
            let now = DateTime.UtcNow
            // Every key that is neither the shared envelope nor a variant of
            // this request — clustercheck_status and its sibling among them.
            let extra = extraRaw (metadataEnvelopeKeys @ found) envelope

            // One row per variant key present: `variant` stays a real filter.
            let rows: AgentMetadataRow[] =
                [| for variant in found ->
                       { TenantID = r.Tenant
                         ReceivedAt = now
                         Variant = variant
                         Hostname = GoJson.lenientString (GoJson.field "hostname" root)
                         ClusterName = GoJson.lenientString (GoJson.field "clustername" root)
                         ClusterID = GoJson.lenientString (GoJson.field "cluster_id" root)
                         Timestamp = if timestamp > 0L then Some(unixSeconds timestamp) else None
                         UUID = GoJson.lenientString (GoJson.field "uuid" root)
                         Payload = rawText variant envelope
                         EnvelopeExtra = extra } |]

            Sink.write r.Sink AgentMetadata.table rows

/// /api/v1/metadata: the inventory payloads. Each is an envelope of hostname,
/// timestamp and uuid plus ONE variant key; the cluster-agent's carry
/// clustername and cluster_id instead of hostname. None of the variants has
/// a published type, so each is stored as its JSON under its key.
let handleMetadata (r: Request) : Response =
    if not (Diagnose.isSweep r) then
        storeMetadata r

    statusOk

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
let handleOpmValidate (r: Request) : Response =
    let digest = SHA256.HashData(Encoding.UTF8.GetBytes("ninjacat-org:" + r.Tenant))
    Response.jsonOf 200 {| data = {| id = Guid(digest.AsSpan(0, 16)).ToString() |} |}

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
let handleIntakeKey (r: Request) : Response =
    let authorization = r.Header "Authorization"

    let scheme, proof =
        match authorization.IndexOf ' ' with
        | -1 -> authorization, ""
        | i -> authorization.Substring(0, i), authorization.Substring(i + 1)

    match r.Key with
    | Some key when key.TenantID <> "" ->
        let row: DelegatedAuthRow =
            { TenantID = key.TenantID
              At = DateTime.UtcNow
              Scheme = scheme
              ProofFingerprint = proofFingerprint proof
              APIKeyID = key.ID }

        Sink.write r.Sink DelegatedAuth.table [| row |]
    | _ -> ()

    let admittedWith = Text.firstNonEmpty [ r.Header "Dd-Api-Key"; r.Query "api_key" ]
    Response.jsonOf 200 {| data = {| attributes = {| api_key = admittedWith |} |} |}

/// GET /api/v1/query, for the cluster-agent's External Metrics Provider.
/// Nothing is queryable here yet, so the answer is a well-formed EMPTY
/// result: it marks each metric invalid on the agent's side, which is the
/// truth, where a 202 would count as an API error.
let handleQuery (r: Request) : Response =
    Response.jsonOf
        200
        {| status = "ok"
           res_type = "time_series"
           query = r.Query "query"
           series = Array.empty<string> |}

/// A single-resource JSON:API document, {"data":{"type","id","attributes"}},
/// read no further than the envelope: the concrete types belong to the agent.
type private JsonApiResource =
    { Id: string
      Attributes: Map<string, JsonElement> }

    member resource.Text(name: string) : string =
        GoJson.lenientString (resource.Attributes.TryFind name)

/// None when the body is not a JSON:API document.
let private tryJsonApi (body: byte[]) : JsonApiResource option =
    match GoJson.parse body with
    | Ok root when root.ValueKind = JsonValueKind.Object ->
        match (GoJson.members root).TryFind "data" with
        | Some data when data.ValueKind = JsonValueKind.Object || data.ValueKind = JsonValueKind.Null ->
            let fields = GoJson.members data

            Some
                { Id = GoJson.lenientString (fields.TryFind "id")
                  Attributes = GoJson.lenientMembers (fields.TryFind "attributes") }
        | _ -> None
    | _ -> None

/// The request's JSON:API resource. A body that is not one is kept raw, with
/// `note`, and gives None.
let private decodeJsonApi (r: Request) (intake: string) (note: string) : JsonApiResource option =
    match tryJsonApi r.Body with
    | Some resource -> Some resource
    | None ->
        r.Log.LogWarning("[{Intake}] {Note}", intake, note)
        Raw.store r intake "decode_error" note r.Body
        None

/// POST /api/v2/profiles/symbols/query, from the host profiler's symbol
/// uploader. The reply lists the build ids already held, and the uploader
/// sends only what is missing; it reads the reply as a JSON:API array, so
/// anything else stops all uploads. We hold nothing, so the array is empty.
/// The question is kept: these build ids are the profiled binaries of the
/// fleet.
let handleSymbolsQuery (r: Request) : Response =
    match decodeJsonApi r "symbols" "not a JSON:API document" with
    | None -> ()
    | Some resource ->
        if r.Tenant <> "" then
            let row: SymbolQueryRow =
                { TenantID = r.Tenant
                  At = DateTime.UtcNow
                  Arch = resource.Text "arch"
                  BuildIDs = GoJson.lenientStrings (resource.Attributes.TryFind "buildIds") |> Option.defaultValue [||]
                  Resource = Text.utf8 r.Body }

            Sink.write r.Sink SymbolQueries.table [| row |]

    Response.json 200 """{"data":[]}"""

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
    let mutable hash = 14695981039346656037UL

    for b in Encoding.UTF8.GetBytes tenant do
        hash <- (hash ^^^ uint64 b) * 1099511628211UL

    int64 (hash >>> 1) ||| 1L

/// POST /api/unstable/on_prem_runners and its api_key_only variant.
///
/// The runner builds urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id>
/// from the reply, splits it on ":" and signs every later request with the
/// pair, so runner_id must not contain a colon. The status must be exactly
/// 200 (anything else is retried forever) and the reply's type must be
/// "createRunnerResponse".
let handleRunnerEnroll (r: Request) : Response =
    match decodeJsonApi r "par" "enroll: not a JSON:API document" with
    | None -> Response.status 400
    | Some resource ->
        let modes = GoJson.lenientStrings (resource.Attributes.TryFind "runner_modes")
        let runnerId = Guid.NewGuid().ToString()
        let orgId = tenantOrgId r.Tenant

        if r.Tenant <> "" then
            let row: RunnerEnrollmentRow =
                { TenantID = r.Tenant
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
                  Attributes = GoJson.compactObject resource.Attributes }

            Sink.write r.Sink RunnerEnrollments.table [| row |]

        let reply =
            Response.jsonOf
                200
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

        { reply with Headers = [ "Content-Type", "application/vnd.api+json" ] }

/// The runner's poll for work. An EMPTY 200 means "no task" and is the normal
/// idle answer: a body would be parsed as a task, and any other status is an
/// error on the runner's side. Every poll is also the only liveness signal a
/// runner with no tasks produces.
let handleRunnerDequeue (r: Request) : Response =
    match decodeJsonApi r "par" "dequeue: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        if r.Tenant <> "" then
            // Both timestamps stay strings: their format is undocumented and
            // an unparseable value must not become 1970.
            let row: RunnerDequeueRow =
                { TenantID = r.Tenant
                  At = DateTime.UtcNow
                  RunnerStartedAt = resource.Text "runner_started_at"
                  LastTaskReceivedAt = resource.Text "last_task_received_at"
                  Version = r.Header "X-Datadog-OnPrem-Version"
                  Modes = r.Header "X-Datadog-OnPrem-Modes" }

            Sink.write r.Sink RunnerDequeues.table [| row |]

    Response.status 200

/// publish-task-update: the outcome of a task. The document id is
/// "succeed_task" or "fail_task"; `payload.outputs` is the action's own
/// result, arbitrary JSON of any size. The runner accepts any status.
let handleRunnerTaskUpdate (r: Request) : Response =
    match decodeJsonApi r "par" "task-update: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        if r.Tenant <> "" then
            let payload = GoJson.lenientMembers (resource.Attributes.TryFind "payload")

            // A numeric enum on the runner's side, and a succeeded task sends
            // none: absent must not become a 0 that reads as an error code.
            let errorCode =
                match payload.TryFind "error_code" with
                | Some code when code.ValueKind = JsonValueKind.Null -> Some 0L
                | Some code -> GoJson.asInt64 code
                | None -> None

            let row: RunnerTaskUpdateRow =
                { TenantID = r.Tenant
                  At = DateTime.UtcNow
                  Outcome = resource.Id
                  TaskID = resource.Text "task_id"
                  ActionFQN = resource.Text "action_fqn"
                  JobID = resource.Text "job_id"
                  Client = rawText "client" resource.Attributes
                  Branch = GoJson.lenientString (payload.TryFind "branch")
                  Outputs = rawText "outputs" payload
                  ErrorCode = errorCode
                  ErrorDetails = GoJson.lenientString (payload.TryFind "error_details")
                  APIError = GoJson.lenientString (payload.TryFind "api_error")
                  Extra =
                    mergeExtra
                        (extraRaw [ "task_id"; "client"; "action_fqn"; "job_id"; "payload" ] resource.Attributes)
                        "payload."
                        (extraRaw [ "branch"; "outputs"; "error_code"; "error_details"; "api_error" ] payload) }

            Sink.write r.Sink RunnerTaskUpdates.table [| row |]

    Response.json 202 "{}"

/// The per-task heartbeat. It must be answered with exactly 200: a 404 tells
/// the runner the job is gone and it stops heartbeating. The rows answer
/// "when did this job stop making progress" — a job that hangs never sends a
/// task update.
let handleRunnerHeartbeat (r: Request) : Response =
    match decodeJsonApi r "par" "heartbeat: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        if r.Tenant <> "" then
            let row: RunnerHeartbeatRow =
                { TenantID = r.Tenant
                  At = DateTime.UtcNow
                  TaskID = resource.Text "task_id"
                  ActionFQN = resource.Text "action_fqn"
                  JobID = resource.Text "job_id"
                  Client = rawText "client" resource.Attributes }

            Sink.write r.Sink RunnerHeartbeats.table [| row |]

    Response.json 200 "{}"

/// GET runner/health-check. The runner ignores the body and reads
/// X-Server-Time; with no X-Retry-After-Ms it polls at its default interval.
let handleRunnerHealthCheck (_: Request) : Response =
    Response.json 200 "{}"
    |> Response.withHeader "X-Server-Time" (DateTime.UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture))

/// POST /api/v2/actions/connections: a connection a runner may use to reach a
/// third-party system. SENSITIVE: `integration.credentials` holds secrets and
/// is stored verbatim, because a connection without them cannot be used. It
/// must never be logged.
let handleActionConnections (r: Request) : Response =
    match decodeJsonApi r "par" "connections: not a JSON:API document" with
    | None -> ()
    | Some resource ->
        if r.Tenant <> "" then
            let integration = GoJson.lenientMembers (resource.Attributes.TryFind "integration")
            let tags = GoJson.lenientStrings (resource.Attributes.TryFind "tags") |> Option.defaultValue [||]

            let row: ActionConnectionRow =
                { TenantID = r.Tenant
                  At = DateTime.UtcNow
                  Name = resource.Text "name"
                  RunnerID = resource.Text "runner_id"
                  Tags = Tags.toMultiMap tags
                  IntegrationType = GoJson.lenientString (integration.TryFind "type")
                  Credentials = rawText "credentials" integration
                  Extra =
                    mergeExtra
                        (extraRaw [ "name"; "runner_id"; "tags"; "integration" ] resource.Attributes)
                        "integration."
                        (extraRaw [ "type"; "credentials" ] integration) }

            Sink.write r.Sink ActionConnections.table [| row |]

    Response.json 202 "{}"
