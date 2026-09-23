package intake

import (
	"encoding/json"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

	netflowpayload "github.com/DataDog/datadog-agent/comp/netflow/payload"
	networkpayload "github.com/DataDog/datadog-agent/pkg/network/payload"
	netpathpayload "github.com/DataDog/datadog-agent/pkg/networkpath/payload"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The agent serialises flows with FlowPayload.MarshalJSON, which spreads
// AdditionalFields over the root object. Decoding must put them back, and
// must not lose 64-bit precision on the way.

func TestNDMDecodeFlowsRestoresAdditionalFields(t *testing.T) {
	sent := []netflowpayload.FlowPayload{{
		FlushTimestamp: 1758326400123,
		FlowType:       "netflow9",
		Bytes:          1 << 63, // beyond float64's exact range
		Packets:        9007199254740993,
		IPProtocol:     "TCP",
		EtherType:      "IPv4",
		TCPFlags:       []string{"SYN", "ACK"},
		Device:         netflowpayload.Device{Namespace: "default"},
		Exporter:       netflowpayload.Exporter{IP: "10.0.0.1"},
		AdditionalFields: netflowpayload.AdditionalFields{
			"bgp_next_hop": "10.0.0.254",
			"vlan_id":      uint64(9007199254740993),
			"custom_obj":   map[string]any{"a": "b"},
		},
	}, {
		FlowType:   "sflow5",
		IPProtocol: "UDP",
	}}
	body, err := json.Marshal(sent)
	if err != nil {
		t.Fatal(err)
	}
	if fields := string(body); !strings.Contains(fields, `"bgp_next_hop"`) || strings.Contains(fields, `"additional_fields"`) {
		t.Fatalf("the Datadog marshaller no longer flattens additional fields: %s", body)
	}

	flows, err := ndmDecodeFlows(body)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(flows) != 2 {
		t.Fatalf("got %d flows, want 2", len(flows))
	}

	f := flows[0]
	if f.Bytes != 1<<63 || f.Packets != 9007199254740993 || f.FlushTimestamp != 1758326400123 {
		t.Errorf("64-bit counters lost precision: bytes=%d packets=%d flush=%d", f.Bytes, f.Packets, f.FlushTimestamp)
	}
	if f.EtherType != "IPv4" || len(f.TCPFlags) != 2 {
		t.Errorf("omit-empty fields not decoded: %+v", f)
	}
	if got := f.AdditionalFields["bgp_next_hop"]; got != "10.0.0.254" {
		t.Errorf("bgp_next_hop = %#v, want the flattened value back", got)
	}
	if got, ok := f.AdditionalFields["vlan_id"].(json.Number); !ok || got.String() != "9007199254740993" {
		t.Errorf("vlan_id = %#v, want json.Number 9007199254740993 (no float64 round trip)", f.AdditionalFields["vlan_id"])
	}
	if obj, ok := f.AdditionalFields["custom_obj"].(map[string]any); !ok || obj["a"] != "b" {
		t.Errorf("custom_obj = %#v, want the nested object back", f.AdditionalFields["custom_obj"])
	}
	for key := range f.AdditionalFields {
		if ndmFlowKnownKeys[key] {
			t.Errorf("struct field %q leaked into AdditionalFields", key)
		}
	}
	if flows[1].AdditionalFields != nil {
		t.Errorf("a flow without extra fields must keep AdditionalFields nil, got %v", flows[1].AdditionalFields)
	}
}

func TestNDMFlowKnownKeysFollowTheMarshaller(t *testing.T) {
	for _, key := range []string{"flush_timestamp", "type", "bytes", "packets", "source", "destination",
		"ingress", "egress", "next_hop", "ether_type", "tcp_flags", "host", "device", "exporter"} {
		if !ndmFlowKnownKeys[key] {
			t.Errorf("%q is a FlowPayload field but is not in the known set", key)
		}
	}
	if ndmFlowKnownKeys["additional_fields"] {
		t.Errorf("additional_fields is never written by the marshaller and must be handled on its own")
	}
}

func TestNDMDecodeFlowsAcceptsLiteralAdditionalFields(t *testing.T) {
	// A hand-made request may spell the map out; its numbers must still not
	// pass through float64.
	body := []byte(`{"type":"netflow9","additional_fields":{"n":9007199254740993},"extra":true}`)
	flows, err := ndmDecodeFlows(body)
	if err != nil {
		t.Fatal(err)
	}
	if got, ok := flows[0].AdditionalFields["n"].(json.Number); !ok || got.String() != "9007199254740993" {
		t.Errorf("n = %#v, want json.Number", flows[0].AdditionalFields["n"])
	}
	if flows[0].AdditionalFields["extra"] != true {
		t.Errorf("root extra key lost: %v", flows[0].AdditionalFields)
	}
}

// ---------------------------------------------------------------------------
// Conversion function unit tests — built from the wire shapes documented in
// the NDM gap report, covering nil sub-messages, absent-vs-zero, multi-valued
// tags and unknown keys, per table.
// ---------------------------------------------------------------------------

// DeviceStatus travels as an int32 on the wire (1=Reachable, 2=Unreachable);
// anything else — including an absent field, which decodes to an empty
// json.Number — must read as "unknown", never a guessed reachable state.
func TestNdmDeviceStatusName(t *testing.T) {
	cases := []struct {
		name string
		in   json.Number
		want string
	}{
		{"reachable", json.Number("1"), "reachable"},
		{"unreachable", json.Number("2"), "unreachable"},
		{"absent field decodes to empty json.Number", json.Number(""), "unknown"},
		{"undefined wire value", json.Number("99"), "unknown"},
		{"garbage", json.Number("not-a-number"), "unknown"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := ndmDeviceStatusName(tc.in); got != tc.want {
				t.Errorf("ndmDeviceStatusName(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

// ndmBoolPtr is the only place *bool (is_physical/meraki_enabled on the wire)
// becomes the *uint8 a Nullable(UInt8) column wants — nil must stay nil, not
// collapse into false, which is why InterfaceMetadata declares *bool at all.
func TestNdmBoolPtr(t *testing.T) {
	trueVal, falseVal := true, false
	cases := []struct {
		name string
		in   *bool
		want *uint8
	}{
		{"not reported", nil, nil},
		{"explicit true", &trueVal, ptrUint8(1)},
		{"explicit false", &falseVal, ptrUint8(0)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := ndmBoolPtr(tc.in)
			switch {
			case tc.want == nil && got != nil:
				t.Errorf("got %v, want nil", *got)
			case tc.want != nil && got == nil:
				t.Errorf("got nil, want %v", *tc.want)
			case tc.want != nil && got != nil && *got != *tc.want:
				t.Errorf("got %v, want %v", *got, *tc.want)
			}
		})
	}
}

// A trap with no ddtags at all must not produce one bogus empty-string tag:
// strings.Split("", ",") returns []string{""}, which tagsToMultiMap would
// turn into a tag with an empty key.
func TestNdmSplitTags(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want []string
	}{
		{"empty", "", nil},
		{"one tag", "env:prod", []string{"env:prod"}},
		{"multiple", "env:prod,namespace:default", []string{"env:prod", "namespace:default"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := ndmSplitTags(tc.in)
			if len(got) != len(tc.want) {
				t.Fatalf("got %v, want %v", got, tc.want)
			}
			for i := range got {
				if got[i] != tc.want[i] {
					t.Errorf("got %v, want %v", got, tc.want)
				}
			}
		})
	}
}

// A key neither ndmMetadataPayload nor any local mirror struct declares must
// not be dropped — it lands in extra as raw JSON text, keyed by name, so a
// newer agent's additions stay visible instead of silently vanishing.
func TestNdmExtraTopLevelKeepsUndeclaredKeys(t *testing.T) {
	known := map[string]bool{"namespace": true, "devices": true}
	raw := json.RawMessage(`{"namespace":"default","devices":[],"future_field":{"nested":true},"count":3}`)

	extra := ndmExtraTopLevel(raw, known)
	if len(extra) != 2 {
		t.Fatalf("extra: got %d keys, want 2 (future_field, count): %v", len(extra), extra)
	}
	if extra["future_field"] != `{"nested":true}` {
		t.Errorf("future_field: got %q", extra["future_field"])
	}
	if extra["count"] != "3" {
		t.Errorf("count: got %q", extra["count"])
	}
	if _, ok := extra["namespace"]; ok {
		t.Errorf("a declared key leaked into extra")
	}
}

// The six "remaining lists" cover shapes both with and without a device_id:
// links carry neither, vpn_tunnels carry both, and scan_status is a single
// OPTIONAL object rather than a list at all — a nil DeviceScanStatus must
// produce zero rows, not a row with empty fields (the nil-sub-message case).
func TestNdmMetadataObjectRows(t *testing.T) {
	ts := time.Unix(1700000000, 0).UTC()
	extra := map[string]string{"x": "1"}

	payload := ndmMetadataPayload{
		Namespace: "default",
		Links:     []json.RawMessage{json.RawMessage(`{"id":"link1","source_type":"lldp"}`)},
		VPNTunnels: []json.RawMessage{
			json.RawMessage(`{"device_id":"dev1","interface_id":"dev1:1","status":"up"}`),
		},
	}
	rows := ndmMetadataObjectRows("test", payload, ts, extra)
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2 — no scan_status was sent, so it must not appear", len(rows))
	}

	byKind := map[string]storage.NDMMetadataObjectRow{}
	for _, r := range rows {
		byKind[r.Kind] = r
	}

	link, ok := byKind["link"]
	if !ok {
		t.Fatal("no link row")
	}
	if link.DeviceID != "" {
		t.Errorf("link device_id: got %q, want empty — links carry no device_id on the wire", link.DeviceID)
	}
	if link.Object != `{"id":"link1","source_type":"lldp"}` {
		t.Errorf("link object: got %q, want the element verbatim", link.Object)
	}

	tunnel, ok := byKind["vpn_tunnel"]
	if !ok {
		t.Fatal("no vpn_tunnel row")
	}
	if tunnel.DeviceID != "dev1" || tunnel.InterfaceID != "dev1:1" {
		t.Errorf("vpn_tunnel ids: got device=%q interface=%q", tunnel.DeviceID, tunnel.InterfaceID)
	}
	if tunnel.Namespace != "default" || !tunnel.CollectTimestamp.Equal(ts) {
		t.Errorf("payload-level fields did not propagate: namespace=%q ts=%v", tunnel.Namespace, tunnel.CollectTimestamp)
	}

	// scan_status present on the wire must produce exactly one row.
	payload.DeviceScanStatus = json.RawMessage(`{"device_id":"dev1","scan_status":"completed"}`)
	rows = ndmMetadataObjectRows("test", payload, ts, extra)
	var sawScanStatus bool
	for _, r := range rows {
		if r.Kind == "scan_status" {
			sawScanStatus = true
			if r.DeviceID != "dev1" {
				t.Errorf("scan_status device_id: got %q", r.DeviceID)
			}
		}
	}
	if !sawScanStatus {
		t.Error("scan_status present on the wire produced no row")
	}

	// A malformed element (not even an object) must still keep its raw bytes
	// rather than aborting the whole batch.
	payload.Diagnoses = []json.RawMessage{json.RawMessage(`"not-an-object"`)}
	rows = ndmMetadataObjectRows("test", payload, ts, extra)
	var diag *storage.NDMMetadataObjectRow
	for i := range rows {
		if rows[i].Kind == "diagnosis" {
			diag = &rows[i]
		}
	}
	if diag == nil {
		t.Fatal("no diagnosis row for a malformed element")
	}
	if diag.Object != `"not-an-object"` {
		t.Errorf("diagnosis object: got %q, want the raw bytes preserved", diag.Object)
	}
}

func TestNdmIPString(t *testing.T) {
	if got := ndmIPString(nil); got != "" {
		t.Errorf("nil IP: got %q, want empty, not Go's \"<nil>\"", got)
	}
	if got := ndmIPString(net.ParseIP("10.0.0.1")); got != "10.0.0.1" {
		t.Errorf("got %q", got)
	}
}

// Source.Via is *payload.Via: nil on the wire means "route unresolved" and
// must not become a Nullable(String) with an empty value.
func TestNdmNetworkPathRowNilVia(t *testing.T) {
	p := netpathpayload.NetworkPath{
		Timestamp: 1758326400123,
		Source:    netpathpayload.NetworkPathSource{Hostname: "h1"},
	}
	row := ndmNetworkPathRow("test", p)
	if row.SourceViaSubnetAlias != nil || row.SourceViaInterfaceHardwareAddr != nil {
		t.Errorf("nil Via produced non-nil via columns: alias=%v hw=%v", row.SourceViaSubnetAlias, row.SourceViaInterfaceHardwareAddr)
	}
	if row.Timestamp.UnixMilli() != 1758326400123 {
		t.Errorf("timestamp: got %v", row.Timestamp)
	}
}

// Via present but its Subnet/Interface fields unresolved (empty, non-pointer
// strings on the wire) must NOT collapse to the same NULL as Via absent
// entirely — that distinction is the whole reason the columns are Nullable.
func TestNdmNetworkPathRowViaPresentButUnresolved(t *testing.T) {
	p := netpathpayload.NetworkPath{
		Source: netpathpayload.NetworkPathSource{
			Hostname: "h1",
			Via:      &networkpayload.Via{}, // present, but Subnet.Alias/Interface.HardwareAddr both empty
		},
	}
	row := ndmNetworkPathRow("test", p)
	if row.SourceViaSubnetAlias == nil {
		t.Error("via present with empty alias: got nil, want a non-nil pointer to \"\"")
	} else if *row.SourceViaSubnetAlias != "" {
		t.Errorf("via subnet alias: got %q, want empty string", *row.SourceViaSubnetAlias)
	}
	if row.SourceViaInterfaceHardwareAddr == nil {
		t.Error("via present with empty hardware_addr: got nil, want a non-nil pointer to \"\"")
	} else if *row.SourceViaInterfaceHardwareAddr != "" {
		t.Errorf("via hardware_addr: got %q, want empty string", *row.SourceViaInterfaceHardwareAddr)
	}
}

// Runs and hops become parallel Array(Array(...)) columns keyed by run index
// — this pins the nesting, an unresponsive hop's absent IP/RTT, and a hop's
// own multi-valued reverse_dns, not just the flat scalar fields.
func TestNdmNetworkPathRowRunsAndHops(t *testing.T) {
	p := netpathpayload.NetworkPath{
		Traceroute: netpathpayload.Traceroute{
			Runs: []netpathpayload.TracerouteRun{
				{
					RunID:  "run1",
					Source: netpathpayload.TracerouteSource{IPAddress: net.ParseIP("10.0.0.1"), Port: 1234},
					Destination: netpathpayload.TracerouteDestination{
						IPAddress: net.ParseIP("10.0.0.2"), Port: 443, ReverseDNS: []string{"dest.example.com"},
					},
					Hops: []netpathpayload.TracerouteHop{
						{TTL: 1, IPAddress: net.ParseIP("10.0.0.254"), Reachable: true, RTT: 1.5},
						// An unresponsive hop: no IP, never measured — RTT stays 0, not a
						// value that would be confused with "measured at 0ms".
						{TTL: 2, IPAddress: nil, Reachable: false},
					},
				},
				{
					RunID: "run2",
					Hops: []netpathpayload.TracerouteHop{
						{TTL: 1, IPAddress: net.ParseIP("10.0.0.253"), Reachable: true, ReverseDNS: []string{"a.example.com", "b.example.com"}},
					},
				},
			},
		},
	}
	row := ndmNetworkPathRow("test", p)

	if len(row.RunIDs) != 2 || row.RunIDs[0] != "run1" || row.RunIDs[1] != "run2" {
		t.Fatalf("run_ids: got %v", row.RunIDs)
	}
	if row.RunSourceIPs[0] != "10.0.0.1" || row.RunDestinationIPs[0] != "10.0.0.2" {
		t.Errorf("run 0 endpoints: got src=%q dst=%q", row.RunSourceIPs[0], row.RunDestinationIPs[0])
	}
	if len(row.RunDestinationReverseDNS[0]) != 1 || row.RunDestinationReverseDNS[0][0] != "dest.example.com" {
		t.Errorf("run 0 destination reverse dns: got %v", row.RunDestinationReverseDNS[0])
	}
	if len(row.HopTTLs) != 2 {
		t.Fatalf("hop_ttls: got %d runs, want 2", len(row.HopTTLs))
	}
	if len(row.HopTTLs[0]) != 2 || row.HopTTLs[0][1] != 2 {
		t.Errorf("run 0 hop ttls: got %v", row.HopTTLs[0])
	}
	if row.HopIPs[0][1] != "" {
		t.Errorf("run 0 hop 1 (unresponsive) ip: got %q, want empty not <nil>", row.HopIPs[0][1])
	}
	if row.HopReachable[0][0] != 1 || row.HopReachable[0][1] != 0 {
		t.Errorf("run 0 reachability: got %v", row.HopReachable[0])
	}
	if len(row.HopReverseDNS[1][0]) != 2 {
		t.Errorf("run 1 hop 0 reverse dns: got %v, want 2 names", row.HopReverseDNS[1][0])
	}
}

// v1 traps carry enterpriseOID/genericTrap/specificTrap; v2/v3 traps do not.
// *int32 is what keeps "not a v1 trap" distinct from "generic trap 0".
func TestNdmTrapRowV1WithEnterpriseOID(t *testing.T) {
	trapRaw := json.RawMessage(`{"ddsource":"snmp-traps","ddtags":"namespace:default,snmp_device:10.0.0.5",` +
		`"timestamp":1700000200,"uptime":123456,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.3",` +
		`"enterpriseOID":"1.3.6.1.4.1.9","genericTrap":2,"specificTrap":0,` +
		`"variables":[{"oid":"1.3.6.1.2.1.2.2.1.1","type":"Integer","value":1}],` +
		`"customEnrichedKey":"resolved-value"}`)
	var trap ndmTrapJSON
	if err := json.Unmarshal(trapRaw, &trap); err != nil {
		t.Fatal(err)
	}
	row := ndmTrapRow("test", trapRaw, trap)

	if row.Device != "10.0.0.5" {
		t.Errorf("device: got %q, want the snmp_device ddtag", row.Device)
	}
	if row.EnterpriseOID != "1.3.6.1.4.1.9" {
		t.Errorf("enterprise_oid: got %q", row.EnterpriseOID)
	}
	if row.GenericTrap == nil || *row.GenericTrap != 2 {
		t.Errorf("generic_trap: got %v, want 2", row.GenericTrap)
	}
	if row.SpecificTrap == nil || *row.SpecificTrap != 0 {
		t.Errorf("specific_trap: got %v, want 0 (present, not absent)", row.SpecificTrap)
	}
	if len(row.VarOIDs) != 1 || row.VarOIDs[0] != "1.3.6.1.2.1.2.2.1.1" || row.VarValues[0] != "1" {
		t.Errorf("variables: oids=%v values=%v", row.VarOIDs, row.VarValues)
	}
	if row.Enriched["customEnrichedKey"] != `"resolved-value"` {
		t.Errorf("enriched: got %v", row.Enriched)
	}
}

// trap.Timestamp is set by the agent from time.Now().UnixMilli()
// (comp/snmptraps/listener/impl/listener.go) and copied verbatim onto the
// wire by FormatPacket (comp/snmptraps/formatter/impl) — a 13-digit
// milliseconds epoch, unlike ndm/ndmconfig's collect_timestamp which really
// is seconds. Decoding it with time.Unix(x, 0) instead of time.UnixMilli(x)
// would land every trap about 1000x too far in the future.
func TestNdmTrapRowTimestampIsMilliseconds(t *testing.T) {
	trapRaw := json.RawMessage(`{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.7",` +
		`"timestamp":1758326400123,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.4"}`)
	var trap ndmTrapJSON
	if err := json.Unmarshal(trapRaw, &trap); err != nil {
		t.Fatal(err)
	}
	row := ndmTrapRow("test", trapRaw, trap)
	want := time.UnixMilli(1758326400123).UTC()
	if !row.Timestamp.Equal(want) {
		t.Errorf("timestamp: got %v, want %v (treating 1758326400123 as seconds would land in the year 57708)", row.Timestamp, want)
	}
}

// A v2/v3 trap has no genericTrap/specificTrap/enterpriseOID at all — the
// three must come back nil/empty, not a guessed 0.
func TestNdmTrapRowV2HasNoV1Fields(t *testing.T) {
	trapRaw := json.RawMessage(`{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.6",` +
		`"timestamp":1700000300,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.4"}`)
	var trap ndmTrapJSON
	if err := json.Unmarshal(trapRaw, &trap); err != nil {
		t.Fatal(err)
	}
	row := ndmTrapRow("test", trapRaw, trap)
	if row.GenericTrap != nil || row.SpecificTrap != nil {
		t.Errorf("v2 trap: got generic=%v specific=%v, want both nil", row.GenericTrap, row.SpecificTrap)
	}
	if row.EnterpriseOID != "" {
		t.Errorf("enterprise_oid: got %q, want empty", row.EnterpriseOID)
	}
}

// AdditionalFields values are re-marshalled to JSON text so a large counter
// (json.Number, from ndmDecodeFlows) survives the Map(String,String) column
// without rounding through float64.
func TestNdmFlowRowAdditionalFieldsAsJSONText(t *testing.T) {
	f := netflowpayload.FlowPayload{
		FlowType: "netflow9",
		Source:   netflowpayload.Endpoint{IP: "10.0.0.2", Port: "443"},
		AdditionalFields: netflowpayload.AdditionalFields{
			"vlan_id": json.Number("9007199254740993"),
			"note":    "hello",
		},
	}
	row := ndmFlowRow("test", f)
	if row.AdditionalFields["vlan_id"] != "9007199254740993" {
		t.Errorf("vlan_id: got %q, want the exact digits, no float64 rounding", row.AdditionalFields["vlan_id"])
	}
	if row.AdditionalFields["note"] != `"hello"` {
		t.Errorf("note: got %q", row.AdditionalFields["note"])
	}
}

func ptrUint8(v uint8) *uint8 { return &v }
func ptrInt32(v int32) *int32 { return &v }

// ---------------------------------------------------------------------------
// Handler tests — real requests through the real engine, asserting on the
// rows that reached storage. See TestHandleLogsStoresEveryItem in
// testserver_test.go for what each of these pins and why.
// ---------------------------------------------------------------------------

// One realistic /api/v2/ndm payload exercising every NDM-metadata table at
// once: a device with a multi-valued id_tags key, an interface with an
// explicit false (not absent) meraki_enabled, an ip_address, a vpn_tunnel
// (has device_id/interface_id) and a link (has neither), and an undeclared
// top-level payload key that must survive in extra.
func TestHandleNDMMetadataStoresDevicesInterfacesIPsAndObjects(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	body := []byte(`[{
		"namespace": "default",
		"subnet": "10.0.0.0/24",
		"integration": "snmp",
		"collect_timestamp": 1700000000,
		"devices": [{
			"id": "default:10.0.0.1",
			"id_tags": ["device_ip:10.0.0.1", "device_ip:10.0.0.1-alt"],
			"ip_address": "10.0.0.1",
			"status": 1,
			"ping_status": 2,
			"name": "router1",
			"vendor": "cisco",
			"device_type": "router"
		}],
		"interfaces": [{
			"device_id": "default:10.0.0.1",
			"index": 1,
			"name": "GigabitEthernet0/1",
			"admin_status": 1,
			"oper_status": 1,
			"is_physical": true,
			"meraki_enabled": false
		}],
		"ip_addresses": [{
			"interface_id": "default:10.0.0.1:1",
			"ip_address": "10.0.0.1",
			"prefixlen": 24
		}],
		"vpn_tunnels": [{"device_id":"default:10.0.0.1","interface_id":"default:10.0.0.1:1","status":"up"}],
		"links": [{"id":"link1","source_type":"lldp"}],
		"custom_experimental_field": "not-yet-documented"
	}]`)

	if w := post(t, e, "/api/v2/ndm", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	devices := Rows[storage.NDMDeviceRow](node)
	if len(devices) != 1 {
		t.Fatalf("devices: got %d, want 1", len(devices))
	}
	d := devices[0]
	if d.TenantID != testTenant {
		t.Errorf("tenant: got %q", d.TenantID)
	}
	if d.Status != "reachable" || d.PingStatus != "unreachable" {
		t.Errorf("status/ping_status: got %q/%q", d.Status, d.PingStatus)
	}
	if got := d.IDTags["device_ip"]; len(got) != 2 || got[0] != "10.0.0.1" || got[1] != "10.0.0.1-alt" {
		t.Errorf("id_tags multiset: got %v, want both values for the shared key", got)
	}
	if d.Extra["custom_experimental_field"] != `"not-yet-documented"` {
		t.Errorf("extra: got %v", d.Extra)
	}

	interfaces := Rows[storage.NDMInterfaceRow](node)
	if len(interfaces) != 1 {
		t.Fatalf("interfaces: got %d, want 1", len(interfaces))
	}
	ifc := interfaces[0]
	if ifc.IsPhysical == nil || *ifc.IsPhysical != 1 {
		t.Errorf("is_physical: got %v, want 1", ifc.IsPhysical)
	}
	if ifc.MerakiEnabled == nil || *ifc.MerakiEnabled != 0 {
		t.Errorf("meraki_enabled: got %v, want 0 (explicit false, not nil)", ifc.MerakiEnabled)
	}

	ips := Rows[storage.NDMIPAddressRow](node)
	if len(ips) != 1 || ips[0].Prefixlen != 24 {
		t.Fatalf("ip_addresses: got %+v", ips)
	}

	objects := Rows[storage.NDMMetadataObjectRow](node)
	if len(objects) != 2 {
		t.Fatalf("metadata objects: got %d, want 2 (one vpn_tunnel, one link)", len(objects))
	}
}

// agent_hostname and inventories are not in the documented NCMPayload but are
// observed on the wire (see the NDM gap report); both must land in their own
// columns, and a still-undocumented key must land in extra instead of being
// dropped.
func TestHandleNDMConfigStoresAgentHostnameAndInventories(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	body := []byte(`[{
		"namespace": "default",
		"collect_timestamp": 1700000000,
		"agent_hostname": "agent01",
		"inventories": [{"foo":"bar"}],
		"configs": [{
			"device_id": "default:10.0.0.1",
			"device_ip": "10.0.0.1",
			"config_type": "running",
			"config_source": "cli",
			"timestamp": 1700000100,
			"tags": ["env:prod", "env:staging"],
			"content": "interface Gi0/1\n no shutdown\n"
		}],
		"extra_undocumented_key": "x"
	}]`)

	if w := post(t, e, "/api/v2/ndmconfig", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.NDMDeviceConfigRow](node)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.AgentHostname != "agent01" {
		t.Errorf("agent_hostname: got %q — the report flags this as observed but undocumented", r.AgentHostname)
	}
	if r.Inventories != `[{"foo":"bar"}]` {
		t.Errorf("inventories: got %q", r.Inventories)
	}
	if r.Extra["extra_undocumented_key"] != `"x"` {
		t.Errorf("extra: got %v", r.Extra)
	}
	if got := r.Tags["env"]; len(got) != 2 || got[0] != "prod" || got[1] != "staging" {
		t.Errorf("tags multiset: got %v", got)
	}
	if r.Content != "interface Gi0/1\n no shutdown\n" {
		t.Errorf("content: got %q", r.Content)
	}
	if r.Timestamp.Unix() != 1700000100 {
		t.Errorf("config timestamp: got %v", r.Timestamp)
	}
}

// A batch mixing a v1 trap (enterprise_oid + generic/specific present) and a
// v2 trap (neither present) — the required realistic v1 case, alongside the
// wire shape that actually dominates traffic today.
func TestHandleNDMTrapsStoresV1AndV2(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	body := []byte(`[
		{"trap":{"ddsource":"snmp-traps","ddtags":"namespace:default,snmp_device:10.0.0.5",` +
		`"timestamp":1700000200,"uptime":123456,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.3",` +
		`"enterpriseOID":"1.3.6.1.4.1.9","genericTrap":2,"specificTrap":0,` +
		`"variables":[{"oid":"1.3.6.1.2.1.2.2.1.1","type":"Integer","value":1}]}},
		{"trap":{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.6",` +
		`"timestamp":1700000300,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.4"}}
	]`)

	if w := post(t, e, "/api/v2/ndmtraps", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	traps := Rows[storage.SNMPTrapRow](node)
	if len(traps) != 2 {
		t.Fatalf("got %d traps, want 2", len(traps))
	}
	v1, v2 := traps[0], traps[1]
	if v1.EnterpriseOID != "1.3.6.1.4.1.9" || v1.GenericTrap == nil || *v1.GenericTrap != 2 {
		t.Errorf("v1 trap: enterprise=%q generic=%v", v1.EnterpriseOID, v1.GenericTrap)
	}
	if v2.GenericTrap != nil || v2.SpecificTrap != nil {
		t.Errorf("v2 trap: got generic=%v specific=%v, want both nil", v2.GenericTrap, v2.SpecificTrap)
	}
}

// The netflow additional_fields case, through the real handler this time
// (TestNDMDecodeFlowsRestoresAdditionalFields above pins the decoder alone).
func TestHandleNDMFlowStoresAdditionalFields(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	sent := []netflowpayload.FlowPayload{{
		FlushTimestamp: 1758326400123,
		FlowType:       "netflow9",
		Bytes:          1 << 40,
		Packets:        1000,
		IPProtocol:     "TCP",
		Start:          1700000000,
		End:            1700000010,
		Device:         netflowpayload.Device{Namespace: "default"},
		Exporter:       netflowpayload.Exporter{IP: "10.0.0.1"},
		Source:         netflowpayload.Endpoint{IP: "10.0.0.2", Port: "443"},
		Destination:    netflowpayload.Endpoint{IP: "10.0.0.3", Port: "5000"},
		AdditionalFields: netflowpayload.AdditionalFields{
			"bgp_next_hop": "10.0.0.254",
		},
	}}
	body, err := json.Marshal(sent)
	if err != nil {
		t.Fatal(err)
	}

	if w := post(t, e, "/api/v2/ndmflow", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.NetflowFlowRow](node)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.AdditionalFields["bgp_next_hop"] != `"10.0.0.254"` {
		t.Errorf("additional_fields: got %v", r.AdditionalFields)
	}
	if r.FlushTimestamp.UnixMilli() != 1758326400123 {
		t.Errorf("flush_timestamp: got %v", r.FlushTimestamp)
	}
	if r.Start.Unix() != 1700000000 {
		t.Errorf("flow_start: got %v", r.Start)
	}
}

func TestHandleNetpathStoresRunsAndE2eProbe(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	sent := []netpathpayload.NetworkPath{{
		Timestamp:    1758326400123,
		Namespace:    "default",
		TestConfigID: "cfg1",
		Origin:       netpathpayload.PathOriginNetworkTraffic,
		Protocol:     netpathpayload.ProtocolTCP,
		Source:       netpathpayload.NetworkPathSource{Hostname: "h1"},
		Destination:  netpathpayload.NetworkPathDestination{Hostname: "h2", Port: 443},
		Traceroute: netpathpayload.Traceroute{
			Runs: []netpathpayload.TracerouteRun{{
				RunID:  "run1",
				Source: netpathpayload.TracerouteSource{IPAddress: net.ParseIP("10.0.0.1"), Port: 1234},
				Hops: []netpathpayload.TracerouteHop{
					{TTL: 1, IPAddress: net.ParseIP("10.0.0.254"), Reachable: true, RTT: 1.2},
				},
			}},
			HopCount: netpathpayload.HopCountStats{Avg: 1, Min: 1, Max: 1},
		},
		E2eProbe: netpathpayload.E2eProbe{RTTs: []float64{1.1, 1.3}, PacketsSent: 2, PacketsReceived: 2},
		Tags:     []string{"env:prod"},
	}}
	body, err := json.Marshal(sent)
	if err != nil {
		t.Fatal(err)
	}

	if w := post(t, e, "/api/v2/netpath", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.NetworkPathRow](node)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if len(r.RunIDs) != 1 || r.RunIDs[0] != "run1" {
		t.Errorf("run_ids: got %v", r.RunIDs)
	}
	if len(r.HopIPs) != 1 || len(r.HopIPs[0]) != 1 || r.HopIPs[0][0] != "10.0.0.254" {
		t.Errorf("hop_ips: got %v", r.HopIPs)
	}
	if len(r.E2eRTTs) != 2 {
		t.Errorf("e2e_rtts: got %v", r.E2eRTTs)
	}
	if got := r.Tags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("tags: got %v", r.Tags)
	}
}

// A body that is not a JSON array at all must not be silently dropped: it
// goes through storeRaw so the payload survives for later reverse-engineering
// (the intake-wide rule, intake/raw.go).
func TestHandleNDMMetadataStoresRawOnDecodeFailure(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeNDM)

	if w := post(t, e, "/api/v2/ndm", []byte(`not json`)); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("got %d raw payload rows, want 1", len(raw))
	}
	if raw[0].Intake != "ndm" || raw[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q", raw[0].Intake, raw[0].Reason)
	}
	if raw[0].Body != "not json" {
		t.Errorf("body: got %q, want the original bytes kept", raw[0].Body)
	}
}

// ---------------------------------------------------------------------------
// Integration: a real ClickHouse round trip. Proves the migration, AppendTo
// and the driver's own column types all agree — the arity tests
// (storagetest package) prove argument COUNT, only this proves TYPES.
// ---------------------------------------------------------------------------

func TestNDMTablesRoundTripThroughClickHouse(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// A real, CURRENT timestamp, not an arbitrary fixed epoch: every NDM
	// table's TTL is days, not years, and a fixed literal far enough in the
	// past reads back as zero rows with no error anywhere — see
	// TestInsertRoundTrip's comment on the same pitfall with time.Time{}.
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

	storagetest.Insert(t, conn, storage.NDMDevicesWriter, []storage.Row{storage.NDMDeviceRow{
		TenantID: "test", Namespace: "default", Subnet: "10.0.0.0/24", Integration: "snmp",
		CollectTimestamp: now,
		Extra:            map[string]string{"k": "v"},
		DeviceID:         "dev1",
		IDTags:           map[string][]string{"device_ip": {"10.0.0.1"}},
		Tags:             map[string][]string{"env": {"prod", "staging"}},
		IPAddress:        "10.0.0.1", Status: "reachable", PingStatus: "unreachable",
		Name: "router1", Vendor: "cisco", DeviceType: "router",
	}})

	storagetest.Insert(t, conn, storage.NDMInterfacesWriter, []storage.Row{storage.NDMInterfaceRow{
		TenantID: "test", Namespace: "default", CollectTimestamp: now,
		DeviceID: "dev1", Index: 1, Name: "Gi0/1",
	}})

	storagetest.Insert(t, conn, storage.NDMIPAddressesWriter, []storage.Row{storage.NDMIPAddressRow{
		TenantID: "test", Namespace: "default", CollectTimestamp: now,
		InterfaceID: "dev1:1", IPAddress: "10.0.0.1", Prefixlen: 24,
	}})

	storagetest.Insert(t, conn, storage.NDMMetadataObjectsWriter, []storage.Row{storage.NDMMetadataObjectRow{
		TenantID: "test", Namespace: "default", CollectTimestamp: now,
		Kind: "link", Object: `{"id":"link1"}`,
	}})

	storagetest.Insert(t, conn, storage.NDMDeviceConfigsWriter, []storage.Row{storage.NDMDeviceConfigRow{
		TenantID: "test", Namespace: "default", CollectTimestamp: now,
		AgentHostname: "agent01", DeviceID: "dev1", DeviceIP: "10.0.0.1",
		ConfigType: "running", ConfigSource: "cli", Timestamp: now,
		Content: "interface Gi0/1\n",
	}})

	storagetest.Insert(t, conn, storage.SNMPTrapsWriter, []storage.Row{storage.SNMPTrapRow{
		TenantID: "test", Timestamp: now,
		DDSource: "snmp-traps", Device: "10.0.0.5", SNMPTrapOID: "1.3.6.1.6.3.1.1.5.3",
		EnterpriseOID: "1.3.6.1.4.1.9", GenericTrap: ptrInt32(2), SpecificTrap: ptrInt32(0),
		VarOIDs: []string{"1.3.6.1.2.1.2.2.1.1"}, VarTypes: []string{"Integer"}, VarValues: []string{"1"},
		Raw: `{"snmpTrapOID":"1.3.6.1.6.3.1.1.5.3"}`,
	}})

	storagetest.Insert(t, conn, storage.NetflowFlowsWriter, []storage.Row{storage.NetflowFlowRow{
		TenantID: "test", FlushTimestamp: now,
		FlowType: "netflow9", Start: now, End: now.Add(10 * time.Second),
		AdditionalFields: map[string]string{"bgp_next_hop": `"10.0.0.254"`},
	}})

	storagetest.Insert(t, conn, storage.NetworkPathsWriter, []storage.Row{storage.NetworkPathRow{
		TenantID: "test", Timestamp: now,
		Namespace: "default", TestConfigID: "cfg1", Origin: "network_traffic", Protocol: "TCP",
		RunIDs: []string{"run1"}, HopTTLs: [][]int32{{1, 2}}, HopIPs: [][]string{{"10.0.0.254", ""}},
		HopReverseDNS: [][][]string{{{"a.example.com"}, nil}},
		HopRTTs:       [][]float64{{1.2, 0}}, HopReachable: [][]uint8{{1, 0}},
		E2eRTTs: []float64{1.1, 1.3},
		Tags:    map[string][]string{"env": {"prod"}},
	}})

	for _, table := range []string{
		"ndm_devices", "ndm_interfaces", "ndm_ip_addresses", "ndm_metadata_objects",
		"ndm_device_configs", "snmp_traps", "netflow_flows", "network_paths",
	} {
		if n := storagetest.Count(t, conn, table); n != 1 {
			t.Errorf("%s: got %d rows, want 1", table, n)
		}
	}

	row := storagetest.QueryRow(t, conn, "SELECT status, ping_status, tags['env'] FROM ndm_devices WHERE device_id = 'dev1'")
	if row[0] != "reachable" || row[1] != "unreachable" {
		t.Errorf("ndm_devices status/ping_status round trip: got %v/%v", row[0], row[1])
	}

	row = storagetest.QueryRow(t, conn, "SELECT hop_ttls[1][2], hop_ips[1][1], e2e_rtts[2] FROM network_paths")
	if row[1] != "10.0.0.254" {
		t.Errorf("network_paths hop_ips round trip: got %v", row[1])
	}

	row = storagetest.QueryRow(t, conn, "SELECT additional_fields['bgp_next_hop'] FROM netflow_flows")
	if row[0] != `"10.0.0.254"` {
		t.Errorf("netflow_flows additional_fields round trip: got %v", row[0])
	}

	row = storagetest.QueryRow(t, conn, "SELECT var_oids[1], enterprise_oid FROM snmp_traps")
	if row[0] != "1.3.6.1.2.1.2.2.1.1" || row[1] != "1.3.6.1.4.1.9" {
		t.Errorf("snmp_traps round trip: got %v", row)
	}
}
