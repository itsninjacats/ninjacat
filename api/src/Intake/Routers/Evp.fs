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
open System.Text
open System.Text.Json
open System.Threading.Tasks
open Google.Protobuf
open Google.Protobuf.WellKnownTypes
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open Datadog.Agentdiscovery
open Datadog.Healthplatform
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

// The JSON tracks are read generically, with the readers of `Json` that
// note nothing: a value is a `JsonElement option`, None when the key is
// absent or null.

/// Adds the keys of a deeper nesting level under a prefix, so that a key of
/// one level cannot shadow the same key of another.
let private mergePrefixed (prefix: string) (inner: Map<string, string>) (outer: Map<string, string>) : Map<string, string> =
    Map.fold (fun merged key value -> Map.add (prefix + key) value merged) outer inner

/// An object, or null, which stands for an object with no members.
let private isObjectOrNull (value: JsonElement) : bool =
    value.ValueKind = JsonValueKind.Object || value.ValueKind = JsonValueKind.Null

/// A body as one JSON object (or null): the first value, whatever follows
/// it.
let parseObject (body: byte[]) : JsonElement option =
    match Json.tryParseFirst body with
    | Ok value when isObjectOrNull value -> Some value
    | _ -> None

/// A batched track: each item becomes rows or is kept raw, so one bad item
/// never takes the good ones with it.
let private handleBatch (body: byte[]) (ctx: HttpContext) (intake: string) (table: Table<'row>) (toRows: DateTime -> JsonElement -> 'row list) : Task =
    if body.Length > 0 then
        match Json.tryParseList body with
        | Error _ ->
            Raw.store ctx intake "unexpected_shape" "body is not a JSON array or object" body
        | Ok items ->
            let now = DateTime.UtcNow
            let rows = ResizeArray<'row>()

            for item in items do
                if not (isObjectOrNull item) then
                    let raw = Encoding.UTF8.GetBytes(item.GetRawText())
                    Raw.store ctx intake "decode_error" "item is not a JSON object" raw
                else
                    rows.AddRange(toRows now item)

            Ctx.write ctx table (rows.ToArray())

    accepted ctx

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
          IngestionTimestamp = Time.ofTimestamp payload.IngestionTimestamp
          ConfigPaths = payload.ConfigFiles |> Seq.map _.Path |> Array.ofSeq
          ConfigContents = payload.ConfigFiles |> Seq.map (fun file -> file.Content.ToByteArray()) |> Array.ofSeq
          ConfigTruncated = payload.ConfigFiles |> Seq.map (fun file -> Text.flag file.Truncated) |> Array.ofSeq
          ConfigFormats = payload.ConfigFiles |> Seq.map (fun file -> ProtoEnum.name file.PayloadFormat) |> Array.ofSeq
          // Values too, not only names: see the migration for why.
          EnvVarNames = payload.EnvVars |> Seq.map _.Name |> Array.ofSeq
          EnvVarValues = payload.EnvVars |> Seq.map _.Value |> Array.ofSeq })
    |> Array.ofSeq

/// agentdiscovery-intake: which integration config files and env vars each
/// agent runtime sees. One AgentDiscoveryPayloadBatch per request.
let handleAgentDiscovery (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx

    if body.Length > 0 then
        let decoded =
            try
                Ok(AgentDiscoveryPayloadBatch.Parser.ParseFrom body)
            with :? InvalidProtocolBufferException as e ->
                Error e.Message

        match decoded with
        | Error e ->
            Raw.store ctx "agentdiscovery" "decode_error" e body
        | Ok batch when batch.Payloads.Count = 0 ->
            // Protobuf skips fields it does not know, so a payload meant for
            // another endpoint decodes into an empty batch. Not a decode
            // error, so it is not kept raw.
            log.LogWarning("[agentdiscovery] decoded to zero payloads ({Bytes} bytes) — wrong payload type?", body.Length)
        | Ok batch ->
            Ctx.write ctx AgentDiscovery.table (agentDiscoveryRows tenant DateTime.UtcNow batch)

    accepted ctx

/// A HealthReport as the agent sends it: the protobuf message written as
/// JSON by Go's encoding/json, so keys are snake_case and enums are numbers.
/// Protobuf's JSON parser reads both.
let decodeHealthReport (body: byte[]) : Result<HealthReport, string> =
    ProtoJson.tryParse<HealthReport> body

/// The body's `issues` object, read generically. Protobuf's Struct holds
/// every number as a float64, so Issue.Extra is taken from here instead, where
/// a number keeps its digits.
let healthIssuesRaw (body: byte[]) : JsonElement option =
    Json.child "issues" (parseObject body) |> Option.bind Json.objectOf

/// Extra as protobuf decoded it: every number has been through a float64.
let private structJson (extra: Struct) : string =
    if isNull extra then
        ""
    else
        match Json.tryParse (Encoding.UTF8.GetBytes(JsonFormatter.Default.Format extra)) with
        | Ok value -> Json.compactSorted value
        | Error _ -> ""

let private issueExtra (extra: Struct) (rawIssue: JsonElement option) : string =
    match rawIssue |> Option.bind (Json.tryProperty "extra") with
    | Some sent -> if sent.ValueKind = JsonValueKind.Null then "" else Json.compactSorted sent
    | None -> structJson extra

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
      Severity = ProtoEnum.name issue.Severity
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
      PersistedState = if hasLifecycle then ProtoEnum.name lifecycle.State else ""
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
            healthIssueRow tenant receivedAt reportID key report.Issues[key] (Json.child key rawIssues |> Option.bind Json.objectOf))
        |> Array.ofSeq

    reportRow, issueRows

/// agenthealth-intake: issues the agent found with its own setup, one
/// HealthReport per request. Not an event platform track.
let handleAgentHealth (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if body.Length > 0 then
        match decodeHealthReport body with
        | Error e ->
            Raw.store ctx "agenthealth" "decode_error" e body
        | Ok report ->
            // The wire has no report id; this one joins the two tables.
            let reportRow, issueRows =
                healthReportRows tenant DateTime.UtcNow (Guid.NewGuid()) report (healthIssuesRaw body)

            Ctx.write ctx AgentHealthReports.table [| reportRow |]
            Ctx.write ctx AgentHealthIssues.table issueRows

    accepted ctx

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
    let data = Json.child "data" top |> Option.bind Json.objectOf
    let attrs = Json.child "attributes" data |> Option.bind Json.objectOf

    match attrs with
    | None -> None
    | Some _ ->
        let timestamp = Json.text (Json.child "timestamp" attrs)

        // Undeclared keys of all three levels land in one map: data's own
        // (a JSON:API "id") and the envelope's ("meta") under a prefix.
        let extra =
            Json.otherMembers eventAttributeKeys Json.compactSorted attrs
            |> mergePrefixed "data." (Json.otherMembers eventDataKeys Json.compactSorted data)
            |> mergePrefixed "top." (Json.otherMembers eventEnvelopeKeys Json.compactSorted top)

        let inner =
            match Json.child "attributes" attrs |> Option.bind Json.objectOf with
            | Some object -> Json.compactSorted object
            | None -> "{}"

        Some
            { TenantID = tenant
              ReceivedAt = receivedAt
              DataType = Json.text (Json.child "type" data)
              Host = Json.text (Json.child "host" attrs)
              Title = Json.text (Json.child "title" attrs)
              Category = Json.text (Json.child "category" attrs)
              IntegrationID = Json.text (Json.child "integration_id" attrs)
              Message = Json.text (Json.child "message" attrs)
              Timestamp = timestamp
              TimestampParsed = Time.tryRfc3339 timestamp
              Tags = Tags.toMultiMap (Json.textList (Json.child "tags" attrs))
              AggregationKey = Json.text (Json.child "aggregation_key" attrs)
              NotableEventType = Json.text (Json.child "event_type" (Json.child "system-notable-events" attrs))
              Attributes = inner
              OuterExtra = extra }

/// event-management-intake: notable events, logon duration, anomaly
/// notifications. A stream track, so one envelope per request.
let handleEventManagement (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    if body.Length > 0 then
        match parseObject body with
        | None ->
            Raw.store ctx eventManagementIntake "decode_error" "body is not a JSON object" body
        | Some envelope ->
            match eventManagementRow tenant DateTime.UtcNow envelope with
            | None ->
                Raw.store ctx eventManagementIntake "unexpected_shape" "data.attributes is missing or not an object" body
            | Some row ->
                Ctx.write ctx EventManagement.table [| row |]

    accepted ctx

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
    let hostSoftware = Json.child "host_software" top

    let payloadExtra =
        Json.otherMembers softwarePayloadKeys Json.compactSorted top
        |> mergePrefixed "host_software." (Json.otherMembers hostSoftwareKeys Json.compactSorted hostSoftware)

    match Json.child "software" hostSoftware with
    | Some entries when entries.ValueKind = JsonValueKind.Array ->
        [ for item in entries.EnumerateArray() do
              if item.ValueKind = JsonValueKind.Object then
                  let entry = Some item
                  let deploymentTime = Json.text (Json.child "deployment_time" entry)

                  { TenantID = tenant
                    ReceivedAt = receivedAt
                    Hostname = Json.text (Json.child "hostname" top)
                    SoftwareType = Json.text (Json.child "software_type" entry)
                    Name = Json.text (Json.child "name" entry)
                    Version = Json.text (Json.child "version" entry)
                    Publisher = Json.text (Json.child "publisher" entry)
                    DeploymentStatus = Json.text (Json.child "deployment_status" entry)
                    DeploymentTime = deploymentTime
                    DeploymentTimeParsed = Time.tryRfc3339 deploymentTime
                    ProductCode = Json.text (Json.child "product_code" entry)
                    Is64Bit = Json.flag (Json.child "is_64_bit" entry)
                    InstallPaths = Json.textList (Json.child "install_paths" entry)
                    Extra = Json.otherMembers softwareEntryKeys Json.compactSorted entry
                    PayloadExtra = payloadExtra } ]
    | _ -> []

/// softinv-intake: installed packages per host, batched.
let handleSoftwareInventory (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    handleBatch body ctx "softinv" HostSoftware.table (hostSoftwareRows tenant)

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
    let test = Json.child "test" top
    let location = Json.child "location" top
    let result = Json.child "result" top
    let netstats = Json.child "netstats" result

    // An assertion that is not an object still takes its place in the arrays.
    let assertions =
        Json.lenientItems (Json.child "assertions" result) |> List.map Some |> Array.ofList

    let isTrue (value: JsonElement option) =
        match value with
        | Some v -> v.ValueKind = JsonValueKind.True
        | None -> false

    let failure = Json.child "failure" result |> Option.bind Json.objectOf
    let started = Json.text (Json.child "testStartedAt" result)
    let finished = Json.text (Json.child "testFinishedAt" result)
    let triggered = Json.text (Json.child "testTriggeredAt" result)

    { TenantID = tenant
      ReceivedAt = receivedAt
      TestID = Json.text (Json.child "id" test)
      TestName = Json.text (Json.child "name" test)
      TestType = Json.text (Json.child "type" test)
      TestSubtype = Json.text (Json.child "subType" test)
      TestVersion = Json.text (Json.child "version" test)
      LocationID = Json.text (Json.child "id" location)
      LocationName = Json.text (Json.child "name" location)
      LocationDisplayName = Json.text (Json.child "displayName" location)
      ResultID = Json.text (Json.child "id" result)
      ResultInitialID = Json.text (Json.child "initialId" result)
      Status = Json.text (Json.child "status" result)
      RunType = Json.text (Json.child "runType" result)
      Duration = Json.text (Json.child "duration" result)
      TestStartedAt = started
      TestStartedAtParsed = Time.tryRfc3339 started
      TestFinishedAt = finished
      TestFinishedAtParsed = Time.tryRfc3339 finished
      TestTriggeredAt = triggered
      TestTriggeredAtParsed = Time.tryRfc3339 triggered
      AssertionType = assertions |> Array.map (fun a -> Json.text (Json.child "type" a))
      AssertionOperator = assertions |> Array.map (fun a -> Json.text (Json.child "operator" a))
      AssertionExpected = assertions |> Array.map (fun a -> Json.compactSortedOrEmpty (Json.child "expected" a))
      AssertionActual = assertions |> Array.map (fun a -> Json.compactSortedOrEmpty (Json.child "actual" a))
      AssertionValid = assertions |> Array.map (fun a -> Text.flag (isTrue (Json.child "valid" a)))
      // The failure object being there at all is the signal.
      FailureCode = failure |> Option.map (fun _ -> Json.text (Json.child "code" failure))
      FailureMessage = failure |> Option.map (fun _ -> Json.text (Json.child "message" failure))
      Config = Json.compactSortedOrEmpty (Json.child "config" result)
      NetstatsPacketsSent = Json.child "packetsSent" netstats |> Option.bind Json.int64Of
      NetstatsPacketsReceived = Json.child "packetsReceived" netstats |> Option.bind Json.int64Of
      NetstatsPacketLossPercentage = Json.child "packetLossPercentage" netstats |> Option.bind Json.floatOf
      NetstatsJitter = Json.child "jitter" netstats |> Option.bind Json.floatOf
      NetstatsLatency = Json.child "latency" netstats |> Option.bind Json.floatOf
      NetstatsHops = Json.child "hops" netstats |> Option.bind Json.int64Of
      Netpath = Json.compactSortedOrEmpty (Json.child "netpath" result)
      DD = Json.compactSortedOrEmpty (Json.child "_dd" top)
      Enrichment = Json.compactSortedOrEmpty (Json.child "enrichment" top)
      V = Json.text (Json.child "v" top)
      Extra = Json.otherMembers syntheticsKeys Json.compactSorted top |> mergePrefixed "result." (Json.otherMembers syntheticsResultKeys Json.compactSorted result) }

/// http-synthetics: results of network tests the agent ran on the server's
/// behalf, batched.
let handleSynthetics (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    handleBatch body ctx "synthetics" SyntheticsResults.table (fun now item -> [ syntheticsResultRow tenant now item ])

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

    { Namespaces = items |> Array.map (fun d -> Json.text (Json.child "namespace" d))
      Names = items |> Array.map (fun d -> Json.text (Json.child "name" d))
      Facets = items |> Array.map (fun d -> Json.compactSortedOrEmpty (Json.child "facets" d)) }

/// Field names follow the published OpenLineage spec (RunEvent).
let openLineageEventRow
    (tenant: string)
    (receivedAt: DateTime)
    (apiVersion: string)
    (via: string)
    (event: JsonElement)
    : OpenLineageEventRow =
    let top = Some event
    let run = Json.child "run" top
    let job = Json.child "job" top
    let eventTime = Json.text (Json.child "eventTime" top)
    let inputs = datasets (Json.child "inputs" top)
    let outputs = datasets (Json.child "outputs" top)

    { TenantID = tenant
      ReceivedAt = receivedAt
      EventType = Json.text (Json.child "eventType" top)
      EventTime = eventTime
      EventTimeParsed = Time.tryRfc3339 eventTime
      Producer = Json.text (Json.child "producer" top)
      SchemaURL = Json.text (Json.child "schemaURL" top)
      RunID = Json.text (Json.child "runId" run)
      RunFacets = Json.compactSortedOrEmpty (Json.child "facets" run)
      JobNamespace = Json.text (Json.child "namespace" job)
      JobName = Json.text (Json.child "name" job)
      JobFacets = Json.compactSortedOrEmpty (Json.child "facets" job)
      InputNamespace = inputs.Namespaces
      InputName = inputs.Names
      InputFacets = inputs.Facets
      OutputNamespace = outputs.Namespaces
      OutputName = outputs.Names
      OutputFacets = outputs.Facets
      APIVersion = apiVersion
      Via = via
      Extra = Json.otherMembers lineageKeys Json.compactSorted top }

/// OpenLineage RunEvents, forwarded as they are by the trace-agent's reverse
/// proxy, with ?api-version=2 when the agent is told to add it. The transport
/// posts one RunEvent per request; an array is accepted ctx in case a client
/// batches.
let handleOpenLineage (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    let apiVersion = Ctx.query ctx "api-version"
    let via = Ctx.header ctx "Via"

    handleBatch body ctx "lineage" OpenLineage.table (fun now event ->
        [ openLineageEventRow tenant now apiVersion via event ])

/// Results of Data Observability query actions, batched. They come from
/// Python integrations and no type documents them, so each entry is stored
/// whole.
let handleQueryActions (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    let origin = Ctx.header ctx "Dd-Evp-Origin"
    let originVersion = Ctx.header ctx "Dd-Evp-Origin-Version"

    handleBatch body ctx "query-actions" QueryActionResults.table (fun now entry ->
        let keys =
            if entry.ValueKind = JsonValueKind.Object then
                entry.EnumerateObject() |> Seq.map _.Name |> Seq.distinct |> Seq.sort |> Array.ofSeq
            else
                [||]

        [ { TenantID = tenant
            ReceivedAt = now
            Result = Json.compactSorted entry
            Keys = keys
            DDEVPOrigin = origin
            DDEVPOriginVersion = originVersion } ])
