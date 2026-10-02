/// The security hosts:
///
///   cws-intake.<site>                         /api/v2/secdump     → cws_activity_dumps, cws_dump_nodes
///   runtime-security-http-intake.logs.<site>  /api/v2/secruntime,
///                                             /api/v2/secinfo     → security_events
///   cspm-intake.<site>                        /api/v2/compliance  → security_events
///   sbom-intake.<site>                        /api/v2/sbom        → sbom_entities, sbom_components, sbom_vulnerabilities
///   sds-intake.<site>                         /api/v2/sdsresult   → raw_payloads, always
///
///   agent config: runtime_security_config.activity_dump.remote_storage.endpoints.logs_dd_url (secdump),
///                 runtime_security_config.endpoints.logs_dd_url (secruntime, secinfo),
///                 compliance_config.endpoints.logs_dd_url (compliance);
///                 sbom and sdsresult are event platform tracks, which follow DD_SITE.
module NinjaCat.Api.Intake.Routers.Security

// A node's syscalls and image_tags are deprecated in the .proto, but agents
// still send them and the node row counts them.
#nowarn "44"

open System
open System.Globalization
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Threading.Tasks
open Cyclonedx.V14
open Datadog.Cws.Dumpsv1
open Datadog.Sbom
open Datadog.Sds
open Google.Protobuf
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// Datadog answers these intakes with an empty object and 202, whatever
/// happened to the body.
let private accepted: EndpointHandler = setStatusCode 202 >=> json {||}

/// A member of a JSON object, exactly as it was written. None when the
/// value is not an object or has no such member.
let private property (name: string) (value: JsonElement) : JsonElement option =
    if value.ValueKind = JsonValueKind.Object then Lenient.property name value else None

/// A JSON string's value; "" for anything else, a missing value included.
let private text (value: JsonElement option) : string =
    match value with
    | Some v when v.ValueKind = JsonValueKind.String -> Lenient.text v
    | _ -> ""

/// The string at the end of a path through nested objects; "" when a step
/// is missing or the end is not a string.
let private nestedText (path: string list) (object: JsonElement) : string =
    let mutable current = Some object

    for name in path do
        current <- current |> Option.bind (property name)

    text current

/// A list of strings: a JSON array of them, or one comma-separated string
/// (some producers send ddtags that way). Anything else is no list at all.
let private strings (value: JsonElement option) : string list =
    match value with
    | Some v when v.ValueKind = JsonValueKind.Array ->
        let items = List.ofSeq (v.EnumerateArray())

        if items |> List.forall (fun i -> i.ValueKind = JsonValueKind.String || i.ValueKind = JsonValueKind.Null) then
            items |> List.map (fun i -> if i.ValueKind = JsonValueKind.String then Lenient.text i else "")
        else
            []
    | Some v when v.ValueKind = JsonValueKind.String && Lenient.text v <> "" -> List.ofArray ((Lenient.text v).Split ',')
    | _ -> []

/// The members of an object without a column of their own, each as the JSON
/// text it arrived as: nothing is converted, nothing is dropped.
let private otherMembers (known: Set<string>) (object: JsonElement) : Map<string, string> =
    object.EnumerateObject()
    |> Seq.map (fun p -> Lenient.nameOf p, p.Value.GetRawText())
    |> Seq.filter (fun (name, _) -> not (known.Contains name))
    |> Map.ofSeq

let private relaxedWriter =
    JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping)

/// A JSON object written again: keys sorted, a repeated key keeping its
/// last value, every value as it was written.
let private rewritten (object: JsonElement) : string =
    let members =
        object.EnumerateObject() |> Seq.map (fun p -> Lenient.nameOf p, p.Value.GetRawText()) |> Map.ofSeq

    use buffer = new MemoryStream()

    do
        use writer = new Utf8JsonWriter(buffer, relaxedWriter)
        writer.WriteStartObject()

        for pair in members do
            writer.WritePropertyName pair.Key
            writer.WriteRawValue(pair.Value, true)

        writer.WriteEndObject()

    Encoding.UTF8.GetString(buffer.ToArray())

/// A message as protobuf JSON; None when it cannot be written as JSON.
let private protoJson (message: IMessage) : string option =
    try
        let formatted = JsonFormatter.Default.Format message
        // The formatter escapes < and >. Written again without that, so the
        // text stays searchable in ClickHouse.
        use document = JsonDocument.Parse(formatted, JsonDocumentOptions(MaxDepth = Lenient.jsonDepth))
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, relaxedWriter)
            document.RootElement.WriteTo writer

        Some(Encoding.UTF8.GetString(buffer.ToArray()))
    with
    | :? InvalidOperationException
    | :? JsonException -> None

/// A repeated field as a JSON array of protobuf JSON objects; "" when empty.
/// An element that cannot be written as JSON still gets an entry, holding
/// its protobuf bytes, so the array is as long as the field.
let private protoJsonArray (items: seq<#IMessage>) : string =
    let entries =
        items
        |> Seq.map (fun item ->
            match protoJson item with
            | Some json -> json
            | None -> JsonSerializer.Serialize {| raw_protobuf_base64 = Convert.ToBase64String(item.ToByteArray()) |})
        |> List.ofSeq

    if entries.IsEmpty then "" else "[" + String.Join(",", entries) + "]"

/// The value's name as the .proto spells it; an unknown number as digits.
let private enumName (enumType: Reflection.EnumDescriptor) (number: int) : string =
    match enumType.FindValueByNumber number with
    | null -> string number
    | value -> value.Name

let private sbomEnum (name: string) = SbomReflection.Descriptor.FindTypeByName<Reflection.EnumDescriptor> name
let private cyclonedxEnum (name: string) = Bom14Reflection.Descriptor.FindTypeByName<Reflection.EnumDescriptor> name

let private generationTypes = ActivityDumpReflection.Descriptor.FindTypeByName<Reflection.EnumDescriptor> "GenerationType"
let private sbomSourceTypes = sbomEnum "SBOMSourceType"
let private sbomStatuses = sbomEnum "SBOMStatus"
let private classifications = cyclonedxEnum "Classification"
let private scopes = cyclonedxEnum "Scope"
let private hashAlgorithms = cyclonedxEnum "HashAlg"
let private analysisStates = cyclonedxEnum "ImpactAnalysisState"
let private analysisJustifications = cyclonedxEnum "ImpactAnalysisJustification"
let private vulnerabilityResponses = cyclonedxEnum "VulnerabilityResponse"

/// None when the time is outside what a DateTime holds.
let private fromMillis (ms: int64) : DateTime option =
    try
        Some(Time.fromUnixMillis ms)
    with :? ArgumentOutOfRangeException ->
        None

/// A protobuf Timestamp as a time; None outside what a DateTime holds.
let private timestampTime (timestamp: WellKnownTypes.Timestamp) : DateTime option =
    try
        Some((Time.fromUnixSeconds timestamp.Seconds).AddTicks(int64 timestamp.Nanos / 100L))
    with :? ArgumentOutOfRangeException ->
        None

/// Header keys with a column of their own; the rest go to header_extra.
let private knownHeaderKeys = set [ "host"; "service"; "ddsource"; "ddtags"; "dns_names" ]

/// The "dump" part as a SecDump. None when the bytes do not decode, or
/// decode to nothing: protobuf reads almost any bytes as some message.
let decodeDump (data: byte[]) : SecDump option =
    try
        let dump = SecDump.Parser.ParseFrom data
        if isNull dump.Metadata && dump.Tree.Count = 0 then None else Some dump
    with :? InvalidProtocolBufferException ->
        None

let rec private countTree (nodes: ProcessActivityNode seq) : int =
    nodes |> Seq.sumBy (fun node -> 1 + countTree node.Children)

/// The cws_activity_dumps row, from whichever of the two parts decoded.
/// `header` is the "event" part (the agent's ActivityDumpHeader) as a JSON
/// object. Both parts are kept as they arrived, decoded or not.
let dumpRow
    (tenant: string)
    (receivedAt: DateTime)
    (dumpId: Guid)
    (header: JsonElement option)
    (headerBytes: byte[])
    (dump: SecDump option)
    (dumpBytes: byte[])
    : CWSActivityDumpRow =
    let empty: CWSActivityDumpRow =
        { TenantID = tenant
          ReceivedAt = receivedAt
          DumpID = dumpId
          HeaderHost = ""
          HeaderService = ""
          HeaderSource = ""
          HeaderTags = Map.empty
          DNSNames = ""
          HeaderExtra = Map.empty
          HeaderRaw = headerBytes
          DumpHost = ""
          DumpService = ""
          DumpSource = ""
          DumpTags = Map.empty
          AgentVersion = ""
          AgentCommit = ""
          KernelVersion = ""
          LinuxDistribution = ""
          Arch = ""
          MetadataName = ""
          ProtobufVersion = ""
          DifferentiateArgs = 0uy
          Comm = ""
          ContainerID = ""
          Start = None
          End = None
          Size = None
          Serialization = ""
          CgroupID = ""
          CgroupManager = ""
          TreeNodeCount = 0u
          Dump = dumpBytes }

    let withHeader =
        match header with
        | None -> empty
        | Some h ->
            { empty with
                HeaderHost = text (property "host" h)
                HeaderService = text (property "service" h)
                HeaderSource = text (property "ddsource" h)
                HeaderTags = Tags.toMultiMap (strings (property "ddtags" h))
                DNSNames =
                    match property "dns_names" h with
                    | Some names -> names.GetRawText()
                    | None -> ""
                HeaderExtra = otherMembers knownHeaderKeys h }

    let withDump =
        match dump with
        | None -> withHeader
        | Some d ->
            { withHeader with
                DumpHost = d.Host
                DumpService = d.Service
                DumpSource = d.Source
                DumpTags = Tags.toMultiMap d.Tags
                TreeNodeCount = uint32 (countTree d.Tree) }

    match dump with
    | Some d when not (isNull d.Metadata) ->
        let metadata = d.Metadata

        { withDump with
            AgentVersion = metadata.AgentVersion
            AgentCommit = metadata.AgentCommit
            KernelVersion = metadata.KernelVersion
            LinuxDistribution = metadata.LinuxDistribution
            Arch = metadata.Arch
            MetadataName = metadata.Name
            ProtobufVersion = metadata.ProtobufVersion
            DifferentiateArgs = Text.flag metadata.DifferentiateArgs
            Comm = metadata.Comm
            ContainerID = metadata.ContainerId
            Start = Some metadata.Start
            End = Some metadata.End
            Size = Some metadata.Size
            Serialization = metadata.Serialization
            CgroupID = metadata.CgroupId
            CgroupManager = metadata.CgroupManager }
    | _ -> withDump

let private nodeRow
    (tenant: string)
    (receivedAt: DateTime)
    (dumpId: Guid)
    (node: ProcessActivityNode)
    (nodePath: uint32[])
    (parentPath: uint32[])
    : CWSDumpNodeRow =
    // The node column holds the node without its subtree: the children have
    // rows of their own.
    let shallow = node.Clone()
    shallow.Children.Clear()

    let proc = if isNull node.Process then ProcessInfo() else node.Process

    { TenantID = tenant
      ReceivedAt = receivedAt
      DumpID = dumpId
      NodePath = nodePath
      Depth = uint16 nodePath.Length
      ParentPath = parentPath
      Node = protoJson shallow |> Option.defaultValue ""
      PID = proc.Pid
      PPID = proc.Ppid
      Comm = proc.Comm
      ContainerID = proc.ContainerId
      FilePath = (if isNull proc.File then "" else proc.File.Path)
      Args = Array.ofSeq proc.Args
      ImageTags = Array.ofSeq node.ImageTags
      MatchedRuleIDs = node.MatchedRules |> Seq.map _.RuleId |> Array.ofSeq
      GenerationType = enumName generationTypes (int node.GenerationType)
      FilesCount = uint32 node.Files.Count
      DNSCount = uint32 node.DnsNames.Count
      SocketsCount = uint32 node.Sockets.Count
      SyscallsCount = uint32 node.Syscalls.Count }

/// One row per node of the dump's process tree, depth-first. NodePath is
/// the index path from the root, ParentPath the parent's.
let flattenTree (tenant: string) (receivedAt: DateTime) (dumpId: Guid) (tree: ProcessActivityNode seq) : CWSDumpNodeRow[] =
    let rows = ResizeArray<CWSDumpNodeRow>()

    let rec walk (nodes: ProcessActivityNode seq) (path: uint32[]) =
        nodes
        |> Seq.iteri (fun i node ->
            let nodePath = Array.append path [| uint32 i |]
            rows.Add(nodeRow tenant receivedAt dumpId node nodePath path)
            walk node.Children nodePath)

    walk tree [||]
    rows.ToArray()

/// POST /api/v2/secdump: gzip(multipart/form-data) with two parts, "event"
/// (the JSON header) and "dump" (SecDump protobuf, which the agent labels
/// application/json — its own bug, the part header is ignored).
///
/// The parts are decoded independently: a broken "event" does not lose the
/// "dump", and the other way round.
let handleSecDump (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx
    let sink = Ctx.sink ctx

    match Multipart.boundary (Ctx.header ctx "Content-Type") with
    | None ->
        log.LogWarning("[secdump] not multipart: content-type {ContentType}", Ctx.header ctx "Content-Type")
        Raw.store ctx "secdump" "decode_error" "content-type is not multipart/form-data" body
    | Some boundary ->
        let parts = Multipart.parts boundary body

        for part in parts do
            if part.Name <> "event" && part.Name <> "dump" then
                log.LogWarning("[secdump] unexpected part {Name} ({Bytes} B)", part.Name, part.Data.Length)

        let partData (name: string) =
            parts |> List.tryFindBack (fun p -> p.Name = name) |> Option.map _.Data

        let headerBytes = partData "event"
        let dumpBytes = partData "dump"
        let header = headerBytes |> Option.bind Lenient.tryObject
        let dump = dumpBytes |> Option.bind decodeDump

        if headerBytes.IsSome && header.IsNone then
            log.LogWarning("[secdump] the event part is not a JSON object ({Bytes} B)", headerBytes.Value.Length)

        if dumpBytes.IsSome && dump.IsNone then
            log.LogWarning("[secdump] the dump part did not decode ({Bytes} B)", dumpBytes.Value.Length)

        if header.IsNone && dump.IsNone then
            Raw.store ctx "secdump" "decode_error" "neither multipart part decoded" body
        elif tenant <> "" then
            let receivedAt = DateTime.UtcNow
            let dumpId = Guid.NewGuid()

            let row =
                dumpRow tenant receivedAt dumpId header (defaultArg headerBytes [||]) dump (defaultArg dumpBytes [||])

            Sink.write sink CwsActivityDumps.table [| row |]

            match dump with
            | Some d -> Sink.write sink CwsDumpNodes.table (flattenTree tenant receivedAt dumpId d.Tree)
            | None -> ()

    accepted ctx

/// Envelope keys with a column of their own; the rest go to extra.
let private knownEnvelopeKeys =
    set [ "message"; "status"; "timestamp"; "hostname"; "service"; "ddsource"; "ddtags" ]

/// The envelope's timestamp: a millisecond epoch as a number or as text, or
/// RFC 3339 text, depending on the producer. None when absent or unreadable.
let parseTimestamp (value: JsonElement option) : DateTime option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.String ->
        let written = Lenient.text v

        match Lenient.rfc3339 written with
        | Some time -> Some time
        | None ->
            match Int64.TryParse(written, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
            | true, ms -> fromMillis ms
            | false, _ -> None
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt64() with
        | true, ms -> fromMillis ms
        | false, _ -> None
    | _ -> None

/// The event inside an envelope's "message": an object, or a string holding
/// one as escaped JSON. None when it is missing or neither.
let innerMessage (message: JsonElement option) : JsonElement option =
    match message with
    | Some m when m.ValueKind = JsonValueKind.Object -> Some m
    | Some m when m.ValueKind = JsonValueKind.String -> Lenient.tryObject (Encoding.UTF8.GetBytes(Lenient.text m))
    | _ -> None

/// A body as a list of envelopes, or why it is not one. A null entry is an
/// empty envelope, a null body an empty batch.
let decodeEnvelopes (body: byte[]) : Result<JsonElement list, string> =
    match Lenient.tryJson body with
    | Error problem -> Error problem
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok []
    | Ok root when root.ValueKind <> JsonValueKind.Array -> Error "the body is JSON, but not an array"
    | Ok root ->
        let envelopes = List.ofSeq (root.EnumerateArray())

        let misfit =
            envelopes
            |> List.tryFindIndex (fun e -> e.ValueKind <> JsonValueKind.Object && e.ValueKind <> JsonValueKind.Null)

        match misfit with
        | Some index -> Error $"entry {index} is not a JSON object"
        | None -> Ok envelopes

/// One row per envelope {message, status, timestamp, hostname, service,
/// ddsource, ddtags}, in arrival order.
///
/// The event sits in "message" as escaped JSON: for secruntime a CWS event,
/// for compliance a check event or resource log. The agent publishes no
/// type for any of them, so the event is kept as JSON. An envelope whose
/// message does not decode still gets its row.
let eventRows (tenant: string) (receivedAt: DateTime) (track: string) (envelopes: JsonElement list) : SecurityEventRow[] =
    envelopes
    |> List.mapi (fun i envelope ->
        let message = property "message" envelope
        let event = innerMessage message
        let eventText (path: string list) = event |> Option.map (nestedText path) |> Option.defaultValue ""

        { TenantID = tenant
          ReceivedAt = receivedAt
          Track = track
          SeqInBatch = uint32 i
          Timestamp = parseTimestamp (property "timestamp" envelope)
          Hostname = text (property "hostname" envelope)
          Service = text (property "service" envelope)
          DDSource = text (property "ddsource" envelope)
          Status = text (property "status" envelope)
          DDTags = Tags.toMultiMap (strings (property "ddtags" envelope))
          Extra =
            (if envelope.ValueKind = JsonValueKind.Object then
                 otherMembers knownEnvelopeKeys envelope
             else
                 Map.empty)
          MessageRaw =
            (match message with
             | Some raw -> raw.GetRawText()
             | None -> "")
          Message = event |> Option.map rewritten |> Option.defaultValue ""
          MessageDecoded = Text.flag event.IsSome
          RuleID = eventText [ "agent"; "rule_id" ]
          PolicyName = eventText [ "agent"; "policy_name" ]
          EvtName = eventText [ "evt"; "name" ]
          EvtCategory = eventText [ "evt"; "category" ]
          Title = eventText [ "title" ]
          EventKind = eventText [ "kind" ] })
    |> Array.ofList

/// A logs-pipeline batch: a JSON array of envelopes. One handler serves
/// three tracks; the rows share a table and carry the track.
let handleTrack (track: string) : byte[] -> EndpointHandler =
    fun body ctx ->
        let tenant = Ctx.tenant ctx
        let log = Ctx.log ctx
        let sink = Ctx.sink ctx

        match decodeEnvelopes body with
        | Error problem ->
            log.LogWarning("[{Track}] not a JSON array of envelopes: {Problem}", track, problem)
            Raw.store ctx track "unexpected_shape" $"top-level body is not a JSON array: {problem}" body
        | Ok envelopes ->
            if tenant <> "" then
                Sink.write sink SecurityEvents.table (eventRows tenant DateTime.UtcNow track envelopes)

        accepted ctx

/// A property list as a multiset: CycloneDX allows a name to repeat, and
/// real scanners do repeat them.
let private propertyMultiMap (properties: Property seq) : Map<string, string[]> =
    properties
    |> Seq.groupBy _.Name
    |> Seq.map (fun (name, entries) -> name, entries |> Seq.map _.Value |> Array.ofSeq)
    |> Map.ofSeq

/// A component's licenses, each by the most specific name it has: the SPDX
/// id, else the license's name, else the expression.
let private licenseNames (licenses: LicenseChoice seq) : string[] =
    licenses
    |> Seq.choose (fun choice ->
        if not (isNull choice.License) then
            Some(if choice.License.Id <> "" then choice.License.Id else choice.License.Name)
        elif choice.Expression <> "" then
            Some choice.Expression
        else
            None)
    |> Array.ofSeq

/// A component list as rows, each component followed by its sub-components.
let rec private componentRows
    (tenant: string)
    (receivedAt: DateTime)
    (entityId: Guid)
    (components: Component seq)
    (parentBomRef: string)
    (depth: uint16)
    : SBOMComponentRow list =
    components
    |> Seq.collect (fun c ->
        let row: SBOMComponentRow =
            { TenantID = tenant
              ReceivedAt = receivedAt
              EntityID = entityId
              BomRef = c.BomRef
              ParentBomRef = parentBomRef
              Depth = depth
              Type = enumName classifications (int c.Type)
              Name = c.Name
              Version = c.Version
              // Optional on the wire: "never set" is NULL, not "".
              Purl = (if c.HasPurl then Some c.Purl else None)
              Cpe = (if c.HasCpe then Some c.Cpe else None)
              Group = (if c.HasGroup then Some c.Group else None)
              Publisher = (if c.HasPublisher then Some c.Publisher else None)
              Author = (if c.HasAuthor then Some c.Author else None)
              Description = (if c.HasDescription then Some c.Description else None)
              Scope = enumName scopes (int c.Scope)
              Licenses = licenseNames c.Licenses
              Hashes = c.Hashes |> Seq.map (fun h -> enumName hashAlgorithms (int h.Alg), h.Value) |> Map.ofSeq
              Properties = propertyMultiMap c.Properties
              ExternalReferences = protoJsonArray c.ExternalReferences
              Evidence = protoJsonArray c.Evidence }

        row :: componentRows tenant receivedAt entityId c.Components c.BomRef (depth + 1us))
    |> List.ofSeq

let private vulnerabilityRows
    (tenant: string)
    (receivedAt: DateTime)
    (entityId: Guid)
    (vulnerabilities: Vulnerability seq)
    : SBOMVulnerabilityRow list =
    vulnerabilities
    |> Seq.map (fun v ->
        let source = Option.ofObj v.Source
        let analysis = Option.ofObj v.Analysis
        let analysisText (pick: VulnerabilityAnalysis -> string) = analysis |> Option.map pick |> Option.defaultValue ""

        { TenantID = tenant
          ReceivedAt = receivedAt
          EntityID = entityId
          BomRef = v.BomRef
          ID = v.Id
          SourceName = source |> Option.bind (fun s -> if s.HasName then Some s.Name else None)
          SourceURL = source |> Option.bind (fun s -> if s.HasUrl then Some s.Url else None)
          Ratings = protoJsonArray v.Ratings
          Cwes = Array.ofSeq v.Cwes
          Description = (if v.HasDescription then Some v.Description else None)
          Detail = (if v.HasDetail then Some v.Detail else None)
          Recommendation = (if v.HasRecommendation then Some v.Recommendation else None)
          Advisories = protoJsonArray v.Advisories
          Created = Option.ofObj v.Created |> Option.bind timestampTime
          Published = Option.ofObj v.Published |> Option.bind timestampTime
          Updated = Option.ofObj v.Updated |> Option.bind timestampTime
          AnalysisState = analysisText (fun a -> enumName analysisStates (int a.State))
          AnalysisJustification = analysisText (fun a -> enumName analysisJustifications (int a.Justification))
          AnalysisResponse =
            (match analysis with
             | Some a -> a.Response |> Seq.map (fun r -> enumName vulnerabilityResponses (int r)) |> Array.ofSeq
             | None -> [||])
          AnalysisDetail = analysisText _.Detail
          AffectsRefs = v.Affects |> Seq.map _.Ref |> Array.ofSeq
          Affects = protoJsonArray v.Affects
          Properties = propertyMultiMap v.Properties })
    |> List.ofSeq

/// A protobuf Duration in whole milliseconds, saturating as Go's
/// AsDuration does when the value does not fit 64 bits of nanoseconds.
let private durationMillis (duration: WellKnownTypes.Duration) : int64 =
    let seconds = duration.Seconds
    let nanos = int64 duration.Nanos
    let total = seconds * 1_000_000_000L + nanos

    let overflows =
        (seconds * 1_000_000_000L) / 1_000_000_000L <> seconds
        || (seconds < 0L && nanos < 0L && total > 0L)
        || (seconds > 0L && nanos > 0L && total < 0L)

    let nanoseconds =
        if overflows && seconds < 0L then Int64.MinValue
        elif overflows && seconds > 0L then Int64.MaxValue
        else total

    nanoseconds / 1_000_000L

/// The rows of an SBOM payload: one per entity, plus every component and
/// vulnerability its CycloneDX BOM unpacks into.
let sbomRows
    (tenant: string)
    (receivedAt: DateTime)
    (payload: SBOMPayload)
    : SBOMEntityRow[] * SBOMComponentRow[] * SBOMVulnerabilityRow[] =
    let entities = ResizeArray<SBOMEntityRow>()
    let components = ResizeArray<SBOMComponentRow>()
    let vulnerabilities = ResizeArray<SBOMVulnerabilityRow>()

    for entity in payload.Entities do
        let entityId = Guid.NewGuid()

        let row: SBOMEntityRow =
            { TenantID = tenant
              ReceivedAt = receivedAt
              EntityID = entityId
              PayloadVersion = payload.Version
              Host = payload.Host
              // Source and dd_env are optional fields of the payload, shared
              // by its entities: unset is NULL, not "".
              Source = (if payload.HasSource then Some payload.Source else None)
              DdEnv = (if payload.HasDdEnv then Some payload.DdEnv else None)
              Type = enumName sbomSourceTypes (int entity.Type)
              ID = entity.Id
              GeneratedAt = Option.ofObj entity.GeneratedAt |> Option.bind timestampTime
              RepoTags = Array.ofSeq entity.RepoTags
              RepoDigests = Array.ofSeq entity.RepoDigests
              InUse = Text.flag entity.InUse
              GenerationDurationMs = Option.ofObj entity.GenerationDuration |> Option.map durationMillis
              DDTags = Tags.toMultiMap entity.DdTags
              Heartbeat = Text.flag entity.Heartbeat
              Hash = entity.Hash
              Status = enumName sbomStatuses (int entity.Status)
              KernelVersion = entity.KernelVersion
              CPUArchitecture = entity.CpuArchitecture
              Error = ""
              Bom = ""
              BomRaw = ""
              ComponentCount = 0u
              VulnerabilityCount = 0u }

        match entity.SbomCase with
        | SBOMEntity.SbomOneofCase.Error -> entities.Add { row with Error = entity.Error }
        | SBOMEntity.SbomOneofCase.Cyclonedx ->
            let bom = entity.Cyclonedx
            let bomJson = protoJson bom
            let entityComponents = componentRows tenant receivedAt entityId bom.Components "" 0us
            let entityVulnerabilities = vulnerabilityRows tenant receivedAt entityId bom.Vulnerabilities
            components.AddRange entityComponents
            vulnerabilities.AddRange entityVulnerabilities

            entities.Add
                { row with
                    Bom = defaultArg bomJson ""
                    // The backstop for a BOM that cannot be written as JSON.
                    BomRaw = (if bomJson.IsSome then "" else Convert.ToBase64String(bom.ToByteArray()))
                    ComponentCount = uint32 entityComponents.Length
                    VulnerabilityCount = uint32 entityVulnerabilities.Length }
        | _ -> entities.Add row

    entities.ToArray(), components.ToArray(), vulnerabilities.ToArray()

/// POST /api/v2/sbom: one SBOMPayload per request, protobuf.
let handleSbom (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx
    let sink = Ctx.sink ctx

    let payload =
        try
            Ok(SBOMPayload.Parser.ParseFrom body)
        with :? InvalidProtocolBufferException as e ->
            Error e.Message

    match payload with
    | Error problem ->
        log.LogWarning("[sbom] protobuf: {Problem} ({Bytes} B)", problem, body.Length)
        Raw.store ctx "sbom" "decode_error" problem body
    | Ok payload when payload.Entities.Count = 0 ->
        // Protobuf reads almost any bytes as some message.
        log.LogWarning("[sbom] decoded to zero entities ({Bytes} B), wrong payload type?", body.Length)
        Raw.store ctx "sbom" "unexpected_shape" "decoded to zero entities" body
    | Ok payload ->
        if tenant <> "" then
            let entities, components, vulnerabilities = sbomRows tenant DateTime.UtcNow payload
            Sink.write sink SbomEntities.table entities
            Sink.write sink SbomComponents.table components
            Sink.write sink SbomVulnerabilities.table vulnerabilities

    accepted ctx

/// A protobuf tag: the field's number and wire type, and how many bytes the
/// tag itself took.
type private WireTag =
    { Field: int
      WireType: int
      Length: int }

let private readTag (data: byte[]) (position: int) : WireTag option =
    match Varint.read data position with
    | Some(tag, length) when tag >>> 3 >= 1UL && tag >>> 3 <= uint64 Int32.MaxValue ->
        Some
            { Field = int (tag >>> 3)
              WireType = int (tag &&& 7UL)
              Length = length }
    | _ -> None

/// The position after a value that is not a group; None when it is cut short.
let private skipScalar (data: byte[]) (position: int) (wireType: int) : int option =
    let fixedWidth (width: int) =
        if position + width <= data.Length then Some(position + width) else None

    match wireType with
    | 0 -> Varint.read data position |> Option.map (fun (_, length) -> position + length)
    | 1 -> fixedWidth 8
    | 5 -> fixedWidth 4
    | 2 ->
        match Varint.read data position with
        | Some(size, length) when size <= uint64 (data.Length - position - length) -> Some(position + length + int size)
        | _ -> None
    | _ -> None

/// As deep as Go's protowire follows nested groups.
let private maxGroupDepth = 10_001

/// The position after the value of a field; None when the value is
/// malformed. A group (wire type 3) runs to the end-group tag of its own
/// field number and may hold further groups: the open ones are kept on a
/// stack, not walked by recursion, so hostile nesting cannot exhaust the
/// thread's stack.
let private skipValue (data: byte[]) (position: int) (number: int) (wireType: int) : int option =
    if wireType <> 3 then
        skipScalar data position wireType
    else
        let openGroups = Collections.Generic.Stack<int>()
        openGroups.Push number
        let mutable at = position
        let mutable failed = false

        while openGroups.Count > 0 && not failed do
            match readTag data at with
            | Some tag when tag.WireType = 4 && tag.Field = openGroups.Peek() ->
                openGroups.Pop() |> ignore
                at <- at + tag.Length
            | Some tag when tag.WireType = 3 && openGroups.Count < maxGroupDepth ->
                openGroups.Push tag.Field
                at <- at + tag.Length
            | Some tag when tag.WireType <> 3 && tag.WireType <> 4 ->
                match skipScalar data (at + tag.Length) tag.WireType with
                | Some next -> at <- next
                | None -> failed <- true
            | _ -> failed <- true

        if failed then None else Some at

/// Consecutive top-level fields of one number, as the layout prints them.
type private FieldRun =
    { Field: int
      WireType: int
      Count: int }

let private wireTypeName (wireType: int) : string =
    match wireType with
    | 0 -> "varint"
    | 5 -> "fixed32"
    | 1 -> "fixed64"
    | 2 -> "bytes"
    | other -> $"type{other}"

/// A protobuf message's top-level fields in wire order, as
/// "<number>:<wire type>[xN]" entries; a run of the same number is one
/// entry. None when the bytes are not a sequence of fields. Nested messages
/// are not entered: without the schema they cannot be told from strings.
let wireLayout (body: byte[]) : string option =
    let runs = ResizeArray<FieldRun>()
    let mutable position = 0
    let mutable failed = false

    while not failed && position < body.Length do
        match readTag body position with
        | None -> failed <- true
        | Some tag ->
            match skipValue body (position + tag.Length) tag.Field tag.WireType with
            | None -> failed <- true
            | Some next ->
                position <- next

                match Seq.tryLast runs with
                | Some last when last.Field = tag.Field -> runs[runs.Count - 1] <- { last with Count = last.Count + 1 }
                | _ -> runs.Add { Field = tag.Field; WireType = tag.WireType; Count = 1 }

    if failed then
        None
    else
        runs
        |> Seq.map (fun run ->
            let entry = $"{run.Field}:{wireTypeName run.WireType}"
            if run.Count > 1 then $"{entry}x{run.Count}" else entry)
        |> String.concat " "
        |> Some

let private sdsOptional (present: bool) (value: 'a) : 'a option = if present then Some value else None

let private sdsTaskStatuses = SdsResultPayload.Types.ScanMetadata.Types.ScanTaskMetadata.Descriptor.EnumTypes[0]

/// The deprecated rules map as one JSON object, rule id to rule; "" when empty.
let private sdsRules (payload: SdsResultPayload) : string =
    if payload.Rules.Count = 0 then
        ""
    else
        let entries =
            payload.Rules
            |> Seq.sortBy _.Key
            |> Seq.map (fun entry -> JsonSerializer.Serialize entry.Key + ":" + JsonFormatter.Default.Format entry.Value)

        "{" + String.Join(",", entries) + "}"

let private sdsTime (stamp: WellKnownTypes.Timestamp) : DateTime option =
    if isNull stamp then None else Some(Time.fromUnixMillis (stamp.Seconds * 1000L + int64 (stamp.Nanos / 1_000_000)))

/// The columns of a result's location. Each kind names its database, schema
/// and table differently; everything else of it stays in the JSON.
let private sdsLocation (location: SdsResultPayload.Types.ScanLocation) : SdsLocation =
    let none: SdsLocation = { Kind = ""; DatabaseName = ""; SchemaName = ""; TableName = ""; Path = "" }

    if isNull location then
        none
    else
        match location.ScanLocationCase with
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.PostgresTable ->
            let table = location.PostgresTable

            { none with
                Kind = "postgres_table"
                DatabaseName = table.DatabaseName
                SchemaName = table.SchemaName
                TableName = table.TableName }
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.SnowflakeTable ->
            let table = location.SnowflakeTable

            { none with
                Kind = "snowflake_table"
                DatabaseName = table.DatabaseName
                SchemaName = table.SchemaName
                TableName = table.TableName }
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.RdsTable ->
            { none with
                Kind = "rds_table"
                DatabaseName = location.RdsTable.DatabaseName
                TableName = location.RdsTable.TableName }
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.S3File -> { none with Kind = "s3_file"; Path = location.S3File.Path }
        | _ ->
            // The deprecated fields, which older scanners still fill.
            match location.ScanLocationTypeCase with
            | SdsResultPayload.Types.ScanLocation.ScanLocationTypeOneofCase.Path -> { none with Kind = "path"; Path = location.Path }
            | SdsResultPayload.Types.ScanLocation.ScanLocationTypeOneofCase.Database ->
                { none with Kind = "database"; DatabaseName = location.Database; TableName = location.Table }
            | _ -> none

/// What a table location says about the table: None where its kind has no
/// such field.
type private SdsTableFacts =
    { TableRowCount: int64 option
      ScannedRowCount: int64 option
      ColumnNames: string[]
      ColumnTypes: string[] }

let private sdsTableFacts (location: SdsResultPayload.Types.ScanLocation) : SdsTableFacts =
    let none = { TableRowCount = None; ScannedRowCount = None; ColumnNames = [||]; ColumnTypes = [||] }

    if isNull location then
        none
    else
        match location.ScanLocationCase with
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.PostgresTable ->
            let table = location.PostgresTable

            { TableRowCount = Some table.TableRowCount
              ScannedRowCount = Some table.ScannedRowCount
              ColumnNames = [| for c in table.ScannedColumns -> c.Name |]
              ColumnTypes = [| for c in table.ScannedColumns -> c.DataType |] }
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.SnowflakeTable ->
            let table = location.SnowflakeTable

            { TableRowCount = Some table.TableRowCount
              ScannedRowCount = Some table.ScannedRowCount
              ColumnNames = [| for c in table.ScannedColumns -> c.Name |]
              ColumnTypes = [| for c in table.ScannedColumns -> c.DataType |] }
        | SdsResultPayload.Types.ScanLocation.ScanLocationOneofCase.RdsTable ->
            let table = location.RdsTable

            { none with
                TableRowCount = sdsOptional table.HasTableRowCount table.TableRowCount
                ScannedRowCount = sdsOptional table.HasScannedRowCount table.ScannedRowCount }
        | _ -> none

/// Who scanned: the kind, and that kind's own fields.
type private SdsSource =
    { Kind: string
      Version: string
      Region: string
      Hostname: string
      ServiceName: string }

let private sdsSource (source: ScanningSource) : SdsSource =
    let none = { Kind = ""; Version = ""; Region = ""; Hostname = ""; ServiceName = "" }

    if isNull source then
        none
    else
        match source.SourceCase with
        | ScanningSource.SourceOneofCase.Agent -> { none with Kind = "agent"; Version = source.Agent.Version; Hostname = source.Agent.Hostname }
        | ScanningSource.SourceOneofCase.Agentless ->
            { none with Kind = "agentless"; Version = source.Agentless.Version; Region = source.Agentless.Region }
        | ScanningSource.SourceOneofCase.DatadogCrawler -> { none with Kind = "datadog_crawler"; ServiceName = source.DatadogCrawler.ServiceName }
        | _ -> none

/// One payload as the rows of its three tables.
type SdsRows =
    { Scan: SdsScanRow
      Results: SdsResultRow list
      Matches: SdsMatchRow list }

let sdsRows (tenant: string) (receivedAt: DateTime) (scanId: Guid) (payload: SdsResultPayload) : SdsRows =
    let resourceType, resourceName =
        if isNull payload.Resource then "", "" else payload.Resource.Type, payload.Resource.Name

    let source = sdsSource payload.ScanningSource

    let stats = payload.ScanStats
    let stat (read: ScanStats -> int64) : int64 option = if isNull stats then None else Some(read stats)

    let scan: SdsScanRow =
        { TenantID = tenant
          ReceivedAt = receivedAt
          ScanID = scanId
          Timestamp = (if payload.Timestamp = 0L then None else Some(Time.fromUnixMillis payload.Timestamp))
          ResourceType = resourceType
          ResourceName = resourceName
          ScanningSource = source.Kind
          SourceVersion = source.Version
          SourceRegion = source.Region
          SourceHostname = source.Hostname
          SourceServiceName = source.ServiceName
          ScannerVersion = (if isNull payload.ScannerMetadata then "" else payload.ScannerMetadata.Version)
          ScannerRegion = (if isNull payload.ScannerMetadata then "" else payload.ScannerMetadata.Region)
          RuleIDs = Array.ofSeq payload.RuleIds
          Rules = sdsRules payload
          ScanDurationMs = stat _.ScanDurationMs
          TotalFilesFound = stat _.TotalFilesFound
          FilesScanned = stat _.FilesScanned
          FilesSkippedUnsupportedType = stat _.FilesSkippedUnsupportedType
          FilesPartiallyScannedSizeLimit = stat _.FilesPartiallyScannedSizeLimit
          TotalDataScannedBytes = stat _.TotalDataScannedBytes
          SkippedFilesByType =
            (if isNull stats then Map.empty else stats.SkippedFilesByType |> Seq.map (fun e -> e.Key, e.Value) |> Map.ofSeq)
          ResultCount = uint32 payload.ScanResults.Count }

    let results = ResizeArray<SdsResultRow>()
    let matches = ResizeArray<SdsMatchRow>()

    payload.ScanResults
    |> Seq.iteri (fun i result ->
        let index = uint32 i
        let location = sdsLocation result.Location
        let table = sdsTableFacts result.Location

        let task =
            if isNull result.ScanMetadata || isNull result.ScanMetadata.ScanTaskMetadata then
                None
            else
                Some result.ScanMetadata.ScanTaskMetadata

        results.Add
            { TenantID = tenant
              ReceivedAt = receivedAt
              ScanID = scanId
              ResultIndex = index
              ResourceType = resourceType
              ResourceName = resourceName
              Location = location
              TableRowCount = table.TableRowCount
              ScannedRowCount = table.ScannedRowCount
              ScannedColumnNames = table.ColumnNames
              ScannedColumnTypes = table.ColumnTypes
              LocationJson = (if isNull result.Location then "" else JsonFormatter.Default.Format result.Location)
              Duration = result.Duration
              TaskID = (task |> Option.map _.TaskId |> Option.defaultValue "")
              SubTaskID = (task |> Option.map _.SubTaskId |> Option.defaultValue "")
              TaskStartedAt = task |> Option.bind (fun t -> sdsTime t.StartedAt)
              TaskEndedAt = task |> Option.bind (fun t -> sdsTime t.EndedAt)
              TaskStatus = (task |> Option.map (fun t -> enumName sdsTaskStatuses (int t.Status)) |> Option.defaultValue "")
              FailureReason = (task |> Option.map _.FailureReason |> Option.defaultValue "")
              MatchCount = uint32 result.Matches.Count
              TableMatchCount = uint32 result.TableMatches.Count }

        let blank: SdsMatchRow =
            { TenantID = tenant
              ReceivedAt = receivedAt
              ScanID = scanId
              ResultIndex = index
              ResourceType = resourceType
              ResourceName = resourceName
              Location = location
              Kind = ""
              RuleID = ""
              Sample = ""
              StartIndex = 0L
              EndIndex = 0L
              MatchPath = None
              Line = None
              Column = None
              StartLine = None
              EndLine = None
              Row = None
              StartIndexInLine = None
              EndIndexInLine = None
              MatchStatus = None
              ColumnName = ""
              CountMatchedRows = 0L
              CountTotalRows = 0L
              CountMatches = 0L }

        for m in result.Matches do
            matches.Add
                { blank with
                    Kind = "match"
                    RuleID = m.RuleId
                    Sample = m.Sample
                    StartIndex = m.StartIndex
                    EndIndex = m.EndIndex
                    MatchPath = sdsOptional m.HasPath m.Path
                    Line = sdsOptional m.HasLine m.Line
                    Column = sdsOptional m.HasColumn m.Column
                    StartLine = sdsOptional m.HasStartLine m.StartLine
                    EndLine = sdsOptional m.HasEndLine m.EndLine
                    Row = sdsOptional m.HasRow m.Row
                    StartIndexInLine = sdsOptional m.HasStartIndexInLine m.StartIndexInLine
                    EndIndexInLine = sdsOptional m.HasEndIndexInLine m.EndIndexInLine
                    MatchStatus = sdsOptional m.HasMatchStatus m.MatchStatus }

        for m in result.TableMatches do
            matches.Add
                { blank with
                    Kind = "table_match"
                    RuleID = m.RuleId
                    ColumnName = m.ColumnName
                    CountMatchedRows = m.CountMatchedRows
                    CountTotalRows = m.CountTotalRows
                    CountMatches = m.CountMatches })

    { Scan = scan
      Results = List.ofSeq results
      Matches = List.ofSeq matches }

let private sdsIsBlank (payload: SdsResultPayload) : bool =
    let unnamed = isNull payload.Resource || (payload.Resource.Type = "" && payload.Resource.Name = "")
    unnamed && payload.ScanResults.Count = 0

/// POST /api/v2/sdsresult: Sensitive Data Scanner results, one
/// SdsResultPayload per request (datadog/sds/sds_result.proto).
///
/// A protobuf parser fails open: bytes meant for another endpoint decode
/// into a message with nothing in it. A payload that names no resource and
/// has no result is therefore kept raw, its wire layout in the note, instead
/// of becoming an empty scan.
let handleSdsResult (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx
    let sink = Ctx.sink ctx

    let layout () =
        match wireLayout body with
        | None -> "protobuf did not parse as a sequence of top-level fields"
        | Some "" -> "no fields (empty or wrong payload type?)"
        | Some layout -> layout

    let parsed =
        try
            Ok(SdsResultPayload.Parser.ParseFrom body)
        with :? InvalidProtocolBufferException as e ->
            Error e.Message

    match parsed with
    | Error problem ->
        log.LogWarning("[sds] protobuf: {Error} ({Bytes} bytes)", problem, body.Length)
        Raw.store ctx "sds" "decode_error" $"protobuf SdsResultPayload: {problem}; {layout ()}" body
    | Ok payload when sdsIsBlank payload ->
        Raw.store ctx "sds" "unexpected_shape" $"not an SdsResultPayload: no resource and no results; {layout ()}" body
    | Ok payload ->
        if tenant <> "" then
            let rows = sdsRows tenant DateTime.UtcNow (Guid.NewGuid()) payload
            Sink.write sink SdsScans.table [| rows.Scan |]
            Sink.write sink SdsResults.table (Array.ofList rows.Results)
            Sink.write sink SdsMatches.table (Array.ofList rows.Matches)

    accepted ctx
