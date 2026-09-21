package intake

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sort"
	"strconv"
	"strings"

	netflowpayload "github.com/DataDog/datadog-agent/comp/netflow/payload"
	netpathpayload "github.com/DataDog/datadog-agent/pkg/networkpath/payload"
	"github.com/gin-gonic/gin"
)

// Network Devices — FOUR hosts, one product, all JSON (docs §1.5).
//
//	ndm-intake.<site>         /api/v2/ndm        []metadata.NetworkDevicesMetadata
//	ndm-intake.<site>         /api/v2/ndmconfig  []report.NCMPayload
//	snmp-traps-intake.<site>  /api/v2/ndmtraps   []map[string]any, {"trap":{…}}
//	ndmflow-intake.<site>     /api/v2/ndmflow    []payload.FlowPayload
//	netpath-intake.<site>     /api/v2/netpath    []payload.NetworkPath
//
//	config: none — event platform, see router_containers.go.
//
// They share a router because they are one product from the operator's point
// of view, even though Datadog gave each its own hostname. The host switch in
// routes.go sends all four here.
//
// Flow and netpath types live in their own Go modules under datadog-agent and
// are imported as is. The other three are in packages that pull the whole
// agent, so they stay []json.RawMessage, one element per object, byte for
// byte; the logs read them as maps, but that view is never what is kept.
// Every track is a JSON array; the event platform batches into one.
//
// Nothing is stored yet. Each handler ends with its track's complete batch in
// a variable of its own, so storage can see per route what arrives.
func (a *Server) routeNDM(g *gin.RouterGroup) {
	g.POST("/api/v2/ndm", a.handleNDMMetadata)     // device metadata and topology
	g.POST("/api/v2/ndmconfig", a.handleNDMConfig) // device configuration backups
	g.POST("/api/v2/ndmtraps", a.handleNDMTraps)   // SNMP traps
	g.POST("/api/v2/ndmflow", a.handleNDMFlow)     // NetFlow / sFlow / IPFIX
	g.POST("/api/v2/netpath", a.handleNetpath)     // traceroute results, JSON
}

// handleNDMMetadata — /api/v2/ndm, []metadata.NetworkDevicesMetadata
// (pkg/networkdevice/metadata).
func (a *Server) handleNDMMetadata(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	metadata, ok := ndmBatch(c, "ndm", ndmLogMetadata)
	if !ok {
		return
	}
	// metadata: one NetworkDevicesMetadata object per element — subnet,
	// namespace, integration, devices, interfaces, ip_addresses, links,
	// vpn_tunnels, netflow_exporters, diagnoses, device_oids, scan_status,
	// collect_timestamp — exactly as sent.
	_ = metadata // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// handleNDMConfig — /api/v2/ndmconfig, []report.NCMPayload.
func (a *Server) handleNDMConfig(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	configs, ok := ndmBatch(c, "ndmconfig", ndmLogConfig)
	if !ok {
		return
	}
	// configs: one NCMPayload object per element — namespace, configs
	// (device_id, device_ip, config_type, config_source, content, ...),
	// collect_timestamp — exactly as sent.
	_ = configs // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// handleNDMTraps — /api/v2/ndmtraps, []{"trap":{…}} from the snmptraps
// formatter (comp/snmptraps/formatter).
func (a *Server) handleNDMTraps(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	traps, ok := ndmBatch(c, "ndmtraps", ndmLogTrap)
	if !ok {
		return
	}
	// traps: one {"trap":{snmpTrapOID, snmpTrapName, snmpTrapMIB, uptime,
	// ddtags, variables, enterpriseOID, genericTrap, specificTrap, ...}}
	// object per element — exactly as sent.
	_ = traps // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// handleNDMFlow decodes []payload.FlowPayload from comp/netflow/payload.
func (a *Server) handleNDMFlow(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ndmflow] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	flows, err := ndmDecodeFlows(body)
	if err != nil {
		log.Printf("[ndmflow] not []FlowPayload (%v), raw follows", err)
		describe("ndmflow", c.GetHeader("Content-Type"), body)
		return
	}
	ndmLogFlows(flows, len(body))

	// The log above is a partial view; flows is the whole batch in the
	// Datadog type, uint64 counters intact and AdditionalFields restored (see
	// ndmDecodeFlows).
	_ = flows // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// handleNetpath decodes []payload.NetworkPath from pkg/networkpath/payload.
func (a *Server) handleNetpath(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[netpath] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	var paths []netpathpayload.NetworkPath
	if err := ndmUnmarshal(body, &paths); err != nil {
		log.Printf("[netpath] not []NetworkPath (%v), raw follows", err)
		describe("netpath", c.GetHeader("Content-Type"), body)
		return
	}
	log.Printf("[netpath] %d paths, %d B", len(paths), len(body))
	for _, p := range paths {
		ndmLogPath(p)
	}

	// The log above is a partial view; paths is the whole batch in the
	// Datadog type: every run and hop of every traceroute, the e2e probe
	// with all its RTTs, tags, and the test identifiers.
	_ = paths // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ndmBatch reads the body, splits the JSON array and logs each element
// through logItem. It returns the elements verbatim: for the tracks without an
// importable type, []json.RawMessage is the fullest form there is. The map the
// logger sees is a read-only view built per element and thrown away.
func ndmBatch(c *gin.Context, label string, logItem func(label string, item map[string]any)) ([]json.RawMessage, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return nil, false
	}
	items, err := ndmItems(body)
	if err != nil {
		log.Printf("[%s] not a JSON array (%v), raw follows", label, err)
		describe(label, c.GetHeader("Content-Type"), body)
		return nil, false
	}
	log.Printf("[%s] %d entries, %d B", label, len(items), len(body))
	for i, raw := range items {
		var item map[string]any
		if err := ndmDecodeNumbers(raw, &item); err != nil {
			log.Printf("[%s] entry %d: %v", label, i, err)
			continue
		}
		logItem(label, item)
	}
	return items, true
}

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
