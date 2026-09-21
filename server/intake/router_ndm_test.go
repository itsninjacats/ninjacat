package intake

import (
	"encoding/json"
	"strings"
	"testing"

	netflowpayload "github.com/DataDog/datadog-agent/comp/netflow/payload"
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
