/// Tests of Routers/Profiling.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.ProfilingTests

open System
open System.IO
open System.IO.Compression
open System.Text
open System.Text.Json
open Google.Protobuf
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Perftools.Profiles
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private utf8 (text: string) : byte[] = Encoding.UTF8.GetBytes text

let private gzip (data: byte[]) : byte[] =
    use compressed = new MemoryStream()

    do
        use gz = new GZipStream(compressed, CompressionLevel.Optimal)
        gz.Write(data, 0, data.Length)

    compressed.ToArray()

/// A small but real gzip-compressed pprof profile: one sample, one location,
/// one function, a sample type and a period, and no mapping at all — which
/// is legitimate (a Go binary with inlined symbols has none).
let private pprof () : byte[] =
    let profile = Profile(TimeNanos = 1_700_000_000_000_000_000L, DurationNanos = 10_000_000_000L, Period = 10_000_000L)
    profile.StringTable.AddRange [ ""; "cpu"; "nanoseconds"; "main.main"; "main.go" ]
    profile.SampleType.Add(ValueType(Type = 1L, Unit = 2L))
    profile.PeriodType <- ValueType(Type = 1L, Unit = 2L)
    let sample = Sample()
    sample.LocationId.Add 1UL
    sample.Value.Add 100L
    profile.Sample.Add sample
    let location = Location(Id = 1UL, Address = 0x1000UL)
    location.Line.Add(Line(FunctionId = 1UL, Line_ = 42L))
    profile.Location.Add location
    profile.Function.Add(Function(Id = 1UL, Name = 3L, SystemName = 3L, Filename = 4L))
    gzip (profile.ToByteArray())

/// A multipart/form-data body of the given parts, and its Content-Type.
let private multipart (parts: (string * byte[]) list) : string * byte[] =
    let boundary = "profiling-tests-boundary"
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
let private post (routeSet: string) (url: string) (contentType: string) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let path, query =
        match url.IndexOf '?' with
        | -1 -> url, ""
        | i -> url.Substring(0, i), url.Substring i

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.QueryString <- QueryString query
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey

    if contentType <> "" then
        http.Request.Headers["Content-Type"] <- StringValues contentType

    Replay.byGoNames deps [ routeSet ] http body, sink

let private jsonValue (text: string) : JsonElement =
    use document = JsonDocument.Parse text
    document.RootElement.Clone()

let private time (text: string) : DateTime =
    DateTime.Parse(text, Globalization.CultureInfo.InvariantCulture, Globalization.DateTimeStyles.RoundtripKind)

/// One part that is pprof, one that is not, and an event with a repeated tag
/// key and a key no column reads.
[<Fact>]
let ``a profile row keeps the event whole and describes each attachment`` () =
    let event =
        utf8
            """{
            "family": "go", "version": "1.2.3", "runtime": "go1.23", "language": "go",
            "end": "2026-09-23T10:30:00Z",
            "tags_profiler": "service:my-svc,env:prod,also:x,also:y",
            "custom_field": "from a newer agent"
        }"""

    let parts =
        Map.ofList [ "event", event; "cpu.pprof", pprof (); "notes.txt", utf8 "not a profile" ]

    let profiles =
        [ "cpu.pprof"; "notes.txt" ]
        |> List.choose (fun name -> Profiling.readPprof parts[name] |> Option.map (fun p -> name, p))
        |> Map.ofList

    let row =
        Profiling.profileRow "test-tenant" "profile" "dd-trace-go" "1.60.0" parts (Lenient.tryObject event) profiles

    Assert.Equal("test-tenant", row.TenantID)
    Assert.Equal("profile", row.Variant)
    Assert.Equal("dd-trace-go", row.DDEvpOrigin)
    Assert.Equal("1.60.0", row.DDEvpOriginVersion)
    Assert.Contains("custom_field", Encoding.UTF8.GetString row.Event)
    Assert.Equal<string list>([ "go"; "1.2.3"; "go1.23"; "go" ], [ row.Family; row.Version; row.Runtime; row.Language ])
    // start was never sent: absent, not a zero time.
    Assert.Equal("", row.StartRaw)
    Assert.Equal(None, row.StartParsed)
    Assert.Equal("2026-09-23T10:30:00Z", row.EndRaw)
    Assert.Equal(Some(time "2026-09-23T10:30:00Z"), row.EndParsed)
    Assert.Equal<string[]>([| "x"; "y" |], row.TagsProfiler["also"])

    // Attachments in name order: cpu.pprof before notes.txt.
    Assert.Equal<string[]>([| "cpu.pprof"; "notes.txt" |], row.AttachName)
    Assert.Equal<uint8[]>([| 1uy; 0uy |], row.AttachParsed)
    Assert.Equal<string[][]>([| [| "cpu" |]; [||] |], row.AttachSampleTypes)
    Assert.Equal<string[][]>([| [| "nanoseconds" |]; [||] |], row.AttachSampleUnits)
    Assert.Equal<string[]>([| "cpu/nanoseconds"; "" |], row.AttachPeriodType)
    Assert.Equal<uint32[]>([| 0u; 0u |], row.AttachMappingCount)
    Assert.Equal<uint32[]>([| 1u; 0u |], row.AttachLocationCount)
    Assert.Equal<uint32[]>([| 1u; 0u |], row.AttachFunctionCount)
    Assert.Equal<uint64[]>([| 1UL; 0UL |], row.AttachSampleCount)
    Assert.Equal<int64[]>([| 10_000_000L; 0L |], row.AttachPeriod)
    Assert.Equal<byte[]>(parts["notes.txt"], row.AttachBytes[1])
    Assert.Equal<uint64[]>([| uint64 parts["cpu.pprof"].Length; 13UL |], row.AttachSize)

[<Fact>]
let ``a profile row without an event part still lists the attachments`` () =
    let parts = Map.ofList [ "cpu.pprof", utf8 "not actually pprof either" ]
    let row = Profiling.profileRow "t" "profile-v1" "" "" parts None Map.empty

    Assert.Empty row.Event
    Assert.Equal("", row.Family)
    Assert.True row.TagsProfiler.IsEmpty
    Assert.Equal<string[]>([| "cpu.pprof" |], row.AttachName)
    Assert.Equal<uint8[]>([| 0uy |], row.AttachParsed)

/// start and end are kept as the text on the wire next to the parsed time.
[<Theory>]
[<InlineData("1758623400", "1758623400")>]
[<InlineData("1.0e3", "1.0e3")>]
[<InlineData("\"2025-09-23T10:30:00Z\"", "2025-09-23T10:30:00Z")>]
[<InlineData("true", "true")>]
[<InlineData("null", "")>]
// What Go's %v printed for these, which is what the Go intake stored.
[<InlineData("""{"b":[1,"x",null,true,{"k":1.50}],"a":"s"}""", "map[a:s b:[1 x <nil> true map[k:1.50]]]")>]
[<InlineData("[]", "[]")>]
[<InlineData("{}", "map[]")>]
let ``start is kept as the text it had on the wire`` (start: string, expected: string) =
    let event = Lenient.tryObject (utf8 $"{{\"start\": {start}}}")
    let row = Profiling.profileRow "t" "profile" "" "" Map.empty event Map.empty
    Assert.Equal(expected, row.StartRaw)

[<Theory>]
[<InlineData("\"2025-09-23T10:30:00Z\"", "2025-09-23T10:30:00.0000000Z")>]
[<InlineData("\"2025-09-23T10:30:00.5Z\"", "2025-09-23T10:30:00.5000000Z")>]
[<InlineData("\"2025-09-23T12:30:00+02:00\"", "2025-09-23T10:30:00.0000000Z")>]
// Two things Go's parser takes beyond RFC 3339.
[<InlineData("\"2025-09-23T1:30:00Z\"", "2025-09-23T01:30:00.0000000Z")>]
[<InlineData("\"2025-09-23T10:30:00,5Z\"", "2025-09-23T10:30:00.5000000Z")>]
// Digits past the seventh are cut off, not rounded up.
[<InlineData("\"2025-09-23T10:30:00.99999999Z\"", "2025-09-23T10:30:00.9999999Z")>]
[<InlineData("\"2025-09-23T10:30:00.123456789012Z\"", "2025-09-23T10:30:00.1234567Z")>]
[<InlineData("1758623400", "2025-09-23T10:30:00.0000000Z")>]
[<InlineData("1758623400000", "2025-09-23T10:30:00.0000000Z")>]
[<InlineData("1758623400000000", "2025-09-23T10:30:00.0000000Z")>]
[<InlineData("1758623400000000000", "2025-09-23T10:30:00.0000000Z")>]
[<InlineData("1758623400123456789", "2025-09-23T10:30:00.1234567Z")>]
[<InlineData("1758623400123456", "2025-09-23T10:30:00.1234560Z")>]
[<InlineData("0", "")>]
[<InlineData("-5", "")>]
[<InlineData("1758623400.5", "")>]
[<InlineData("1e9", "")>]
[<InlineData("\"not a time\"", "")>]
[<InlineData("\"1758623400\"", "")>]
[<InlineData("true", "")>]
// Seconds past the year 9999: Go has such a time, a DateTime does not.
// The first second of year 10000: kept at the limit.
[<InlineData("253402300800", "9999-12-31T23:59:59.0000000Z")>]
let ``timestamps are RFC 3339 text or an epoch whose magnitude gives the unit`` (value: string, expected: string) =
    let parsed = Profiling.parseTimestamp (Some(jsonValue value))

    let shown =
        match parsed with
        | Some t -> t.ToString "o"
        | None -> ""

    Assert.Equal(expected, shown)

    if parsed.IsSome then
        Assert.Equal(DateTimeKind.Utc, parsed.Value.Kind)

[<Fact>]
let ``an absent timestamp is no time`` () =
    Assert.Equal(None, Profiling.parseTimestamp None)

let private describe (summary: Pprof.Summary option) : string =
    match summary with
    | None -> "rejected"
    | Some s ->
        let types = String.Join(",", s.SampleTypes)
        let units = String.Join(",", s.SampleUnits)

        $"types={types} units={units} samples={s.SampleCount} time={s.TimeNanos} duration={s.DurationNanos} "
        + $"period={s.PeriodType}:{s.Period} mappings={s.MappingCount} locations={s.LocationCount} functions={s.FunctionCount}"

/// Bytes (base64) and what github.com/google/pprof/profile's ParseData made
/// of them: these verdicts were produced by running that library on the same
/// bytes.
let pprofVerdicts: obj[] seq =
    [ [| box "full"; box "CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIE"; box "types=cpu units=nanoseconds samples=1 time=1700000000000000000 duration=10000000000 period=cpu/nanoseconds:10000000 mappings=0 locations=1 functions=1" |]
      [| box "full gzip"; box "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAA=="; box "types=cpu units=nanoseconds samples=1 time=1700000000000000000 duration=10000000000 period=cpu/nanoseconds:10000000 mappings=0 locations=1 functions=1" |]
      [| box "only an empty string"; box "MgA="; box "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "empty input"; box ""; box "rejected" |]
      [| box "gzip of nothing"; box "H4sIAAAAAAAA/wMAAAAAAAAAAAA="; box "rejected" |]
      [| box "no string table"; box "YAU="; box "rejected" |]
      [| box "first string not empty"; box "MgF4"; box "rejected" |]
      [| box "two sample types one period"; box "CgQIARACCgQIAxAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nb2AH"; box "types=cpu,main.main units=nanoseconds,main.go samples=0 time=0 duration=0 period=/:7 mappings=0 locations=0 functions=0" |]
      [| box "sample with too many values"; box "CgQIARACEgcIARBkEMgBIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "sample without sample types"; box "EgIIASILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "sample at unknown location"; box "CgQIARACEgQICRBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "sample without locations"; box "CgQIARACEgIQZDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "types=cpu units=nanoseconds samples=1 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "duplicate function ids"; box "KggIARADGAMgBCoICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "function id zero"; box "KggIABADGAMgBDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "rejected" |]
      [| box "duplicate location ids"; box "IgsIARiAICIECAEQKiILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "location id zero"; box "IgsIABiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "line without function"; box "IgsIARiAICIECAAQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "line with unknown function"; box "IgsIARiAICIECAcQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "location without lines"; box "IgQIARAHMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=1 functions=0" |]
      [| box "mapping id zero"; box "GgMQgCAyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "duplicate mapping ids"; box "GgIIARoCCAEyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "one mapping"; box "GgQIASgEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "types= units= samples=0 time=0 duration=0 period=/:0 mappings=1 locations=0 functions=0" |]
      [| box "mapping file out of range"; box "GgQIASgJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "sample type out of range"; box "CgQIARAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "negative string index"; box "Cg0IARD///////////8BMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "function name out of range"; box "KgQIARBjMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "period type out of range"; box "WgQICRABMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "drop frames out of range"; box "OAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "comment out of range"; box "aAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "default sample type out of range"; box "cAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "doc url out of range"; box "eAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "label key out of range"; box "CgQIARACEgYQARoCCAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "label str out of range"; box "CgQIARACEggQARoECAEQCTIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "rejected" |]
      [| box "label unit out of range"; box "CgQIARACEgoQARoGCAEYBSAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "label unit ignored beside str"; box "CgQIARACEgoQARoGCAEQAiAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "types=cpu units=nanoseconds samples=1 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "time_nanos twice"; box "SAVIBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "rejected" |]
      [| box "time_nanos zero then set"; box "SABIBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "types= units= samples=0 time=6 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "field of the wrong wire type"; box "SgFBMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "rejected" |]
      [| box "unknown field of any type"; box "mgYBQZAGB7UGAQIDBDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "group wire type"; box "owakBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "rejected" |]
      [| box "strings that are not UTF-8"; box "CgQIARABMgAyAv/+"; box "types=�� units=�� samples=0 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "drop frames twice last wins"; box "OAk4ATIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "period type twice"; box "WgQIARACWgIIAzIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28="; box "types= units= samples=0 time=0 duration=0 period=main.main/:0 mappings=0 locations=0 functions=0" |]
      [| box "packed and single values"; box "CgQIARACCgQIARACEgQSAgECEgQQARACMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw=="; box "types=cpu,cpu units=nanoseconds,nanoseconds samples=2 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0" |]
      [| box "varint of eleven bytes"; box "YAFg/////////////wEyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "rejected" |]
      [| box "varint of ten bytes"; box "YP///////////38yADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv"; box "types= units= samples=0 time=0 duration=0 period=/:-1 mappings=0 locations=0 functions=0" |]
      [| box "length past the end"; box "MgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbzIJYQ=="; box "rejected" |]
      [| box "gzip that breaks off"; box "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCArQ=="; box "rejected" |]
      [| box "gzip then garbage"; box "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAAECAw=="; box "rejected" |]
      [| box "two gzip members"; box "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAB+LCAAAAAAAAP8AZgCZ/woECAEQAhIECAEQZCILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nb0iAgKix45/nyxdQgMivoCVaBAgBEAJggK3iBAMAEcDF7mYAAAA="; box "rejected" |]
      [| box "not a profile"; box "bm90IGEgcHJvZmlsZQ=="; box "rejected" |]
      [| box "json"; box "eyJhIjoxfQ=="; box "rejected" |]
      [| box "gzip of text"; box "H4sIAAAAAAAA/wANAPL/bm90IGEgcHJvZmlsZQMAdMNUDg0AAAA="; box "rejected" |] ]

[<Theory>]
[<MemberData(nameof pprofVerdicts)>]
let ``pprof is accepted, rejected and summarised as Go's pprof library does`` (name: string, data: string, expected: string) =
    Assert.True(name <> "")
    Assert.Equal(expected, describe (Profiling.readPprof (Convert.FromBase64String data)))

[<Fact>]
let ``the test profile is one the reader accepts`` () =
    Assert.Equal(
        "types=cpu units=nanoseconds samples=1 time=1700000000000000000 duration=10000000000 period=cpu/nanoseconds:10000000 mappings=0 locations=1 functions=1",
        describe (Profiling.readPprof (pprof ()))
    )

let private texts (entries: byte[] list option) : string list =
    match entries with
    | Some found -> found |> List.map Encoding.UTF8.GetString
    | None -> [ "(none)" ]

[<Fact>]
let ``a JSON array of logs gives each element as it was written`` () =
    let body = utf8 """ [{"service":"svc-a","ddsource":"dd_debugger"}, {"service" : "svc-b", "n": 1.50},null,7] """

    Assert.Equal<string list>(
        [ """{"service":"svc-a","ddsource":"dd_debugger"}"""; """{"service" : "svc-b", "n": 1.50}"""; "null"; "7" ],
        texts (Profiling.decodeLogs body)
    )

/// The browser SDK's debugger track sends one object per line.
[<Fact>]
let ``NDJSON logs give one entry per non-empty line`` () =
    let body = utf8 "\n{\"service\":\"svc-a\"}\r\n\n  {\"service\":\"svc-b\"} \t\n{broken\n"

    Assert.Equal<string list>(
        [ """{"service":"svc-a"}"""; """{"service":"svc-b"}"""; "{broken" ],
        texts (Profiling.decodeLogs body)
    )

[<Theory>]
[<InlineData("not json at all")>]
[<InlineData("   ")>]
[<InlineData("")>]
[<InlineData("[1, 2")>]
[<InlineData("[1] trailing")>]
[<InlineData("\"text\"")>]
let ``a body that is neither an array nor NDJSON has no log entries`` (body: string) =
    Assert.Equal(None, Profiling.decodeLogs (utf8 body))

[<Fact>]
let ``an empty array is a batch with no entries, not a failure`` () =
    Assert.Equal(Some [], Profiling.decodeLogs (utf8 "[]"))

[<Fact>]
let ``NDJSON logs are stored one row per line`` () =
    let body = utf8 "{\"service\":\"svc-a\",\"b\":1,\"a\":2,\"a\":3}\nnot json\n"
    let response, sink = post "routeDebugger" "/api/v2/debugger?ddtags=env:prod,env:dev" "text/plain" body

    Assert.Equal(202, response.Status)
    let rows = sink.Rows<DebuggerLogRow>()
    Assert.Equal(2, rows.Length)
    Assert.Equal("svc-a", rows[0].Service)
    Assert.Equal<string[]>([| "a"; "b" |], rows[0].ExtraKeys)
    Assert.Equal<string[]>([| "prod"; "dev" |], rows[0].DDTags["env"])
    // A line that is not an object keeps its bytes and has no columns.
    Assert.Equal("not json", Encoding.UTF8.GetString rows[1].Entry)
    Assert.Equal("", rows[1].Service)
    Assert.Empty rows[1].ExtraKeys
    Assert.Empty(sink.Rows<RawPayloadRow>())

/// Lines are trimmed of what Go's bytes.TrimSpace takes for white space,
/// which is Unicode's.
[<Fact>]
let ``NDJSON lines lose Unicode white space at both ends`` () =
    let body = utf8 "{\"service\":\"a\"}\u00a0\n\u2003{\"service\":\"b\"}\u0085\n\v{\"service\":\"c\"}\f\n"

    Assert.Equal<string list>(
        [ """{"service":"a"}"""; """{"service":"b"}"""; """{"service":"c"}""" ],
        texts (Profiling.decodeLogs body)
    )

/// Go's decoder puts U+FFFD where a string is not text and carries on. The
/// entry itself is kept byte for byte.
[<Fact>]
let ``text that is not valid Unicode is replaced, not refused`` () =
    let entry =
        Array.concat
            [ utf8 "{\"service\":\"a"
              [| 0xFFuy |]
              utf8 "\",\"ddsource\":\"x\\ud83d\\n\\\"\\u00e9\",\"k\\udc00\":1,\"z\\ud83d\\ude00\":2}" ]

    let body = Array.concat [ utf8 "["; entry; utf8 "]" ]
    let response, sink = post "routeDebugger" "/api/v2/debugger" "application/json" body

    Assert.Equal(202, response.Status)
    let row = Assert.Single(sink.Rows<DebuggerLogRow>())
    Assert.Equal("a\uFFFD", row.Service)
    Assert.Equal("x\uFFFD\n\"\u00e9", row.DDSource)
    Assert.Equal<string[]>([| "k\uFFFD"; "z\U0001F600" |], row.ExtraKeys)
    Assert.Equal<byte[]>(entry, row.Entry)

[<Fact>]
let ``a profile event with broken text is still read`` () =
    let event =
        Array.concat
            [ utf8 "{\"family\":\"g"
              [| 0xFFuy |]
              utf8 "o\",\"version\":\"\\ud800x\",\"start\":\""
              [| 0xFFuy |]
              utf8 "\"}" ]

    let row = Profiling.profileRow "t" "profile" "" "" (Map.ofList [ "event", event ]) (Lenient.tryObject event) Map.empty

    Assert.Equal("g\uFFFDo", row.Family)
    Assert.Equal("\uFFFDx", row.Version)
    Assert.Equal("\uFFFD", row.StartRaw)
    Assert.Equal(None, row.StartParsed)
    Assert.Equal<byte[]>(event, row.Event)

/// .NET stops at 64 unless told otherwise; the intake reads to Json.maxDepth.
[<Fact>]
let ``JSON nested far deeper than .NET's default is still read`` () =
    let deep = String('[', 300) + String(']', 300)
    let event = utf8 $"{{\"family\": \"go\", \"start\": {deep}}}"
    let row = Profiling.profileRow "t" "profile" "" "" Map.empty (Lenient.tryObject event) Map.empty

    Assert.Equal("go", row.Family)
    Assert.Equal(deep, row.StartRaw)

    let entries = Profiling.decodeLogs (utf8 $"[{{\"deep\": {deep}}}]")
    Assert.Equal(1, (Option.get entries).Length)

[<Fact>]
let ``logs that are not JSON are kept raw`` () =
    let response, sink = post "routeDebugger" "/api/v2/debugger" "text/plain" (utf8 "not json at all")

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("debugger", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal("logs variant: not a JSON array or NDJSON, content-type text/plain", raw.Note)
    Assert.Equal(1, sink.Writes.Length)

[<Fact>]
let ``a diagnostics entry that is not an object still gets its row`` () =
    let contentType, body = multipart [ "event", utf8 """[{"service":"svc","timestamp":"2025-09-23T10:30:00Z","debugger":{"diagnostics":{"probeId":"p1"}}}, 42]""" ]
    let _, sink = post "routeDebugger" "/api/v2/debugger" contentType body

    let rows = sink.Rows<DebuggerDiagnosticRow>()
    Assert.Equal(2, rows.Length)
    Assert.Equal("p1", rows[0].ProbeID)
    Assert.Equal(Some(time "2025-09-23T10:30:00Z"), rows[0].Timestamp)
    // No exception object: both are absent, not empty strings.
    Assert.Equal(None, rows[0].ExceptionType)
    Assert.Equal(None, rows[0].ExceptionMessage)
    Assert.Equal("42", Encoding.UTF8.GetString rows[1].Message)
    Assert.Equal("", rows[1].Service)
    Assert.Equal(None, rows[1].Timestamp)

[<Fact>]
let ``an exception without a message is an empty message, not an absent one`` () =
    let message = utf8 """{"debugger":{"diagnostics":{"exception":{"type":"NPE"}}}}"""
    let row = Profiling.diagnosticRow "t" DateTime.UtcNow Map.empty message (Lenient.tryObject message)

    Assert.Equal(Some "NPE", row.ExceptionType)
    Assert.Equal(Some "", row.ExceptionMessage)

[<Fact>]
let ``an empty diagnostics batch stores nothing`` () =
    let contentType, body = multipart [ "event", utf8 "[]" ]
    let response, sink = post "routeDebugger" "/api/v2/debugger" contentType body

    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``a symdb row keeps both spellings of the upload`` () =
    let file =
        gzip (
            utf8
                """{
            "service": "env-svc", "version": "9.9.9", "language": "go",
            "upload_id": "env-upload-1", "batch_num": 7, "final": true,
            "scopes": [{"scope_type": "package", "name": "main", "scopes": []}, null]
        }"""
        )

    let event =
        utf8
            """{
            "service": "evt-svc", "version": "1.0.0", "language": "go",
            "runtimeId": "rt-1", "uploadId": "evt-upload-1", "batchNum": "3",
            "final": false, "attachmentSize": 4096
        }"""

    let symdb = Profiling.decodeSymdbFile file
    let row = Profiling.symdbUploadRow "t" event file "env:prod,team:x" (Lenient.tryObject event) symdb

    Assert.Equal<byte[]>(event, row.Event)
    Assert.Equal<byte[]>(file, row.File)
    Assert.Equal<string[]>([| "prod" |], row.DDTags["env"])
    Assert.Equal<string list>([ "evt-svc"; "env-svc" ], [ row.Service; row.EnvService ])
    Assert.Equal<string list>([ "evt-upload-1"; "env-upload-1" ], [ row.UploadID; row.EnvUploadID ])
    // A string loses its quotes; anything else keeps its JSON form.
    Assert.Equal<string list>([ "3"; "7"; "true" ], [ row.BatchNum; row.EnvBatchNum; row.EnvFinal ])
    Assert.Equal("rt-1", row.RuntimeID)
    Assert.Equal(Some 0uy, row.Final)
    Assert.Equal(Some 4096UL, row.AttachmentSize)
    Assert.Equal(1uy, row.ScopesOK)
    Assert.Equal(2u, row.ScopeCount)
    Assert.True(row.InflatedSize > 0UL)

[<Theory>]
[<InlineData("""{"service": "s", "upload_id": "u"}""")>]
[<InlineData("""{"service": "s", "scopes": null}""")>]
[<InlineData("""{"service": "s", "scopes": {"not": "a list"}}""")>]
[<InlineData("""{"service": "s", "scopes": [{"name": "ok"}, "not a scope"]}""")>]
let ``missing or malformed scopes leave the envelope standing`` (envelope: string) =
    let symdb = Profiling.decodeSymdbFile (gzip (utf8 envelope))

    Assert.True symdb.Envelope.IsSome
    Assert.Equal(None, symdb.ScopeCount)
    Assert.Equal(envelope.Length, symdb.InflatedSize)

    let row = Profiling.symdbUploadRow "t" [||] [||] "" None symdb
    Assert.Equal("s", row.EnvService)
    Assert.Equal(0uy, row.ScopesOK)

[<Fact>]
let ``a file that is not gzip keeps its bytes and nothing else`` () =
    let garbage = utf8 "not gzip at all"
    let symdb = Profiling.decodeSymdbFile garbage
    let row = Profiling.symdbUploadRow "t" (utf8 """{"service":"s"}""") garbage "" None symdb

    Assert.Equal<byte[]>(garbage, row.File)
    Assert.Equal(0uy, row.ScopesOK)
    Assert.Equal(0UL, row.InflatedSize)
    Assert.Equal("", row.EnvService)
    Assert.True row.DDTags.IsEmpty

[<Fact>]
let ``gzip that does not hold a JSON object still reports its size`` () =
    let symdb = Profiling.decodeSymdbFile (gzip (utf8 "[1, 2, 3]"))

    Assert.Equal(None, symdb.Envelope)
    Assert.Equal(None, symdb.ScopeCount)
    Assert.Equal(9, symdb.InflatedSize)

/// GZipStream reads both without complaint; Go's reader refuses them.
[<Fact>]
let ``a gzip stream that breaks off, or has bytes after its end, is not read`` () =
    let whole = gzip (utf8 """{"service": "s", "scopes": []}""")
    Assert.True (Profiling.decodeSymdbFile whole).Envelope.IsSome

    for file in [ whole[.. whole.Length - 7]; Array.append whole [| 9uy; 9uy |]; [||]; [| 0x1Fuy; 0x8Buy |] ] do
        let symdb = Profiling.decodeSymdbFile file
        Assert.True symdb.Envelope.IsNone
        Assert.Equal(None, symdb.ScopeCount)
        Assert.Equal(0, symdb.InflatedSize)

/// uploadId and batchNum are identifiers: a producer that sends them as
/// bare numbers must not lose them.
[<Fact>]
let ``a numeric upload id is kept as its digits`` () =
    let event = utf8 """{"uploadId": 42, "batchNum": 12345678901234567890}"""
    let row = Profiling.symdbUploadRow "t" event [||] "" (Lenient.tryObject event) (Profiling.decodeSymdbFile [||])

    Assert.Equal("42", row.UploadID)
    Assert.Equal("12345678901234567890", row.BatchNum)
    Assert.Equal(None, row.Final)
    Assert.Equal(None, row.AttachmentSize)

[<Theory>]
[<InlineData("-1")>]
[<InlineData("1.5")>]
[<InlineData("\"4096\"")>]
let ``an attachment size that is not a whole, non-negative number is absent`` (size: string) =
    let event = utf8 $"{{\"attachmentSize\": {size}, \"final\": \"yes\"}}"
    let row = Profiling.symdbUploadRow "t" event [||] "" (Lenient.tryObject event) (Profiling.decodeSymdbFile [||])

    Assert.Equal(None, row.AttachmentSize)
    Assert.Equal(None, row.Final)

/// The six ident bytes elfInfo reads, padded to the sixteen a real ident has.
let private fakeElf (elfClass: byte) (endianness: byte) : byte[] =
    Array.append [| 0x7Fuy; 'E'B; 'L'B; 'F'B; elfClass; endianness |] (Array.zeroCreate 10)

[<Fact>]
let ``elfInfo reads the class and byte order off the ident bytes`` () =
    Assert.Equal(("ELF64", "LE"), Profiling.elfInfo (fakeElf 2uy 1uy))
    Assert.Equal(("ELF32", "BE"), Profiling.elfInfo (fakeElf 1uy 2uy))
    Assert.Equal(("ELF?", "?"), Profiling.elfInfo (fakeElf 9uy 0uy))
    Assert.Equal(("", ""), Profiling.elfInfo (utf8 "not an elf file"))
    Assert.Equal(("", ""), Profiling.elfInfo [| 0x7Fuy; 'E'B; 'L'B; 'F'B; 2uy |])

[<Fact>]
let ``a symbol upload row reads the metadata and keeps the parts it does not name`` () =
    let meta =
        utf8
            """{
            "type": "elf_symbol_file", "arch": "amd64",
            "gnu_build_id": "abc123", "go_build_id": "",
            "file_hash": "deadbeef", "symbol_source": "agent",
            "origin": "profiler", "origin_version": "7.60.0",
            "filename": "myapp"
        }"""

    let parts =
        Map.ofList
            [ "event", meta
              "elf_symbol_file", fakeElf 2uy 1uy
              "debug_extra", utf8 "a part this router does not name" ]

    let row = Profiling.symbolUploadRow "t" parts (Lenient.tryObject meta)

    Assert.Equal<string list>(
        [ "elf_symbol_file"; "amd64"; "abc123"; ""; "deadbeef"; "agent"; "profiler"; "7.60.0"; "myapp" ],
        [ row.Type; row.Arch; row.GNUBuildID; row.GoBuildID; row.FileHash; row.SymbolSource; row.Origin; row.OriginVersion; row.Filename ]
    )

    Assert.Equal(1uy, row.HasELF)
    Assert.Equal("ELF64", row.ELFClass)
    Assert.Equal("LE", row.ELFEndianness)
    Assert.Equal(16UL, row.ELFSize)
    Assert.Equal<byte[]>(parts["elf_symbol_file"], row.ELF)
    Assert.Equal<byte[]>(meta, row.Meta)
    Assert.Equal<string list>([ "debug_extra" ], List.ofSeq row.OtherParts.Keys)
    Assert.Equal("a part this router does not name", Encoding.UTF8.GetString row.OtherParts["debug_extra"])

[<Fact>]
let ``a symbol upload without an ELF part says so`` () =
    let meta = utf8 """{"type":"x"}"""
    let row = Profiling.symbolUploadRow "t" (Map.ofList [ "event", meta ]) (Lenient.tryObject meta)

    Assert.Equal("x", row.Type)
    Assert.Equal(0uy, row.HasELF)
    Assert.Equal("", row.ELFClass)
    Assert.Equal("", row.ELFEndianness)
    Assert.Equal(0UL, row.ELFSize)
    Assert.Empty row.ELF
    Assert.True row.OtherParts.IsEmpty

[<Fact>]
let ``the old profile path stores the same row under its own variant`` () =
    let contentType, body = multipart [ "event", utf8 """{"family":"go"}"""; "cpu.pprof", pprof () ]
    let response, sink = post "routeProfile" "/v1/input" contentType body

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    let row = Assert.Single(sink.Rows<ProfileRow>())
    Assert.Equal("profile-v1", row.Variant)
    Assert.Equal(Replay.testTenant, row.TenantID)
    Assert.Equal<uint8[]>([| 1uy |], row.AttachParsed)

[<Fact>]
let ``a repeated part name keeps the last part`` () =
    let contentType, body = multipart [ "notes", utf8 "first"; "event", utf8 "{}"; "notes", utf8 "second" ]
    let _, sink = post "routeProfile" "/api/v2/profile" contentType body

    let row = Assert.Single(sink.Rows<ProfileRow>())
    Assert.Equal<string[]>([| "notes" |], row.AttachName)
    Assert.Equal("second", Encoding.UTF8.GetString row.AttachBytes[0])

/// The note quotes what Go's mime.ParseMediaType said of the header.
[<Theory>]
[<InlineData("", "not multipart, content-type (no content-type): mime: no media type")>]
[<InlineData("text/plain", "not multipart, content-type text/plain: content-type is text/plain")>]
[<InlineData("TEXT/Plain ; charset=utf-8", "not multipart, content-type TEXT/Plain ; charset=utf-8: content-type is text/plain")>]
[<InlineData("multipart/form-data", "not multipart, content-type multipart/form-data: content-type is multipart without boundary")>]
[<InlineData("multipart/form-data;", "not multipart, content-type multipart/form-data;: content-type is multipart without boundary")>]
[<InlineData("multipart/form-data; boundary=", "not multipart, content-type multipart/form-data; boundary=: mime: invalid media parameter")>]
[<InlineData("multipart/form-data; garbage", "not multipart, content-type multipart/form-data; garbage: mime: invalid media parameter")>]
[<InlineData("multipart/form-data; boundary=a; boundary=b", "not multipart, content-type multipart/form-data; boundary=a; boundary=b: mime: duplicate parameter name")>]
[<InlineData("multipart/", "not multipart, content-type multipart/: mime: expected token after slash")>]
[<InlineData("a/b/c", "not multipart, content-type a/b/c: mime: unexpected content after media subtype")>]
[<InlineData("a b", "not multipart, content-type a b: mime: expected slash after first token")>]
let ``a profile that is not multipart is kept raw, with the reason`` (contentType: string, note: string) =
    let response, sink = post "routeProfile" "/api/v2/profile" contentType (utf8 "not multipart")

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("profiling", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal(note, raw.Note)
    Assert.Equal("not multipart", Encoding.UTF8.GetString raw.Body)

[<Fact>]
let ``a quoted boundary and a boundary in other letter case are understood`` () =
    let _, body = multipart [ "event", utf8 """{"family":"go"}""" ]
    let _, sink = post "routeProfile" "/api/v2/profile" "Multipart/Form-Data; Boundary=\"profiling-tests-boundary\"" body

    Assert.Equal("go", (Assert.Single(sink.Rows<ProfileRow>())).Family)

/// Half a submission is not stored as if it were a whole one.
[<Theory>]
[<InlineData("routeProfile", "/api/v2/profile", "profiling", "not multipart, content-type ")>]
[<InlineData("routeDebugger", "/api/v2/debugger", "debugger", "multipart, content-type ")>]
[<InlineData("routeSourcemap", "/api/v2/srcmap", "srcmap", "multipart, content-type ")>]
let ``a multipart body that breaks off is kept raw`` (routeSet: string, path: string, intake: string, notePrefix: string) =
    let contentType, body = multipart [ "event", utf8 """{"family":"go"}"""; "cpu.pprof", pprof () ]
    let cut = body[.. body.Length - 40]
    let response, sink = post routeSet path contentType cut

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal(intake, raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.StartsWith(notePrefix + contentType + ": ", raw.Note)
    Assert.Equal<byte[]>(cut, raw.Body)
    Assert.Equal(1, sink.Writes.Length)

[<Fact>]
let ``a symbol upload that is not multipart is kept raw`` () =
    let response, sink = post "routeSourcemap" "/api/v2/srcmap" "application/octet-stream" (utf8 "\u007fELF")

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("srcmap", raw.Intake)
    Assert.Equal("multipart, content-type application/octet-stream: content-type is application/octet-stream", raw.Note)

[<Fact>]
let ``a symbol upload whose event is not JSON still stores the file`` () =
    let contentType, body = multipart [ "event", utf8 "not json"; "elf_symbol_file", fakeElf 2uy 2uy ]
    let _, sink = post "routeSourcemap" "/api/v2/srcmap" contentType body

    let row = Assert.Single(sink.Rows<SymbolUploadRow>())
    Assert.Equal("not json", Encoding.UTF8.GetString row.Meta)
    Assert.Equal("", row.Type)
    Assert.Equal("BE", row.ELFEndianness)
