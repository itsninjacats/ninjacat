/// Payloads recorded from real senders (Fixtures/real), not written by hand
/// and not replayed from the Go server: what the browser SDK and the agent's
/// postgres check actually put on the wire.
module NinjaCat.Api.Intake.Tests.RealPayloadTests

open System
open System.IO
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private fixture (name: string) : byte[] =
    File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "Fixtures", "real", name))

/// Posts a body to one route set with the test key; returns what was written.
let private post (routeSet: string) (path: string) (query: string) (body: byte[]) : CapturingSink =
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
    http.Request.QueryString <- QueryString query
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey
    let response = Replay.byGoNames deps [ routeSet ] http body
    Assert.Equal(202, response.Status)
    sink

let private tally (names: string list) : (string * int) list =
    names |> List.countBy id |> List.sort

[<Theory>]
[<InlineData("browser-sdk-7.15.0-chromium.ndjson", 2, "action:3,error:2,long_task:1,resource:10")>]
[<InlineData("browser-sdk-7.15.0-firefox.ndjson", 2, "action:3,error:2,resource:9")>]
let ``a batch from the browser SDK becomes views and events, with nothing left raw`` (file: string, views: int, events: string) =
    let sink = post "routeRUM" "/api/v2/rum" "?ddsource=browser&dd-evp-origin=browser&dd-evp-origin-version=7.15.0" (fixture file)

    Assert.Empty(sink.Rows<RawPayloadRow>())
    Assert.Equal(views, sink.Rows<RumViewRow>().Length)

    let stored =
        sink.Rows<RumEventRow>()
        |> List.map _.EventType
        |> tally
        |> List.map (fun (kind, count) -> $"{kind}:{count}")
        |> String.concat ","

    Assert.Equal(events, stored)

    // What every event of a session carries, read from the real thing.
    for row in sink.Rows<RumEventRow>() do
        Assert.Equal("11111111-2222-3333-4444-555555555555", row.ApplicationID)
        Assert.NotEqual<string>("", row.SessionID)
        Assert.NotEqual<string>("", row.ViewID)

[<Fact>]
let ``the browser SDK's telemetry becomes telemetry rows`` () =
    let sink = post "routeRUM" "/api/v2/rum" "?ddsource=browser" (fixture "browser-sdk-7.15.0-telemetry.ndjson")
    Assert.Empty(sink.Rows<RawPayloadRow>())

    Assert.Equal<(string * int) list>(
        [ "configuration", 1; "usage", 8 ],
        sink.Rows<RumTelemetryRow>() |> List.map _.TelemetryType |> tally
    )

/// Every recorded DBM file: `<integration and database>-<track>.json`, a JSON
/// array of events as the agent's forwarder posts them.
let dbmFixtures () : seq<obj[]> =
    let tracks = [ "databasequery"; "dbmactivity"; "dbmhealth"; "dbmmetadata"; "dbmmetrics"; "dbmcolumnstatistics" ]

    Directory.GetFiles(Path.Combine(AppContext.BaseDirectory, "Fixtures", "real"), "*.json")
    |> Seq.map Path.GetFileName
    |> Seq.sort
    |> Seq.choose (fun file ->
        tracks
        |> List.tryFind (fun track -> file.EndsWith($"-{track}.json", StringComparison.Ordinal))
        |> Option.map (fun track -> [| box file; box track |]))

[<Theory>]
[<MemberData(nameof dbmFixtures)>]
let ``what a real DBM integration sends is stored event for event, with no key left undecoded`` (file: string, track: string) =
    let body = fixture file
    let sink = post "routeDBM" $"/api/v2/{track}" "" body

    Assert.Empty(sink.Rows<RawPayloadRow>())
    let rows = sink.Rows<DBMEventRow>()

    use sent = System.Text.Json.JsonDocument.Parse body
    Assert.Equal(sent.RootElement.GetArrayLength(), rows.Length)

    for row in rows do
        Assert.Equal(track, row.Track)
        Assert.Empty row.UndecodedKeys

    // A plan is always recognised as one: its signature is read whenever the
    // check collected a plan at all.
    for sentEvent, row in Seq.zip (sent.RootElement.EnumerateArray()) rows do
        match sentEvent.TryGetProperty "db" with
        | true, db ->
            match db.TryGetProperty "plan" with
            | true, plan when plan.ValueKind = System.Text.Json.JsonValueKind.Object ->
                let signature = plan.GetProperty "signature"
                let expected = if signature.ValueKind = System.Text.Json.JsonValueKind.String then Some(signature.GetString()) else None
                Assert.Equal(expected, row.PlanSignature)
            | _ -> Assert.Equal(None, row.PlanSignature)
        | _ -> ()

[<Fact>]
let ``an oracle plan is a list of steps, and they are counted`` () =
    let sink = post "routeDBM" "/api/v2/databasequery" "" (fixture "oracle-check-agent-7.84.0-oracle-23-databasequery.json")
    let plan = sink.Rows<DBMEventRow>() |> List.find (fun row -> row.DBMType = "plan")
    Assert.Equal(Some 2u, plan.PlanDefinitionSteps)
    Assert.True plan.PlanSignature.IsSome

[<Fact>]
let ``a postgres plan is EXPLAIN output in a string, or null when none could be collected`` () =
    let sink = post "routeDBM" "/api/v2/databasequery" "" (fixture "postgres-check-23.12.0-databasequery.json")

    Assert.Equal<(string option * bool) list>(
        [ Some "fqt", false; Some "plan", false; Some "plan", true ],
        sink.Rows<DBMEventRow>()
        |> List.map (fun row -> Option.ofObj row.DBMType, row.PlanSignature.IsSome)
        |> List.sort
    )

[<Fact>]
let ``the agent's resources snapshot becomes one row per process group`` () =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString "/intake/"
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey
    let response = Replay.byGoNames deps [ "routeAPI" ] http (fixture "agent-7.84.0-intake-resources.json")
    Assert.Equal(200, response.Status)
    Assert.Empty(sink.Rows<RawPayloadRow>())

    let groups = sink.Rows<ProcessGroupRow>()
    Assert.Equal(7, groups.Length)
    Assert.All(groups, (fun g -> Assert.Equal(("dbmlab-mysql", DateTime(2026, 10, 1, 18, 17, 35, DateTimeKind.Utc)), (g.Host, g.Timestamp))))

    let supervisors = groups |> List.find (fun g -> g.Name = "s6-supervise")
    Assert.Equal<string[]>([| "root" |], supervisors.Usernames)
    Assert.Equal((8u, 1736704UL, 593920UL), (supervisors.ProcessCount, supervisors.VMS, supervisors.RSS))
    Assert.True(supervisors.MemPct > 0.0 && supervisors.MemPct < 1.0)

/// What the agent's data security check sent after scanning three tables of
/// a Postgres: one with planted e-mail addresses and a card number, one with
/// nothing to find, one that does not exist.
let private scanned (file: string) : SdsScanRow * SdsResultRow * SdsMatchRow list =
    let sink = post "routeSDS" "/api/v2/sdsresult" "" (fixture file)
    Assert.Empty(sink.Rows<RawPayloadRow>())
    Assert.Single(sink.Rows<SdsScanRow>()), Assert.Single(sink.Rows<SdsResultRow>()), sink.Rows<SdsMatchRow>()

[<Fact>]
let ``a real scan that found sensitive data says which rule matched in which column`` () =
    let scan, result, matches = scanned "agent-7.84.0-sdsresult-matches.bin"

    Assert.Equal(("postgres_table", "lab-instance.shop.public.users", "agent"), (scan.ResourceType, scan.ResourceName, scan.ScanningSource))
    Assert.Equal<string[]>([| "email"; "visa-card" |], scan.RuleIDs)
    Assert.True(scan.Timestamp.IsSome)

    Assert.Equal<SdsLocation>({ Kind = "postgres_table"; DatabaseName = "shop"; SchemaName = "public"; TableName = "users"; Path = "" }, result.Location)
    Assert.Equal<string[]>([| "email"; "name"; "note" |], result.ScannedColumnNames)
    Assert.Equal<string[]>([| "text"; "varchar"; "text" |], result.ScannedColumnTypes)
    Assert.Equal(Some 3L, result.ScannedRowCount)
    Assert.Equal(("task-lab-1", "sub-users", "SUCCESS"), (result.TaskID, result.SubTaskID, result.TaskStatus))

    Assert.Equal<(string * string * string * int64 * int64) list>(
        [ "table_match", "email", "email", 2L, 2L
          "table_match", "email", "note", 1L, 2L
          "table_match", "visa-card", "note", 1L, 1L ],
        matches |> List.map (fun m -> m.Kind, m.RuleID, m.ColumnName, m.CountMatchedRows, m.CountMatches) |> List.sort
    )

[<Fact>]
let ``a real scan that found nothing still records the table it scanned`` () =
    let _, result, matches = scanned "agent-7.84.0-sdsresult-clean.bin"
    Assert.Equal(("orders", "SUCCESS", 0u), (result.Location.TableName, result.TaskStatus, result.TableMatchCount))
    Assert.Empty matches

[<Fact>]
let ``a real scan that failed carries the scanner's reason`` () =
    let _, result, matches = scanned "agent-7.84.0-sdsresult-failed.bin"
    Assert.Equal(("no_such_table", "ERROR"), (result.Location.TableName, result.TaskStatus))
    Assert.Contains("relation \"no_such_table\" does not exist", result.FailureReason)
    Assert.Empty matches

[<Fact>]
let ``what the agent's SNMP check learns about a device becomes devices, interfaces and addresses`` () =
    let sink = post "routeNDM" "/api/v2/ndm" "" (fixture "agent-7.84.0-ndm.json")
    Assert.Empty(sink.Rows<RawPayloadRow>())

    let device = Assert.Single(sink.Rows<NDMDeviceRow>())
    Assert.Equal(("lab-switch-1", "1.3.6.1.4.1.8072.3.2.10", "snmp"), (device.Name, device.SysObjectID, device.Integration))
    Assert.Equal(2, sink.Rows<NDMInterfaceRow>().Length)
    Assert.Equal(2, sink.Rows<NDMIPAddressRow>().Length)

    // What has no table of its own is kept by kind: the exporter NetFlow
    // came from, the device scan's status and OIDs, the check's diagnosis.
    Assert.Equal<(string * int) list>(
        [ "device_oid", 4; "diagnosis", 1; "netflow_exporter", 1; "scan_status", 1 ],
        sink.Rows<NDMMetadataObjectRow>() |> List.map _.Kind |> tally
    )

[<Fact>]
let ``traps arrive with the names the agent's own trap database gave them, or as bare OIDs`` () =
    let sink = post "routeNDM" "/api/v2/ndmtraps" "" (fixture "agent-7.84.0-ndmtraps.json")
    Assert.Empty(sink.Rows<RawPayloadRow>())

    Assert.Equal<(string * string * string) list>(
        [ "1.3.6.1.4.1.99999.0.7", "", ""
          "1.3.6.1.6.3.1.1.5.1", "coldStart", "START-MIB"
          "1.3.6.1.6.3.1.1.5.3", "linkDown", "IF-MIB" ],
        sink.Rows<SNMPTrapRow>() |> List.map (fun t -> t.SNMPTrapOID, t.SNMPTrapName, t.SNMPTrapMIB) |> List.sort
    )

    // The agent resolves a known variable to its name and its enum to a word.
    let linkDown = sink.Rows<SNMPTrapRow>() |> List.find (fun t -> t.SNMPTrapName = "linkDown")
    Assert.Equal("\"down\"", linkDown.Enriched["ifOperStatus"])

    // A vendor trap it has no definition for keeps its variables by OID.
    let unknown = sink.Rows<SNMPTrapRow>() |> List.find (fun t -> t.SNMPTrapName = "")
    Assert.Equal<string[]>([| "1.3.6.1.4.1.99999.1.1"; "1.3.6.1.4.1.99999.1.2" |], unknown.VarOIDs)
    Assert.Equal<string[]>([| "\"fan tray 2 failed\""; "42" |], unknown.VarValues)
    Assert.Empty unknown.Enriched

[<Fact>]
let ``NetFlow v5 records the agent aggregated become flows`` () =
    let sink = post "routeNDM" "/api/v2/ndmflow" "" (fixture "agent-7.84.0-ndmflow.json")
    Assert.Empty(sink.Rows<RawPayloadRow>())

    let flows = sink.Rows<NetflowFlowRow>()
    Assert.Equal(4, flows.Length)
    Assert.All(flows, (fun f -> Assert.Equal("netflow5", f.FlowType)))

    let dns = flows |> List.find (fun f -> f.DestinationPort = "53")
    Assert.Equal(("10.1.0.7", "8.8.8.8", "UDP"), (dns.SourceIP, dns.DestinationIP, dns.IPProtocol))

[<Fact>]
let ``a traceroute the agent ran becomes one network path with its hops`` () =
    let sink = post "routeNDM" "/api/v2/netpath" "" (fixture "agent-7.84.0-netpath.json")
    Assert.Empty(sink.Rows<RawPayloadRow>())

    let path = Assert.Single(sink.Rows<NetworkPathRow>())
    Assert.Equal(("TCP", "app.ninjacat.lab", 8080us), (path.Protocol, path.DestinationHostname, path.DestinationPort))
    // Three traceroute runs, one hop each: the two containers share a network.
    Assert.Equal(3, path.RunIDs.Length)
    Assert.All(path.HopTTLs, (fun ttls -> Assert.Equal(1, ttls.Length)))

[<Fact>]
let ``logs from Datadog's .NET tracer, which have no message field, become log rows`` () =
    let sink = post "routeLogs" "/api/v2/logs" "" (fixture "dd-trace-dotnet-3.54.0-logs.json")

    Assert.Empty(sink.Rows<RawPayloadRow>())

    match sink.Rows<LogRow>() with
    | [ started; refused ] ->
        Assert.Equal("Now listening on: http://[::]:8080", started.Message)
        Assert.Equal("info", started.Status)
        Assert.Equal("ninjacat-api", started.Service)
        Assert.Equal("csharp", started.Source)
        Assert.Equal("9f06fd4497f5", started.Host)
        Assert.Equal(DateTime(2026, 10, 1, 20, 43, 16, DateTimeKind.Utc).AddTicks 4091692L, started.Timestamp)
        Assert.Equal("compact_string", started.TimestampSource)
        // What became a column is not repeated among the attributes.
        Assert.DoesNotContain("\"@m\"", started.Attributes)
        Assert.DoesNotContain("\"@t\"", started.Attributes)
        Assert.DoesNotContain("dd_service", started.Attributes)
        Assert.Contains("\"Category\":\"Microsoft.Hosting.Lifetime\"", started.Attributes)

        Assert.Equal("warn", refused.Status)
        Assert.Contains("\"dd_trace_id\":\"6abec5e5000000002f9a60afb73b9615\"", refused.Attributes)
    | rows -> Assert.Fail $"expected two log rows, got {rows.Length}"

[<Fact>]
let ``an empty object among the logs is not stored`` () =
    let sink = post "routeLogs" "/api/v2/logs" "" (Text.Encoding.UTF8.GetBytes """[{}, {"message":"kept"}]""")
    Assert.Empty(sink.Rows<RawPayloadRow>())
    Assert.Equal<string list>([ "kept" ], sink.Rows<LogRow>() |> List.map _.Message)
