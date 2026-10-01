/// Six single-purpose hosts of the event platform.
///
///   agentdiscovery-intake.<site>     POST /api/v2/agentdiscovery   protobuf
///   agenthealth-intake.<site>        POST /api/v2/agenthealth      JSON
///   event-management-intake.<site>   POST /api/v2/events           JSON
///   softinv-intake.<site>            POST /api/v2/softinv          JSON
///   http-synthetics.<site>           POST /api/v2/synthetics       JSON
///   data-obs-intake.<site>           POST /api/v1/lineage          JSON
///                                    POST /api/v2/query-actions    JSON
///
///   agent config: none for the event platform tracks (DD_SITE only);
///   agenthealth is a plain HTTP client under `dd_url`, lineage is the
///   trace-agent's reverse proxy under `ol_proxy_config.dd_url`.
///
/// How the event platform frames a body decides what a handler expects:
/// protobuf and stream tracks carry ONE message per request (agentdiscovery,
/// events); the other JSON tracks are batched into an array (softinv,
/// synthetics, query-actions). The agent's `diagnose` sends an EMPTY body
/// down every track, so an empty body is a probe, not a fault.
///
/// Only agentdiscovery and agenthealth have published types. The rest is read
/// by its documented field names, and every other key is kept as JSON.
///
/// A body that does not decode, or a batch item that is not an object, goes
/// to raw_payloads; a body that became rows never does.
module NinjaCat.Api.Intake.Routers.Evp

open System
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open Google.Protobuf
open Google.Protobuf.WellKnownTypes
open Microsoft.Extensions.Logging
open Datadog.Agentdiscovery
open Datadog.Healthplatform
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// Every handler here answers 202 with an empty object, whatever happened.
let private accepted = Response.json 202 "{}"

// The JSON tracks are read generically. A value is a `JsonElement option`,
// where None is Go's nil: a key that is absent, or JSON null.

let private notNull (value: JsonElement) : JsonElement option =
    if value.ValueKind = JsonValueKind.Null then None else Some value

/// The value under a key. None as well when `parent` is not an object.
let private child (key: string) (parent: JsonElement option) : JsonElement option =
    match parent with
    | Some object when object.ValueKind = JsonValueKind.Object ->
        match object.TryGetProperty key with
        | true, value -> notNull value
        | false, _ -> None
    | _ -> None

let private asObject (value: JsonElement option) : JsonElement option =
    value |> Option.filter (fun v -> v.ValueKind = JsonValueKind.Object)

/// Writes a value as Go re-marshals a decoded one: object keys sorted, a
/// repeated key keeping its last value, numbers digit for digit.
let rec private writeCanonical (writer: Utf8JsonWriter) (value: JsonElement) : unit =
    match value.ValueKind with
    | JsonValueKind.Object ->
        let mutable properties: Map<string, JsonElement> = Map.empty

        for property in value.EnumerateObject() do
            properties <- properties.Add(property.Name, property.Value)

        writer.WriteStartObject()

        for pair in properties do
            writer.WritePropertyName pair.Key
            writeCanonical writer pair.Value

        writer.WriteEndObject()
    | JsonValueKind.Array ->
        writer.WriteStartArray()

        for item in value.EnumerateArray() do
            writeCanonical writer item

        writer.WriteEndArray()
    | _ -> value.WriteTo writer

let private canonicalJson (value: JsonElement) : string =
    use buffer = new MemoryStream()

    do
        use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
        writeCanonical writer value

    Encoding.UTF8.GetString(buffer.ToArray())

/// A value as JSON text for a column; "" when there is none.
let private json (value: JsonElement option) : string =
    match value with
    | Some v -> canonicalJson v
    | None -> ""

/// A value as the text a column holds: a string as it is, a number digit for
/// digit (an id above 2^53 must not pass through a float), anything else as
/// JSON; "" when there is none.
let private text (value: JsonElement option) : string =
    match value with
    | None -> ""
    | Some v ->
        match v.ValueKind with
        | JsonValueKind.String -> v.GetString()
        | JsonValueKind.Number -> v.GetRawText()
        | JsonValueKind.True -> "true"
        | JsonValueKind.False -> "false"
        | _ -> canonicalJson v

/// An array as strings. Some producers send a single tag as a bare string;
/// that is a list of one.
let private strings (value: JsonElement option) : string[] =
    match value with
    | Some v when v.ValueKind = JsonValueKind.Array ->
        v.EnumerateArray() |> Seq.map (fun item -> text (notNull item)) |> Array.ofSeq
    | Some v when v.ValueKind = JsonValueKind.String && v.GetString() <> "" -> [| v.GetString() |]
    | _ -> [||]

/// None unless the value is a JSON bool: "never said" is not "false".
let private flagOption (value: JsonElement option) : uint8 option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.True -> Some(Text.flag true)
    | Some v when v.ValueKind = JsonValueKind.False -> Some(Text.flag false)
    | _ -> None

/// None unless the value is a number written as a plain 64-bit integer.
let private int64Option (value: JsonElement option) : int64 option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt64() with
        | true, n -> Some n
        | false, _ -> None
    | _ -> None

let private floatOption (value: JsonElement option) : float option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetDouble() with
        // .NET reads a number too large for a float as infinity; Go refuses it.
        | true, n when not (Double.IsInfinity n) -> Some n
        | _ -> None
    | _ -> None

/// The keys of an object that are not in `known`, each value as JSON.
let private unknownKeys (known: Set<string>) (value: JsonElement option) : Map<string, string> =
    match asObject value with
    | None -> Map.empty
    | Some object ->
        let mutable found: Map<string, string> = Map.empty

        for property in object.EnumerateObject() do
            if not (known.Contains property.Name) then
                found <- found.Add(property.Name, json (notNull property.Value))

        found

/// Adds the keys of a deeper nesting level under a prefix, so that a key of
/// one level cannot shadow the same key of another.
let private mergePrefixed (prefix: string) (inner: Map<string, string>) (outer: Map<string, string>) : Map<string, string> =
    Map.fold (fun merged key value -> Map.add (prefix + key) value merged) outer inner

/// What Go decodes into a map: an object, or null (a map without keys).
let private isObjectOrNull (value: JsonElement) : bool =
    value.ValueKind = JsonValueKind.Object || value.ValueKind = JsonValueKind.Null

/// A body as one JSON object (or null): the first value, whatever follows
/// it. Repaired first and read as deep as every other intake reads, or half
/// a surrogate pair would throw later, when the string is taken out.
let parseObject (body: byte[]) : JsonElement option =
    let options = JsonReaderOptions(AllowMultipleValues = true, MaxDepth = Json.maxDepth)
    let mutable reader = Utf8JsonReader(ReadOnlySpan(Json.repair body), options)

    try
        if reader.Read() then
            Some(JsonElement.ParseValue(&reader)) |> Option.filter isObjectOrNull
        else
            None
    with :? JsonException ->
        None

/// The items of a batched track: the elements of an array, or the one value
/// of a client that skipped the batcher. A body of `null` has no items, as
/// Go unmarshals it into an empty slice.
let private parseItems (body: byte[]) : Result<JsonElement list, string> =
    match Json.tryParse body with
    | Error e -> Error e
    | Ok root ->
        match root.ValueKind with
        | JsonValueKind.Array -> Ok(List.ofSeq (root.EnumerateArray()))
        | JsonValueKind.Null -> Ok []
        | _ -> Ok [ root ]

/// A batched track: each item becomes rows or is kept raw, so one bad item
/// never takes the good ones with it.
let private handleBatch (r: Request) (intake: string) (table: Table<'row>) (toRows: DateTime -> JsonElement -> 'row list) : Response =
    if r.Body.Length > 0 then
        match parseItems r.Body with
        | Error e ->
            r.Log.LogWarning("[{Intake}] json: {Error} ({Bytes} bytes)", intake, e, r.Body.Length)
            Raw.store r intake "unexpected_shape" "body is not a JSON array or object" r.Body
        | Ok items ->
            let now = DateTime.UtcNow
            let rows = ResizeArray<'row>()

            for item in items do
                if not (isObjectOrNull item) then
                    let raw = Encoding.UTF8.GetBytes(item.GetRawText())
                    r.Log.LogWarning("[{Intake}] not a JSON object ({Bytes} bytes)", intake, raw.Length)
                    Raw.store r intake "decode_error" "item is not a JSON object" raw
                elif r.Tenant <> "" then
                    rows.AddRange(toRows now item)

            Sink.write r.Sink table (rows.ToArray())

    accepted

/// The enum's name as Go prints it; a number the schema does not know as digits.
let private payloadFormatName (format: AgentDiscoveryConfigFilePayloadFormat) : string =
    match int format with
    | 0 -> "PAYLOAD_FORMAT_UNKNOWN"
    | 1 -> "PAYLOAD_FORMAT_JSON"
    | 2 -> "PAYLOAD_FORMAT_YAML"
    | 3 -> "PAYLOAD_FORMAT_TOML"
    | 4 -> "PAYLOAD_FORMAT_INI"
    | 5 -> "PAYLOAD_FORMAT_XML"
    | 6 -> "PAYLOAD_FORMAT_PROPERTIES"
    | 7 -> "PAYLOAD_FORMAT_HCL"
    | 8 -> "PAYLOAD_FORMAT_REDIS_CONF"
    | other -> string other

/// None when absent — never the Unix epoch — or outside what a DateTime holds.
let private timestampOption (timestamp: Timestamp) : DateTime option =
    if isNull timestamp then
        None
    else
        try
            Some((Time.fromUnixSeconds timestamp.Seconds).AddTicks(int64 timestamp.Nanos / 100L))
        with :? ArgumentOutOfRangeException ->
            None

/// One row per payload.
let agentDiscoveryRows (tenant: string) (receivedAt: DateTime) (batch: AgentDiscoveryPayloadBatch) : AgentDiscoveryRow[] =
    batch.Payloads
    |> Seq.map (fun payload ->
        { TenantID = tenant
          ReceivedAt = receivedAt
          HostID = batch.HostId
          Integration = payload.Integration
          Runtime = payload.Runtime
          RuntimeID = payload.RuntimeId
          IngestionTimestamp = timestampOption payload.IngestionTimestamp
          ConfigPaths = payload.ConfigFiles |> Seq.map _.Path |> Array.ofSeq
          ConfigContents = payload.ConfigFiles |> Seq.map (fun file -> file.Content.ToByteArray()) |> Array.ofSeq
          ConfigTruncated = payload.ConfigFiles |> Seq.map (fun file -> Text.flag file.Truncated) |> Array.ofSeq
          ConfigFormats = payload.ConfigFiles |> Seq.map (fun file -> payloadFormatName file.PayloadFormat) |> Array.ofSeq
          // Values too, not only names: see the migration for why.
          EnvVarNames = payload.EnvVars |> Seq.map _.Name |> Array.ofSeq
          EnvVarValues = payload.EnvVars |> Seq.map _.Value |> Array.ofSeq })
    |> Array.ofSeq

/// agentdiscovery-intake: which integration config files and env vars each
/// agent runtime sees. One AgentDiscoveryPayloadBatch per request.
let handleAgentDiscovery (r: Request) : Response =
    if r.Body.Length > 0 then
        let decoded =
            try
                Ok(AgentDiscoveryPayloadBatch.Parser.ParseFrom r.Body)
            with :? InvalidProtocolBufferException as e ->
                Error e.Message

        match decoded with
        | Error e ->
            r.Log.LogWarning("[agentdiscovery] protobuf: {Error} ({Bytes} bytes)", e, r.Body.Length)
            Raw.store r "agentdiscovery" "decode_error" e r.Body
        | Ok batch when batch.Payloads.Count = 0 ->
            // Protobuf skips fields it does not know, so a payload meant for
            // another endpoint decodes into an empty batch. Not a decode
            // error, so it is not kept raw.
            r.Log.LogWarning("[agentdiscovery] decoded to zero payloads ({Bytes} bytes) — wrong payload type?", r.Body.Length)
        | Ok batch ->
            if r.Tenant <> "" then
                Sink.write r.Sink AgentDiscovery.table (agentDiscoveryRows r.Tenant DateTime.UtcNow batch)

    accepted

let agentDiscoveryRoutes: Route list = [ Route.post "/api/v2/agentdiscovery" handleAgentDiscovery ]

/// Raised by the readers below; decodeHealthReport turns it into an Error.
exception private HealthReportMismatch of string

let private mismatch (path: string) (name: string) (wanted: string) (value: JsonElement) : 'a =
    raise (HealthReportMismatch $"{path}.{name}: expected {wanted}, got {value.ValueKind}")

/// A key's value as encoding/json finds it: names match ignoring case, the
/// last match wins, and null is the same as absent.
let private goField (name: string) (object: JsonElement) : JsonElement option =
    let mutable found = None

    for property in object.EnumerateObject() do
        if property.Name.Equals(name, StringComparison.OrdinalIgnoreCase) then
            found <- notNull property.Value

    found

let private goString (path: string) (name: string) (object: JsonElement) : string =
    match goField name object with
    | None -> ""
    | Some v when v.ValueKind = JsonValueKind.String -> v.GetString()
    | Some v -> mismatch path name "a string" v

/// An int32 field, which is also what an enum is: a number written without a
/// fraction or an exponent.
let private goInt32 (path: string) (name: string) (object: JsonElement) : int =
    match goField name object with
    | None -> 0
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt32() with
        | true, n -> n
        | false, _ -> mismatch path name "a 32-bit integer" v
    | Some v -> mismatch path name "a 32-bit integer" v

let private goBool (path: string) (name: string) (object: JsonElement) : bool =
    match goField name object with
    | None -> false
    | Some v when v.ValueKind = JsonValueKind.True -> true
    | Some v when v.ValueKind = JsonValueKind.False -> false
    | Some v -> mismatch path name "a bool" v

let private goStrings (path: string) (name: string) (object: JsonElement) : string list =
    match goField name object with
    | None -> []
    | Some v when v.ValueKind = JsonValueKind.Array ->
        [ for item in v.EnumerateArray() do
              match item.ValueKind with
              | JsonValueKind.String -> item.GetString()
              | JsonValueKind.Null -> ""
              | _ -> mismatch path name "an array of strings" item ]
    | Some v -> mismatch path name "an array" v

let private goObject (path: string) (name: string) (object: JsonElement) : JsonElement option =
    match goField name object with
    | None -> None
    | Some v when v.ValueKind = JsonValueKind.Object -> Some v
    | Some v -> mismatch path name "an object" v

let private decodeScript (path: string) (object: JsonElement) : Script =
    let script = Script()
    script.Language <- goString path "language" object
    script.LanguageVersion <- goString path "language_version" object
    script.Filename <- goString path "filename" object
    script.RequiresRoot <- goBool path "requires_root" object
    script.Content <- goString path "content" object
    script

let private decodeRemediation (path: string) (object: JsonElement) : Remediation =
    let remediation = Remediation()
    remediation.Summary <- goString path "summary" object

    match goField "steps" object with
    | None -> ()
    | Some steps when steps.ValueKind = JsonValueKind.Array ->
        for item in steps.EnumerateArray() do
            match item.ValueKind with
            | JsonValueKind.Object ->
                let step = RemediationStep()
                step.Order <- goInt32 $"{path}.steps" "order" item
                step.Text <- goString $"{path}.steps" "text" item
                remediation.Steps.Add step
            // Go keeps a null step as a nil pointer and then skips it.
            | JsonValueKind.Null -> ()
            | _ -> mismatch path "steps" "an array of objects" item
    | Some other -> mismatch path "steps" "an array" other

    match goObject path "script" object with
    | Some script -> remediation.Script <- decodeScript $"{path}.script" script
    | None -> ()

    remediation

let private decodePersistedIssue (path: string) (object: JsonElement) : PersistedIssue =
    let lifecycle = PersistedIssue()
    lifecycle.State <- enum<IssueState> (goInt32 path "state" object)
    lifecycle.FirstSeen <- goString path "first_seen" object
    lifecycle.LastSeen <- goString path "last_seen" object

    // Optional on the wire: unresolved is not "resolved at ''".
    if (goField "resolved_at" object).IsSome then
        lifecycle.ResolvedAt <- goString path "resolved_at" object

    lifecycle

let private decodeIssue (path: string) (object: JsonElement) : Issue =
    let issue = Issue()
    issue.Id <- goString path "id" object
    issue.IssueName <- goString path "issue_name" object
    issue.Title <- goString path "title" object
    issue.Description <- goString path "description" object
    issue.Category <- goString path "category" object
    issue.Location <- goString path "location" object
    issue.Severity <- enum<IssueSeverity> (goInt32 path "severity" object)
    issue.DetectedAt <- goString path "detected_at" object
    issue.Source <- goString path "source" object
    issue.Tags.AddRange(goStrings path "tags" object)
    issue.IssueType <- goString path "issue_type" object

    match goField "extra" object with
    | None -> ()
    | Some extra ->
        // Extra is a protobuf Struct, read by protobuf's own JSON rules: it
        // must be an object.
        try
            issue.Extra <- JsonParser.Default.Parse<Struct>(extra.GetRawText())
        with
        | :? InvalidProtocolBufferException as e -> raise (HealthReportMismatch $"{path}.extra: {e.Message}")
        | :? InvalidJsonException as e -> raise (HealthReportMismatch $"{path}.extra: {e.Message}")

    match goObject path "remediation" object with
    | Some remediation -> issue.Remediation <- decodeRemediation $"{path}.remediation" remediation
    | None -> ()

    match goObject path "persisted_issue" object with
    | Some lifecycle -> issue.PersistedIssue <- decodePersistedIssue $"{path}.persisted_issue" lifecycle
    | None -> ()

    issue

let private decodeHost (path: string) (object: JsonElement) : HostInfo =
    let host = HostInfo()
    host.Hostname <- goString path "hostname" object

    if (goField "agent_version" object).IsSome then
        host.AgentVersion <- goString path "agent_version" object

    host.ParIds.AddRange(goStrings path "par_ids" object)
    host

/// Reads a HealthReport the way Go's encoding/json reads it into the
/// generated struct: unknown keys are ignored, null leaves a field empty, and
/// a value of the wrong type refuses the whole report.
///
/// That is the sender's own encoding: the agent marshals the protobuf struct
/// with encoding/json, so keys are snake_case and enums are NUMBERS.
let decodeHealthReport (body: byte[]) : Result<HealthReport, string> =
    match Json.tryParse body with
    | Error e -> Error e
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok(HealthReport())
    | Ok root when root.ValueKind <> JsonValueKind.Object -> Error $"HealthReport: expected an object, got {root.ValueKind}"
    | Ok root ->
        try
            let path = "HealthReport"
            let report = HealthReport()
            report.SchemaVersion <- goString path "schema_version" root
            report.EventType <- goString path "event_type" root
            report.EmittedAt <- goString path "emitted_at" root
            report.Service <- goString path "service" root

            match goObject path "host" root with
            | Some host -> report.Host <- decodeHost $"{path}.host" host
            | None -> ()

            match goObject path "issues" root with
            | None -> ()
            | Some issues ->
                for entry in issues.EnumerateObject() do
                    match entry.Value.ValueKind with
                    | JsonValueKind.Object -> report.Issues[entry.Name] <- decodeIssue $"{path}.issues.{entry.Name}" entry.Value
                    // Go keeps a null issue as a nil pointer, which reads as an empty issue.
                    | JsonValueKind.Null -> report.Issues[entry.Name] <- Issue()
                    | _ -> mismatch $"{path}.issues" entry.Name "an object" entry.Value

            Ok report
        with HealthReportMismatch message ->
            Error message

/// The body's `issues` object, read generically. Protobuf's Struct holds
/// every number as a float64, so Issue.Extra is taken from here instead, where
/// a number keeps its digits.
let healthIssuesRaw (body: byte[]) : JsonElement option =
    asObject (child "issues" (parseObject body))

/// Extra as protobuf decoded it: every number has been through a float64.
let private structJson (extra: Struct) : string =
    if isNull extra then
        ""
    else
        match Json.tryParse (Encoding.UTF8.GetBytes(JsonFormatter.Default.Format extra)) with
        | Ok value -> canonicalJson value
        | Error _ -> ""

let private issueExtra (extra: Struct) (rawIssue: JsonElement option) : string =
    match rawIssue with
    | Some raw ->
        match raw.TryGetProperty "extra" with
        | true, value -> json (notNull value)
        | false, _ -> structJson extra
    | None -> structJson extra

let private severityName (severity: IssueSeverity) : string =
    match int severity with
    | 0 -> "ISSUE_SEVERITY_UNSPECIFIED"
    | 1 -> "ISSUE_SEVERITY_LOW"
    | 2 -> "ISSUE_SEVERITY_MEDIUM"
    | 3 -> "ISSUE_SEVERITY_HIGH"
    | other -> string other

let private stateName (state: IssueState) : string =
    match int state with
    | 0 -> "ISSUE_STATE_UNSPECIFIED"
    | 1 -> "ISSUE_STATE_NEW"
    | 2 -> "ISSUE_STATE_ONGOING"
    | 3 -> "ISSUE_STATE_RESOLVED"
    | 4 -> "ISSUE_STATE_ACTIVE"
    | other -> string other

let private healthIssueRow
    (tenant: string)
    (receivedAt: DateTime)
    (reportID: Guid)
    (key: string)
    (issue: Issue)
    (rawIssue: JsonElement option)
    : AgentHealthIssueRow =
    // An absent sub-message reads as an empty one, except where its absence
    // is itself the information.
    let remediation = if isNull issue.Remediation then Remediation() else issue.Remediation
    let hasScript = not (isNull remediation.Script)
    let script = if hasScript then remediation.Script else Script()
    let hasLifecycle = not (isNull issue.PersistedIssue)
    let lifecycle = if hasLifecycle then issue.PersistedIssue else PersistedIssue()

    { TenantID = tenant
      ReceivedAt = receivedAt
      ReportID = reportID
      IssueKey = key
      ID = issue.Id
      IssueName = issue.IssueName
      Title = issue.Title
      Description = issue.Description
      Category = issue.Category
      Location = issue.Location
      Severity = severityName issue.Severity
      DetectedAt = issue.DetectedAt
      DetectedAtParsed = Time.tryRfc3339 issue.DetectedAt
      Source = issue.Source
      Extra = issueExtra issue.Extra rawIssue
      RemediationSummary = remediation.Summary
      RemediationStepOrder = remediation.Steps |> Seq.map _.Order |> Array.ofSeq
      RemediationStepText = remediation.Steps |> Seq.map _.Text |> Array.ofSeq
      ScriptLanguage = script.Language
      ScriptLanguageVersion = script.LanguageVersion
      ScriptFilename = script.Filename
      ScriptRequiresRoot = if hasScript then Some(Text.flag script.RequiresRoot) else None
      ScriptContent = script.Content
      Tags = Tags.toMultiMap issue.Tags
      PersistedState = if hasLifecycle then stateName lifecycle.State else ""
      FirstSeen = lifecycle.FirstSeen
      LastSeen = lifecycle.LastSeen
      ResolvedAt = if lifecycle.HasResolvedAt then Some lifecycle.ResolvedAt else None
      IssueType = issue.IssueType }

/// The report's row and one row per issue, in the order of the issue keys so
/// that two ingests of the same report give the same rows. `rawIssues` is
/// what healthIssuesRaw found, used only for Extra.
let healthReportRows
    (tenant: string)
    (receivedAt: DateTime)
    (reportID: Guid)
    (report: HealthReport)
    (rawIssues: JsonElement option)
    : AgentHealthReportRow * AgentHealthIssueRow[] =
    let host = if isNull report.Host then HostInfo() else report.Host

    let reportRow =
        { TenantID = tenant
          ReceivedAt = receivedAt
          ReportID = reportID
          SchemaVersion = report.SchemaVersion
          EventType = report.EventType
          EmittedAt = report.EmittedAt
          EmittedAtParsed = Time.tryRfc3339 report.EmittedAt
          Service = report.Service
          Host = host.Hostname
          AgentVersion = if host.HasAgentVersion then Some host.AgentVersion else None
          ParIDs = Array.ofSeq host.ParIds
          IssueCount = uint32 report.Issues.Count }

    let issueRows =
        report.Issues.Keys
        |> Seq.sort
        |> Seq.map (fun key ->
            healthIssueRow tenant receivedAt reportID key report.Issues[key] (asObject (child key rawIssues)))
        |> Array.ofSeq

    reportRow, issueRows

/// agenthealth-intake: issues the agent found with its own setup, one
/// HealthReport per request. Not an event platform track.
let handleAgentHealth (r: Request) : Response =
    if r.Body.Length > 0 then
        match decodeHealthReport r.Body with
        | Error e ->
            r.Log.LogWarning("[agenthealth] json: {Error} ({Bytes} bytes)", e, r.Body.Length)
            Raw.store r "agenthealth" "decode_error" e r.Body
        | Ok report ->
            if r.Tenant <> "" then
                // The wire has no report id; this one joins the two tables.
                let reportRow, issueRows =
                    healthReportRows r.Tenant DateTime.UtcNow (Guid.NewGuid()) report (healthIssuesRaw r.Body)

                Sink.write r.Sink AgentHealthReports.table [| reportRow |]
                Sink.write r.Sink AgentHealthIssues.table issueRows

    accepted

let agentHealthRoutes: Route list = [ Route.post "/api/v2/agenthealth" handleAgentHealth ]

/// The label in raw_payloads. Not "events": /api/v1/events on api.<site>
/// already has that one, and the two tracks are unrelated.
let private eventManagementIntake = "event-management"

let private eventAttributeKeys =
    set
        [ "host"; "title"; "category"; "integration_id"; "message"; "timestamp"; "tags"; "aggregation_key"
          "attributes"; "system-notable-events" ]

let private eventDataKeys = set [ "type"; "attributes" ]
let private eventEnvelopeKeys = set [ "data" ]

/// One JSON:API-like envelope:
///
///   {"data": {"type": "event", "attributes": {host, title, category,
///             integration_id, message, timestamp, tags, aggregation_key,
///             attributes: {...}, "system-notable-events": {event_type}}}}
///
/// The inner `attributes` is each producer's own: {status, priority, custom}
/// for notable events, {changed_resource, author, ...} for change events.
///
/// None when data.attributes is missing or not an object: the one thing the
/// envelope must have.
let eventManagementRow (tenant: string) (receivedAt: DateTime) (envelope: JsonElement) : EventManagementEventRow option =
    let top = Some envelope
    let data = asObject (child "data" top)
    let attrs = asObject (child "attributes" data)

    match attrs with
    | None -> None
    | Some _ ->
        let timestamp = text (child "timestamp" attrs)

        // Undeclared keys of all three levels land in one map: data's own
        // (a JSON:API "id") and the envelope's ("meta") under a prefix.
        let extra =
            unknownKeys eventAttributeKeys attrs
            |> mergePrefixed "data." (unknownKeys eventDataKeys data)
            |> mergePrefixed "top." (unknownKeys eventEnvelopeKeys top)

        let inner =
            match asObject (child "attributes" attrs) with
            | Some object -> canonicalJson object
            | None -> "{}"

        Some
            { TenantID = tenant
              ReceivedAt = receivedAt
              DataType = text (child "type" data)
              Host = text (child "host" attrs)
              Title = text (child "title" attrs)
              Category = text (child "category" attrs)
              IntegrationID = text (child "integration_id" attrs)
              Message = text (child "message" attrs)
              Timestamp = timestamp
              TimestampParsed = Time.tryRfc3339 timestamp
              Tags = Tags.toMultiMap (strings (child "tags" attrs))
              AggregationKey = text (child "aggregation_key" attrs)
              NotableEventType = text (child "event_type" (child "system-notable-events" attrs))
              Attributes = inner
              OuterExtra = extra }

/// event-management-intake: notable events, logon duration, anomaly
/// notifications. A stream track, so one envelope per request.
let handleEventManagement (r: Request) : Response =
    if r.Body.Length > 0 then
        match parseObject r.Body with
        | None ->
            r.Log.LogWarning("[event-management] not a JSON object ({Bytes} bytes)", r.Body.Length)
            Raw.store r eventManagementIntake "decode_error" "body is not a JSON object" r.Body
        | Some envelope ->
            match eventManagementRow r.Tenant DateTime.UtcNow envelope with
            | None ->
                Raw.store r eventManagementIntake "unexpected_shape" "data.attributes is missing or not an object" r.Body
            | Some row ->
                if r.Tenant <> "" then
                    Sink.write r.Sink EventManagement.table [| row |]

    accepted

let eventManagementRoutes: Route list = [ Route.post "/api/v2/events" handleEventManagement ]

let private softwareEntryKeys =
    set
        [ "software_type"; "name"; "version"; "publisher"; "deployment_status"; "deployment_time"; "product_code"
          "is_64_bit"; "install_paths" ]

let private softwarePayloadKeys = set [ "hostname"; "host_software" ]
let private hostSoftwareKeys = set [ "software" ]

/// One row per software entry of a {hostname, host_software: {software: [...]}}
/// payload. An entry that is not an object is dropped.
let hostSoftwareRows (tenant: string) (receivedAt: DateTime) (payload: JsonElement) : HostSoftwareRow list =
    let top = Some payload
    let hostSoftware = child "host_software" top

    let payloadExtra =
        unknownKeys softwarePayloadKeys top
        |> mergePrefixed "host_software." (unknownKeys hostSoftwareKeys hostSoftware)

    match child "software" hostSoftware with
    | Some entries when entries.ValueKind = JsonValueKind.Array ->
        [ for item in entries.EnumerateArray() do
              if item.ValueKind = JsonValueKind.Object then
                  let entry = Some item
                  let deploymentTime = text (child "deployment_time" entry)

                  { TenantID = tenant
                    ReceivedAt = receivedAt
                    Hostname = text (child "hostname" top)
                    SoftwareType = text (child "software_type" entry)
                    Name = text (child "name" entry)
                    Version = text (child "version" entry)
                    Publisher = text (child "publisher" entry)
                    DeploymentStatus = text (child "deployment_status" entry)
                    DeploymentTime = deploymentTime
                    DeploymentTimeParsed = Time.tryRfc3339 deploymentTime
                    ProductCode = text (child "product_code" entry)
                    Is64Bit = flagOption (child "is_64_bit" entry)
                    InstallPaths = strings (child "install_paths" entry)
                    Extra = unknownKeys softwareEntryKeys entry
                    PayloadExtra = payloadExtra } ]
    | _ -> []

/// softinv-intake: installed packages per host, batched.
let handleSoftwareInventory (r: Request) : Response =
    handleBatch r "softinv" HostSoftware.table (hostSoftwareRows r.Tenant)

let softwareInventoryRoutes: Route list = [ Route.post "/api/v2/softinv" handleSoftwareInventory ]

let private syntheticsKeys = set [ "test"; "location"; "result"; "_dd"; "enrichment"; "v" ]

let private syntheticsResultKeys =
    set
        [ "id"; "initialId"; "status"; "runType"; "duration"; "testStartedAt"; "testFinishedAt"; "testTriggeredAt"
          "assertions"; "failure"; "config"; "netstats"; "netpath" ]

/// One test result:
///
///   {test: {id, name, type, subType, version}, location: {id, name, displayName},
///    result: {id, initialId, status, runType, duration, testStartedAt,
///             testFinishedAt, testTriggeredAt, assertions: [{type, operator,
///             expected, actual, valid}], failure: {code, message},
///             config: {request: {host, port, ...}}, netstats: {packetsSent,
///             packetsReceived, packetLossPercentage, jitter, latency, hops},
///             netpath},
///    _dd: {...}, enrichment, v}
///
/// netpath is the same type netpath-intake carries.
let syntheticsResultRow (tenant: string) (receivedAt: DateTime) (item: JsonElement) : SyntheticsResultRow =
    let top = Some item
    let test = child "test" top
    let location = child "location" top
    let result = child "result" top
    let netstats = child "netstats" result

    // An assertion that is not an object still takes its place in the arrays.
    let assertions =
        match child "assertions" result with
        | Some list when list.ValueKind = JsonValueKind.Array -> list.EnumerateArray() |> Seq.map Some |> Array.ofSeq
        | _ -> [||]

    let isTrue (value: JsonElement option) =
        match value with
        | Some v -> v.ValueKind = JsonValueKind.True
        | None -> false

    let failure = asObject (child "failure" result)
    let started = text (child "testStartedAt" result)
    let finished = text (child "testFinishedAt" result)
    let triggered = text (child "testTriggeredAt" result)

    { TenantID = tenant
      ReceivedAt = receivedAt
      TestID = text (child "id" test)
      TestName = text (child "name" test)
      TestType = text (child "type" test)
      TestSubtype = text (child "subType" test)
      TestVersion = text (child "version" test)
      LocationID = text (child "id" location)
      LocationName = text (child "name" location)
      LocationDisplayName = text (child "displayName" location)
      ResultID = text (child "id" result)
      ResultInitialID = text (child "initialId" result)
      Status = text (child "status" result)
      RunType = text (child "runType" result)
      Duration = text (child "duration" result)
      TestStartedAt = started
      TestStartedAtParsed = Time.tryRfc3339 started
      TestFinishedAt = finished
      TestFinishedAtParsed = Time.tryRfc3339 finished
      TestTriggeredAt = triggered
      TestTriggeredAtParsed = Time.tryRfc3339 triggered
      AssertionType = assertions |> Array.map (fun a -> text (child "type" a))
      AssertionOperator = assertions |> Array.map (fun a -> text (child "operator" a))
      AssertionExpected = assertions |> Array.map (fun a -> json (child "expected" a))
      AssertionActual = assertions |> Array.map (fun a -> json (child "actual" a))
      AssertionValid = assertions |> Array.map (fun a -> Text.flag (isTrue (child "valid" a)))
      // The failure object being there at all is the signal.
      FailureCode = failure |> Option.map (fun _ -> text (child "code" failure))
      FailureMessage = failure |> Option.map (fun _ -> text (child "message" failure))
      Config = json (child "config" result)
      NetstatsPacketsSent = int64Option (child "packetsSent" netstats)
      NetstatsPacketsReceived = int64Option (child "packetsReceived" netstats)
      NetstatsPacketLossPercentage = floatOption (child "packetLossPercentage" netstats)
      NetstatsJitter = floatOption (child "jitter" netstats)
      NetstatsLatency = floatOption (child "latency" netstats)
      NetstatsHops = int64Option (child "hops" netstats)
      Netpath = json (child "netpath" result)
      DD = json (child "_dd" top)
      Enrichment = json (child "enrichment" top)
      V = text (child "v" top)
      Extra = unknownKeys syntheticsKeys top |> mergePrefixed "result." (unknownKeys syntheticsResultKeys result) }

/// http-synthetics: results of network tests the agent ran on the server's
/// behalf, batched.
let handleSynthetics (r: Request) : Response =
    handleBatch r "synthetics" SyntheticsResults.table (fun now item -> [ syntheticsResultRow r.Tenant now item ])

let syntheticsRoutes: Route list = [ Route.post "/api/v2/synthetics" handleSynthetics ]

let private lineageKeys =
    set [ "eventType"; "eventTime"; "producer"; "schemaURL"; "run"; "job"; "inputs"; "outputs" ]

/// An inputs[] or outputs[] list as three index-aligned arrays.
type private Datasets =
    { Namespaces: string[]
      Names: string[]
      Facets: string[] }

let private datasets (value: JsonElement option) : Datasets =
    let items =
        match value with
        | Some list when list.ValueKind = JsonValueKind.Array -> list.EnumerateArray() |> Seq.map Some |> Array.ofSeq
        | _ -> [||]

    { Namespaces = items |> Array.map (fun d -> text (child "namespace" d))
      Names = items |> Array.map (fun d -> text (child "name" d))
      Facets = items |> Array.map (fun d -> json (child "facets" d)) }

/// Field names follow the published OpenLineage spec (RunEvent).
let openLineageEventRow
    (tenant: string)
    (receivedAt: DateTime)
    (apiVersion: string)
    (via: string)
    (event: JsonElement)
    : OpenLineageEventRow =
    let top = Some event
    let run = child "run" top
    let job = child "job" top
    let eventTime = text (child "eventTime" top)
    let inputs = datasets (child "inputs" top)
    let outputs = datasets (child "outputs" top)

    { TenantID = tenant
      ReceivedAt = receivedAt
      EventType = text (child "eventType" top)
      EventTime = eventTime
      EventTimeParsed = Time.tryRfc3339 eventTime
      Producer = text (child "producer" top)
      SchemaURL = text (child "schemaURL" top)
      RunID = text (child "runId" run)
      RunFacets = json (child "facets" run)
      JobNamespace = text (child "namespace" job)
      JobName = text (child "name" job)
      JobFacets = json (child "facets" job)
      InputNamespace = inputs.Namespaces
      InputName = inputs.Names
      InputFacets = inputs.Facets
      OutputNamespace = outputs.Namespaces
      OutputName = outputs.Names
      OutputFacets = outputs.Facets
      APIVersion = apiVersion
      Via = via
      Extra = unknownKeys lineageKeys top }

/// OpenLineage RunEvents, forwarded as they are by the trace-agent's reverse
/// proxy, with ?api-version=2 when the agent is told to add it. The transport
/// posts one RunEvent per request; an array is accepted in case a client
/// batches.
let handleOpenLineage (r: Request) : Response =
    let apiVersion = r.Query "api-version"
    let via = r.Header "Via"

    handleBatch r "lineage" OpenLineage.table (fun now event ->
        [ openLineageEventRow r.Tenant now apiVersion via event ])

/// Results of Data Observability query actions, batched. They come from
/// Python integrations and no type documents them, so each entry is stored
/// whole.
let handleQueryActions (r: Request) : Response =
    let origin = r.Header "Dd-Evp-Origin"
    let originVersion = r.Header "Dd-Evp-Origin-Version"

    handleBatch r "query-actions" QueryActionResults.table (fun now entry ->
        let keys =
            if entry.ValueKind = JsonValueKind.Object then
                entry.EnumerateObject() |> Seq.map _.Name |> Seq.distinct |> Seq.sort |> Array.ofSeq
            else
                [||]

        [ { TenantID = r.Tenant
            ReceivedAt = now
            Result = canonicalJson entry
            Keys = keys
            DDEVPOrigin = origin
            DDEVPOriginVersion = originVersion } ])

/// data-obs-intake. The lineage proxy carries the key as "Authorization:
/// Bearer <key>", the OpenLineage client's convention, which is why this host
/// has its own guard in Routes.fs.
let dataObsRoutes: Route list =
    [ Route.post "/api/v1/lineage" handleOpenLineage
      Route.post "/api/v2/query-actions" handleQueryActions ]
