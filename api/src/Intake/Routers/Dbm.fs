/// dbm-metrics-intake.<site> — Database Monitoring.
///
///   config: none; an event platform track.
///
/// Six tracks on one host, all JSON arrays of events the agent never parses:
/// Python integrations hand it bytes and the forwarder batches them. No
/// schema is published for them, so the fields the producers agree on become
/// columns of `dbm_events` and the whole event is kept beside them. A body
/// or an element that cannot be read goes to raw_payloads.
module NinjaCat.Api.Intake.Routers.Dbm

open System
open System.Collections.Generic
open System.Globalization
open System.Text
open System.Text.Json
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The part of an event that every track shares. The wire is not consistent
/// across tracks, and the envelope absorbs that:
///
///   - tags come as `ddtags` (one comma-joined string on samples, a list on
///     activity) or as `tags` (a list on metrics and metadata);
///   - the agent version is `ddagentversion`, except on dbmmetadata, which
///     says `agent_version`;
///   - the interval is `collection_interval` or `min_collection_interval`.
type Envelope =
    { /// `timestamp`, Unix milliseconds, as the number was written: the
      /// oracle check sends an integer, Python integrations a fraction. ""
      /// when not sent.
      Timestamp: string
      Host: string
      DatabaseInstance: string
      AgentHostname: string
      AgentVersion: string
      Source: string
      DBMType: string
      Kind: string
      DBMS: string
      DBMSVersion: string
      /// None when neither key was sent: 0 seconds is a value.
      CollectionInterval: float option
      Tags: string list
      /// Every key the envelope does not describe (the engine's row arrays,
      /// the `db` object of a sample), and every key whose value did not fit.
      Extra: Map<string, JsonElement>
      /// Envelope keys whose value did not fit; they stay in Extra.
      Undecoded: string list }

/// One event of a batch: its text exactly as sent, and that text parsed.
type Event = { Text: string; Json: JsonElement }

/// Splits a body into events. One bare object, which the agent never sends
/// but a hand-made request might, is a batch of one.
let splitEvents (body: byte[]) : Result<Event list, string> =
    match GoJson.parse body with
    | Error e -> Error e
    | Ok root ->
        match root.ValueKind with
        | JsonValueKind.Array ->
            Ok(root.EnumerateArray() |> Seq.map (fun item -> { Text = item.GetRawText(); Json = item }) |> List.ofSeq)
        | JsonValueKind.Object -> Ok [ { Text = Text.utf8 body; Json = root } ]
        | JsonValueKind.Null -> Ok []
        | _ -> Error $"the body is a JSON {GoJson.kind root}"

/// A tag list in either spelling: one comma-joined string, or a list.
let private tagsOf (value: JsonElement) : string list option =
    match value.ValueKind with
    | JsonValueKind.String ->
        value.GetString().Split ','
        |> Array.map _.Trim()
        |> Array.filter (fun tag -> tag <> "")
        |> List.ofArray
        |> Some
    | _ -> GoJson.stringsOf value |> Option.map List.ofArray

/// Reads the envelope field by field, so one malformed value cannot take the
/// others down with it: its key is noted and left among the extra keys.
let decodeEnvelope (event: JsonElement) : Result<Envelope, string> =
    match event.ValueKind with
    // Without this, a null element would become a row of empty columns.
    | JsonValueKind.Null -> Error "event is null, not a JSON object"
    | JsonValueKind.Object ->
        let left = Dictionary<string, JsonElement>()

        for property in event.EnumerateObject() do
            left[property.Name] <- property.Value

        let undecoded = ResizeArray<string>()

        let take (key: string) (read: JsonElement -> 'a option) : 'a option =
            match left.TryGetValue key with
            | false, _ -> None
            | true, value when value.ValueKind = JsonValueKind.Null ->
                // Go reads null into any of these fields as "nothing sent".
                left.Remove key |> ignore
                None
            | true, value ->
                match read value with
                | Some taken ->
                    left.Remove key |> ignore
                    Some taken
                | None ->
                    undecoded.Add key
                    None

        let text (key: string) : string = take key GoJson.stringOf |> Option.defaultValue ""

        // Go's decoder allocates the interval before it finds that the value
        // does not fit. So a malformed one reads as 0 (an infinity, when it
        // is a number too large), not as absent; its key is noted all the same.
        let interval (key: string) : float option =
            match take key GoJson.floatOf with
            | Some seconds -> Some seconds
            | None ->
                match left.TryGetValue key with
                | true, value when value.ValueKind = JsonValueKind.Number ->
                    Some(Double.Parse(value.GetRawText(), NumberStyles.Float, CultureInfo.InvariantCulture))
                | true, _ -> Some 0.0
                | false, _ -> None

        // The order is the order of `undecoded`.
        let timestamp = take "timestamp" GoJson.numberText |> Option.defaultValue ""
        let host = text "host"
        let databaseInstance = text "database_instance"
        let agentHostname = text "ddagenthostname"
        let source = text "ddsource"
        let dbmType = text "dbm_type"
        let kind = text "kind"
        let dbms = text "dbms"
        let dbmsVersion = text "dbms_version"
        let agentVersion = text "ddagentversion"
        let otherAgentVersion = text "agent_version"
        let collectionInterval = interval "collection_interval"
        let minInterval = interval "min_collection_interval"
        let ddtags = take "ddtags" tagsOf |> Option.defaultValue []
        let tags = take "tags" tagsOf |> Option.defaultValue []

        Ok
            { Timestamp = timestamp
              Host = host
              DatabaseInstance = databaseInstance
              AgentHostname = agentHostname
              AgentVersion = (if agentVersion = "" then otherAgentVersion else agentVersion)
              Source = source
              DBMType = dbmType
              Kind = kind
              DBMS = dbms
              DBMSVersion = dbmsVersion
              CollectionInterval = (if collectionInterval.IsSome then collectionInterval else minInterval)
              Tags = ddtags @ tags
              Extra = left |> Seq.map (fun pair -> pair.Key, pair.Value) |> Map.ofSeq
              Undecoded = List.ofSeq undecoded }
    | _ -> Error $"it is a JSON {GoJson.kind event}"

/// The envelope's timestamp as a time; None when it was not sent, which must
/// never become 1970 or "now".
///
/// An integer is read as one, so it never passes through a float. A fraction
/// of a millisecond (Python's time.time()*1000) is rounded: the column holds
/// milliseconds anyway.
let timestamp (wire: string) : DateTime option =
    if wire = "" then
        None
    else
        match Int64.TryParse(wire, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
        | true, millis -> Some(GoTime.fromUnixMillis millis)
        | false, _ ->
            match Double.TryParse(wire, NumberStyles.Float, CultureInfo.InvariantCulture) with
            | true, millis when Double.IsFinite millis ->
                Some(GoTime.fromUnixMillis (int64 (Math.Round(millis, MidpointRounding.AwayFromZero))))
            | _ -> None

/// What `db.plan.definition` says about the plan's steps. Three facts, and
/// the column can hold only two of them (NULL or a count).
type PlanSteps =
    | NoDefinition
    | NotAList
    | Steps of uint32

let planSteps (definition: JsonElement option) : PlanSteps =
    match definition with
    | None -> NoDefinition
    | Some value when value.ValueKind = JsonValueKind.Array -> Steps(uint32 (value.GetArrayLength()))
    | Some value when value.ValueKind = JsonValueKind.Null -> Steps 0u
    | Some _ -> NotAList

/// The `db` object of a databasequery event, as the oracle check's FQT and
/// plan payloads agree on it.
type private QuerySample =
    { Instance: string
      QuerySignature: string
      Statement: string
      /// The plan's signature and steps; None when no plan was sent.
      Plan: (string * PlanSteps) option }

/// None when a field of the object has the wrong type: then none of it is used.
let private querySample (db: JsonElement) : QuerySample option =
    let bad = GoJson.Mismatches()
    let fields = GoJson.fields bad "db" db

    let sample =
        { Instance = fields.String "instance"
          QuerySignature = fields.String "query_signature"
          Statement = fields.String "statement"
          Plan =
            fields.OptionalObject "plan"
            |> Option.map (fun plan -> plan.String "signature", planSteps (plan.Find "definition")) }

    if bad.Count = 0 then Some sample else None

let private nonEmpty (text: string) : string option = if text = "" then None else Some text

let toRow (tenant: string) (track: string) (receivedAt: DateTime) (event: string) (envelope: Envelope) : DBMEventRow =
    let sample = envelope.Extra.TryFind "db" |> Option.bind querySample
    let plan = sample |> Option.bind _.Plan

    // A plan whose definition is not a list of steps is not "no plan". The
    // column cannot say so, so the list of undecoded keys does.
    let undecoded =
        match plan with
        | Some(_, NotAList) -> envelope.Undecoded @ [ "db.plan.definition" ]
        | _ -> envelope.Undecoded

    { TenantID = tenant
      ReceivedAt = receivedAt
      Track = track
      Timestamp = timestamp envelope.Timestamp
      Host = envelope.Host
      DatabaseInstance = envelope.DatabaseInstance
      AgentHostname = envelope.AgentHostname
      AgentVersion = envelope.AgentVersion
      Source = envelope.Source
      DBMType = envelope.DBMType
      Kind = envelope.Kind
      DBMS = envelope.DBMS
      DBMSVersion = envelope.DBMSVersion
      CollectionInterval = envelope.CollectionInterval
      Tags = Tags.toMultiMap envelope.Tags
      ExtraKeys = envelope.Extra.Keys |> Seq.sort |> Array.ofSeq
      UndecodedKeys = Array.ofList undecoded
      Event = event
      DBInstance = sample |> Option.bind (fun s -> nonEmpty s.Instance)
      QuerySignature = sample |> Option.bind (fun s -> nonEmpty s.QuerySignature)
      Statement = sample |> Option.bind (fun s -> nonEmpty s.Statement)
      PlanSignature = plan |> Option.bind (fun (signature, _) -> nonEmpty signature)
      PlanDefinitionSteps =
        match plan with
        | Some(_, Steps count) -> Some count
        | _ -> None }

/// Stores one track's batch. `track` is the route's name, and what the rows
/// say they arrived on.
let handle (track: string) (r: Request) : Response =
    match splitEvents r.Body with
    | Error e ->
        r.Log.LogWarning("[{Track}] not a JSON array: {Error}", track, e)
        Raw.store r "dbm" "decode_error" $"[{track}] not a JSON array or object: {e}" r.Body
    | Ok events ->
        let receivedAt = DateTime.UtcNow
        let rows = ResizeArray<DBMEventRow>()

        events
        |> List.iteri (fun i event ->
            match decodeEnvelope event.Json with
            | Error e ->
                r.Log.LogWarning("[{Track}] event {Index}: not an object ({Error}), kept raw", track, i, e)
                Raw.store r "dbm" "decode_error" $"[{track}] event {i}: not an object: {e}" (Encoding.UTF8.GetBytes event.Text)
            | Ok envelope ->
                if r.Tenant <> "" then
                    rows.Add(toRow r.Tenant track receivedAt event.Text envelope))

        Sink.write r.Sink DbmEvents.table (rows.ToArray())

    Response.json 202 "{}"

let routes: Route list =
    [ Route.post "/api/v2/dbmmetrics" (handle "dbmmetrics") // query and lock metrics
      Route.post "/api/v2/dbmactivity" (handle "dbmactivity") // active sessions
      Route.post "/api/v2/databasequery" (handle "databasequery") // query samples and plans
      Route.post "/api/v2/dbmmetadata" (handle "dbmmetadata") // instances and schemas
      // No producer of these two is in the agent's source: only the envelope
      // can be claimed about them.
      Route.post "/api/v2/dbmhealth" (handle "dbmhealth")
      Route.post "/api/v2/dbmcolumnstatistics" (handle "dbmcolumnstatistics") ]
