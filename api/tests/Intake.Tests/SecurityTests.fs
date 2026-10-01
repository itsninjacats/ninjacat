/// Tests of Routers/Security.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.SecurityTests

open System
open System.IO
open System.Text
open System.Text.Json
open Google.Protobuf
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

type private Dump = Datadog.Cws.Dumpsv1.SecDump
type private Node = Datadog.Cws.Dumpsv1.ProcessActivityNode
type private Bom = Cyclonedx.V14.Bom
type private Component = Cyclonedx.V14.Component
type private Vulnerability = Cyclonedx.V14.Vulnerability
type private SbomPayload = Datadog.Sbom.SBOMPayload
type private SbomEntity = Datadog.Sbom.SBOMEntity

let private utf8 (text: string) : byte[] = Encoding.UTF8.GetBytes text

let private jsonValue (text: string) : JsonElement =
    use document = JsonDocument.Parse text
    document.RootElement.Clone()

let private time (text: string) : DateTime =
    DateTime.Parse(text, Globalization.CultureInfo.InvariantCulture, Globalization.DateTimeStyles.RoundtripKind)

let private dumpId = Guid.Parse "01020304-0506-0708-090a-0b0c0d0e0f10"
let private now = DateTime(2026, 9, 23, 10, 0, 0, DateTimeKind.Utc)

/// A multipart/form-data body of the given parts, and its Content-Type.
let private multipart (parts: (string * byte[]) list) : string * byte[] =
    let boundary = "security-tests-boundary"
    use body = new MemoryStream()
    let write (data: byte[]) = body.Write(data, 0, data.Length)

    for name, data in parts do
        write (utf8 $"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
        write data
        write (utf8 "\r\n")

    write (utf8 $"--{boundary}--\r\n")
    $"multipart/form-data; boundary={boundary}", body.ToArray()

/// One POST through a route set, as the Go tests sent them: the answer and
/// what was written.
let private post (routeSet: string) (path: string) (contentType: string) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey

    if contentType <> "" then
        http.Request.Headers["Content-Type"] <- StringValues contentType

    Routes.byGoNames deps [ routeSet ] http body, sink

/// init → bash (-c "cat file", matched by r1) → cat: one root, one child
/// with a rule match and args, one grandchild.
let private threeLevelTree () : Node =
    let grandchild = Node(Process = Datadog.Cws.Dumpsv1.ProcessInfo(Pid = 3u, Ppid = 2u, Comm = "cat"))
    grandchild.Process.File <- Datadog.Cws.Dumpsv1.FileInfo(Path = "/bin/cat")
    let child = Node(Process = Datadog.Cws.Dumpsv1.ProcessInfo(Pid = 2u, Ppid = 1u, Comm = "bash"))
    child.Process.Args.AddRange [ "-c"; "cat file > out" ]
    child.MatchedRules.Add(Datadog.Cws.Dumpsv1.MatchedRule(RuleId = "r1", PolicyName = "default"))
    child.Children.Add grandchild
    let root = Node(Process = Datadog.Cws.Dumpsv1.ProcessInfo(Pid = 1u, Comm = "init"))
    root.Children.Add child
    root

[<Fact>]
let ``a tree is flattened depth-first, each node knowing its path`` () =
    let second = Node(GenerationType = enum<Datadog.Cws.Dumpsv1.GenerationType> 7)
    let rows = Security.flattenTree "default" now dumpId [ threeLevelTree (); second ]

    Assert.Equal(4, rows.Length)
    let root, child, grandchild, other = rows[0], rows[1], rows[2], rows[3]

    Assert.Equal<uint32[]>([| 0u |], root.NodePath)
    Assert.Empty root.ParentPath
    Assert.Equal(1us, root.Depth)
    Assert.Equal("init", root.Comm)
    Assert.Equal(1u, root.PID)
    Assert.Equal("UNKNOWN", root.GenerationType)

    Assert.Equal<uint32[]>([| 0u; 0u |], child.NodePath)
    Assert.Equal<uint32[]>(root.NodePath, child.ParentPath)
    Assert.Equal(2us, child.Depth)
    Assert.Equal<string[]>([| "-c"; "cat file > out" |], child.Args)
    Assert.Equal<string[]>([| "r1" |], child.MatchedRuleIDs)
    Assert.Equal(1u, child.PPID)

    Assert.Equal<uint32[]>([| 0u; 0u; 0u |], grandchild.NodePath)
    Assert.Equal<uint32[]>(child.NodePath, grandchild.ParentPath)
    Assert.Equal(3us, grandchild.Depth)
    Assert.Equal("/bin/cat", grandchild.FilePath)

    // A node without a process, and a generation type this build does not know.
    Assert.Equal<uint32[]>([| 1u |], other.NodePath)
    Assert.Equal("", other.Comm)
    Assert.Equal("7", other.GenerationType)

    for row in rows do
        Assert.Equal(dumpId, row.DumpID)
        Assert.Equal("default", row.TenantID)

[<Fact>]
let ``a node's JSON leaves its children out and stays readable`` () =
    let rows = Security.flattenTree "default" now dumpId [ threeLevelTree () ]
    let child = jsonValue rows[1].Node

    Assert.False(fst (child.TryGetProperty "children"))
    Assert.Equal("bash", child.GetProperty("process").GetProperty("comm").GetString())
    let rule = child.GetProperty("matchedRules").EnumerateArray() |> Seq.head
    Assert.Equal("r1", rule.GetProperty("ruleId").GetString())
    // The > is not escaped: the text is searched as it is in ClickHouse.
    Assert.Contains("cat file > out", rows[1].Node)

/// Without metadata the counters are absent, not zero.
[<Fact>]
let ``a dump without metadata has no start, end or size`` () =
    let dump = Dump(Host = "h1")
    dump.Tree.Add(threeLevelTree ())
    let row = Security.dumpRow "default" now dumpId None [||] (Some dump) [| 0x08uy; 0x01uy |]

    Assert.Equal(None, row.Start)
    Assert.Equal(None, row.End)
    Assert.Equal(None, row.Size)
    Assert.Equal(3u, row.TreeNodeCount)
    Assert.Equal("h1", row.DumpHost)
    Assert.Equal<byte[]>([| 0x08uy; 0x01uy |], row.Dump)

/// A start of 0 with metadata present is a real reading: the counters are
/// relative to kernel boot.
[<Fact>]
let ``metadata with a zero counter is a present zero`` () =
    let dump = Dump(Metadata = Datadog.Cws.Dumpsv1.Metadata(AgentVersion = "7.58.2", Start = 0UL, End = 500UL, DifferentiateArgs = true))
    dump.Tags.AddRange [ "env:prod"; "kube_service:a"; "kube_service:b" ]
    let row = Security.dumpRow "default" now dumpId None [||] (Some dump) [||]

    Assert.Equal(Some 0UL, row.Start)
    Assert.Equal(Some 500UL, row.End)
    Assert.Equal(Some 0UL, row.Size)
    Assert.Equal("7.58.2", row.AgentVersion)
    Assert.Equal(1uy, row.DifferentiateArgs)
    Assert.Equal<string[]>([| "a"; "b" |], row.DumpTags["kube_service"])

[<Fact>]
let ``the header's tags are a multiset and its unknown keys are kept`` () =
    let header =
        jsonValue
            """{"host": "h1", "service": "cws-agent", "ddsource": "security-agent",
                "ddtags": ["env:prod","kube_service:a","kube_service:b"],
                "dns_names": ["example.com"], "unexpected": {"nested":true}, "n": 1.50}"""

    let row = Security.dumpRow "default" now dumpId (Some header) [||] None [||]

    Assert.Equal<string list>([ "h1"; "cws-agent"; "security-agent" ], [ row.HeaderHost; row.HeaderService; row.HeaderSource ])
    Assert.Equal<string[]>([| "a"; "b" |], row.HeaderTags["kube_service"])
    Assert.Equal("""["example.com"]""", row.DNSNames)
    Assert.Equal("""{"nested":true}""", row.HeaderExtra["unexpected"])
    Assert.Equal("1.50", row.HeaderExtra["n"])
    Assert.Equal(2, row.HeaderExtra.Count)

[<Theory>]
[<InlineData("\"env:prod,team:sec\"", "env=prod team=sec")>]
[<InlineData("[\"a:b\", null, \"a:c\"]", "= a=b,c")>]
[<InlineData("[\"a:b\", 7]", "")>]
[<InlineData("\"\"", "")>]
[<InlineData("null", "")>]
[<InlineData("5", "")>]
let ``ddtags may be a list or one comma-separated string`` (ddtags: string, expected: string) =
    let header = jsonValue $"{{\"ddtags\": {ddtags}}}"
    let row = Security.dumpRow "default" now dumpId (Some header) [||] None [||]

    let shown =
        row.HeaderTags |> Seq.map (fun pair -> pair.Key + "=" + String.Join(",", pair.Value)) |> String.concat " "

    Assert.Equal(expected, shown)

[<Fact>]
let ``an event part that is not JSON keeps its bytes beside a good dump`` () =
    let badEvent = utf8 "not a json object"
    let dump = Dump(Host = "h1", Metadata = Datadog.Cws.Dumpsv1.Metadata(AgentVersion = "7.58.2"))
    let row = Security.dumpRow "default" now dumpId None badEvent (Some dump) [| 0x08uy; 0x01uy |]

    Assert.Equal<byte[]>(badEvent, row.HeaderRaw)
    Assert.Equal("", row.HeaderHost)
    Assert.Equal("h1", row.DumpHost)
    Assert.Equal("7.58.2", row.AgentVersion)

[<Fact>]
let ``bytes that decode to an empty dump are not a dump`` () =
    Assert.True((Security.decodeDump [||]).IsNone)
    // A valid message of some other type: field 1 only, no metadata, no tree.
    Assert.True((Security.decodeDump [| 0x0Auy; 0x02uy; byte 'h'; byte '1' |]).IsNone)
    Assert.True((Security.decodeDump (utf8 "not protobuf")).IsNone)
    Assert.True((Security.decodeDump (Dump(Metadata = Datadog.Cws.Dumpsv1.Metadata()).ToByteArray())).IsSome)

[<Fact>]
let ``a dump with a header only is stored without nodes`` () =
    let contentType, body = multipart [ "event", utf8 """{"host":"h1"}""" ]
    let response, sink = post "routeCWS" "/api/v2/secdump" contentType body

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    Assert.Equal("h1", (Assert.Single(sink.Rows<CWSActivityDumpRow>())).HeaderHost)
    Assert.Equal(1, sink.Writes.Length)

[<Theory>]
[<InlineData("application/json", "content-type is not multipart/form-data")>]
[<InlineData("", "content-type is not multipart/form-data")>]
[<InlineData("multipart/form-data", "content-type is not multipart/form-data")>]
[<InlineData("multipart/form-data; boundary=security-tests-boundary", "neither multipart part decoded")>]
let ``a dump request with nothing to decode is kept raw`` (contentType: string, note: string) =
    let _, body = multipart [ "event", utf8 "[1]"; "dump", [||]; "other", utf8 "x" ]
    let response, sink = post "routeCWS" "/api/v2/secdump" contentType body

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("secdump", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal(note, raw.Note)
    Assert.Equal<byte[]>(body, raw.Body)
    Assert.Equal(1, sink.Writes.Length)

let private envelopes (body: string) : JsonElement list =
    match Security.decodeEnvelopes (utf8 body) with
    | Ok found -> found
    | Error problem -> failwith problem

[<Fact>]
let ``an envelope whose message does not decode still gets its row`` () =
    let batch =
        envelopes
            """[
            {"hostname":"h1","service":"cws-agent","ddsource":"security-agent","status":"info",
             "timestamp":1700000000000,"ddtags":["env:prod","team:sec"],
             "message":"{\"agent\":{\"rule_id\":\"r1\",\"policy_name\":\"default\"},\"evt\":{\"name\":\"exec\",\"category\":\"process\"}}"},
            {"hostname":"h2","service":"cws-agent","ddsource":"security-agent","status":"info","message":"not json"}
        ]"""

    let rows = Security.eventRows "default" now "secruntime" batch
    Assert.Equal(2, rows.Length)
    let ok, bad = rows[0], rows[1]

    Assert.Equal(1uy, ok.MessageDecoded)
    Assert.Equal<string list>([ "r1"; "default"; "exec"; "process" ], [ ok.RuleID; ok.PolicyName; ok.EvtName; ok.EvtCategory ])
    Assert.Equal<string[]>([| "sec" |], ok.DDTags["team"])
    Assert.Equal(Some(time "2023-11-14T22:13:20Z"), ok.Timestamp)
    Assert.Equal(0u, ok.SeqInBatch)
    Assert.Equal(1u, bad.SeqInBatch)
    Assert.Equal<string list>([ "h2"; "cws-agent"; "security-agent"; "info"; "secruntime" ], [ bad.Hostname; bad.Service; bad.DDSource; bad.Status; bad.Track ])

    Assert.Equal(0uy, bad.MessageDecoded)
    Assert.Equal("", bad.Message)
    Assert.Equal("\"not json\"", bad.MessageRaw)
    Assert.Equal(None, bad.Timestamp)

[<Fact>]
let ``an event is kept as JSON with its numbers intact, a repeated key keeping its last value`` () =
    let batch =
        envelopes
            """[{"custom":{"a":[1,2]},"n":1.50,"hostname":5,
                 "message":"{\"title\":\"T <b>\",\"kind\":\"alert\",\"big\":12345678901234567890,\"z\":1,\"z\":2,\"f\":1.0}"}]"""

    let row = Assert.Single(Security.eventRows "default" now "compliance" batch)

    Assert.Equal("""{"big":12345678901234567890,"f":1.0,"kind":"alert","title":"T <b>","z":2}""", row.Message)
    Assert.Equal("T <b>", row.Title)
    Assert.Equal("alert", row.EventKind)
    // What is not a known key is kept as the JSON it arrived as.
    Assert.Equal("""{"a":[1,2]}""", row.Extra["custom"])
    Assert.Equal("1.50", row.Extra["n"])
    Assert.Equal(2, row.Extra.Count)
    // A hostname that is not a string is no hostname.
    Assert.Equal("", row.Hostname)

/// The inner event: an object, or a string holding one.
[<Theory>]
[<InlineData("{\"agent\":{\"rule_id\":\"r1\"}}", 1, "r1")>]
[<InlineData("\"{\\\"agent\\\":{\\\"rule_id\\\":\\\"r1\\\"}}\"", 1, "r1")>]
[<InlineData("{\"agent\":{\"rule_id\":5}}", 1, "")>]
[<InlineData("{\"agent\":\"not an object\"}", 1, "")>]
[<InlineData("\"{}\"", 1, "")>]
[<InlineData("\"null\"", 0, "")>]
[<InlineData("\"[1,2]\"", 0, "")>]
[<InlineData("\"{} trailing\"", 0, "")>]
[<InlineData("\"\"", 0, "")>]
[<InlineData("null", 0, "")>]
[<InlineData("7", 0, "")>]
let ``a message is decoded when it is an object or holds one`` (message: string, decoded: int, ruleId: string) =
    let row = Assert.Single(Security.eventRows "t" now "secinfo" (envelopes $"[{{\"message\": {message}}}]"))

    Assert.Equal(uint8 decoded, row.MessageDecoded)
    Assert.Equal(ruleId, row.RuleID)
    Assert.Equal(message, row.MessageRaw)
    Assert.Equal((decoded = 1), (row.Message <> ""))

[<Theory>]
[<InlineData("1700000000123", "2023-11-14T22:13:20.1230000Z")>]
[<InlineData("\"1700000000456\"", "2023-11-14T22:13:20.4560000Z")>]
[<InlineData("\"2025-09-23T10:30:00.250+02:00\"", "2025-09-23T08:30:00.2500000Z")>]
[<InlineData("\"2025-09-23T10:30:00.123456789Z\"", "2025-09-23T10:30:00.1234567Z")>]
// Zero and negative epochs are times here, unlike in the other intakes.
[<InlineData("0", "1970-01-01T00:00:00.0000000Z")>]
[<InlineData("-1000", "1969-12-31T23:59:59.0000000Z")>]
[<InlineData("1700000000.5", "")>]
[<InlineData("\"soon\"", "")>]
[<InlineData("\" 1700000000456\"", "")>]
[<InlineData("null", "")>]
[<InlineData("true", "")>]
// Milliseconds past the year 9999: Go has such a time, a DateTime does not.
// Past year 9999: kept at the limit.
[<InlineData("999999999999999999", "9999-12-31T23:59:59.9990000Z")>]
let ``an envelope's timestamp is a millisecond epoch or RFC 3339 text`` (value: string, expected: string) =
    let shown =
        match Security.parseTimestamp (Some(jsonValue value)) with
        | Some t -> t.ToString "o"
        | None -> ""

    Assert.Equal(expected, shown)

[<Fact>]
let ``a missing timestamp is no time`` () =
    Assert.Equal(None, Security.parseTimestamp None)

/// Go's decoder puts U+FFFD where a string is not text and carries on.
[<Fact>]
let ``an envelope with text that is not valid Unicode keeps its row`` () =
    let body =
        Array.concat
            [ utf8 "[{\"hostname\":\"h"
              [| 0xFFuy |]
              utf8 "\",\"service\":\"s\\ud800\",\"k\\udc00\":{\"a\\ud800\":1},"
              utf8 "\"message\":{\"title\":\"t\\ud800\",\"k\\udc00\":2,\"agent\":{\"rule_id\":\"r\\ud800\"}}}]" ]

    let response, sink = post "routeRuntimeSecurity" "/api/v2/secruntime" "application/json" body

    Assert.Equal(202, response.Status)
    let row = Assert.Single(sink.Rows<SecurityEventRow>())
    Assert.Equal("h\uFFFD", row.Hostname)
    Assert.Equal("s\uFFFD", row.Service)
    Assert.Equal("t\uFFFD", row.Title)
    Assert.Equal("r\uFFFD", row.RuleID)
    Assert.Equal(1uy, row.MessageDecoded)
    Assert.Equal("{\"a\\ud800\":1}", row.Extra["k\uFFFD"])
    Assert.Equal(2, (jsonValue row.Message).GetProperty("k\uFFFD").GetInt32())

[<Fact>]
let ``an envelope nested far deeper than .NET's default is still read`` () =
    let deep = String('[', 600) + String(']', 600)
    let rows = Security.eventRows "t" now "secruntime" (envelopes $"[{{\"hostname\": \"h\", \"deep\": {deep}}}]")

    Assert.Equal("h", (Assert.Single rows).Hostname)
    Assert.Equal(deep, rows[0].Extra["deep"])

[<Fact>]
let ``null entries are empty envelopes and a null body an empty batch`` () =
    let rows = Security.eventRows "t" now "secruntime" (envelopes "[null, {}]")

    Assert.Equal(2, rows.Length)
    Assert.Equal<string list>([ ""; "" ], [ rows[0].MessageRaw; rows[1].MessageRaw ])
    Assert.True rows[0].Extra.IsEmpty
    Assert.Empty(envelopes "null")
    Assert.Empty(envelopes " [] ")

[<Theory>]
[<InlineData("routeRuntimeSecurity", "/api/v2/secruntime", "secruntime")>]
[<InlineData("routeRuntimeSecurity", "/api/v2/secinfo", "secinfo")>]
[<InlineData("routeCSPM", "/api/v2/compliance", "compliance")>]
let ``each track stores its envelopes under its own name`` (routeSet: string, path: string, track: string) =
    let body = utf8 """[{"hostname":"h1","message":"{\"kind\":\"remediation\"}"}]"""
    let response, sink = post routeSet path "application/json" body

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    let row = Assert.Single(sink.Rows<SecurityEventRow>())
    Assert.Equal(track, row.Track)
    Assert.Equal(Replay.testTenant, row.TenantID)
    Assert.Equal("remediation", row.EventKind)

[<Theory>]
[<InlineData("""{"hostname":"h1"}""", "top-level body is not a JSON array: the body is JSON, but not an array")>]
[<InlineData("\"text\"", "top-level body is not a JSON array: the body is JSON, but not an array")>]
[<InlineData("""[{"hostname":"h1"}, 7]""", "top-level body is not a JSON array: entry 1 is not a JSON object")>]
let ``a batch that is not a list of envelopes is kept raw under its track`` (body: string, note: string) =
    let response, sink = post "routeCSPM" "/api/v2/compliance" "application/json" (utf8 body)

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("compliance", raw.Intake)
    Assert.Equal("unexpected_shape", raw.Reason)
    Assert.Equal(note, raw.Note)
    Assert.Equal(1, sink.Writes.Length)

[<Fact>]
let ``a batch that is not JSON is kept raw with the parser's reason`` () =
    let _, sink = post "routeRuntimeSecurity" "/api/v2/secinfo" "application/json" (utf8 "not json")

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("secinfo", raw.Intake)
    Assert.Equal("unexpected_shape", raw.Reason)
    Assert.StartsWith("top-level body is not a JSON array: ", raw.Note)

[<Theory>]
[<InlineData("null")>]
[<InlineData("[]")>]
[<InlineData("")>]
[<InlineData("{}")>]
let ``an empty batch or a probe stores nothing`` (body: string) =
    let response, sink = post "routeRuntimeSecurity" "/api/v2/secruntime" "application/json" (utf8 body)

    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

let private payloadOf (entity: SbomEntity) : SbomPayload =
    let payload = SbomPayload(Version = 1, Host = "h1")
    payload.Entities.Add entity
    payload

/// An entity with two components, one nested under the other, and one
/// vulnerability.
[<Fact>]
let ``an SBOM is unpacked into its entity, components and vulnerabilities`` () =
    let nested = Component(BomRef = "pkg:c1-sub", Name = "libbar", Version = "2.0", Type = Cyclonedx.V14.Classification.Library)
    let top = Component(BomRef = "pkg:c1", Name = "libfoo", Version = "1.0", Type = Cyclonedx.V14.Classification.Library)
    top.Licenses.Add(Cyclonedx.V14.LicenseChoice(License = Cyclonedx.V14.License(Id = "MIT")))
    top.Hashes.Add(Cyclonedx.V14.Hash(Alg = Cyclonedx.V14.HashAlg.Sha256, Value = "abcd"))
    top.Components.Add nested

    let vulnerability = Vulnerability(BomRef = "pkg:c1", Id = "CVE-2024-1234", Source = Cyclonedx.V14.Source(Name = "NVD"))
    vulnerability.Cwes.Add 79
    vulnerability.Affects.Add(Cyclonedx.V14.VulnerabilityAffects(Ref = "pkg:c1"))

    let bom = Bom(SpecVersion = "1.4")
    bom.Components.Add top
    bom.Vulnerabilities.Add vulnerability

    let payload = payloadOf (SbomEntity(Type = Datadog.Sbom.SBOMSourceType.ContainerImageLayers, Id = "e1", Cyclonedx = bom))
    let entities, components, vulnerabilities = Security.sbomRows "default" now payload

    let entity = Assert.Single entities
    Assert.Equal(2u, entity.ComponentCount)
    Assert.Equal(1u, entity.VulnerabilityCount)
    Assert.Equal("CONTAINER_IMAGE_LAYERS", entity.Type)
    Assert.Equal("SUCCESS", entity.Status)
    Assert.Equal("1.4", (jsonValue entity.Bom).GetProperty("specVersion").GetString())
    Assert.Equal("", entity.BomRaw)
    Assert.Equal("", entity.Error)
    // Never set on the payload: absent, not "".
    Assert.Equal(None, entity.Source)
    Assert.Equal(None, entity.DdEnv)
    Assert.Equal(None, entity.GeneratedAt)
    Assert.Equal(None, entity.GenerationDurationMs)

    Assert.Equal(2, components.Length)
    let parent, child = components[0], components[1]
    Assert.Equal<string list>([ "pkg:c1"; ""; "pkg:c1-sub"; "pkg:c1" ], [ parent.BomRef; parent.ParentBomRef; child.BomRef; child.ParentBomRef ])
    Assert.Equal<uint16 list>([ 0us; 1us ], [ parent.Depth; child.Depth ])
    Assert.Equal("CLASSIFICATION_LIBRARY", parent.Type)
    Assert.Equal("SCOPE_UNSPECIFIED", parent.Scope)
    Assert.Equal<string[]>([| "MIT" |], parent.Licenses)
    Assert.Equal("abcd", parent.Hashes["HASH_ALG_SHA_256"])
    Assert.Equal<string option list>([ None; None; None; None; None; None ], [ parent.Purl; parent.Cpe; parent.Group; parent.Publisher; parent.Author; parent.Description ])
    Assert.Equal<string list>([ ""; "" ], [ parent.ExternalReferences; parent.Evidence ])
    // The id minted for the entity is what its rows join on.
    Assert.Equal<Guid list>([ entity.EntityID; entity.EntityID ], [ parent.EntityID; child.EntityID ])

    let found = Assert.Single vulnerabilities
    Assert.Equal("CVE-2024-1234", found.ID)
    Assert.Equal(Some "NVD", found.SourceName)
    Assert.Equal(None, found.SourceURL)
    Assert.Equal<int32[]>([| 79 |], found.Cwes)
    Assert.Equal<string[]>([| "pkg:c1" |], found.AffectsRefs)
    Assert.Equal("""[{"ref":"pkg:c1"}]""", found.Affects)
    Assert.Equal(entity.EntityID, found.EntityID)
    // No analysis at all: empty, not the zero value's name.
    Assert.Equal<string list>([ ""; "" ], [ found.AnalysisState; found.AnalysisJustification ])

[<Fact>]
let ``an entity that failed carries its error and no BOM`` () =
    let entity = SbomEntity(Id = "e1", Error = "generation timed out", Status = Datadog.Sbom.SBOMStatus.Failed)
    entity.Type <- enum<Datadog.Sbom.SBOMSourceType> 42
    let entities, components, vulnerabilities = Security.sbomRows "default" now (payloadOf entity)

    let row = Assert.Single entities
    Assert.Equal("generation timed out", row.Error)
    Assert.Equal("", row.Bom)
    Assert.Equal("FAILED", row.Status)
    // A source type this build does not know keeps its number.
    Assert.Equal("42", row.Type)
    Assert.Empty components
    Assert.Empty vulnerabilities

/// Scanners repeat a property name (trivy's PkgID, for one): every value
/// must survive, on a component and on a vulnerability.
[<Fact>]
let ``properties that repeat a name keep every value`` () =
    let property (name: string) (value: string) = Cyclonedx.V14.Property(Name = name, Value = value)
    let comp = Component(BomRef = "pkg:c1", Name = "libfoo")
    comp.Properties.AddRange [ property "trivy:PkgID" "libfoo@1.0"; property "other" "x"; property "trivy:PkgID" "libfoo@1.0-alt" ]
    comp.Properties.Add(Cyclonedx.V14.Property(Name = "no value"))
    let vulnerability = Vulnerability(Id = "CVE-2024-1234")
    vulnerability.Properties.AddRange [ property "trivy:SrcName" "libfoo"; property "trivy:SrcName" "libfoo-src" ]

    let bom = Bom(SpecVersion = "1.4")
    bom.Components.Add comp
    bom.Vulnerabilities.Add vulnerability
    let _, components, vulnerabilities = Security.sbomRows "default" now (payloadOf (SbomEntity(Id = "e1", Cyclonedx = bom)))

    let properties = (Assert.Single components).Properties
    Assert.Equal<string[]>([| "libfoo@1.0"; "libfoo@1.0-alt" |], properties["trivy:PkgID"])
    Assert.Equal<string[]>([| "" |], properties["no value"])
    Assert.Equal<string[]>([| "libfoo"; "libfoo-src" |], (Assert.Single vulnerabilities).Properties["trivy:SrcName"])

[<Fact>]
let ``what the payload and the entity say about themselves reaches the row`` () =
    let entity =
        SbomEntity(
            Id = "img@sha256:1",
            InUse = true,
            Heartbeat = true,
            Hash = "h",
            KernelVersion = "6.1",
            CpuArchitecture = "arm64",
            GeneratedAt = WellKnownTypes.Timestamp(Seconds = 1714979289L, Nanos = 123456789),
            GenerationDuration = WellKnownTypes.Duration(Seconds = 2L, Nanos = 345678901)
        )

    entity.RepoTags.AddRange [ "nginx:1"; "nginx:latest" ]
    entity.RepoDigests.Add "nginx@sha256:1"
    entity.DdTags.AddRange [ "team:a"; "team:b" ]
    let payload = payloadOf entity
    payload.Source <- "agent"
    payload.DdEnv <- ""

    let entities, _, _ = Security.sbomRows "default" now payload
    let row = Assert.Single entities

    Assert.Equal(Some "agent", row.Source)
    // Set to the empty string is not the same as never set.
    Assert.Equal(Some "", row.DdEnv)
    Assert.Equal(Some(time "2024-05-06T07:08:09.1234567Z"), row.GeneratedAt)
    Assert.Equal(Some 2345L, row.GenerationDurationMs)
    Assert.Equal<string[]>([| "nginx:1"; "nginx:latest" |], row.RepoTags)
    Assert.Equal<string[]>([| "nginx@sha256:1" |], row.RepoDigests)
    Assert.Equal<string[]>([| "a"; "b" |], row.DDTags["team"])
    Assert.Equal<uint8 list>([ 1uy; 1uy ], [ row.InUse; row.Heartbeat ])
    Assert.Equal<string list>([ "h"; "6.1"; "arm64"; "h1" ], [ row.Hash; row.KernelVersion; row.CPUArchitecture; row.Host ])
    Assert.Equal(1, row.PayloadVersion)
    // Neither arm of the oneof.
    Assert.Equal<string list>([ ""; "" ], [ row.Error; row.Bom ])

/// What Go's Duration.AsDuration().Milliseconds() gives for these.
[<Theory>]
[<InlineData(2L, 345678901, 2345L)>]
[<InlineData(-1L, 999999, -999L)>]
[<InlineData(0L, 999999, 0L)>]
[<InlineData(9223372036854775807L, 5, 9223372036854L)>]
[<InlineData(-9223372036854775808L, 0, -9223372036854L)>]
let ``a generation duration is whole milliseconds, saturating`` (seconds: int64, nanos: int, expected: int64) =
    let entity = SbomEntity(Id = "e", GenerationDuration = WellKnownTypes.Duration(Seconds = seconds, Nanos = nanos))
    let entities, _, _ = Security.sbomRows "t" now (payloadOf entity)

    Assert.Equal(Some expected, (Assert.Single entities).GenerationDurationMs)

[<Fact>]
let ``licenses are named by id, then name, then expression`` () =
    let license (make: Cyclonedx.V14.License -> unit) =
        let l = Cyclonedx.V14.License()
        make l
        Cyclonedx.V14.LicenseChoice(License = l)

    let comp = Component(Name = "libbar")

    comp.Licenses.AddRange
        [ license (fun l -> l.Id <- "MIT")
          license (fun l -> l.Name <- "Custom")
          Cyclonedx.V14.LicenseChoice(Expression = "MIT OR Apache-2.0")
          // A license with neither id nor name still takes its place.
          license (fun l -> l.Url <- "http://x")
          Cyclonedx.V14.LicenseChoice(Expression = "")
          Cyclonedx.V14.LicenseChoice() ]

    let bom = Bom()
    bom.Components.Add comp
    let _, components, _ = Security.sbomRows "t" now (payloadOf (SbomEntity(Id = "e", Cyclonedx = bom)))

    Assert.Equal<string[]>([| "MIT"; "Custom"; "MIT OR Apache-2.0"; "" |], (Assert.Single components).Licenses)

[<Fact>]
let ``optional strings that were set to nothing are empty, not absent`` () =
    let comp = Component(Name = "libbar", Purl = "", Description = "a <b> & \"c\"", Scope = Cyclonedx.V14.Scope.Optional)
    comp.Hashes.Add(Cyclonedx.V14.Hash(Alg = Cyclonedx.V14.HashAlg.Sha256, Value = "first"))
    comp.Hashes.Add(Cyclonedx.V14.Hash(Alg = Cyclonedx.V14.HashAlg.Sha256, Value = "last wins"))
    comp.Hashes.Add(Cyclonedx.V14.Hash(Alg = enum<Cyclonedx.V14.HashAlg> 77, Value = "unknown"))
    comp.ExternalReferences.Add(Cyclonedx.V14.ExternalReference(Type = Cyclonedx.V14.ExternalReferenceType.Vcs, Url = "https://x/<y>"))
    comp.ExternalReferences.Add(Cyclonedx.V14.ExternalReference(Url = "u2"))
    let bom = Bom()
    bom.Components.Add comp
    let _, components, _ = Security.sbomRows "t" now (payloadOf (SbomEntity(Id = "e", Cyclonedx = bom)))
    let row = Assert.Single components

    Assert.Equal(Some "", row.Purl)
    Assert.Equal(Some "a <b> & \"c\"", row.Description)
    Assert.Equal(None, row.Cpe)
    Assert.Equal("SCOPE_OPTIONAL", row.Scope)
    Assert.Equal("", row.BomRef)
    Assert.Equal("last wins", row.Hashes["HASH_ALG_SHA_256"])
    Assert.Equal("unknown", row.Hashes["77"])
    Assert.Equal("""[{"type":"EXTERNAL_REFERENCE_TYPE_VCS","url":"https://x/<y>"},{"url":"u2"}]""", row.ExternalReferences)

[<Fact>]
let ``a vulnerability's analysis, dates and ratings reach its row`` () =
    let analysis = Cyclonedx.V14.VulnerabilityAnalysis(State = Cyclonedx.V14.ImpactAnalysisState.NotAffected, Detail = "not reachable")
    analysis.Response.Add Cyclonedx.V14.VulnerabilityResponse.Update
    analysis.Response.Add(enum<Cyclonedx.V14.VulnerabilityResponse> 42)

    let vulnerability =
        Vulnerability(
            Id = "CVE-1",
            Detail = "",
            Analysis = analysis,
            Created = WellKnownTypes.Timestamp(Seconds = 1700000000L),
            Updated = WellKnownTypes.Timestamp(Seconds = -1L, Nanos = 500),
            Source = Cyclonedx.V14.Source(Url = "")
        )

    vulnerability.Ratings.Add(Cyclonedx.V14.VulnerabilityRating(Score = 7.5, Severity = Cyclonedx.V14.Severity.High))
    vulnerability.Ratings.Add(Cyclonedx.V14.VulnerabilityRating(Score = 9.0))
    let bare = Vulnerability(Analysis = Cyclonedx.V14.VulnerabilityAnalysis())

    let bom = Bom()
    bom.Vulnerabilities.AddRange [ vulnerability; bare ]
    let _, _, rows = Security.sbomRows "t" now (payloadOf (SbomEntity(Id = "e", Cyclonedx = bom)))

    Assert.Equal(2, rows.Length)
    let full, empty = rows[0], rows[1]
    Assert.Equal("IMPACT_ANALYSIS_STATE_NOT_AFFECTED", full.AnalysisState)
    Assert.Equal("IMPACT_ANALYSIS_JUSTIFICATION_NULL", full.AnalysisJustification)
    Assert.Equal<string[]>([| "VULNERABILITY_RESPONSE_UPDATE"; "42" |], full.AnalysisResponse)
    Assert.Equal("not reachable", full.AnalysisDetail)
    Assert.Equal(Some(time "2023-11-14T22:13:20Z"), full.Created)
    Assert.Equal(Some(time "1969-12-31T23:59:59.0000005Z"), full.Updated)
    Assert.Equal(None, full.Published)
    Assert.Equal(Some "", full.Detail)
    Assert.Equal(None, full.Description)
    Assert.Equal(None, full.SourceName)
    Assert.Equal(Some "", full.SourceURL)
    Assert.Equal("""[{"score":7.5,"severity":"SEVERITY_HIGH"},{"score":9}]""", full.Ratings)

    // An analysis that says nothing still names its zero values.
    Assert.Equal("IMPACT_ANALYSIS_STATE_NULL", empty.AnalysisState)
    Assert.Equal("", empty.ID)
    Assert.Empty empty.AnalysisResponse

[<Fact>]
let ``an SBOM that is not protobuf is kept raw`` () =
    let response, sink = post "routeSBOM" "/api/v2/sbom" "application/x-protobuf" (utf8 "definitely not protobuf")

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("sbom", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.NotEqual<string>("", raw.Note)
    Assert.Equal(1, sink.Writes.Length)

/// Protobuf reads almost any bytes as some message: a payload without
/// entities is taken for another type's.
[<Fact>]
let ``an SBOM without entities is kept raw`` () =
    let body = SbomPayload(Version = 1, Host = "h3").ToByteArray()
    let _, sink = post "routeSBOM" "/api/v2/sbom" "application/x-protobuf" body

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("unexpected_shape", raw.Reason)
    Assert.Equal("decoded to zero entities", raw.Note)
    Assert.Equal<byte[]>(body, raw.Body)

[<Fact>]
let ``an entity without components writes only the entity`` () =
    let body = (payloadOf (SbomEntity(Id = "e"))).ToByteArray()
    let _, sink = post "routeSBOM" "/api/v2/sbom" "application/x-protobuf" body

    Assert.Equal<string list>([ "storage_sbom_entities" ], sink.Writes |> List.map _.Writer)

/// Bodies (hex) and the layout the Go intake noted for them: these were
/// produced by running its wire walk on the same bytes.
[<Theory>]
[<InlineData("", "")>]
[<InlineData("0801", "1:varint")>]
[<InlineData("08011002", "1:varint 2:varint")>]
[<InlineData("0801080208031205616263646512001a00", "1:varintx3 2:bytesx2 3:bytes")>]
[<InlineData("0d01020304", "1:fixed32")>]
[<InlineData("090102030405060708", "1:fixed64")>]
[<InlineData("0d010203", "malformed")>]
[<InlineData("09010203", "malformed")>]
[<InlineData("1203616263", "2:bytes")>]
[<InlineData("12056162", "malformed")>]
[<InlineData("0b0801100c0c", "1:type3")>]
[<InlineData("0b08010c0b08020c", "1:type3x2")>]
[<InlineData("0b0801", "malformed")>]
[<InlineData("0b08011c", "malformed")>]
[<InlineData("0b13081c140c", "1:type3")>]
[<InlineData("0c", "malformed")>]
[<InlineData("0e00", "malformed")>]
[<InlineData("0f00", "malformed")>]
[<InlineData("00", "malformed")>]
[<InlineData("0001", "malformed")>]
[<InlineData("08", "malformed")>]
[<InlineData("0880", "malformed")>]
[<InlineData("08ffffffffffffffffff01", "1:varint")>]
[<InlineData("08ffffffffffffffffff02", "malformed")>]
[<InlineData("08ffffffffffffffffffff01", "malformed")>]
[<InlineData("f8ffffff0701", "268435455:varint")>]
[<InlineData("f8ffffff0f01", "536870911:varint")>]
[<InlineData("8000", "malformed")>]
[<InlineData("8080808080808080800100", "malformed")>]
[<InlineData("3a0208013a0208024a01614a0162", "7:bytesx2 9:bytesx2")>]
[<InlineData("1a0208011a020802220022000801", "3:bytesx2 4:bytesx2 1:varint")>]
let ``the wire layout lists top-level fields in order, runs folded`` (body: string, expected: string) =
    let layout = Security.wireLayout (Convert.FromHexString body)
    Assert.Equal(expected, defaultArg layout "malformed")

/// Go's wire walk follows groups 10 001 deep and no further. Here the open
/// groups are counted, not recursed into, so depth costs no stack.
[<Theory>]
[<InlineData(10001, "1:type3")>]
[<InlineData(10002, "malformed")>]
[<InlineData(200000, "malformed")>]
let ``nested groups are followed as deep as Go follows them`` (depth: int, expected: string) =
    let body = Array.append (Array.create depth 0x0Buy) (Array.create depth 0x0Cuy)
    Assert.Equal(expected, defaultArg (Security.wireLayout body) "malformed")

[<Theory>]
[<InlineData("0801080212036162631a00", "1:varintx2 2:bytes 3:bytes")>]
[<InlineData("08", "protobuf did not parse as a sequence of top-level fields")>]
[<InlineData("706c61696e2074657874", "protobuf did not parse as a sequence of top-level fields")>]
let ``scanner results are always kept raw, the note saying what was there`` (body: string, note: string) =
    let bytes = Convert.FromHexString body
    let response, sink = post "routeSDS" "/api/v2/sdsresult" "application/x-protobuf" bytes

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("sds", raw.Intake)
    Assert.Equal("no_schema", raw.Reason)
    Assert.Equal(note, raw.Note)
    Assert.Equal<byte[]>(bytes, raw.Body)
    Assert.Equal(1, sink.Writes.Length)

[<Fact>]
let ``an empty scanner result is a probe and is not stored`` () =
    let _, sink = post "routeSDS" "/api/v2/sdsresult" "application/x-protobuf" [||]
    Assert.Empty sink.Writes
