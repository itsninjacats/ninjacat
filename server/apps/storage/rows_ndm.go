package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// Network Device Monitoring — ndm-intake, snmp-traps-intake, ndmflow-intake
// and netpath-intake, all decoded in intake/router_ndm.go.
//
// Eight tables, eight writers, one file: ndm_devices, ndm_interfaces,
// ndm_ip_addresses and ndm_metadata_objects all come off /api/v2/ndm and
// share the payload-level namespace/subnet/integration/collect_timestamp
// columns plus an extra Map(String,String) of undeclared top-level payload
// keys (schema/migrations/0007_ndm.sql). ndm_device_configs comes off
// /api/v2/ndmconfig, snmp_traps off /api/v2/ndmtraps, netflow_flows off
// /api/v2/ndmflow, network_paths off /api/v2/netpath.

// orEmptySlice2 guards a slice of slices: the driver rejects a nil slice
// wherever it appears, including one level down inside an Array(Array(...))
// column, so both the outer slice and every inner one must be non-nil.
// network_paths is the first table with nested array columns (the per-run,
// per-hop traceroute data), hence the new helper rather than reusing
// orEmptySlice as-is.
func orEmptySlice2[T any](s [][]T) [][]T {
	if s == nil {
		s = [][]T{}
	}
	out := make([][]T, len(s))
	for i, inner := range s {
		out[i] = orEmptySlice(inner)
	}
	return out
}

// orEmptySlice3 is the same guard one level deeper, for hop_reverse_dns:
// a hop's own reverse_dns is itself a list of names, so the column is
// Array(Array(Array(String))).
func orEmptySlice3[T any](s [][][]T) [][][]T {
	if s == nil {
		s = [][][]T{}
	}
	out := make([][][]T, len(s))
	for i, inner := range s {
		out[i] = orEmptySlice2(inner)
	}
	return out
}

// ---------------------------------------------------------------------------
// ndm_devices, ndm_interfaces, ndm_ip_addresses, ndm_metadata_objects
// ---------------------------------------------------------------------------

const (
	NDMDevicesWriter         = gen.Atom("storage_ndm_devices")
	NDMInterfacesWriter      = gen.Atom("storage_ndm_interfaces")
	NDMIPAddressesWriter     = gen.Atom("storage_ndm_ip_addresses")
	NDMMetadataObjectsWriter = gen.Atom("storage_ndm_metadata_objects")
	NDMDeviceConfigsWriter   = gen.Atom("storage_ndm_device_configs")
	SNMPTrapsWriter          = gen.Atom("storage_snmp_traps")
	NetflowFlowsWriter       = gen.Atom("storage_netflow_flows")
	NetworkPathsWriter       = gen.Atom("storage_network_paths")
)

type WriteNDMDevices struct{ Devices []NDMDeviceRow }
type WriteNDMInterfaces struct{ Interfaces []NDMInterfaceRow }
type WriteNDMIPAddresses struct{ Addresses []NDMIPAddressRow }
type WriteNDMMetadataObjects struct{ Objects []NDMMetadataObjectRow }
type WriteNDMDeviceConfigs struct{ Configs []NDMDeviceConfigRow }
type WriteSNMPTraps struct{ Traps []SNMPTrapRow }
type WriteNetflowFlows struct{ Flows []NetflowFlowRow }
type WriteNetworkPaths struct{ Paths []NetworkPathRow }

func (m WriteNDMDevices) rows() []Row         { return toRows(m.Devices) }
func (m WriteNDMInterfaces) rows() []Row      { return toRows(m.Interfaces) }
func (m WriteNDMIPAddresses) rows() []Row     { return toRows(m.Addresses) }
func (m WriteNDMMetadataObjects) rows() []Row { return toRows(m.Objects) }
func (m WriteNDMDeviceConfigs) rows() []Row   { return toRows(m.Configs) }
func (m WriteSNMPTraps) rows() []Row          { return toRows(m.Traps) }
func (m WriteNetflowFlows) rows() []Row       { return toRows(m.Flows) }
func (m WriteNetworkPaths) rows() []Row       { return toRows(m.Paths) }

// NDMDeviceRow is one device in ninjacat.ndm_devices — every DeviceMetadata
// field, plus the collection pass's own namespace/subnet/integration and
// whatever top-level payload keys the local mirror struct does not declare.
type NDMDeviceRow struct {
	TenantID         string
	Namespace        string
	Subnet           string
	Integration      string
	CollectTimestamp time.Time
	Extra            map[string]string

	DeviceID   string
	IDTags     map[string][]string
	Tags       map[string][]string
	IPAddress  string
	Status     string // Enum8 name: "reachable" | "unreachable" | "unknown"
	PingStatus string

	Name           string
	Description    string
	SysObjectID    string
	Location       string
	Profile        string
	ProfileVersion uint64
	Vendor         string

	// DeviceSubnet/DeviceIntegration are DeviceMetadata's OWN subnet/
	// integration fields, kept alongside the payload-level ones above —
	// see the migration's comment on why both survive.
	DeviceSubnet      string
	SerialNumber      string
	Version           string
	ProductName       string
	Model             string
	OSName            string
	OSVersion         string
	OSHostname        string
	DeviceIntegration string
	DeviceType        string
}

func (r NDMDeviceRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Namespace, r.Subnet, r.Integration, r.CollectTimestamp, orEmpty(r.Extra),
		r.DeviceID, orEmpty(r.IDTags), orEmpty(r.Tags), r.IPAddress, r.Status, r.PingStatus,
		r.Name, r.Description, r.SysObjectID, r.Location, r.Profile, r.ProfileVersion, r.Vendor,
		r.DeviceSubnet, r.SerialNumber, r.Version, r.ProductName, r.Model,
		r.OSName, r.OSVersion, r.OSHostname, r.DeviceIntegration, r.DeviceType)
}

// NDMInterfaceRow is one interface in ninjacat.ndm_interfaces.
//
// IsPhysical and MerakiEnabled are *uint8, not bool: the wire's *bool makes a
// three-way distinction (true / false / not reported) that a plain UInt8
// would collapse, so they travel as Nullable(UInt8) — nil is NULL, and the
// intake side (ndmBoolPtr) is the only place that turns *bool into *uint8.
type NDMInterfaceRow struct {
	TenantID         string
	Namespace        string
	Subnet           string
	Integration      string
	CollectTimestamp time.Time
	Extra            map[string]string

	DeviceID    string
	IDTags      map[string][]string
	Index       int32
	RawID       string
	RawIDType   string
	Name        string
	Alias       string
	Description string
	MacAddress  string

	// AdminStatus/OperStatus/Type are kept as the raw wire numbers — see the
	// migration's comment on why they are not decoded into named enums here.
	AdminStatus uint8
	OperStatus  uint8
	Type        uint32

	IsPhysical    *uint8
	MerakiEnabled *uint8
	MerakiStatus  string
}

func (r NDMInterfaceRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Namespace, r.Subnet, r.Integration, r.CollectTimestamp, orEmpty(r.Extra),
		r.DeviceID, orEmpty(r.IDTags), r.Index, r.RawID, r.RawIDType, r.Name, r.Alias, r.Description, r.MacAddress,
		r.AdminStatus, r.OperStatus, r.Type, r.IsPhysical, r.MerakiEnabled, r.MerakiStatus)
}

// NDMIPAddressRow is one IPAddressMetadata entry in ninjacat.ndm_ip_addresses.
type NDMIPAddressRow struct {
	TenantID         string
	Namespace        string
	Subnet           string
	Integration      string
	CollectTimestamp time.Time
	Extra            map[string]string

	InterfaceID string
	IPAddress   string
	Prefixlen   int32
}

func (r NDMIPAddressRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Namespace, r.Subnet, r.Integration, r.CollectTimestamp, orEmpty(r.Extra),
		r.InterfaceID, r.IPAddress, r.Prefixlen)
}

// NDMMetadataObjectRow is one element of links, vpn_tunnels,
// netflow_exporters, diagnoses, device_oids or scan_status — the payload's
// remaining lists, kept whole as JSON (Object) rather than typed into six
// more tables. See the migration's comment for why.
type NDMMetadataObjectRow struct {
	TenantID         string
	Namespace        string
	Subnet           string
	Integration      string
	CollectTimestamp time.Time
	Extra            map[string]string

	Kind        string
	DeviceID    string
	InterfaceID string
	Object      string
}

func (r NDMMetadataObjectRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Namespace, r.Subnet, r.Integration, r.CollectTimestamp, orEmpty(r.Extra),
		r.Kind, r.DeviceID, r.InterfaceID, r.Object)
}

// NDMDeviceConfigRow is one NetworkDeviceConfig in ninjacat.ndm_device_configs
// (/api/v2/ndmconfig, report.NCMPayload). Namespace/CollectTimestamp/
// AgentHostname/Inventories/Extra are the enclosing NCMPayload's fields,
// repeated onto every config it carried.
type NDMDeviceConfigRow struct {
	TenantID         string
	Namespace        string
	CollectTimestamp time.Time
	AgentHostname    string
	Inventories      string
	Extra            map[string]string

	DeviceID     string
	DeviceIP     string
	ConfigType   string
	ConfigSource string
	Timestamp    time.Time
	Tags         map[string][]string
	Content      string
}

func (r NDMDeviceConfigRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Namespace, r.CollectTimestamp, r.AgentHostname, r.Inventories, orEmpty(r.Extra),
		r.DeviceID, r.DeviceIP, r.ConfigType, r.ConfigSource, r.Timestamp, orEmpty(r.Tags), r.Content)
}

// SNMPTrapRow is one trap in ninjacat.snmp_traps (/api/v2/ndmtraps).
//
// GenericTrap/SpecificTrap are v1-only fields on the wire (comp/snmptraps/
// formatter): *int32 keeps "not a v1 trap" distinct from "generic trap 0".
type SNMPTrapRow struct {
	TenantID  string
	Timestamp time.Time

	DDSource string
	DDTags   map[string][]string
	Device   string // pulled from ddtags' snmp_device:<ip>

	Uptime        uint32
	SNMPTrapOID   string
	SNMPTrapName  string
	SNMPTrapMIB   string
	EnterpriseOID string
	GenericTrap   *int32
	SpecificTrap  *int32

	VarOIDs   []string
	VarTypes  []string
	VarValues []string

	Enriched map[string]string
	Raw      string
}

func (r SNMPTrapRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.DDSource, orEmpty(r.DDTags), r.Device,
		r.Uptime, r.SNMPTrapOID, r.SNMPTrapName, r.SNMPTrapMIB, r.EnterpriseOID,
		r.GenericTrap, r.SpecificTrap,
		orEmptySlice(r.VarOIDs), orEmptySlice(r.VarTypes), orEmptySlice(r.VarValues),
		orEmpty(r.Enriched), r.Raw)
}

// NetflowFlowRow is one comp/netflow/payload.FlowPayload flattened into
// ninjacat.netflow_flows. AdditionalFields is the map ndmDecodeFlows restores
// (FlowPayload's own MarshalJSON spreads it over the root object instead of
// an "additional_fields" key), re-marshalled to JSON text per value so a
// json.Number never rounds through float64.
type NetflowFlowRow struct {
	TenantID       string
	FlushTimestamp time.Time
	FlowType       string
	SamplingRate   uint64
	Direction      string
	Start          time.Time
	End            time.Time
	Bytes          uint64
	Packets        uint64
	EtherType      string
	IPProtocol     string
	TOS            uint32
	DSCP           uint32
	DSCPName       string

	DeviceNamespace string
	ExporterIP      string

	SourceIP                 string
	SourcePort               string
	SourceMac                string
	SourceMask               string
	SourceReverseDNSHostname string

	DestinationIP                 string
	DestinationPort               string
	DestinationMac                string
	DestinationMask               string
	DestinationReverseDNSHostname string

	IngressInterfaceIndex uint32
	EgressInterfaceIndex  uint32

	Host             string
	TCPFlags         []string
	NextHopIP        string
	AdditionalFields map[string]string
}

func (r NetflowFlowRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.FlushTimestamp, r.FlowType, r.SamplingRate, r.Direction,
		r.Start, r.End, r.Bytes, r.Packets, r.EtherType, r.IPProtocol, r.TOS, r.DSCP, r.DSCPName,
		r.DeviceNamespace, r.ExporterIP,
		r.SourceIP, r.SourcePort, r.SourceMac, r.SourceMask, r.SourceReverseDNSHostname,
		r.DestinationIP, r.DestinationPort, r.DestinationMac, r.DestinationMask, r.DestinationReverseDNSHostname,
		r.IngressInterfaceIndex, r.EgressInterfaceIndex,
		r.Host, orEmptySlice(r.TCPFlags), r.NextHopIP, orEmpty(r.AdditionalFields))
}

// NetworkPathRow is one pkg/networkpath/payload.NetworkPath flattened into
// ninjacat.network_paths. Runs and their hops become parallel arrays keyed by
// run index (Array(Array(...))), one level deeper for a hop's own
// ReverseDNS ([]string per hop, so Array(Array(Array(String)))).
type NetworkPathRow struct {
	TenantID       string
	Timestamp      time.Time
	AgentVersion   string
	Namespace      string
	TestConfigID   string
	TestConfigName string
	TestResultID   string
	TestRunID      string

	Origin           string
	TestRunType      string
	TestConfigSource string
	SourceProduct    string
	CollectorType    string
	Protocol         string

	SourceName        string
	SourceDisplayName string
	SourceHostname    string
	// nil means Source.Via was nil on the wire — the agent could not resolve
	// a route, not "resolved to an empty value".
	SourceViaSubnetAlias           *string
	SourceViaInterfaceHardwareAddr *string
	SourceNetworkID                string
	SourceService                  string
	SourceContainerID              string
	SourcePublicIP                 string

	DestinationHostname string
	DestinationPort     uint16
	DestinationService  string

	HopCountAvg float64
	HopCountMin int32
	HopCountMax int32

	RunIDs                   []string
	RunSourceIPs             []string
	RunSourcePorts           []uint16
	RunDestinationIPs        []string
	RunDestinationPorts      []uint16
	RunDestinationReverseDNS [][]string

	HopTTLs       [][]int32
	HopIPs        [][]string
	HopReverseDNS [][][]string
	HopRTTs       [][]float64
	HopReachable  [][]uint8

	E2eRTTs                 []float64
	E2ePacketsSent          int32
	E2ePacketsReceived      int32
	E2ePacketLossPercentage float32
	E2eJitter               float64
	E2eRTTAvg               float64
	E2eRTTMin               float64
	E2eRTTMax               float64

	Tags map[string][]string
}

func (r NetworkPathRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.AgentVersion, r.Namespace,
		r.TestConfigID, r.TestConfigName, r.TestResultID, r.TestRunID,
		r.Origin, r.TestRunType, r.TestConfigSource, r.SourceProduct, r.CollectorType, r.Protocol,
		r.SourceName, r.SourceDisplayName, r.SourceHostname,
		r.SourceViaSubnetAlias, r.SourceViaInterfaceHardwareAddr,
		r.SourceNetworkID, r.SourceService, r.SourceContainerID, r.SourcePublicIP,
		r.DestinationHostname, r.DestinationPort, r.DestinationService,
		r.HopCountAvg, r.HopCountMin, r.HopCountMax,
		orEmptySlice(r.RunIDs), orEmptySlice(r.RunSourceIPs), orEmptySlice(r.RunSourcePorts),
		orEmptySlice(r.RunDestinationIPs), orEmptySlice(r.RunDestinationPorts), orEmptySlice2(r.RunDestinationReverseDNS),
		orEmptySlice2(r.HopTTLs), orEmptySlice2(r.HopIPs), orEmptySlice3(r.HopReverseDNS),
		orEmptySlice2(r.HopRTTs), orEmptySlice2(r.HopReachable),
		orEmptySlice(r.E2eRTTs), r.E2ePacketsSent, r.E2ePacketsReceived, r.E2ePacketLossPercentage,
		r.E2eJitter, r.E2eRTTAvg, r.E2eRTTMin, r.E2eRTTMax,
		orEmpty(r.Tags))
}

func init() {
	registerWriter(NDMDevicesWriter, WriterConfig{
		Name: "ndm_devices",
		Insert: `INSERT INTO ndm_devices
			(tenant_id, namespace, subnet, integration, collect_timestamp, extra,
			 device_id, id_tags, tags, ip_address, status, ping_status,
			 name, description, sys_object_id, location, profile, profile_version, vendor,
			 device_subnet, serial_number, version, product_name, model,
			 os_name, os_version, os_hostname, device_integration, device_type)`,
		// Bursty-per-pass, like k8s_resources: a full device inventory lands
		// at once, not incrementally.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, NDMDeviceRow{})

	registerWriter(NDMInterfacesWriter, WriterConfig{
		Name: "ndm_interfaces",
		Insert: `INSERT INTO ndm_interfaces
			(tenant_id, namespace, subnet, integration, collect_timestamp, extra,
			 device_id, id_tags, if_index, raw_id, raw_id_type, name, alias, description, mac_address,
			 admin_status, oper_status, type, is_physical, meraki_enabled, meraki_status)`,
		// Same bursty-per-pass class as ndm_devices, and there are usually
		// several interfaces per device.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, NDMInterfaceRow{})

	registerWriter(NDMIPAddressesWriter, WriterConfig{
		Name: "ndm_ip_addresses",
		Insert: `INSERT INTO ndm_ip_addresses
			(tenant_id, namespace, subnet, integration, collect_timestamp, extra,
			 interface_id, ip_address, prefixlen)`,
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, NDMIPAddressRow{})

	registerWriter(NDMMetadataObjectsWriter, WriterConfig{
		Name: "ndm_metadata_objects",
		Insert: `INSERT INTO ndm_metadata_objects
			(tenant_id, namespace, subnet, integration, collect_timestamp, extra,
			 kind, device_id, interface_id, object)`,
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, NDMMetadataObjectRow{})

	registerWriter(NDMDeviceConfigsWriter, WriterConfig{
		Name: "ndm_device_configs",
		Insert: `INSERT INTO ndm_device_configs
			(tenant_id, namespace, collect_timestamp, agent_hostname, inventories, extra,
			 device_id, device_ip, config_type, config_source, timestamp, tags, content)`,
		// Whole-document payloads, like k8s_manifests: row counts stand for
		// megabytes of config text, so tight ceilings and one flush in flight.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, NDMDeviceConfigRow{})

	registerWriter(SNMPTrapsWriter, WriterConfig{
		Name: "snmp_traps",
		Insert: `INSERT INTO snmp_traps
			(tenant_id, timestamp, ddsource, ddtags, device,
			 uptime, snmp_trap_oid, snmp_trap_name, snmp_trap_mib,
			 enterprise_oid, generic_trap, specific_trap,
			 var_oids, var_types, var_values, enriched, raw)`,
		// Human/system-scale like events: traps are asynchronous and
		// occasional, not a steady stream.
		MaxRows: 200, FlushInterval: 2 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, SNMPTrapRow{})

	registerWriter(NetflowFlowsWriter, WriterConfig{
		Name: "netflow_flows",
		Insert: `INSERT INTO netflow_flows
			(tenant_id, flush_timestamp, flow_type, sampling_rate, direction,
			 flow_start, flow_end, bytes, packets, ether_type, ip_protocol, tos, dscp, dscp_name,
			 device_namespace, exporter_ip,
			 source_ip, source_port, source_mac, source_mask, source_reverse_dns_hostname,
			 destination_ip, destination_port, destination_mac, destination_mask, destination_reverse_dns_hostname,
			 ingress_interface_index, egress_interface_index,
			 host, tcp_flags, next_hop_ip, additional_fields)`,
		// The highest-volume NDM table, like logs: bigger batches, bigger
		// safety margin.
		MaxRows: 20000, FlushInterval: 1 * time.Second,
		BufferLimit: 500_000, MaxInFlight: 2,
	}, NetflowFlowRow{})

	registerWriter(NetworkPathsWriter, WriterConfig{
		Name: "network_paths",
		Insert: `INSERT INTO network_paths
			(tenant_id, timestamp, agent_version, namespace,
			 test_config_id, test_config_name, test_result_id, test_run_id,
			 origin, test_run_type, test_config_source, source_product, collector_type, protocol,
			 source_name, source_display_name, source_hostname,
			 source_via_subnet_alias, source_via_interface_hardware_addr,
			 source_network_id, source_service, source_container_id, source_public_ip,
			 destination_hostname, destination_port, destination_service,
			 hop_count_avg, hop_count_min, hop_count_max,
			 run_ids, run_source_ips, run_source_ports,
			 run_destination_ips, run_destination_ports, run_destination_reverse_dns,
			 hop_ttls, hop_ips, hop_reverse_dns, hop_rtts, hop_reachable,
			 e2e_rtts, e2e_packets_sent, e2e_packets_received, e2e_packet_loss_percentage,
			 e2e_jitter, e2e_rtt_avg, e2e_rtt_min, e2e_rtt_max,
			 tags)`,
		// Low-frequency status, like check_runs: scheduled test runs trickle
		// in, so a small threshold and a longer timer.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, NetworkPathRow{})

	registerTypes(
		WriteNDMDevices{}, NDMDeviceRow{},
		WriteNDMInterfaces{}, NDMInterfaceRow{},
		WriteNDMIPAddresses{}, NDMIPAddressRow{},
		WriteNDMMetadataObjects{}, NDMMetadataObjectRow{},
		WriteNDMDeviceConfigs{}, NDMDeviceConfigRow{},
		WriteSNMPTraps{}, SNMPTrapRow{},
		WriteNetflowFlows{}, NetflowFlowRow{},
		WriteNetworkPaths{}, NetworkPathRow{},
	)
}
