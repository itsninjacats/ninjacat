/// Network Devices — four hosts, one product, all JSON.
///
///   ndm-intake.<site>         /api/v2/ndm        devices, interfaces, addresses, topology
///   ndm-intake.<site>         /api/v2/ndmconfig  device configuration backups
///   snmp-traps-intake.<site>  /api/v2/ndmtraps   SNMP traps, each under a "trap" key
///   ndmflow-intake.<site>     /api/v2/ndmflow    NetFlow / sFlow / IPFIX
///   netpath-intake.<site>     /api/v2/netpath    traceroute results
///
///   config: none; event platform tracks.
///
/// They share a router because they are one product to the operator, even
/// though Datadog gave each its own hostname. Field names are the json tags
/// of the agent's own types: comp/netflow/payload, pkg/networkpath/payload,
/// pkg/networkdevice/metadata, pkg/networkconfigmanagement/report and
/// comp/snmptraps/formatter.
module NinjaCat.Api.Intake.Routers.Ndm

open System
open System.Globalization
open System.Net
open System.Net.Sockets
open System.Text
open System.Text.Json
open System.Text.RegularExpressions
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

let private accepted = Response.json 202 "{}"

/// One entry of a batch: its JSON text, and that text parsed.
type Entry =
    { Text: string
      Json: Result<JsonElement, string> }

/// A JSON array as its entries. A single object, which the event platform
/// never sends but a curl might, is a batch of one; it is parsed only when
/// the entry is read, so a broken one is refused as an entry.
let entries (body: byte[]) : Result<Entry list, string> =
    let trimmed = (Text.utf8 body).Trim()

    if trimmed.StartsWith '{' then
        Ok [ { Text = trimmed; Json = GoJson.parse (Encoding.UTF8.GetBytes trimmed) } ]
    else
        match GoJson.parse body with
        | Error e -> Error e
        | Ok root ->
            match root.ValueKind with
            | JsonValueKind.Array ->
                Ok(root.EnumerateArray() |> Seq.map (fun item -> { Text = item.GetRawText(); Json = Ok item }) |> List.ofSeq)
            | JsonValueKind.Null -> Ok []
            | _ -> Error $"expected a JSON array, got {GoJson.kind root}"

/// The elements of a body that must be a list of structs, a single object
/// again being a batch of one.
let private elements (body: byte[]) : Result<JsonElement list, string> =
    let trimmed = (Text.utf8 body).Trim()
    let text = if trimmed.StartsWith '{' then "[" + trimmed + "]" else trimmed

    match GoJson.parse (Encoding.UTF8.GetBytes text) with
    | Error e -> Error e
    | Ok root ->
        match root.ValueKind with
        | JsonValueKind.Array -> Ok(List.ofSeq (root.EnumerateArray()))
        | JsonValueKind.Null -> Ok []
        | _ -> Error $"expected a JSON array, got {GoJson.kind root}"

/// The root keys of an object that `known` does not list, as JSON text by
/// name: an undeclared key is kept, not dropped.
let extraTopLevel (known: Set<string>) (value: JsonElement) : Map<string, string> =
    GoJson.members value
    |> Map.filter (fun name _ -> not (known.Contains name))
    |> Map.map (fun _ undeclared -> undeclared.GetRawText())

/// An empty ddtags gives no tags, not one empty tag.
let splitTags (ddtags: string) : string list =
    if ddtags = "" then [] else List.ofArray (ddtags.Split ',')

/// The value of the first `key:value` tag in a comma-joined ddtags, or "".
let private tagValue (ddtags: string) (key: string) : string =
    ddtags.Split ','
    |> Array.tryFind (fun tag -> tag.StartsWith(key + ":", StringComparison.Ordinal))
    |> Option.map (fun tag -> tag.Substring(key.Length + 1))
    |> Option.defaultValue ""

let private wholeNumber (text: string) : int64 option =
    match Int64.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
    | true, n -> Some n
    | false, _ -> None

/// The payload's root keys: metadata.NetworkDevicesMetadata.
let private metadataKeys =
    set
        [ "subnet"; "namespace"; "integration"; "devices"; "interfaces"; "ip_addresses"; "links"; "vpn_tunnels"
          "netflow_exporters"; "diagnoses"; "device_oids"; "scan_status"; "collect_timestamp" ]

/// The wire sends 1 for reachable and 2 for unreachable. Anything else, an
/// absent field included, is "unknown" rather than a guess.
let deviceStatusName (number: string) : string =
    match wholeNumber number with
    | Some 1L -> "reachable"
    | Some 2L -> "unreachable"
    | _ -> "unknown"

type MetadataRows =
    { Devices: NDMDeviceRow list
      Interfaces: NDMInterfaceRow list
      Addresses: NDMIPAddressRow list
      Objects: NDMMetadataObjectRow list }

/// The rows of one NetworkDevicesMetadata, or what did not fit.
let metadataRows (tenant: string) (payload: JsonElement) : Result<MetadataRows, string> =
    let bad = GoJson.Mismatches()
    let p = GoJson.fields bad "" payload

    let space = p.String "namespace"
    let subnet = p.String "subnet"
    let integration = p.String "integration"
    // Whole seconds: the agent fills it from time.Unix().
    let collected = Time.fromUnixSeconds (p.Int64 "collect_timestamp")
    let extra = extraTopLevel metadataKeys payload

    let deviceRow (d: GoJson.Fields) : NDMDeviceRow =
        { TenantID = tenant
          Namespace = space
          Subnet = subnet
          Integration = integration
          CollectTimestamp = collected
          Extra = extra
          DeviceID = d.String "id"
          IDTags = Tags.toMultiMap (d.Strings "id_tags")
          Tags = Tags.toMultiMap (d.Strings "tags")
          IPAddress = d.String "ip_address"
          Status = deviceStatusName (d.Number "status")
          PingStatus = deviceStatusName (d.Number "ping_status")
          Name = d.String "name"
          Description = d.String "description"
          SysObjectID = d.String "sys_object_id"
          Location = d.String "location"
          Profile = d.String "profile"
          ProfileVersion = d.UInt64 "profile_version"
          Vendor = d.String "vendor"
          DeviceSubnet = d.String "subnet"
          SerialNumber = d.String "serial_number"
          Version = d.String "version"
          ProductName = d.String "product_name"
          Model = d.String "model"
          OSName = d.String "os_name"
          OSVersion = d.String "os_version"
          OSHostname = d.String "os_hostname"
          DeviceIntegration = d.String "integration"
          DeviceType = d.String "device_type" }

    // The interface's status numbers are stored as sent; one that is not a
    // whole number is 0.
    let statusNumber (number: string) : uint8 =
        wholeNumber number |> Option.map uint8 |> Option.defaultValue 0uy

    let interfaceRow (i: GoJson.Fields) : NDMInterfaceRow =
        { TenantID = tenant
          Namespace = space
          Subnet = subnet
          Integration = integration
          CollectTimestamp = collected
          Extra = extra
          DeviceID = i.String "device_id"
          IDTags = Tags.toMultiMap (i.Strings "id_tags")
          Index = i.Int32 "index"
          RawID = i.String "raw_id"
          RawIDType = i.String "raw_id_type"
          Name = i.String "name"
          Alias = i.String "alias"
          Description = i.String "description"
          MacAddress = i.String "mac_address"
          AdminStatus = statusNumber (i.Number "admin_status")
          OperStatus = statusNumber (i.Number "oper_status")
          Type = uint32 (i.Int32 "type")
          IsPhysical = i.OptionalBool "is_physical" |> Option.map Text.flag
          MerakiEnabled = i.OptionalBool "meraki_enabled" |> Option.map Text.flag
          MerakiStatus = i.String "meraki_status" }

    let addressRow (a: GoJson.Fields) : NDMIPAddressRow =
        { TenantID = tenant
          Namespace = space
          Subnet = subnet
          Integration = integration
          CollectTimestamp = collected
          Extra = extra
          InterfaceID = a.String "interface_id"
          IPAddress = a.String "ip_address"
          Prefixlen = a.Int32 "prefixlen" }

    // The other lists are kept whole. Only some of their shapes carry a
    // device or an interface id; the ones that do not leave it empty, and a
    // malformed element still keeps its text.
    let objectRow (kind: string) (item: JsonElement) : NDMMetadataObjectRow =
        let ids = GoJson.Fields(GoJson.Mismatches(), "", item)

        { TenantID = tenant
          Namespace = space
          Subnet = subnet
          Integration = integration
          CollectTimestamp = collected
          Extra = extra
          Kind = kind
          DeviceID = ids.String "device_id"
          InterfaceID = ids.String "interface_id"
          Object = item.GetRawText() }

    let objectsOf (kind: string) (list: string) : NDMMetadataObjectRow list = p.Items list |> List.map (objectRow kind)

    let rows =
        { Devices = p.Objects "devices" |> List.map deviceRow
          Interfaces = p.Objects "interfaces" |> List.map interfaceRow
          Addresses = p.Objects "ip_addresses" |> List.map addressRow
          Objects =
            objectsOf "link" "links"
            @ objectsOf "vpn_tunnel" "vpn_tunnels"
            @ objectsOf "netflow_exporter" "netflow_exporters"
            @ objectsOf "diagnosis" "diagnoses"
            @ objectsOf "device_oid" "device_oids"
            @ (p.Find "scan_status" |> Option.map (objectRow "scan_status") |> Option.toList) }

    if bad.Count = 0 then Ok rows else Error bad[0]

let handleMetadata (r: Request) : Response =
    match entries r.Body with
    | Error e ->
        r.Log.LogWarning("[ndm] not a JSON array: {Error}", e)
        Raw.store r "ndm" "decode_error" e r.Body
    | Ok batch ->
        let devices = ResizeArray<NDMDeviceRow>()
        let interfaces = ResizeArray<NDMInterfaceRow>()
        let addresses = ResizeArray<NDMIPAddressRow>()
        let objects = ResizeArray<NDMMetadataObjectRow>()

        batch
        |> List.iteri (fun i entry ->
            match entry.Json |> Result.bind (metadataRows r.Tenant) with
            | Error e ->
                r.Log.LogWarning("[ndm] entry {Index}: {Error}", i, e)
                Raw.store r "ndm" "decode_error" $"entry {i}: {e}" (Encoding.UTF8.GetBytes entry.Text)
            | Ok rows ->
                if r.Tenant <> "" then
                    devices.AddRange rows.Devices
                    interfaces.AddRange rows.Interfaces
                    addresses.AddRange rows.Addresses
                    objects.AddRange rows.Objects)

        Sink.write r.Sink NdmDevices.table (devices.ToArray())
        Sink.write r.Sink NdmInterfaces.table (interfaces.ToArray())
        Sink.write r.Sink NdmIpAddresses.table (addresses.ToArray())
        Sink.write r.Sink NdmMetadataObjects.table (objects.ToArray())

    accepted

/// report.NCMPayload, plus agent_hostname and inventories: both are on the
/// wire though the agent's struct does not document them.
let private configKeys =
    set [ "namespace"; "configs"; "collect_timestamp"; "agent_hostname"; "inventories" ]

/// The rows of one NCMPayload, or what did not fit.
let configRows (tenant: string) (payload: JsonElement) : Result<NDMDeviceConfigRow list, string> =
    let bad = GoJson.Mismatches()
    let p = GoJson.fields bad "" payload

    let space = p.String "namespace"
    // Seconds, like a config's own timestamp below: both are time.Unix().
    let collected = Time.fromUnixSeconds (p.Int64 "collect_timestamp")
    let agentHostname = p.String "agent_hostname"
    let inventories = p.Raw "inventories"
    let extra = extraTopLevel configKeys payload

    let rows =
        p.Objects "configs"
        |> List.map (fun config ->
            { TenantID = tenant
              Namespace = space
              CollectTimestamp = collected
              AgentHostname = agentHostname
              Inventories = inventories
              Extra = extra
              DeviceID = config.String "device_id"
              DeviceIP = config.String "device_ip"
              ConfigType = config.String "config_type"
              ConfigSource = config.String "config_source"
              Timestamp = Time.fromUnixSeconds (config.Int64 "timestamp")
              Tags = Tags.toMultiMap (config.Strings "tags")
              Content = config.String "content" }: NDMDeviceConfigRow)

    if bad.Count = 0 then Ok rows else Error bad[0]

let handleConfig (r: Request) : Response =
    match entries r.Body with
    | Error e ->
        r.Log.LogWarning("[ndmconfig] not a JSON array: {Error}", e)
        Raw.store r "ndmconfig" "decode_error" e r.Body
    | Ok batch ->
        let rows = ResizeArray<NDMDeviceConfigRow>()

        batch
        |> List.iteri (fun i entry ->
            match entry.Json |> Result.bind (configRows r.Tenant) with
            | Error e ->
                r.Log.LogWarning("[ndmconfig] entry {Index}: {Error}", i, e)
                Raw.store r "ndmconfig" "decode_error" $"entry {i}: {e}" (Encoding.UTF8.GetBytes entry.Text)
            | Ok configs ->
                if r.Tenant <> "" then
                    rows.AddRange configs)

        Sink.write r.Sink NdmDeviceConfigs.table (rows.ToArray())

    accepted

/// What the formatter writes under "trap" for every trap. Any other key is
/// an enrichment: a variable the MIB resolved to a name.
let private trapKeys =
    set
        [ "ddsource"; "ddtags"; "timestamp"; "uptime"; "snmpTrapOID"; "snmpTrapName"; "snmpTrapMIB"; "enterpriseOID"
          "genericTrap"; "specificTrap"; "variables" ]

/// The row of one trap (the object under "trap"), or what did not fit.
let trapRow (tenant: string) (trap: JsonElement) : Result<SNMPTrapRow, string> =
    let bad = GoJson.Mismatches()
    let t = GoJson.fields bad "" trap
    let ddtags = t.String "ddtags"
    let variables = t.Objects "variables"

    let row: SNMPTrapRow =
        { TenantID = tenant
          // Milliseconds, unlike the collect timestamps above: the listener
          // stamps a trap with UnixMilli().
          Timestamp = Time.fromUnixMillis (t.Int64 "timestamp")
          DDSource = t.String "ddsource"
          DDTags = Tags.toMultiMap (splitTags ddtags)
          Device = tagValue ddtags "snmp_device"
          Uptime = wholeNumber (t.Number "uptime") |> Option.map uint32 |> Option.defaultValue 0u
          SNMPTrapOID = t.String "snmpTrapOID"
          SNMPTrapName = t.String "snmpTrapName"
          SNMPTrapMIB = t.String "snmpTrapMIB"
          EnterpriseOID = t.String "enterpriseOID"
          GenericTrap = t.OptionalInt32 "genericTrap"
          SpecificTrap = t.OptionalInt32 "specificTrap"
          VarOIDs = variables |> List.map (fun v -> v.String "oid") |> Array.ofList
          VarTypes = variables |> List.map (fun v -> v.String "type") |> Array.ofList
          // A variable's value has the type of its OID, so it stays JSON.
          VarValues = variables |> List.map (fun v -> v.Raw "value") |> Array.ofList
          Enriched = extraTopLevel trapKeys trap
          Raw = trap.GetRawText() }

    if bad.Count = 0 then Ok row else Error bad[0]

let handleTraps (r: Request) : Response =
    match entries r.Body with
    | Error e ->
        r.Log.LogWarning("[ndmtraps] not a JSON array: {Error}", e)
        Raw.store r "ndmtraps" "decode_error" e r.Body
    | Ok batch ->
        let rows = ResizeArray<SNMPTrapRow>()

        batch
        |> List.iteri (fun i entry ->
            let keep (reason: string) (note: string) =
                let body = Encoding.UTF8.GetBytes entry.Text

                // The agent's start-up probe is `{}`: not worth a warning.
                if not (Raw.isProbe body) then
                    r.Log.LogWarning("[ndmtraps] {Note}", note)

                Raw.store r "ndmtraps" reason note body

            match entry.Json with
            | Error e -> keep "decode_error" $"entry {i}: {e}"
            | Ok envelope when envelope.ValueKind <> JsonValueKind.Object && envelope.ValueKind <> JsonValueKind.Null ->
                keep "decode_error" $"entry {i}: expected an object, got {GoJson.kind envelope}"
            | Ok envelope ->
                match (GoJson.members envelope).TryFind "trap" with
                | None -> keep "unexpected_shape" $"entry {i}: no trap object"
                | Some trap ->
                    match trapRow r.Tenant trap with
                    | Error e -> keep "decode_error" $"entry {i}: trap: {e}"
                    | Ok row ->
                        if r.Tenant <> "" then
                            rows.Add row)

        Sink.write r.Sink SnmpTraps.table (rows.ToArray())

    accepted

/// The root keys the agent's FlowPayload writes for its own fields. Every
/// other root key is one of the flow's additional fields.
let flowKeys =
    set
        [ "flush_timestamp"; "type"; "sampling_rate"; "direction"; "start"; "end"; "bytes"; "packets"; "ether_type"
          "ip_protocol"; "tos"; "dscp"; "dscp_name"; "device"; "exporter"; "source"; "destination"; "ingress"; "egress"
          "host"; "tcp_flags"; "next_hop" ]

/// The agent never writes an "additional_fields" key: its marshaller spreads
/// the configured extra fields over the root object, next to "bytes" and
/// "source". They are collected back here, each value as JSON text with its
/// numbers digit for digit. A hand-made request may spell the map out.
let private additionalFields (flow: JsonElement) : Map<string, string> =
    let mutable found: Map<string, string> = Map.empty

    if flow.ValueKind = JsonValueKind.Object then
        for property in flow.EnumerateObject() do
            if property.Name = "additional_fields" then
                for nested in GoJson.members property.Value do
                    found <- found.Add(nested.Key, GoJson.compact nested.Value)
            elif not (flowKeys.Contains property.Name) then
                found <- found.Add(property.Name, GoJson.compact property.Value)

    found

let private flowRow (bad: GoJson.Mismatches) (tenant: string) (index: int) (flow: JsonElement) : NetflowFlowRow =
    let f = GoJson.fields bad (string index) flow
    let source = f.Object "source"
    let destination = f.Object "destination"
    // The struct's own map: nothing is read from it here, but a value that
    // is not an object is refused all the same.
    f.Object "additional_fields" |> ignore

    { TenantID = tenant
      FlushTimestamp = Time.fromUnixMillis (f.Int64 "flush_timestamp")
      FlowType = f.String "type"
      SamplingRate = f.UInt64 "sampling_rate"
      Direction = f.String "direction"
      Start = Time.fromUnixSeconds (int64 (f.UInt64 "start"))
      End = Time.fromUnixSeconds (int64 (f.UInt64 "end"))
      Bytes = f.UInt64 "bytes"
      Packets = f.UInt64 "packets"
      EtherType = f.String "ether_type"
      IPProtocol = f.String "ip_protocol"
      TOS = f.UInt32 "tos"
      DSCP = f.UInt32 "dscp"
      DSCPName = f.String "dscp_name"
      DeviceNamespace = (f.Object "device").String "namespace"
      ExporterIP = (f.Object "exporter").String "ip"
      SourceIP = source.String "ip"
      // A port is text: a number, or `*` for an ephemeral one.
      SourcePort = source.String "port"
      SourceMac = source.String "mac"
      SourceMask = source.String "mask"
      SourceReverseDNSHostname = source.String "reverse_dns_hostname"
      DestinationIP = destination.String "ip"
      DestinationPort = destination.String "port"
      DestinationMac = destination.String "mac"
      DestinationMask = destination.String "mask"
      DestinationReverseDNSHostname = destination.String "reverse_dns_hostname"
      IngressInterfaceIndex = ((f.Object "ingress").Object "interface").UInt32 "index"
      EgressInterfaceIndex = ((f.Object "egress").Object "interface").UInt32 "index"
      Host = f.String "host"
      TCPFlags = f.Strings "tcp_flags"
      NextHopIP = (f.Object "next_hop").String "ip"
      AdditionalFields = additionalFields flow }

/// The rows of a batch of flows. One value that does not fit refuses the
/// whole batch.
let decodeFlows (tenant: string) (body: byte[]) : Result<NetflowFlowRow[], string> =
    match elements body with
    | Error e -> Error e
    | Ok flows ->
        let bad = GoJson.Mismatches()
        let rows = flows |> List.mapi (flowRow bad tenant) |> Array.ofList
        if bad.Count = 0 then Ok rows else Error bad[0]

let handleFlow (r: Request) : Response =
    match decodeFlows r.Tenant r.Body with
    | Error e ->
        r.Log.LogWarning("[ndmflow] not a list of flows: {Error}", e)
        Raw.store r "ndmflow" "decode_error" e r.Body
    | Ok rows ->
        if r.Tenant <> "" then
            Sink.write r.Sink NetflowFlows.table rows

    accepted

let private ipv4Syntax =
    Regex(@"\A(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\z", RegexOptions.Compiled)

let private ipv6Syntax = Regex(@"\A[0-9a-fA-F:.]+\z", RegexOptions.Compiled)

/// An address in its canonical text; None when it is not one.
/// "" is no address (a hop that did not answer) and stays "". Stricter than
/// IPAddress.Parse: four plain decimal parts for IPv4, no zone for IPv6.
let ipText (text: string) : string option =
    if text = "" then
        Some ""
    elif ipv4Syntax.IsMatch text then
        match IPAddress.TryParse text with
        | true, address -> Some(address.ToString())
        | false, _ -> None
    elif text.Contains ':' && ipv6Syntax.IsMatch text then
        match IPAddress.TryParse text with
        | true, address when address.AddressFamily = AddressFamily.InterNetworkV6 ->
            if address.IsIPv4MappedToIPv6 then
                Some(address.MapToIPv4().ToString())
            else
                Some(address.ToString())
        | _ -> None
    else
        None

let private ip (fields: GoJson.Fields) (name: string) : string =
    fields.Read(name, "an IP address", "", (fun value -> GoJson.stringOf value |> Option.bind ipText))

/// One traceroute run, with the parts of it that are read more than once.
type private Run =
    { Run: GoJson.Fields
      From: GoJson.Fields
      To: GoJson.Fields
      Hops: GoJson.Fields list }

let private pathRow (bad: GoJson.Mismatches) (tenant: string) (index: int) (path: JsonElement) : NetworkPathRow =
    let p = GoJson.fields bad (string index) path
    let source = p.Object "source"
    let via = source.OptionalObject "via"
    let destination = p.Object "destination"
    let traceroute = p.Object "traceroute"
    let hopCount = traceroute.Object "hop_count"
    let probe = p.Object "e2e_probe"
    let rtt = probe.Object "rtt"

    let runs =
        traceroute.Objects "runs"
        |> List.map (fun run ->
            { Run = run
              From = run.Object "source"
              To = run.Object "destination"
              Hops = run.Objects "hops" })
        |> Array.ofList

    let perRun (read: Run -> 'a) : 'a[] = runs |> Array.map read

    let perHop (read: GoJson.Fields -> 'a) : 'a[][] =
        runs |> Array.map (fun run -> run.Hops |> List.map read |> Array.ofList)

    { TenantID = tenant
      Timestamp = Time.fromUnixMillis (p.Int64 "timestamp")
      AgentVersion = p.String "agent_version"
      Namespace = p.String "namespace"
      TestConfigID = p.String "test_config_id"
      TestConfigName = p.String "test_config_name"
      TestResultID = p.String "test_result_id"
      TestRunID = p.String "test_run_id"
      Origin = p.String "origin"
      TestRunType = p.String "test_run_type"
      TestConfigSource = p.String "test_config_source"
      SourceProduct = p.String "source_product"
      CollectorType = p.String "collector_type"
      Protocol = p.String "protocol"
      SourceName = source.String "name"
      SourceDisplayName = source.String "display_name"
      SourceHostname = source.String "hostname"
      // With a `via`, an unresolved alias is "", not NULL.
      SourceViaSubnetAlias = via |> Option.map (fun v -> (v.Object "subnet").String "alias")
      SourceViaInterfaceHardwareAddr = via |> Option.map (fun v -> (v.Object "interface").String "hardware_addr")
      SourceNetworkID = source.String "network_id"
      SourceService = source.String "service"
      SourceContainerID = source.String "container_id"
      SourcePublicIP = source.String "public_ip"
      DestinationHostname = destination.String "hostname"
      DestinationPort = destination.UInt16 "port"
      DestinationService = destination.String "service"
      HopCountAvg = hopCount.Float "avg"
      HopCountMin = int32 (hopCount.Int64 "min")
      HopCountMax = int32 (hopCount.Int64 "max")
      RunIDs = perRun (fun run -> run.Run.String "run_id")
      RunSourceIPs = perRun (fun run -> ip run.From "ip_address")
      RunSourcePorts = perRun (fun run -> run.From.UInt16 "port")
      RunDestinationIPs = perRun (fun run -> ip run.To "ip_address")
      RunDestinationPorts = perRun (fun run -> run.To.UInt16 "port")
      RunDestinationReverseDNS = perRun (fun run -> run.To.Strings "reverse_dns")
      HopTTLs = perHop (fun hop -> int32 (hop.Int64 "ttl"))
      HopIPs = perHop (fun hop -> ip hop "ip_address")
      HopReverseDNS = perHop (fun hop -> hop.Strings "reverse_dns")
      HopRTTs = perHop (fun hop -> hop.Float "rtt")
      HopReachable = perHop (fun hop -> Text.flag (hop.Bool "reachable"))
      E2eRTTs = probe.Floats "rtts"
      E2ePacketsSent = int32 (probe.Int64 "packets_sent")
      E2ePacketsReceived = int32 (probe.Int64 "packets_received")
      E2ePacketLossPercentage = probe.Float32 "packet_loss_percentage"
      E2eJitter = probe.Float "jitter"
      E2eRTTAvg = rtt.Float "avg"
      E2eRTTMin = rtt.Float "min"
      E2eRTTMax = rtt.Float "max"
      Tags = Tags.toMultiMap (p.Strings "tags") }

/// The rows of a batch of network paths. One value that does not fit refuses
/// the whole batch.
let decodePaths (tenant: string) (body: byte[]) : Result<NetworkPathRow[], string> =
    match elements body with
    | Error e -> Error e
    | Ok paths ->
        let bad = GoJson.Mismatches()
        let rows = paths |> List.mapi (pathRow bad tenant) |> Array.ofList
        if bad.Count = 0 then Ok rows else Error bad[0]

let handleNetpath (r: Request) : Response =
    match decodePaths r.Tenant r.Body with
    | Error e ->
        r.Log.LogWarning("[netpath] not a list of network paths: {Error}", e)
        Raw.store r "netpath" "decode_error" e r.Body
    | Ok rows ->
        if r.Tenant <> "" then
            Sink.write r.Sink NetworkPaths.table rows

    accepted
