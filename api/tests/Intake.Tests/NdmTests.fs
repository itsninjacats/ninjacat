/// Tests of Routers/Ndm.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.NdmTests

open System.Text
open System.Text.Json
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private json (text: string) : JsonElement =
    use doc = JsonDocument.Parse text
    doc.RootElement.Clone()

let private ok (result: Result<'a, string>) : 'a =
    match result with
    | Ok value -> value
    | Error e -> failwith $"expected rows, got: {e}"

let private flows (body: string) : NetflowFlowRow[] =
    ok (Ndm.decodeFlows "test" (Encoding.UTF8.GetBytes body))

let private path (body: string) : NetworkPathRow =
    ok (Ndm.decodePaths "test" (Encoding.UTF8.GetBytes body)) |> Array.exactlyOne

/// Posts a body through the engine with the test key; returns the answer and
/// what was written.
let private post (path: string) (body: string) : Response * CapturingSink =
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
    Routes.byGoNames deps [ "routeNDM" ] http (Encoding.UTF8.GetBytes body), sink

[<Fact>]
let ``additional fields spread over the root come back, 64-bit digits intact`` () =
    // What the agent's FlowPayload.MarshalJSON writes: no "additional_fields"
    // key, the extra fields next to the struct's own.
    let rows =
        flows
            """[{"bgp_next_hop":"10.0.0.254","bytes":9223372036854775808,"custom_obj":{"a":"b"},
                 "destination":{"ip":"","port":"","mac":"","mask":""},"device":{"namespace":"default"},
                 "direction":"","dscp":0,"dscp_name":"","egress":{"interface":{"index":0}},"end":0,
                 "ether_type":"IPv4","exporter":{"ip":"10.0.0.1"},"flush_timestamp":1758326400123,"host":"",
                 "ingress":{"interface":{"index":0}},"ip_protocol":"TCP","next_hop":{"ip":""},
                 "packets":9007199254740993,"sampling_rate":0,"source":{"ip":"","port":"","mac":"","mask":""},
                 "start":0,"tcp_flags":["SYN","ACK"],"tos":0,"type":"netflow9","vlan_id":9007199254740993},
                {"type":"sflow5","ip_protocol":"UDP"}]"""

    Assert.Equal(2, rows.Length)
    let f = rows[0]
    // Beyond what a float holds exactly.
    Assert.Equal(9223372036854775808UL, f.Bytes)
    Assert.Equal(9007199254740993UL, f.Packets)
    Assert.Equal(Time.fromUnixMillis 1758326400123L, f.FlushTimestamp)
    Assert.Equal("IPv4", f.EtherType)
    Assert.Equal<string[]>([| "SYN"; "ACK" |], f.TCPFlags)

    Assert.Equal<Map<string, string>>(
        Map [ "bgp_next_hop", "\"10.0.0.254\""; "custom_obj", """{"a":"b"}"""; "vlan_id", "9007199254740993" ],
        f.AdditionalFields
    )

    Assert.True rows[1].AdditionalFields.IsEmpty

[<Fact>]
let ``the struct's keys are known, additional_fields is not one of them`` () =
    for key in
        [ "flush_timestamp"; "type"; "bytes"; "packets"; "source"; "destination"; "ingress"; "egress"; "next_hop"
          "ether_type"; "tcp_flags"; "host"; "device"; "exporter" ] do
        Assert.Contains(key, Ndm.flowKeys)

    Assert.DoesNotContain("additional_fields", Ndm.flowKeys)

[<Fact>]
let ``a spelled-out additional_fields object is taken too, next to root extras`` () =
    let rows = flows """{"type":"netflow9","additional_fields":{"n":9007199254740993},"extra":true}"""
    Assert.Equal<Map<string, string>>(Map [ "extra", "true"; "n", "9007199254740993" ], rows[0].AdditionalFields)

[<Fact>]
let ``an additional field's value is JSON text: a string keeps its quotes, an object is compact with sorted keys`` () =
    let rows = flows """[{"note":"hello","nested":{ "b" : 1.50, "a" : [1E3, null] }}]"""

    Assert.Equal<Map<string, string>>(
        Map [ "nested", """{"a":[1E3,null],"b":1.50}"""; "note", "\"hello\"" ],
        rows[0].AdditionalFields
    )

[<Fact>]
let ``a key in another case fills the field and is an additional field as well`` () =
    // Go matches struct fields ignoring case, but tells known keys from extra ones exactly.
    let row = flows """[{"TYPE":"upper","Source":{"IP":"1.1.1.1"}}]""" |> Array.exactlyOne
    Assert.Equal("upper", row.FlowType)
    Assert.Equal("1.1.1.1", row.SourceIP)

    Assert.Equal<Map<string, string>>(
        Map [ "Source", """{"IP":"1.1.1.1"}"""; "TYPE", "\"upper\"" ],
        row.AdditionalFields
    )

[<Fact>]
let ``bare objects separated by commas are a batch, because the body is wrapped in brackets`` () =
    Assert.Equal(2, (flows """{"type":"a"},{"type":"b"}""").Length)

[<Fact>]
let ``half a surrogate pair in a flow reads as U+FFFD, in fields and in additional fields`` () =
    let row = flows """[{"x":"\ud800","type":"t\ud800","nested":{"k":"\udc00v"}}]""" |> Array.exactlyOne
    Assert.Equal("t\uFFFD", row.FlowType)

    Assert.Equal<Map<string, string>>(
        Map [ "nested", "{\"k\":\"\uFFFDv\"}"; "x", "\"\uFFFD\"" ],
        row.AdditionalFields
    )

[<Fact>]
let ``a null flow is a flow of zero values, as it is for Go's decoder`` () =
    let row = flows "[null]" |> Array.exactlyOne
    Assert.Equal("", row.FlowType)
    Assert.Equal(0UL, row.Bytes)
    Assert.Equal(Time.fromUnixSeconds 0L, row.Start)

[<Fact>]
let ``one value of the wrong type refuses the whole batch of flows, and the body is kept raw`` () =
    let body = """[{"type":"netflow9","bytes":1},{"type":"netflow9","bytes":"many"}]"""
    Assert.True((Ndm.decodeFlows "test" (Encoding.UTF8.GetBytes body)).IsError)

    let response, sink = post "/api/v2/ndmflow" body
    Assert.Equal(202, response.Status)
    Assert.Empty(sink.Rows<NetflowFlowRow>())
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("ndmflow", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal(body, Encoding.UTF8.GetString raw.Body)

[<Theory>]
[<InlineData("1", "reachable")>]
[<InlineData("2", "unreachable")>]
[<InlineData("", "unknown")>]
[<InlineData("99", "unknown")>]
[<InlineData("not-a-number", "unknown")>]
let ``a device status is reachable, unreachable, or unknown rather than a guess`` (number: string, name: string) =
    Assert.Equal(name, Ndm.deviceStatusName number)

[<Fact>]
let ``an interface flag is true, false or not reported`` () =
    let rows =
        ok (
            Ndm.metadataRows
                "test"
                (json """{"interfaces":[{"is_physical":true,"meraki_enabled":false},{"device_id":"d"}]}""")
        )

    Assert.Equal(Some 1uy, rows.Interfaces[0].IsPhysical)
    Assert.Equal(Some 0uy, rows.Interfaces[0].MerakiEnabled)
    Assert.Equal(None, rows.Interfaces[1].IsPhysical)
    Assert.Equal(None, rows.Interfaces[1].MerakiEnabled)

[<Fact>]
let ``an empty ddtags gives no tags, not one empty tag`` () =
    Assert.Equal<string list>([], Ndm.splitTags "")
    Assert.Equal<string list>([ "env:prod" ], Ndm.splitTags "env:prod")
    Assert.Equal<string list>([ "env:prod"; "namespace:default" ], Ndm.splitTags "env:prod,namespace:default")

[<Fact>]
let ``root keys nothing declares are kept as JSON text`` () =
    let extra =
        Ndm.extraTopLevel
            (set [ "namespace"; "devices" ])
            (json """{"namespace":"default","devices":[],"future_field":{"nested":true},"count":3}""")

    Assert.Equal<Map<string, string>>(Map [ "count", "3"; "future_field", """{"nested":true}""" ], extra)

[<Fact>]
let ``the other lists are kept whole, with the ids the element carries`` () =
    let rows =
        ok (
            Ndm.metadataRows
                "test"
                (json
                    """{"namespace":"default","collect_timestamp":1700000000,"x":1,
                        "links":[{"id":"link1","source_type":"lldp"}],
                        "vpn_tunnels":[{"device_id":"dev1","interface_id":"dev1:1","status":"up"}]}""")
        )

    // No scan_status was sent, so there is no row for one.
    Assert.Equal<string list>([ "link"; "vpn_tunnel" ], rows.Objects |> List.map _.Kind)

    let link = rows.Objects[0]
    Assert.Equal("", link.DeviceID)
    Assert.Equal("""{"id":"link1","source_type":"lldp"}""", link.Object)

    let tunnel = rows.Objects[1]
    Assert.Equal("dev1", tunnel.DeviceID)
    Assert.Equal("dev1:1", tunnel.InterfaceID)
    Assert.Equal("default", tunnel.Namespace)
    Assert.Equal(Time.fromUnixSeconds 1700000000L, tunnel.CollectTimestamp)
    Assert.Equal<Map<string, string>>(Map [ "x", "1" ], tunnel.Extra)

[<Fact>]
let ``scan_status is one object and one row; a malformed list element keeps its text`` () =
    let rows =
        ok (
            Ndm.metadataRows
                "test"
                (json """{"scan_status":{"device_id":"dev1","scan_status":"completed"},"diagnoses":["not-an-object"]}""")
        )

    Assert.Equal<string list>([ "diagnosis"; "scan_status" ], rows.Objects |> List.map _.Kind)
    Assert.Equal("\"not-an-object\"", rows.Objects[0].Object)
    Assert.Equal("", rows.Objects[0].DeviceID)
    Assert.Equal("dev1", rows.Objects[1].DeviceID)

[<Fact>]
let ``an entry with a value of the wrong type is kept raw, and the entries beside it are stored`` () =
    let good = """{"namespace":"default","devices":[{"id":"default:10.0.0.1","status":1}]}"""
    let bad = """{"namespace":"default","devices":[{"id":17}]}"""
    let response, sink = post "/api/v2/ndm" $"[{good},{bad}]"

    Assert.Equal(202, response.Status)
    let device = Assert.Single(sink.Rows<NDMDeviceRow>())
    Assert.Equal("default:10.0.0.1", device.DeviceID)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("ndm", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.StartsWith("entry 1: ", raw.Note)
    Assert.Equal(bad, Encoding.UTF8.GetString raw.Body)

[<Fact>]
let ``a config payload repeats its own fields on every config`` () =
    let rows =
        ok (
            Ndm.configRows
                "test"
                (json
                    """{"namespace":"default","collect_timestamp":1700000000,"agent_hostname":"agent01",
                        "configs":[{"device_id":"a","timestamp":1700000100},{"device_id":"b","tags":["env:prod"]}]}""")
        )

    Assert.Equal<string list>([ "a"; "b" ], rows |> List.map _.DeviceID)
    Assert.All(rows, (fun row -> Assert.Equal("agent01", row.AgentHostname)))
    Assert.Equal("", rows[0].Inventories)
    Assert.Equal(Time.fromUnixSeconds 1700000100L, rows[0].Timestamp)
    Assert.Equal(Time.fromUnixSeconds 0L, rows[1].Timestamp)

[<Fact>]
let ``a v1 trap carries its enterprise OID and both trap numbers, zero included`` () =
    let row =
        ok (
            Ndm.trapRow
                "test"
                (json
                    """{"ddsource":"snmp-traps","ddtags":"namespace:default,snmp_device:10.0.0.5",
                        "timestamp":1700000200,"uptime":123456,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.3",
                        "enterpriseOID":"1.3.6.1.4.1.9","genericTrap":2,"specificTrap":0,
                        "variables":[{"oid":"1.3.6.1.2.1.2.2.1.1","type":"Integer","value":1}],
                        "customEnrichedKey":"resolved-value"}""")
        )

    Assert.Equal("10.0.0.5", row.Device)
    Assert.Equal("1.3.6.1.4.1.9", row.EnterpriseOID)
    Assert.Equal(Some 2, row.GenericTrap)
    Assert.Equal(Some 0, row.SpecificTrap)
    Assert.Equal(123456u, row.Uptime)
    Assert.Equal<string[]>([| "1.3.6.1.2.1.2.2.1.1" |], row.VarOIDs)
    Assert.Equal<string[]>([| "Integer" |], row.VarTypes)
    Assert.Equal<string[]>([| "1" |], row.VarValues)
    Assert.Equal<Map<string, string>>(Map [ "customEnrichedKey", "\"resolved-value\"" ], row.Enriched)

[<Fact>]
let ``a trap's timestamp is milliseconds`` () =
    let row =
        ok (
            Ndm.trapRow
                "test"
                (json
                    """{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.7","timestamp":1758326400123,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.4"}""")
        )

    Assert.Equal(Time.fromUnixMillis 1758326400123L, row.Timestamp)

[<Fact>]
let ``a v2 trap has no v1 fields, not zeros`` () =
    let row =
        ok (
            Ndm.trapRow
                "test"
                (json
                    """{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.6","timestamp":1700000300,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.4"}""")
        )

    Assert.Equal(None, row.GenericTrap)
    Assert.Equal(None, row.SpecificTrap)
    Assert.Equal("", row.EnterpriseOID)
    Assert.True(row.DDTags.ContainsKey "snmp_device")

[<Fact>]
let ``an entry without a trap is kept as an unexpected shape, a broken trap as a decode error`` () =
    let _, sink =
        post "/api/v2/ndmtraps" """[{"other":1},{"trap":{"timestamp":"soon"}},{"trap":{"snmpTrapOID":"1.3"}},7]"""

    let trap = Assert.Single(sink.Rows<SNMPTrapRow>())
    Assert.Equal("1.3", trap.SNMPTrapOID)

    let raw = sink.Rows<RawPayloadRow>()
    Assert.Equal<string list>([ "unexpected_shape"; "decode_error"; "decode_error" ], raw |> List.map _.Reason)
    Assert.Equal("entry 0: no trap object", raw[0].Note)
    Assert.StartsWith("entry 1: trap: ", raw[1].Note)
    Assert.Equal("""{"trap":{"timestamp":"soon"}}""", Encoding.UTF8.GetString raw[1].Body)
    Assert.StartsWith("entry 3: ", raw[2].Note)

[<Theory>]
[<InlineData("", "")>]
[<InlineData("10.0.0.1", "10.0.0.1")>]
[<InlineData("::ffff:1.2.3.4", "1.2.3.4")>]
[<InlineData("2001:DB8:0:0:0:0:0:1", "2001:db8::1")>]
[<InlineData("::1.2.3.4", "::102:304")>]
[<InlineData("1:0:0:2:0:0:0:3", "1:0:0:2::3")>]
[<InlineData("1:0:0:2:3:0:0:4", "1::2:3:0:0:4")>]
[<InlineData("0:0:0:0:0:0:0:0", "::")>]
[<InlineData("1:2:3:4:5:6:7:0", "1:2:3:4:5:6:7:0")>]
[<InlineData("1:2:3:0:5:6:7:8", "1:2:3:0:5:6:7:8")>]
let ``an address is printed as Go prints it`` (sent: string, stored: string) =
    Assert.Equal(Some stored, Ndm.ipText sent)

[<Theory>]
[<InlineData("bad")>]
[<InlineData("1.2.3.04")>]
[<InlineData(" 1.2.3.4")>]
[<InlineData("1.2.3")>]
[<InlineData("1.2.3.256")>]
[<InlineData("fe80::1%eth0")>]
[<InlineData("[::1]")>]
let ``what Go does not take as an address is refused`` (sent: string) = Assert.Equal(None, Ndm.ipText sent)

[<Fact>]
let ``no via gives NULL for both via columns`` () =
    let row = path """[{"timestamp":1758326400123,"source":{"hostname":"h1"}}]"""
    Assert.Equal(None, row.SourceViaSubnetAlias)
    Assert.Equal(None, row.SourceViaInterfaceHardwareAddr)
    Assert.Equal(Time.fromUnixMillis 1758326400123L, row.Timestamp)

[<Fact>]
let ``a via that resolved to nothing gives empty strings, not NULL`` () =
    let row = path """[{"source":{"hostname":"h1","via":{}}}]"""
    Assert.Equal(Some "", row.SourceViaSubnetAlias)
    Assert.Equal(Some "", row.SourceViaInterfaceHardwareAddr)

[<Fact>]
let ``a via gives its subnet alias and its interface address`` () =
    let row =
        path """[{"source":{"via":{"subnet":{"alias":"subnet-1"},"interface":{"hardware_addr":"aa:bb"}}}}]"""

    Assert.Equal(Some "subnet-1", row.SourceViaSubnetAlias)
    Assert.Equal(Some "aa:bb", row.SourceViaInterfaceHardwareAddr)

[<Fact>]
let ``runs are parallel arrays, and what belongs to a hop is an array per run`` () =
    let row =
        path
            """[{"traceroute":{"runs":[
                  {"run_id":"run1",
                   "source":{"ip_address":"10.0.0.1","port":1234},
                   "destination":{"ip_address":"10.0.0.2","port":443,"reverse_dns":["dest.example.com"]},
                   "hops":[{"ttl":1,"ip_address":"10.0.0.254","rtt":1.5,"reachable":true},
                           {"ttl":2,"ip_address":"","reachable":false}]},
                  {"run_id":"run2",
                   "source":{"ip_address":"","port":0},
                   "destination":{"ip_address":"","port":0},
                   "hops":[{"ttl":1,"ip_address":"10.0.0.253","reverse_dns":["a.example.com","b.example.com"],"reachable":true}]}]}}]"""

    Assert.Equal<string[]>([| "run1"; "run2" |], row.RunIDs)
    Assert.Equal<string[]>([| "10.0.0.1"; "" |], row.RunSourceIPs)
    Assert.Equal<uint16[]>([| 1234us; 0us |], row.RunSourcePorts)
    Assert.Equal<string[]>([| "10.0.0.2"; "" |], row.RunDestinationIPs)
    Assert.Equal<uint16[]>([| 443us; 0us |], row.RunDestinationPorts)
    Assert.Equal<string[][]>([| [| "dest.example.com" |]; [||] |], row.RunDestinationReverseDNS)
    Assert.Equal<int32[][]>([| [| 1; 2 |]; [| 1 |] |], row.HopTTLs)
    // A hop that did not answer has no address and no time.
    Assert.Equal<string[][]>([| [| "10.0.0.254"; "" |]; [| "10.0.0.253" |] |], row.HopIPs)
    Assert.Equal<float[][]>([| [| 1.5; 0.0 |]; [| 0.0 |] |], row.HopRTTs)
    Assert.Equal<uint8[][]>([| [| 1uy; 0uy |]; [| 1uy |] |], row.HopReachable)
    Assert.Equal<string[][][]>([| [| [||]; [||] |]; [| [| "a.example.com"; "b.example.com" |] |] |], row.HopReverseDNS)

[<Fact>]
let ``an address that is not one refuses the batch of paths`` () =
    let body = """[{"traceroute":{"runs":[{"hops":[{"ip_address":"nowhere"}]}]}}]"""
    Assert.True((Ndm.decodePaths "test" (Encoding.UTF8.GetBytes body)).IsError)

    let _, sink = post "/api/v2/netpath" body
    Assert.Empty(sink.Rows<NetworkPathRow>())
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("netpath", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)

[<Fact>]
let ``a bare object is a batch of one on every route`` () =
    let _, flowSink = post "/api/v2/ndmflow" """ {"type":"netflow5"} """
    Assert.Equal("netflow5", (Assert.Single(flowSink.Rows<NetflowFlowRow>())).FlowType)

    let _, pathSink = post "/api/v2/netpath" """{"namespace":"default"}"""
    Assert.Equal("default", (Assert.Single(pathSink.Rows<NetworkPathRow>())).Namespace)

    let _, deviceSink = post "/api/v2/ndm" """{"devices":[{"id":"d1"}]}"""
    Assert.Equal("d1", (Assert.Single(deviceSink.Rows<NDMDeviceRow>())).DeviceID)
