namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

// Network Device Monitoring: eight tables. The first four come off
// /api/v2/ndm and repeat the payload's namespace, subnet, integration and
// collect time, plus `Extra`: the payload's top-level keys nothing here
// declares, as JSON text by name.

type NDMDeviceRow =
    { TenantID: string
      Namespace: string
      Subnet: string
      Integration: string
      CollectTimestamp: DateTime
      Extra: Map<string, string>
      DeviceID: string
      IDTags: Map<string, string[]>
      Tags: Map<string, string[]>
      IPAddress: string
      /// "reachable", "unreachable" or "unknown".
      Status: string
      PingStatus: string
      Name: string
      Description: string
      SysObjectID: string
      Location: string
      Profile: string
      ProfileVersion: uint64
      Vendor: string
      /// The device's own subnet and integration, next to the payload's.
      DeviceSubnet: string
      SerialNumber: string
      Version: string
      ProductName: string
      Model: string
      OSName: string
      OSVersion: string
      OSHostname: string
      DeviceIntegration: string
      DeviceType: string }

type NDMInterfaceRow =
    { TenantID: string
      Namespace: string
      Subnet: string
      Integration: string
      CollectTimestamp: DateTime
      Extra: Map<string, string>
      DeviceID: string
      IDTags: Map<string, string[]>
      Index: int32
      RawID: string
      RawIDType: string
      Name: string
      Alias: string
      Description: string
      MacAddress: string
      /// The wire's numbers, not decoded into names.
      AdminStatus: uint8
      OperStatus: uint8
      Type: uint32
      /// None is "not reported", which is neither true nor false.
      IsPhysical: uint8 option
      MerakiEnabled: uint8 option
      MerakiStatus: string }

type NDMIPAddressRow =
    { TenantID: string
      Namespace: string
      Subnet: string
      Integration: string
      CollectTimestamp: DateTime
      Extra: Map<string, string>
      InterfaceID: string
      IPAddress: string
      Prefixlen: int32 }

/// One element of the payload's other lists (links, vpn_tunnels,
/// netflow_exporters, diagnoses, device_oids) or its scan_status, kept whole.
type NDMMetadataObjectRow =
    { TenantID: string
      Namespace: string
      Subnet: string
      Integration: string
      CollectTimestamp: DateTime
      Extra: Map<string, string>
      Kind: string
      DeviceID: string
      InterfaceID: string
      Object: string }

/// One device configuration backup from /api/v2/ndmconfig. The payload's
/// fields are repeated on every config it carried.
type NDMDeviceConfigRow =
    { TenantID: string
      Namespace: string
      CollectTimestamp: DateTime
      AgentHostname: string
      Inventories: string
      Extra: Map<string, string>
      DeviceID: string
      DeviceIP: string
      ConfigType: string
      ConfigSource: string
      Timestamp: DateTime
      Tags: Map<string, string[]>
      Content: string }

type SNMPTrapRow =
    { TenantID: string
      Timestamp: DateTime
      DDSource: string
      DDTags: Map<string, string[]>
      /// The `snmp_device` tag: the device that sent the trap.
      Device: string
      Uptime: uint32
      SNMPTrapOID: string
      SNMPTrapName: string
      SNMPTrapMIB: string
      EnterpriseOID: string
      /// SNMP v1 only: None on a v2/v3 trap, which is not "generic trap 0".
      GenericTrap: int32 option
      SpecificTrap: int32 option
      VarOIDs: string[]
      VarTypes: string[]
      VarValues: string[]
      /// The trap's keys with no column, as JSON text by name.
      Enriched: Map<string, string>
      Raw: string }

type NetflowFlowRow =
    { TenantID: string
      FlushTimestamp: DateTime
      FlowType: string
      SamplingRate: uint64
      Direction: string
      Start: DateTime
      End: DateTime
      Bytes: uint64
      Packets: uint64
      EtherType: string
      IPProtocol: string
      TOS: uint32
      DSCP: uint32
      DSCPName: string
      DeviceNamespace: string
      ExporterIP: string
      SourceIP: string
      SourcePort: string
      SourceMac: string
      SourceMask: string
      SourceReverseDNSHostname: string
      DestinationIP: string
      DestinationPort: string
      DestinationMac: string
      DestinationMask: string
      DestinationReverseDNSHostname: string
      IngressInterfaceIndex: uint32
      EgressInterfaceIndex: uint32
      Host: string
      TCPFlags: string[]
      NextHopIP: string
      /// The flow's configured extra fields, each value as JSON text.
      AdditionalFields: Map<string, string> }

/// One traceroute result. Runs are parallel arrays; what belongs to a hop is
/// an array per run.
type NetworkPathRow =
    { TenantID: string
      Timestamp: DateTime
      AgentVersion: string
      Namespace: string
      TestConfigID: string
      TestConfigName: string
      TestResultID: string
      TestRunID: string
      Origin: string
      TestRunType: string
      TestConfigSource: string
      SourceProduct: string
      CollectorType: string
      Protocol: string
      SourceName: string
      SourceDisplayName: string
      SourceHostname: string
      /// None when the payload had no `via`: the agent could not resolve a
      /// route, which is not the same as resolving one to an empty value.
      SourceViaSubnetAlias: string option
      SourceViaInterfaceHardwareAddr: string option
      SourceNetworkID: string
      SourceService: string
      SourceContainerID: string
      SourcePublicIP: string
      DestinationHostname: string
      DestinationPort: uint16
      DestinationService: string
      HopCountAvg: float
      HopCountMin: int32
      HopCountMax: int32
      RunIDs: string[]
      RunSourceIPs: string[]
      RunSourcePorts: uint16[]
      RunDestinationIPs: string[]
      RunDestinationPorts: uint16[]
      RunDestinationReverseDNS: string[][]
      HopTTLs: int32[][]
      HopIPs: string[][]
      HopReverseDNS: string[][][]
      HopRTTs: float[][]
      HopReachable: uint8[][]
      E2eRTTs: float[]
      E2ePacketsSent: int32
      E2ePacketsReceived: int32
      E2ePacketLossPercentage: float32
      E2eJitter: float
      E2eRTTAvg: float
      E2eRTTMin: float
      E2eRTTMax: float
      Tags: Map<string, string[]> }

module NdmDevices =
    let table: Table<NDMDeviceRow> =
        { Table.create
              "storage_ndm_devices"
              "ndm_devices"
              [ "tenant_id"; "namespace"; "subnet"; "integration"; "collect_timestamp"; "extra"
                "device_id"; "id_tags"; "tags"; "ip_address"; "status"; "ping_status"
                "name"; "description"; "sys_object_id"; "location"; "profile"; "profile_version"; "vendor"
                "device_subnet"; "serial_number"; "version"; "product_name"; "model"
                "os_name"; "os_version"; "os_hostname"; "device_integration"; "device_type" ]
              (fun (r: NDMDeviceRow) ->
                  [| r.TenantID; r.Namespace; r.Subnet; r.Integration; r.CollectTimestamp; r.Extra
                     r.DeviceID; r.IDTags; r.Tags; r.IPAddress; r.Status; r.PingStatus
                     r.Name; r.Description; r.SysObjectID; r.Location; r.Profile; r.ProfileVersion; r.Vendor
                     r.DeviceSubnet; r.SerialNumber; r.Version; r.ProductName; r.Model
                     r.OSName; r.OSVersion; r.OSHostname; r.DeviceIntegration; r.DeviceType |])
          with
              // A whole inventory lands at once, not bit by bit.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module NdmInterfaces =
    let table: Table<NDMInterfaceRow> =
        { Table.create
              "storage_ndm_interfaces"
              "ndm_interfaces"
              [ "tenant_id"; "namespace"; "subnet"; "integration"; "collect_timestamp"; "extra"
                "device_id"; "id_tags"; "if_index"; "raw_id"; "raw_id_type"; "name"; "alias"; "description"; "mac_address"
                "admin_status"; "oper_status"; "type"; "is_physical"; "meraki_enabled"; "meraki_status" ]
              (fun (r: NDMInterfaceRow) ->
                  [| r.TenantID; r.Namespace; r.Subnet; r.Integration; r.CollectTimestamp; r.Extra
                     r.DeviceID; r.IDTags; r.Index; r.RawID; r.RawIDType; r.Name; r.Alias; r.Description; r.MacAddress
                     r.AdminStatus; r.OperStatus; r.Type; Col.opt r.IsPhysical; Col.opt r.MerakiEnabled; r.MerakiStatus |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module NdmIpAddresses =
    let table: Table<NDMIPAddressRow> =
        { Table.create
              "storage_ndm_ip_addresses"
              "ndm_ip_addresses"
              [ "tenant_id"; "namespace"; "subnet"; "integration"; "collect_timestamp"; "extra"
                "interface_id"; "ip_address"; "prefixlen" ]
              (fun (r: NDMIPAddressRow) ->
                  [| r.TenantID; r.Namespace; r.Subnet; r.Integration; r.CollectTimestamp; r.Extra
                     r.InterfaceID; r.IPAddress; r.Prefixlen |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module NdmMetadataObjects =
    let table: Table<NDMMetadataObjectRow> =
        { Table.create
              "storage_ndm_metadata_objects"
              "ndm_metadata_objects"
              [ "tenant_id"; "namespace"; "subnet"; "integration"; "collect_timestamp"; "extra"
                "kind"; "device_id"; "interface_id"; "object" ]
              (fun (r: NDMMetadataObjectRow) ->
                  [| r.TenantID; r.Namespace; r.Subnet; r.Integration; r.CollectTimestamp; r.Extra
                     r.Kind; r.DeviceID; r.InterfaceID; r.Object |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module NdmDeviceConfigs =
    let table: Table<NDMDeviceConfigRow> =
        { Table.create
              "storage_ndm_device_configs"
              "ndm_device_configs"
              [ "tenant_id"; "namespace"; "collect_timestamp"; "agent_hostname"; "inventories"; "extra"
                "device_id"; "device_ip"; "config_type"; "config_source"; "timestamp"; "tags"; "content" ]
              (fun (r: NDMDeviceConfigRow) ->
                  [| r.TenantID; r.Namespace; r.CollectTimestamp; r.AgentHostname; r.Inventories; r.Extra
                     r.DeviceID; r.DeviceIP; r.ConfigType; r.ConfigSource; r.Timestamp; r.Tags; r.Content |])
          with
              // A row is a whole configuration file: megabytes, not bytes.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

module SnmpTraps =
    let table: Table<SNMPTrapRow> =
        { Table.create
              "storage_snmp_traps"
              "snmp_traps"
              [ "tenant_id"; "timestamp"; "ddsource"; "ddtags"; "device"
                "uptime"; "snmp_trap_oid"; "snmp_trap_name"; "snmp_trap_mib"
                "enterprise_oid"; "generic_trap"; "specific_trap"
                "var_oids"; "var_types"; "var_values"; "enriched"; "raw" ]
              (fun (r: SNMPTrapRow) ->
                  [| r.TenantID; r.Timestamp; r.DDSource; r.DDTags; r.Device
                     r.Uptime; r.SNMPTrapOID; r.SNMPTrapName; r.SNMPTrapMIB
                     r.EnterpriseOID; Col.opt r.GenericTrap; Col.opt r.SpecificTrap
                     r.VarOIDs; r.VarTypes; r.VarValues; r.Enriched; r.Raw |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0
              BufferLimit = 20_000
              MaxInFlight = 2 }

module NetflowFlows =
    let table: Table<NetflowFlowRow> =
        { Table.create
              "storage_netflow_flows"
              "netflow_flows"
              [ "tenant_id"; "flush_timestamp"; "flow_type"; "sampling_rate"; "direction"
                "flow_start"; "flow_end"; "bytes"; "packets"; "ether_type"; "ip_protocol"; "tos"; "dscp"; "dscp_name"
                "device_namespace"; "exporter_ip"
                "source_ip"; "source_port"; "source_mac"; "source_mask"; "source_reverse_dns_hostname"
                "destination_ip"; "destination_port"; "destination_mac"; "destination_mask"; "destination_reverse_dns_hostname"
                "ingress_interface_index"; "egress_interface_index"
                "host"; "tcp_flags"; "next_hop_ip"; "additional_fields" ]
              (fun (r: NetflowFlowRow) ->
                  [| r.TenantID; r.FlushTimestamp; r.FlowType; r.SamplingRate; r.Direction
                     r.Start; r.End; r.Bytes; r.Packets; r.EtherType; r.IPProtocol; r.TOS; r.DSCP; r.DSCPName
                     r.DeviceNamespace; r.ExporterIP
                     r.SourceIP; r.SourcePort; r.SourceMac; r.SourceMask; r.SourceReverseDNSHostname
                     r.DestinationIP; r.DestinationPort; r.DestinationMac; r.DestinationMask; r.DestinationReverseDNSHostname
                     r.IngressInterfaceIndex; r.EgressInterfaceIndex
                     r.Host; r.TCPFlags; r.NextHopIP; r.AdditionalFields |])
          with
              // The busiest NDM table, in the class of logs.
              MaxRows = 20000
              FlushInterval = TimeSpan.FromSeconds 1.0
              BufferLimit = 500_000
              MaxInFlight = 2 }

module NetworkPaths =
    let table: Table<NetworkPathRow> =
        { Table.create
              "storage_network_paths"
              "network_paths"
              [ "tenant_id"; "timestamp"; "agent_version"; "namespace"
                "test_config_id"; "test_config_name"; "test_result_id"; "test_run_id"
                "origin"; "test_run_type"; "test_config_source"; "source_product"; "collector_type"; "protocol"
                "source_name"; "source_display_name"; "source_hostname"
                "source_via_subnet_alias"; "source_via_interface_hardware_addr"
                "source_network_id"; "source_service"; "source_container_id"; "source_public_ip"
                "destination_hostname"; "destination_port"; "destination_service"
                "hop_count_avg"; "hop_count_min"; "hop_count_max"
                "run_ids"; "run_source_ips"; "run_source_ports"
                "run_destination_ips"; "run_destination_ports"; "run_destination_reverse_dns"
                "hop_ttls"; "hop_ips"; "hop_reverse_dns"; "hop_rtts"; "hop_reachable"
                "e2e_rtts"; "e2e_packets_sent"; "e2e_packets_received"; "e2e_packet_loss_percentage"
                "e2e_jitter"; "e2e_rtt_avg"; "e2e_rtt_min"; "e2e_rtt_max"
                "tags" ]
              (fun (r: NetworkPathRow) ->
                  [| r.TenantID; r.Timestamp; r.AgentVersion; r.Namespace
                     r.TestConfigID; r.TestConfigName; r.TestResultID; r.TestRunID
                     r.Origin; r.TestRunType; r.TestConfigSource; r.SourceProduct; r.CollectorType; r.Protocol
                     r.SourceName; r.SourceDisplayName; r.SourceHostname
                     Col.opt r.SourceViaSubnetAlias; Col.opt r.SourceViaInterfaceHardwareAddr
                     r.SourceNetworkID; r.SourceService; r.SourceContainerID; r.SourcePublicIP
                     r.DestinationHostname; r.DestinationPort; r.DestinationService
                     r.HopCountAvg; r.HopCountMin; r.HopCountMax
                     r.RunIDs; r.RunSourceIPs; r.RunSourcePorts
                     r.RunDestinationIPs; r.RunDestinationPorts; r.RunDestinationReverseDNS
                     r.HopTTLs; r.HopIPs; r.HopReverseDNS; r.HopRTTs; r.HopReachable
                     r.E2eRTTs; r.E2ePacketsSent; r.E2ePacketsReceived; r.E2ePacketLossPercentage
                     r.E2eJitter; r.E2eRTTAvg; r.E2eRTTMin; r.E2eRTTMax
                     r.Tags |])
          with
              // Scheduled tests trickle in.
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }
