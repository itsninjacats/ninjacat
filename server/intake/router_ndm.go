package intake

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"net/http"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"time"

	netflowpayload "github.com/DataDog/datadog-agent/comp/netflow/payload"
	netpathpayload "github.com/DataDog/datadog-agent/pkg/networkpath/payload"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// Network Devices — FOUR hosts, one product, all JSON (docs §1.5).
//
//	ndm-intake.<site>         /api/v2/ndm        []metadata.NetworkDevicesMetadata -> ndm_devices, ndm_interfaces, ndm_ip_addresses, ndm_metadata_objects
//	ndm-intake.<site>         /api/v2/ndmconfig  []report.NCMPayload              -> ndm_device_configs
//	snmp-traps-intake.<site>  /api/v2/ndmtraps   []{"trap":{…}}                    -> snmp_traps
//	ndmflow-intake.<site>     /api/v2/ndmflow    []payload.FlowPayload             -> netflow_flows
//	netpath-intake.<site>     /api/v2/netpath    []payload.NetworkPath             -> network_paths
//
//	config: none — event platform, see router_containers.go.
//
// They share a router because they are one product from the operator's point
// of view, even though Datadog gave each its own hostname. The host switch in
// routes.go sends all four here.
//
// netflow_flows and network_paths decode into the REAL vendored types
// (comp/netflow/payload.FlowPayload, pkg/networkpath/payload.NetworkPath) —
// both are their own Go module, small and dependency-free. The other three
// tracks have no such luck: their real types live in
// pkg/networkdevice/metadata, pkg/networkconfigmanagement/report and
// comp/snmptraps/formatter, all of which sit in packages that pull in the
// whole datadog-agent module. So ndm/ndmconfig/ndmtraps decode into local
// mirror structs below, built from the wire empirically (docs/ + captures)
// rather than imported — the decode-twice pattern (typed struct, plus a
// second pass into a raw map for whatever the struct does not declare) is the
// same one router_kubeops.go's kubeActionDecode uses for the same reason.
func (a *Server) routeNDM(g *gin.RouterGroup) {
	g.POST("/api/v2/ndm", a.handleNDMMetadata)     // device metadata and topology
	g.POST("/api/v2/ndmconfig", a.handleNDMConfig) // device configuration backups
	g.POST("/api/v2/ndmtraps", a.handleNDMTraps)   // SNMP traps
	g.POST("/api/v2/ndmflow", a.handleNDMFlow)     // NetFlow / sFlow / IPFIX
	g.POST("/api/v2/netpath", a.handleNetpath)     // traceroute results, JSON
}

// ---------------------------------------------------------------------------
// /api/v2/ndm — device metadata, interfaces, IP addresses, and everything
// else (links, vpn_tunnels, netflow_exporters, diagnoses, device_oids,
// scan_status).
// ---------------------------------------------------------------------------

// ndmMetadataPayload mirrors metadata.NetworkDevicesMetadata
// (pkg/networkdevice/metadata/payload.go). Devices/Interfaces/IPAddresses
// decode fully typed because their tables have real columns for every field;
// the remaining six lists stay json.RawMessage per element, because
// ndm_metadata_objects keeps them whole rather than typed (see the migration
// header) and a per-element device_id/interface_id extraction — see
// ndmObjectIDs — is all that is needed beyond the raw bytes.
type ndmMetadataPayload struct {
	Subnet           string             `json:"subnet,omitempty"`
	Namespace        string             `json:"namespace"`
	Integration      string             `json:"integration"`
	Devices          []ndmDeviceJSON    `json:"devices,omitempty"`
	Interfaces       []ndmInterfaceJSON `json:"interfaces,omitempty"`
	IPAddresses      []ndmIPAddressJSON `json:"ip_addresses,omitempty"`
	Links            []json.RawMessage  `json:"links,omitempty"`
	VPNTunnels       []json.RawMessage  `json:"vpn_tunnels,omitempty"`
	NetflowExporters []json.RawMessage  `json:"netflow_exporters,omitempty"`
	Diagnoses        []json.RawMessage  `json:"diagnoses,omitempty"`
	DeviceOIDs       []json.RawMessage  `json:"device_oids,omitempty"`
	DeviceScanStatus json.RawMessage    `json:"scan_status,omitempty"`
	CollectTimestamp int64              `json:"collect_timestamp"`
}

// ndmDeviceJSON mirrors DeviceMetadata. Status/PingStatus stay json.Number —
// the wire sends a numeric enum (1=Reachable, 2=Unreachable) and this keeps
// the exact digit rather than routing it through float64, so
// ndmDeviceStatusName never has to guess at a fractional status.
type ndmDeviceJSON struct {
	ID             string      `json:"id"`
	IDTags         []string    `json:"id_tags,omitempty"`
	Tags           []string    `json:"tags,omitempty"`
	IPAddress      string      `json:"ip_address,omitempty"`
	Status         json.Number `json:"status"`
	PingStatus     json.Number `json:"ping_status"`
	Name           string      `json:"name,omitempty"`
	Description    string      `json:"description,omitempty"`
	SysObjectID    string      `json:"sys_object_id,omitempty"`
	Location       string      `json:"location,omitempty"`
	Profile        string      `json:"profile,omitempty"`
	ProfileVersion uint64      `json:"profile_version,omitempty"`
	Vendor         string      `json:"vendor,omitempty"`
	Subnet         string      `json:"subnet,omitempty"`
	SerialNumber   string      `json:"serial_number,omitempty"`
	Version        string      `json:"version,omitempty"`
	ProductName    string      `json:"product_name,omitempty"`
	Model          string      `json:"model,omitempty"`
	OsName         string      `json:"os_name,omitempty"`
	OsVersion      string      `json:"os_version,omitempty"`
	OsHostname     string      `json:"os_hostname,omitempty"`
	Integration    string      `json:"integration,omitempty"`
	DeviceType     string      `json:"device_type,omitempty"`
}

// ndmInterfaceJSON mirrors InterfaceMetadata. IsPhysical/MerakiEnabled stay
// *bool so ndmBoolPtr can tell "not reported" from an explicit false.
type ndmInterfaceJSON struct {
	DeviceID      string      `json:"device_id"`
	IDTags        []string    `json:"id_tags,omitempty"`
	Index         int32       `json:"index"`
	RawID         string      `json:"raw_id,omitempty"`
	RawIDType     string      `json:"raw_id_type,omitempty"`
	Name          string      `json:"name,omitempty"`
	Alias         string      `json:"alias,omitempty"`
	Description   string      `json:"description,omitempty"`
	MacAddress    string      `json:"mac_address,omitempty"`
	AdminStatus   json.Number `json:"admin_status,omitempty"`
	OperStatus    json.Number `json:"oper_status,omitempty"`
	Type          int32       `json:"type,omitempty"`
	IsPhysical    *bool       `json:"is_physical,omitempty"`
	MerakiEnabled *bool       `json:"meraki_enabled,omitempty"`
	MerakiStatus  string      `json:"meraki_status,omitempty"`
}

// ndmIPAddressJSON mirrors IPAddressMetadata.
type ndmIPAddressJSON struct {
	InterfaceID string `json:"interface_id"`
	IPAddress   string `json:"ip_address"`
	Prefixlen   int32  `json:"prefixlen,omitempty"`
}

// ndmObjectIDs is a minimal probe for the device_id/interface_id one of the
// six remaining list shapes MAY carry — vpn_tunnels and device_oids/
// scan_status have one, links/netflow_exporters/diagnoses do not, per the NDM
// gap report. Unmarshalling a shape that lacks the key simply leaves it "",
// which is the correct answer, not an error.
type ndmObjectIDs struct {
	DeviceID    string `json:"device_id"`
	InterfaceID string `json:"interface_id"`
}

// ndmMetadataKnownKeys drives ndmExtraTopLevel for the /api/v2/ndm payload:
// every root key ndmMetadataPayload does not declare is undocumented and goes
// to extra rather than being dropped.
var ndmMetadataKnownKeys = ndmKnownJSONKeys(reflect.TypeOf(ndmMetadataPayload{}))

// handleNDMMetadata — /api/v2/ndm, []metadata.NetworkDevicesMetadata
// (pkg/networkdevice/metadata).
func (a *Server) handleNDMMetadata(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ndm] cannot read body: %v", err)
		return
	}
	items, err := ndmItems(body)
	if err != nil {
		log.Printf("[ndm] not a JSON array (%v), raw follows", err)
		describe("ndm", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "ndm", "decode_error", err.Error(), body)
		return
	}
	log.Printf("[ndm] %d entries, %d B", len(items), len(body))

	tenant := TenantFromContext(c)
	var devices []storage.NDMDeviceRow
	var interfaces []storage.NDMInterfaceRow
	var ipAddrs []storage.NDMIPAddressRow
	var objects []storage.NDMMetadataObjectRow

	for i, raw := range items {
		var view map[string]any
		if err := ndmDecodeNumbers(raw, &view); err == nil {
			ndmLogMetadata("ndm", view)
		}

		var payload ndmMetadataPayload
		if err := json.Unmarshal(raw, &payload); err != nil {
			log.Printf("[ndm] entry %d: %v", i, err)
			a.storeRaw(c, "ndm", "decode_error", fmt.Sprintf("entry %d: %v", i, err), raw)
			continue
		}
		if tenant == "" {
			continue // still answered above; nothing to key rows on
		}

		extra := ndmExtraTopLevel(raw, ndmMetadataKnownKeys)
		ts := time.Unix(payload.CollectTimestamp, 0).UTC()

		for _, d := range payload.Devices {
			devices = append(devices, ndmDeviceRow(tenant, payload, ts, extra, d))
		}
		for _, ifc := range payload.Interfaces {
			interfaces = append(interfaces, ndmInterfaceRow(tenant, payload, ts, extra, ifc))
		}
		for _, ip := range payload.IPAddresses {
			ipAddrs = append(ipAddrs, ndmIPAddressRow(tenant, payload, ts, extra, ip))
		}
		objects = append(objects, ndmMetadataObjectRows(tenant, payload, ts, extra)...)
	}

	a.store(storage.NDMDevicesWriter, storage.WriteNDMDevices{Devices: devices}, len(devices))
	a.store(storage.NDMInterfacesWriter, storage.WriteNDMInterfaces{Interfaces: interfaces}, len(interfaces))
	a.store(storage.NDMIPAddressesWriter, storage.WriteNDMIPAddresses{Addresses: ipAddrs}, len(ipAddrs))
	a.store(storage.NDMMetadataObjectsWriter, storage.WriteNDMMetadataObjects{Objects: objects}, len(objects))
}

// ndmDeviceStatusName maps DeviceStatus's numeric wire value (1=Reachable,
// 2=Unreachable) to the Enum8 name ndm_devices.status/ping_status expects.
// Anything else — including an absent field, which decodes to "" — is
// "unknown" rather than a guess.
func ndmDeviceStatusName(n json.Number) string {
	switch v, err := n.Int64(); {
	case err != nil:
		return "unknown"
	case v == 1:
		return "reachable"
	case v == 2:
		return "unreachable"
	default:
		return "unknown"
	}
}

// ndmNumberUint8 reads a json.Number into a UInt8 wire enum, 0 on anything
// that does not parse — the same "absent means unknown" rule as
// ndmDeviceStatusName, just without named values to fall back to (see the
// migration's comment on admin_status/oper_status/type).
func ndmNumberUint8(n json.Number) uint8 {
	v, err := n.Int64()
	if err != nil {
		return 0
	}
	return uint8(v)
}

// ndmBoolPtr turns a *bool from the wire into the *uint8 the driver wants for
// a Nullable(UInt8) column, keeping nil (not reported) distinct from both
// true and false.
func ndmBoolPtr(b *bool) *uint8 {
	if b == nil {
		return nil
	}
	var v uint8
	if *b {
		v = 1
	}
	return &v
}

func ndmDeviceRow(tenant string, p ndmMetadataPayload, ts time.Time, extra map[string]string, d ndmDeviceJSON) storage.NDMDeviceRow {
	return storage.NDMDeviceRow{
		TenantID: tenant, Namespace: p.Namespace, Subnet: p.Subnet, Integration: p.Integration,
		CollectTimestamp: ts, Extra: extra,

		DeviceID:   d.ID,
		IDTags:     tagsToMultiMap(d.IDTags),
		Tags:       tagsToMultiMap(d.Tags),
		IPAddress:  d.IPAddress,
		Status:     ndmDeviceStatusName(d.Status),
		PingStatus: ndmDeviceStatusName(d.PingStatus),

		Name: d.Name, Description: d.Description, SysObjectID: d.SysObjectID, Location: d.Location,
		Profile: d.Profile, ProfileVersion: d.ProfileVersion, Vendor: d.Vendor,

		DeviceSubnet: d.Subnet, SerialNumber: d.SerialNumber, Version: d.Version,
		ProductName: d.ProductName, Model: d.Model,
		OSName: d.OsName, OSVersion: d.OsVersion, OSHostname: d.OsHostname,
		DeviceIntegration: d.Integration, DeviceType: d.DeviceType,
	}
}

func ndmInterfaceRow(tenant string, p ndmMetadataPayload, ts time.Time, extra map[string]string, ifc ndmInterfaceJSON) storage.NDMInterfaceRow {
	return storage.NDMInterfaceRow{
		TenantID: tenant, Namespace: p.Namespace, Subnet: p.Subnet, Integration: p.Integration,
		CollectTimestamp: ts, Extra: extra,

		DeviceID: ifc.DeviceID, IDTags: tagsToMultiMap(ifc.IDTags), Index: ifc.Index,
		RawID: ifc.RawID, RawIDType: ifc.RawIDType, Name: ifc.Name, Alias: ifc.Alias,
		Description: ifc.Description, MacAddress: ifc.MacAddress,
		AdminStatus: ndmNumberUint8(ifc.AdminStatus), OperStatus: ndmNumberUint8(ifc.OperStatus),
		Type: uint32(ifc.Type), IsPhysical: ndmBoolPtr(ifc.IsPhysical), MerakiEnabled: ndmBoolPtr(ifc.MerakiEnabled),
		MerakiStatus: ifc.MerakiStatus,
	}
}

func ndmIPAddressRow(tenant string, p ndmMetadataPayload, ts time.Time, extra map[string]string, ip ndmIPAddressJSON) storage.NDMIPAddressRow {
	return storage.NDMIPAddressRow{
		TenantID: tenant, Namespace: p.Namespace, Subnet: p.Subnet, Integration: p.Integration,
		CollectTimestamp: ts, Extra: extra,
		InterfaceID: ip.InterfaceID, IPAddress: ip.IPAddress, Prefixlen: ip.Prefixlen,
	}
}

// ndmMetadataObjectRows converts the six remaining lists into one row family:
// kind names which list the element came from, device_id/interface_id are
// read off whatever the element declares (empty for the shapes that carry
// neither), and object is the element's own bytes, untouched.
func ndmMetadataObjectRows(tenant string, p ndmMetadataPayload, ts time.Time, extra map[string]string) []storage.NDMMetadataObjectRow {
	var out []storage.NDMMetadataObjectRow
	add := func(kind string, raw json.RawMessage) {
		var ids ndmObjectIDs
		_ = json.Unmarshal(raw, &ids) // best effort: a malformed element still keeps its raw bytes below
		out = append(out, storage.NDMMetadataObjectRow{
			TenantID: tenant, Namespace: p.Namespace, Subnet: p.Subnet, Integration: p.Integration,
			CollectTimestamp: ts, Extra: extra,
			Kind: kind, DeviceID: ids.DeviceID, InterfaceID: ids.InterfaceID, Object: string(raw),
		})
	}
	for _, raw := range p.Links {
		add("link", raw)
	}
	for _, raw := range p.VPNTunnels {
		add("vpn_tunnel", raw)
	}
	for _, raw := range p.NetflowExporters {
		add("netflow_exporter", raw)
	}
	for _, raw := range p.Diagnoses {
		add("diagnosis", raw)
	}
	for _, raw := range p.DeviceOIDs {
		add("device_oid", raw)
	}
	if len(p.DeviceScanStatus) > 0 {
		add("scan_status", p.DeviceScanStatus)
	}
	return out
}

// ---------------------------------------------------------------------------
// /api/v2/ndmconfig — device configuration backups.
// ---------------------------------------------------------------------------

// ndmConfigPayload mirrors report.NCMPayload (pkg/networkconfigmanagement/
// report), plus AgentHostname/Inventories: the NDM gap report flags both as
// observed on the wire but undocumented in the struct. Giving them real json
// tags here means ndmExtraTopLevel treats them as declared, not unknown, so
// they land in their own columns instead of a JSON blob in extra.
type ndmConfigPayload struct {
	Namespace        string                `json:"namespace"`
	Configs          []ndmDeviceConfigJSON `json:"configs"`
	CollectTimestamp int64                 `json:"collect_timestamp"`
	AgentHostname    string                `json:"agent_hostname,omitempty"`
	Inventories      json.RawMessage       `json:"inventories,omitempty"`
}

type ndmDeviceConfigJSON struct {
	DeviceID     string   `json:"device_id"`
	DeviceIP     string   `json:"device_ip"`
	ConfigType   string   `json:"config_type"`
	ConfigSource string   `json:"config_source"`
	Timestamp    int64    `json:"timestamp"`
	Tags         []string `json:"tags"`
	Content      string   `json:"content"`
}

var ndmConfigKnownKeys = ndmKnownJSONKeys(reflect.TypeOf(ndmConfigPayload{}))

// handleNDMConfig — /api/v2/ndmconfig, []report.NCMPayload.
func (a *Server) handleNDMConfig(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ndmconfig] cannot read body: %v", err)
		return
	}
	items, err := ndmItems(body)
	if err != nil {
		log.Printf("[ndmconfig] not a JSON array (%v), raw follows", err)
		describe("ndmconfig", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "ndmconfig", "decode_error", err.Error(), body)
		return
	}
	log.Printf("[ndmconfig] %d entries, %d B", len(items), len(body))

	tenant := TenantFromContext(c)
	var rows []storage.NDMDeviceConfigRow
	for i, raw := range items {
		var view map[string]any
		if err := ndmDecodeNumbers(raw, &view); err == nil {
			ndmLogConfig("ndmconfig", view)
		}

		var payload ndmConfigPayload
		if err := json.Unmarshal(raw, &payload); err != nil {
			log.Printf("[ndmconfig] entry %d: %v", i, err)
			a.storeRaw(c, "ndmconfig", "decode_error", fmt.Sprintf("entry %d: %v", i, err), raw)
			continue
		}
		if tenant == "" {
			continue
		}

		extra := ndmExtraTopLevel(raw, ndmConfigKnownKeys)
		ts := time.Unix(payload.CollectTimestamp, 0).UTC()
		var inventories string
		if len(payload.Inventories) > 0 {
			inventories = string(payload.Inventories)
		}

		for _, cfg := range payload.Configs {
			rows = append(rows, storage.NDMDeviceConfigRow{
				TenantID: tenant, Namespace: payload.Namespace, CollectTimestamp: ts,
				AgentHostname: payload.AgentHostname, Inventories: inventories, Extra: extra,
				DeviceID: cfg.DeviceID, DeviceIP: cfg.DeviceIP, ConfigType: cfg.ConfigType, ConfigSource: cfg.ConfigSource,
				Timestamp: time.Unix(cfg.Timestamp, 0).UTC(), Tags: tagsToMultiMap(cfg.Tags), Content: cfg.Content,
			})
		}
	}
	a.store(storage.NDMDeviceConfigsWriter, storage.WriteNDMDeviceConfigs{Configs: rows}, len(rows))
}

// ---------------------------------------------------------------------------
// /api/v2/ndmtraps — SNMP traps.
// ---------------------------------------------------------------------------

// ndmTrapJSON mirrors the object under the "trap" key
// (comp/snmptraps/formatter/impl/formatter.go's FormatPacket/formatTrap).
// GenericTrap/SpecificTrap are v1-only on the wire and stay *int32 so a v2/v3
// trap (both absent) is distinguishable from a v1 trap reporting 0.
type ndmTrapJSON struct {
	DDSource      string            `json:"ddsource"`
	DDTags        string            `json:"ddtags"`
	Timestamp     int64             `json:"timestamp"`
	Uptime        json.Number       `json:"uptime"`
	SNMPTrapOID   string            `json:"snmpTrapOID"`
	SNMPTrapName  string            `json:"snmpTrapName,omitempty"`
	SNMPTrapMIB   string            `json:"snmpTrapMIB,omitempty"`
	EnterpriseOID string            `json:"enterpriseOID,omitempty"`
	GenericTrap   *int32            `json:"genericTrap,omitempty"`
	SpecificTrap  *int32            `json:"specificTrap,omitempty"`
	Variables     []ndmTrapVariable `json:"variables,omitempty"`
}

// ndmTrapVariable is one {"oid","type","value"} entry. Value stays raw JSON:
// its type varies per OID (string, number, or something MIB-specific), which
// is a genuine property of the SNMP data, not a decode gap.
type ndmTrapVariable struct {
	OID   string          `json:"oid"`
	Type  string          `json:"type"`
	Value json.RawMessage `json:"value"`
}

var ndmTrapKnownKeys = ndmKnownJSONKeys(reflect.TypeOf(ndmTrapJSON{}))

// handleNDMTraps — /api/v2/ndmtraps, []{"trap":{…}} from the snmptraps
// formatter (comp/snmptraps/formatter).
func (a *Server) handleNDMTraps(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ndmtraps] cannot read body: %v", err)
		return
	}
	items, err := ndmItems(body)
	if err != nil {
		log.Printf("[ndmtraps] not a JSON array (%v), raw follows", err)
		describe("ndmtraps", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "ndmtraps", "decode_error", err.Error(), body)
		return
	}
	log.Printf("[ndmtraps] %d entries, %d B", len(items), len(body))

	tenant := TenantFromContext(c)
	var rows []storage.SNMPTrapRow
	for i, raw := range items {
		var view map[string]any
		if err := ndmDecodeNumbers(raw, &view); err == nil {
			ndmLogTrap("ndmtraps", view)
		}

		var envelope map[string]json.RawMessage
		if err := json.Unmarshal(raw, &envelope); err != nil {
			log.Printf("[ndmtraps] entry %d: %v", i, err)
			a.storeRaw(c, "ndmtraps", "decode_error", fmt.Sprintf("entry %d: %v", i, err), raw)
			continue
		}
		trapRaw, ok := envelope["trap"]
		if !ok {
			log.Printf("[ndmtraps] entry %d: no trap object", i)
			a.storeRaw(c, "ndmtraps", "unexpected_shape", fmt.Sprintf("entry %d: no trap object", i), raw)
			continue
		}
		var trap ndmTrapJSON
		if err := json.Unmarshal(trapRaw, &trap); err != nil {
			log.Printf("[ndmtraps] entry %d: trap: %v", i, err)
			a.storeRaw(c, "ndmtraps", "decode_error", fmt.Sprintf("entry %d: trap: %v", i, err), raw)
			continue
		}
		if tenant == "" {
			continue
		}
		rows = append(rows, ndmTrapRow(tenant, trapRaw, trap))
	}
	a.store(storage.SNMPTrapsWriter, storage.WriteSNMPTraps{Traps: rows}, len(rows))
}

func ndmTrapRow(tenant string, trapRaw json.RawMessage, trap ndmTrapJSON) storage.SNMPTrapRow {
	oids := make([]string, 0, len(trap.Variables))
	types := make([]string, 0, len(trap.Variables))
	values := make([]string, 0, len(trap.Variables))
	for _, v := range trap.Variables {
		oids = append(oids, v.OID)
		types = append(types, v.Type)
		values = append(values, string(v.Value))
	}
	uptime, _ := trap.Uptime.Int64()
	return storage.SNMPTrapRow{
		TenantID: tenant, Timestamp: time.Unix(trap.Timestamp, 0).UTC(),
		DDSource: trap.DDSource, DDTags: tagsToMultiMap(ndmSplitTags(trap.DDTags)),
		Device: ndmTag(trap.DDTags, "snmp_device"),
		Uptime: uint32(uptime), SNMPTrapOID: trap.SNMPTrapOID, SNMPTrapName: trap.SNMPTrapName,
		SNMPTrapMIB: trap.SNMPTrapMIB, EnterpriseOID: trap.EnterpriseOID,
		GenericTrap: trap.GenericTrap, SpecificTrap: trap.SpecificTrap,
		VarOIDs: oids, VarTypes: types, VarValues: values,
		Enriched: ndmExtraTopLevel(trapRaw, ndmTrapKnownKeys),
		Raw:      string(trapRaw),
	}
}

// ndmSplitTags splits a comma-joined ddtags string, the way dbmTags does for
// DBM — but unlike strings.Split, an empty string yields no tags at all
// rather than one bogus empty-string tag (strings.Split("", ",") is
// []string{""}, len 1).
func ndmSplitTags(s string) []string {
	if s == "" {
		return nil
	}
	return strings.Split(s, ",")
}

// ---------------------------------------------------------------------------
// /api/v2/ndmflow — NetFlow / sFlow / IPFIX.
// ---------------------------------------------------------------------------

// handleNDMFlow decodes []payload.FlowPayload from comp/netflow/payload.
func (a *Server) handleNDMFlow(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ndmflow] cannot read body: %v", err)
		return
	}
	flows, err := ndmDecodeFlows(body)
	if err != nil {
		log.Printf("[ndmflow] not []FlowPayload (%v), raw follows", err)
		describe("ndmflow", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "ndmflow", "decode_error", err.Error(), body)
		return
	}
	ndmLogFlows(flows, len(body))

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := make([]storage.NetflowFlowRow, 0, len(flows))
	for _, f := range flows {
		rows = append(rows, ndmFlowRow(tenant, f))
	}
	a.store(storage.NetflowFlowsWriter, storage.WriteNetflowFlows{Flows: rows}, len(rows))
}

// ndmFlowRow flattens one restored FlowPayload (see ndmDecodeFlows) into its
// row. AdditionalFields values are re-marshalled to JSON text: they arrive as
// json.Number/string/map[string]any/etc from ndmDecodeFlows, and the text
// form is what the Map(String,String) column holds, precision intact.
func ndmFlowRow(tenant string, f netflowpayload.FlowPayload) storage.NetflowFlowRow {
	additional := make(map[string]string, len(f.AdditionalFields))
	for k, v := range f.AdditionalFields {
		b, err := json.Marshal(v)
		if err != nil {
			continue
		}
		additional[k] = string(b)
	}
	return storage.NetflowFlowRow{
		TenantID:       tenant,
		FlushTimestamp: time.UnixMilli(f.FlushTimestamp).UTC(),
		FlowType:       f.FlowType, SamplingRate: f.SamplingRate, Direction: f.Direction,
		Start: time.Unix(int64(f.Start), 0).UTC(), End: time.Unix(int64(f.End), 0).UTC(),
		Bytes: f.Bytes, Packets: f.Packets, EtherType: f.EtherType, IPProtocol: f.IPProtocol,
		TOS: f.TOS, DSCP: f.DSCP, DSCPName: f.DSCPName,
		DeviceNamespace: f.Device.Namespace, ExporterIP: f.Exporter.IP,
		SourceIP: f.Source.IP, SourcePort: f.Source.Port, SourceMac: f.Source.Mac, SourceMask: f.Source.Mask,
		SourceReverseDNSHostname: f.Source.ReverseDNSHostname,
		DestinationIP:            f.Destination.IP, DestinationPort: f.Destination.Port,
		DestinationMac: f.Destination.Mac, DestinationMask: f.Destination.Mask,
		DestinationReverseDNSHostname: f.Destination.ReverseDNSHostname,
		IngressInterfaceIndex:         f.Ingress.Interface.Index, EgressInterfaceIndex: f.Egress.Interface.Index,
		Host: f.Host, TCPFlags: f.TCPFlags, NextHopIP: f.NextHop.IP, AdditionalFields: additional,
	}
}

// ---------------------------------------------------------------------------
// /api/v2/netpath — traceroute results.
// ---------------------------------------------------------------------------

// handleNetpath decodes []payload.NetworkPath from pkg/networkpath/payload.
func (a *Server) handleNetpath(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[netpath] cannot read body: %v", err)
		return
	}
	var paths []netpathpayload.NetworkPath
	if err := ndmUnmarshal(body, &paths); err != nil {
		log.Printf("[netpath] not []NetworkPath (%v), raw follows", err)
		describe("netpath", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "netpath", "decode_error", err.Error(), body)
		return
	}
	log.Printf("[netpath] %d paths, %d B", len(paths), len(body))
	for _, p := range paths {
		ndmLogPath(p)
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := make([]storage.NetworkPathRow, 0, len(paths))
	for _, p := range paths {
		rows = append(rows, ndmNetworkPathRow(tenant, p))
	}
	a.store(storage.NetworkPathsWriter, storage.WriteNetworkPaths{Paths: rows}, len(rows))
}

// ndmIPString renders a net.IP the way the wire would — "" for nil, never
// Go's "<nil>" — for IP fields that unmarshal via net.IP's own
// UnmarshalText and may simply not be set (e.g. an unreachable hop).
func ndmIPString(ip net.IP) string {
	if ip == nil {
		return ""
	}
	return ip.String()
}

// ndmNetworkPathRow flattens one NetworkPath into its row: the traceroute
// runs and their hops become parallel arrays keyed by run index, and the
// single E2eProbe becomes flat arrays (it is not per-run).
func ndmNetworkPathRow(tenant string, p netpathpayload.NetworkPath) storage.NetworkPathRow {
	var viaSubnetAlias, viaIfaceHW *string
	if p.Source.Via != nil {
		if alias := p.Source.Via.Subnet.Alias; alias != "" {
			viaSubnetAlias = &alias
		}
		if hw := p.Source.Via.Interface.HardwareAddr; hw != "" {
			viaIfaceHW = &hw
		}
	}

	n := len(p.Traceroute.Runs)
	runIDs := make([]string, 0, n)
	runSrcIPs := make([]string, 0, n)
	runSrcPorts := make([]uint16, 0, n)
	runDstIPs := make([]string, 0, n)
	runDstPorts := make([]uint16, 0, n)
	runDstReverseDNS := make([][]string, 0, n)
	hopTTLs := make([][]int32, 0, n)
	hopIPs := make([][]string, 0, n)
	hopReverseDNS := make([][][]string, 0, n)
	hopRTTs := make([][]float64, 0, n)
	hopReachable := make([][]uint8, 0, n)

	for _, run := range p.Traceroute.Runs {
		runIDs = append(runIDs, run.RunID)
		runSrcIPs = append(runSrcIPs, ndmIPString(run.Source.IPAddress))
		runSrcPorts = append(runSrcPorts, run.Source.Port)
		runDstIPs = append(runDstIPs, ndmIPString(run.Destination.IPAddress))
		runDstPorts = append(runDstPorts, run.Destination.Port)
		runDstReverseDNS = append(runDstReverseDNS, run.Destination.ReverseDNS)

		ttls := make([]int32, 0, len(run.Hops))
		ips := make([]string, 0, len(run.Hops))
		rdns := make([][]string, 0, len(run.Hops))
		rtts := make([]float64, 0, len(run.Hops))
		reach := make([]uint8, 0, len(run.Hops))
		for _, hop := range run.Hops {
			ttls = append(ttls, int32(hop.TTL))
			ips = append(ips, ndmIPString(hop.IPAddress))
			rdns = append(rdns, hop.ReverseDNS)
			rtts = append(rtts, hop.RTT)
			var r uint8
			if hop.Reachable {
				r = 1
			}
			reach = append(reach, r)
		}
		hopTTLs = append(hopTTLs, ttls)
		hopIPs = append(hopIPs, ips)
		hopReverseDNS = append(hopReverseDNS, rdns)
		hopRTTs = append(hopRTTs, rtts)
		hopReachable = append(hopReachable, reach)
	}

	return storage.NetworkPathRow{
		TenantID: tenant, Timestamp: time.UnixMilli(p.Timestamp).UTC(),
		AgentVersion: p.AgentVersion, Namespace: p.Namespace,
		TestConfigID: p.TestConfigID, TestConfigName: p.TestConfigName,
		TestResultID: p.TestResultID, TestRunID: p.TestRunID,
		Origin: string(p.Origin), TestRunType: string(p.TestRunType),
		TestConfigSource: string(p.TestConfigSource), SourceProduct: string(p.SourceProduct),
		CollectorType: string(p.CollectorType), Protocol: string(p.Protocol),

		SourceName: p.Source.Name, SourceDisplayName: p.Source.DisplayName, SourceHostname: p.Source.Hostname,
		SourceViaSubnetAlias: viaSubnetAlias, SourceViaInterfaceHardwareAddr: viaIfaceHW,
		SourceNetworkID: p.Source.NetworkID, SourceService: p.Source.Service,
		SourceContainerID: p.Source.ContainerID, SourcePublicIP: p.Source.PublicIP,

		DestinationHostname: p.Destination.Hostname, DestinationPort: p.Destination.Port,
		DestinationService: p.Destination.Service,

		HopCountAvg: p.Traceroute.HopCount.Avg, HopCountMin: int32(p.Traceroute.HopCount.Min),
		HopCountMax: int32(p.Traceroute.HopCount.Max),

		RunIDs: runIDs, RunSourceIPs: runSrcIPs, RunSourcePorts: runSrcPorts,
		RunDestinationIPs: runDstIPs, RunDestinationPorts: runDstPorts, RunDestinationReverseDNS: runDstReverseDNS,

		HopTTLs: hopTTLs, HopIPs: hopIPs, HopReverseDNS: hopReverseDNS, HopRTTs: hopRTTs, HopReachable: hopReachable,

		E2eRTTs: p.E2eProbe.RTTs, E2ePacketsSent: int32(p.E2eProbe.PacketsSent),
		E2ePacketsReceived: int32(p.E2eProbe.PacketsReceived), E2ePacketLossPercentage: p.E2eProbe.PacketLossPercentage,
		E2eJitter: p.E2eProbe.Jitter, E2eRTTAvg: p.E2eProbe.RTT.Avg, E2eRTTMin: p.E2eProbe.RTT.Min, E2eRTTMax: p.E2eProbe.RTT.Max,

		Tags: tagsToMultiMap(p.Tags),
	}
}

// ---------------------------------------------------------------------------
// Shared decode helpers
// ---------------------------------------------------------------------------

// ndmDecodeFlows decodes []payload.FlowPayload and puts back what the
// standard decoder drops.
//
// FlowPayload.MarshalJSON (comp/netflow/payload) never writes an
// "additional_fields" key: it spreads the map's entries over the root object,
// next to "bytes" and "source". The type has no UnmarshalJSON to undo that, so
// json.Unmarshal into the struct silently discards every configured extra
// field. A second pass over the raw objects moves every root key the struct
// does not declare back into AdditionalFields, with numbers as json.Number so
// nothing goes through float64. If a request does carry a literal
// "additional_fields" object (not the agent, but a hand-made one), its entries
// are re-read the same way and win over the struct's own float64 decode.
func ndmDecodeFlows(body []byte) ([]netflowpayload.FlowPayload, error) {
	var flows []netflowpayload.FlowPayload
	if err := ndmUnmarshal(body, &flows); err != nil {
		return nil, err
	}
	var objects []map[string]json.RawMessage
	if err := ndmUnmarshal(body, &objects); err != nil {
		return nil, err
	}
	if len(objects) != len(flows) {
		return nil, fmt.Errorf("decoded %d flows from %d objects", len(flows), len(objects))
	}
	for i := range flows {
		extra := netflowpayload.AdditionalFields{}
		for key, raw := range objects[i] {
			switch {
			case key == "additional_fields":
				var nested map[string]any
				if err := ndmDecodeNumbers(raw, &nested); err != nil {
					return nil, fmt.Errorf("flow %d: additional_fields: %w", i, err)
				}
				for k, v := range nested {
					extra[k] = v
				}
			case ndmFlowKnownKeys[key]:
				// A struct field; already decoded.
			default:
				var v any
				if err := ndmDecodeNumbers(raw, &v); err != nil {
					return nil, fmt.Errorf("flow %d: %s: %w", i, key, err)
				}
				extra[key] = v
			}
		}
		if len(extra) > 0 {
			flows[i].AdditionalFields = extra
		} else {
			flows[i].AdditionalFields = nil
		}
	}
	return flows, nil
}

// ndmDecodeNumbers decodes JSON with numbers kept as json.Number, so the
// log views print uptimes, indexes and counters as sent rather than in float
// notation, and additional flow fields never pass through float64.
func ndmDecodeNumbers(raw json.RawMessage, dst any) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	return dec.Decode(dst)
}

// ndmFlowKnownKeys is the set of root keys FlowPayload.MarshalJSON writes for
// the struct's own fields. It is taken from the marshaller itself, on a probe
// value with the two omit-empty fields set, so the set follows whatever
// version of the module is imported. Every other root key on the wire is an
// additional field.
var ndmFlowKnownKeys = func() map[string]bool {
	probe := netflowpayload.FlowPayload{EtherType: "-", TCPFlags: []string{}}
	data, err := json.Marshal(probe)
	if err != nil {
		panic("ndmflow: cannot marshal a probe FlowPayload: " + err.Error())
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		panic("ndmflow: cannot read back a probe FlowPayload: " + err.Error())
	}
	keys := make(map[string]bool, len(fields))
	for k := range fields {
		keys[k] = true
	}
	return keys
}()

// ndmKnownJSONKeys reads a struct type's json tags into a set, the same way
// router_kubeops.go's kubeActionKnownKeys does — used to tell an undeclared
// wire key from a declared one without a second, hand-maintained list.
func ndmKnownJSONKeys(t reflect.Type) map[string]bool {
	keys := make(map[string]bool, t.NumField())
	for i := 0; i < t.NumField(); i++ {
		name, _, _ := strings.Cut(t.Field(i).Tag.Get("json"), ",")
		if name != "" && name != "-" {
			keys[name] = true
		}
	}
	return keys
}

// ndmExtraTopLevel decodes raw's root object a second time and returns every
// key not in known, as raw JSON text — the decode-twice half of the pattern
// documented at the top of this file. A raw that is not an object (or fails
// to decode at all, which should not happen here since the caller already
// decoded it once) yields no extra keys rather than an error: this is a
// best-effort completeness net, not a second place decoding can fail.
func ndmExtraTopLevel(raw json.RawMessage, known map[string]bool) map[string]string {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return nil
	}
	var extra map[string]string
	for k, v := range fields {
		if known[k] {
			continue
		}
		if extra == nil {
			extra = make(map[string]string)
		}
		extra[k] = string(v)
	}
	return extra
}

// ndmLogFlows summarises a flow batch: who exported, over which protocols,
// and how much traffic the batch accounts for.
func ndmLogFlows(flows []netflowpayload.FlowPayload, size int) {
	var bytes, packets uint64
	exporters := map[string]struct{}{}
	protocols := map[string]int{}
	types := map[string]int{}
	for _, f := range flows {
		bytes += f.Bytes
		packets += f.Packets
		exporters[f.Device.Namespace+"/"+f.Exporter.IP] = struct{}{}
		protocols[f.IPProtocol]++
		types[f.FlowType]++
	}
	log.Printf("[ndmflow] %d flows, %d B: %d exporters, bytes=%d packets=%d types=%s protocols=%s",
		len(flows), size, len(exporters), bytes, packets, ndmCounts(types), ndmCounts(protocols))
	for _, f := range flows {
		log.Printf("   %s %s:%s -> %s:%s %s bytes=%d packets=%d exporter=%s in=%d out=%d",
			f.IPProtocol, f.Source.IP, f.Source.Port, f.Destination.IP, f.Destination.Port,
			f.Direction, f.Bytes, f.Packets, f.Exporter.IP,
			f.Ingress.Interface.Index, f.Egress.Interface.Index)
	}
}

// ndmLogPath reports one traceroute: endpoints, hop count and probe loss.
func ndmLogPath(p netpathpayload.NetworkPath) {
	hops := 0
	for _, run := range p.Traceroute.Runs {
		hops = max(hops, len(run.Hops))
	}
	log.Printf("[netpath] %s %s %s -> %s:%d origin=%s runs=%d hops=%d (min=%d max=%d) loss=%.1f%% rtt_avg=%.2fms",
		p.Protocol, p.Namespace, p.Source.Hostname, p.Destination.Hostname, p.Destination.Port,
		p.Origin, len(p.Traceroute.Runs), hops,
		p.Traceroute.HopCount.Min, p.Traceroute.HopCount.Max,
		p.E2eProbe.PacketLossPercentage, p.E2eProbe.RTT.Avg)
}

// ndmLogMetadata reports one NetworkDevicesMetadata: the namespace and how
// many of each collection it carries.
func ndmLogMetadata(label string, item map[string]any) {
	log.Printf("[%s] namespace=%s integration=%s devices=%d interfaces=%d ip_addresses=%d links=%d vpn_tunnels=%d netflow_exporters=%d diagnoses=%d",
		label, ndmStr(item, "namespace"), ndmStr(item, "integration"),
		ndmLen(item, "devices"), ndmLen(item, "interfaces"), ndmLen(item, "ip_addresses"),
		ndmLen(item, "links"), ndmLen(item, "vpn_tunnels"), ndmLen(item, "netflow_exporters"),
		ndmLen(item, "diagnoses"))
	for _, d := range ndmList(item, "devices") {
		log.Printf("   device id=%s ip=%s name=%s status=%v profile=%s",
			ndmStr(d, "id"), ndmStr(d, "ip_address"), ndmStr(d, "name"), d["status"], ndmStr(d, "profile"))
	}
}

// ndmLogConfig reports one NCMPayload: which devices had a config collected.
func ndmLogConfig(label string, item map[string]any) {
	log.Printf("[%s] namespace=%s agent=%s configs=%d inventories=%d",
		label, ndmStr(item, "namespace"), ndmStr(item, "agent_hostname"),
		ndmLen(item, "configs"), ndmLen(item, "inventories"))
	for _, cfg := range ndmList(item, "configs") {
		content, _ := cfg["content"].(string)
		log.Printf("   config device=%s ip=%s type=%s source=%s %d B",
			ndmStr(cfg, "device_id"), ndmStr(cfg, "device_ip"),
			ndmStr(cfg, "config_type"), ndmStr(cfg, "config_source"), len(content))
	}
}

// ndmLogTrap reports one {"trap":{…}} envelope: OID, name and the sending
// device, which the formatter puts in ddtags as snmp_device:<ip>.
func ndmLogTrap(label string, item map[string]any) {
	trap, _ := item["trap"].(map[string]any)
	if trap == nil {
		log.Printf("[%s] no trap object, keys: %s", label, ndmKeys(item))
		return
	}
	log.Printf("[%s] oid=%s name=%s mib=%s device=%s uptime=%v variables=%d ddtags=%q",
		label, ndmStr(trap, "snmpTrapOID"), ndmStr(trap, "snmpTrapName"), ndmStr(trap, "snmpTrapMIB"),
		ndmTag(ndmStr(trap, "ddtags"), "snmp_device"), trap["uptime"],
		ndmLen(trap, "variables"), ndmStr(trap, "ddtags"))
}

// ndmItems splits a JSON array into its elements. A single object, which the
// event platform never sends but a curl might, becomes a one-element batch.
func ndmItems(body []byte) ([]json.RawMessage, error) {
	trimmed := strings.TrimSpace(string(body))
	if strings.HasPrefix(trimmed, "{") {
		return []json.RawMessage{json.RawMessage(trimmed)}, nil
	}
	var items []json.RawMessage
	if err := json.Unmarshal(body, &items); err != nil {
		return nil, err
	}
	return items, nil
}

// ndmUnmarshal decodes a batch into a typed slice, accepting a bare object
// the same way ndmItems does.
func ndmUnmarshal(body []byte, out any) error {
	trimmed := strings.TrimSpace(string(body))
	if strings.HasPrefix(trimmed, "{") {
		trimmed = "[" + trimmed + "]"
	}
	return json.Unmarshal([]byte(trimmed), out)
}

func ndmStr(m map[string]any, key string) string {
	s, _ := m[key].(string)
	return s
}

func ndmLen(m map[string]any, key string) int {
	arr, _ := m[key].([]any)
	return len(arr)
}

func ndmList(m map[string]any, key string) []map[string]any {
	arr, _ := m[key].([]any)
	out := make([]map[string]any, 0, len(arr))
	for _, v := range arr {
		if obj, ok := v.(map[string]any); ok {
			out = append(out, obj)
		}
	}
	return out
}

func ndmKeys(m map[string]any) string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return strings.Join(keys, ", ")
}

// ndmTag pulls key:value out of a comma-separated ddtags string.
func ndmTag(ddtags, key string) string {
	for _, t := range strings.Split(ddtags, ",") {
		if v, ok := strings.CutPrefix(t, key+":"); ok {
			return v
		}
	}
	return ""
}

// ndmCounts renders a name->count map as name=n,name=n in stable order.
func ndmCounts(m map[string]int) string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, k+"="+strconv.Itoa(m[k]))
	}
	return strings.Join(parts, ",")
}
