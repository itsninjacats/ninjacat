package intake

import (
	"encoding/json"
	"fmt"
	"io"
	"log"
	"mime"
	"mime/multipart"
	"net/http"
	"sort"
	"strings"

	"github.com/DataDog/agent-payload/v5/cws/dumpsv1"
	"github.com/DataDog/agent-payload/v5/sbom"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/encoding/protowire"
	"google.golang.org/protobuf/proto"
)

// Security hosts: CWS activity dumps, runtime security events, CSPM
// compliance, SBOM and Sensitive Data Scanner results.
//
//	config: runtime_security_config.activity_dump.remote_storage.endpoints.logs_dd_url  (secdump)
//	        runtime_security_config.endpoints.logs_dd_url                               (secruntime, secinfo)
//	        compliance_config.endpoints.logs_dd_url                                     (compliance)
//	        sbom.dd_url / sds_result.forwarder.dd_url are event platform prefixes;
//	        see router_containers.go for why DD_SITE is the practical switch.
//
// Decoded but not stored yet. Every handler ends with the complete decoded
// payload in a local, one per variant, ready to be taken over.

// cws-intake.<site> — CWS activity dumps.
//
// gzip(multipart/form-data) with two parts, in order: "event" (JSON
// ActivityDumpHeader) and "dump" (dumpsv1.SecDump protobuf, declared as
// application/json — the agent's own bug, ignore the part header). The gzip
// covers the whole multipart body; Decompress() strips it before we get here
// and the boundary in Content-Type is untouched, so a plain multipart reader
// works.
func (a *Server) routeCWS(g *gin.RouterGroup) {
	g.POST("/api/v2/secdump", a.HandleSecDump)
}

// runtime-security-http-intake.logs.<site> — CWS runtime events and
// remediation info. Logs-pipeline envelope, JSON array.
func (a *Server) routeRuntimeSecurity(g *gin.RouterGroup) {
	g.POST("/api/v2/secruntime", a.handleSecLogsTrack("secruntime"))
	g.POST("/api/v2/secinfo", a.handleSecLogsTrack("secinfo"))
}

// cspm-intake.<site> — CSPM compliance check results. Same envelope.
func (a *Server) routeCSPM(g *gin.RouterGroup) {
	g.POST("/api/v2/compliance", a.handleSecLogsTrack("compliance"))
}

// sbom-intake.<site> — container image SBOMs, protobuf, one payload per
// request (protobuf event platform pipelines use the stream strategy).
func (a *Server) routeSBOM(g *gin.RouterGroup) {
	g.POST("/api/v2/sbom", a.HandleSBOM)
}

// sds-intake.<site> — Sensitive Data Scanner results, protobuf, one payload
// per request.
func (a *Server) routeSDS(g *gin.RouterGroup) {
	g.POST("/api/v2/sdsresult", a.HandleSDSResult)
}

// HandleSecDump accepts a CWS activity dump.
//
// The two multipart parts are decoded independently and both survive to the
// end of the handler: a broken "event" part does not lose the "dump" and
// vice versa.
func (a *Server) HandleSecDump(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	_, params, err := mime.ParseMediaType(c.GetHeader("Content-Type"))
	if err != nil || params["boundary"] == "" {
		log.Printf("[secdump] not multipart: content-type=%q err=%v", c.GetHeader("Content-Type"), err)
		return
	}

	// header: the "event" part, profile.ActivityDumpHeader as a JSON object,
	// top-level values kept as raw JSON (not importable, see secDecodeDumpHeader).
	// dump: the "dump" part, the activity dump itself.
	// Either is nil when its part was missing or did not decode.
	var (
		header map[string]json.RawMessage
		dump   *dumpsv1.SecDump
	)

	mr := multipart.NewReader(c.Request.Body, params["boundary"])
	for {
		part, err := mr.NextPart()
		if err == io.EOF {
			break
		}
		if err != nil {
			log.Printf("[secdump] multipart: %v", err)
			break
		}
		data, err := io.ReadAll(part)
		part.Close()
		if err != nil {
			log.Printf("[secdump] cannot read part %q: %v", part.FormName(), err)
			break
		}

		switch part.FormName() {
		case "event":
			header = secDecodeDumpHeader(data)
		case "dump":
			dump = secDecodeDump(data)
		default:
			log.Printf("[secdump] unexpected part %q (%d bytes)", part.FormName(), len(data))
		}
	}

	if header == nil && dump == nil {
		return
	}

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = header // "event" part: ActivityDumpHeader keys → raw JSON
	_ = dump   // "dump" part: *dumpsv1.SecDump, whole tree, unknown fields kept
}

// secDecodeDumpHeader decodes the "event" part: profile.ActivityDumpHeader,
// not importable. Known keys: host, service, ddsource, ddtags, dns_names.
// Returns nil when the part is not a JSON object.
func secDecodeDumpHeader(data []byte) map[string]json.RawMessage {
	var header map[string]json.RawMessage
	if err := json.Unmarshal(data, &header); err != nil {
		log.Printf("[secdump] event part is not a JSON object: %v (%d bytes)", err, len(data))
		return nil
	}
	log.Printf("[secdump] event: host=%s service=%s ddsource=%s keys: %s",
		secString(header["host"]), secString(header["service"]),
		secString(header["ddsource"]), secKeys(header))
	return header
}

// secDecodeDump decodes the "dump" part into dumpsv1.SecDump. Returns nil
// when the bytes do not decode or decode to an empty message.
func secDecodeDump(data []byte) *dumpsv1.SecDump {
	dump := &dumpsv1.SecDump{}
	if err := proto.Unmarshal(data, dump); err != nil {
		log.Printf("[secdump] protobuf: %v (%d bytes)", err, len(data))
		return nil
	}

	if dump.GetMetadata() == nil && len(dump.GetTree()) == 0 {
		// proto.Unmarshal fails open — see HandleContainerLifecycle.
		log.Printf("[secdump] decoded to no metadata and no tree (%d bytes) — wrong payload type?", len(data))
		return nil
	}

	// Field 7 "mounts" exists upstream but not in agent-payload v5.0.207;
	// proto.Unmarshal keeps it as an unknown field, so nothing is lost.
	md := dump.GetMetadata()
	log.Printf("[secdump] host=%s service=%s source=%s name=%s container=%s agent=%s — %d tree roots, tags=%v",
		dump.GetHost(), dump.GetService(), dump.GetSource(), md.GetName(), md.GetContainerId(),
		md.GetAgentVersion(), len(dump.GetTree()), dump.GetTags())
	return dump
}

// handleSecLogsTrack accepts a logs-pipeline batch: a JSON array of
// envelopes {message, status, timestamp, hostname, service, ddsource, ddtags}.
//
// The interesting bit sits in "message" as an escaped JSON string: for
// secruntime a BackendEvent merged textually with the easyjson event
// serializer, for compliance a compliance.CheckEvent or ResourceLog. None of
// those types is importable, so the event is kept as its top-level keys with
// raw JSON values — nothing is converted, nothing is dropped.
//
// One handler serves three tracks; the hand-off at the end is per track so
// each can grow its own tables.
func (a *Server) handleSecLogsTrack(label string) gin.HandlerFunc {
	return func(c *gin.Context) {
		defer c.JSON(http.StatusAccepted, gin.H{})

		body, err := c.GetRawData()
		if err != nil {
			log.Printf("[%s] cannot read body: %v", label, err)
			return
		}
		// The raw payload outlives every path out of this handler: a decoder
		// that fails or does not exist yet must not make the bytes disappear.
		defer func() { _ = body }()

		// envelopes[i]: the logs-pipeline envelope, top-level values as raw
		// JSON. "message" stays in it as the original escaped string.
		var envelopes []map[string]json.RawMessage
		if err := json.Unmarshal(body, &envelopes); err != nil {
			log.Printf("[%s] not a JSON array: %v", label, err)
			describe(label, c.GetHeader("Content-Type"), body)
			return
		}

		// events[i]: envelopes[i]["message"] decoded — the actual CWS or
		// compliance event, top-level values as raw JSON. nil when "message"
		// was missing or not a JSON object; the raw value is still in the
		// envelope. Index-aligned with envelopes.
		events := make([]map[string]json.RawMessage, len(envelopes))

		log.Printf("[%s] %d entries (%d bytes)", label, len(envelopes), len(body))
		for i, e := range envelopes {
			inner, err := secInnerMessage(e["message"])
			if err != nil {
				log.Printf("   host=%s service=%s ddsource=%s message: %v",
					secString(e["hostname"]), secString(e["service"]), secString(e["ddsource"]), err)
				continue
			}
			events[i] = inner
			log.Printf("   host=%s service=%s ddsource=%s keys: %s",
				secString(e["hostname"]), secString(e["service"]), secString(e["ddsource"]), secKeys(inner))
		}

		switch label {
		case "secruntime":
			// CWS runtime events (BackendEvent + easyjson event).
			// TODO(ninjacat): tables. Complete, unconverted, ready to take.
			_ = envelopes
			_ = events
		case "secinfo":
			// CWS remediation / self-info events.
			// TODO(ninjacat): tables. Complete, unconverted, ready to take.
			_ = envelopes
			_ = events
		case "compliance":
			// CSPM compliance.CheckEvent / ResourceLog.
			// TODO(ninjacat): tables. Complete, unconverted, ready to take.
			_ = envelopes
			_ = events
		default:
			log.Printf("[%s] no hand-off for this track; %d entries dropped", label, len(envelopes))
		}
	}
}

// secInnerMessage decodes the envelope's "message": either a JSON string
// holding escaped JSON, or an object already.
func secInnerMessage(raw json.RawMessage) (map[string]json.RawMessage, error) {
	if len(raw) == 0 {
		return nil, fmt.Errorf("missing")
	}
	if raw[0] == '"' {
		var s string
		if err := json.Unmarshal(raw, &s); err != nil {
			return nil, err
		}
		raw = json.RawMessage(s)
	}
	var m map[string]json.RawMessage
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("not a JSON object (%d bytes)", len(raw))
	}
	return m, nil
}

// HandleSBOM accepts a container image SBOM batch.
func (a *Server) HandleSBOM(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[sbom] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	payload := &sbom.SBOMPayload{}
	if err := proto.Unmarshal(body, payload); err != nil {
		log.Printf("[sbom] protobuf: %v (%d bytes)", err, len(body))
		return
	}

	if len(payload.GetEntities()) == 0 {
		// proto.Unmarshal fails open — see HandleContainerLifecycle.
		log.Printf("[sbom] decoded to zero entities (%d bytes) — wrong payload type?", len(body))
		return
	}

	log.Printf("[sbom] host=%s source=%s version=%d — %d entities",
		payload.GetHost(), payload.GetSource(), payload.GetVersion(), len(payload.GetEntities()))

	for _, e := range payload.GetEntities() {
		log.Printf("   %-16s id=%s status=%s inUse=%t heartbeat=%t tags=%v",
			e.GetType(), e.GetId(), e.GetStatus(), e.GetInUse(), e.GetHeartbeat(), e.GetRepoTags())
	}

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	// Each entity carries its full CycloneDX BOM (components, dependencies,
	// vulnerabilities) in GetCyclonedx(), or GetError() for a failed
	// generation. GeneratedAt / GenerationDuration are pointers: nil means
	// not provided, not epoch zero.
	_ = payload
}

// HandleSDSResult accepts Sensitive Data Scanner results.
//
// The schema is datadog/sds/sds_result.proto (SdsResultPayload), but the
// generated package pkg/proto/pbgo/sds exists only on datadog-agent main —
// no published pkg/proto version through v0.84.0-devel ships it (checked:
// pbgo/ in both v0.83.2 and v0.84.0-devel has core, dogstatsdhttp,
// languagedetection, mocks, privateactionrunner, process, procmgr, sbom,
// trace — no sds). Until it does, the raw protobuf body is the payload: kept
// whole, alongside the top-level wire layout read from it.
//
// Top-level fields per the .proto: 1 scan_source, 2 timestamp, 3 resource,
// 4 scan_results (repeated), 5 scan_stats, 6 scanner_metadata, 7 rules (map),
// 8 scanning_source, 9 rule_ids (repeated).
//
// TODO(ninjacat): once pkg/proto publishes pbgo/sds, replace secWireFields
// with proto.Unmarshal(body, &sds.SdsResultPayload{}) and hand that over
// instead of the raw body. Note for then: timestamps and counters in that
// schema are int64 — keep them int64, and treat 0 as not provided.
func (a *Server) HandleSDSResult(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[sdsresult] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	layout, err := secWireFields(body)
	if err != nil {
		log.Printf("[sdsresult] protobuf: %v (%d bytes)", err, len(body))
		return
	}
	if len(layout) == 0 {
		log.Printf("[sdsresult] no fields (%d bytes) — empty or wrong payload type?", len(body))
		return
	}

	log.Printf("[sdsresult] %d bytes, fields: %s", len(body), secWireLayoutString(layout))

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = body   // the whole SdsResultPayload, wire format, decodable once pbgo/sds exists
	_ = layout // top-level field numbers, wire types and counts, in order
}

// secWireField is one run of a top-level protobuf field as it appears on the
// wire: consecutive occurrences of the same number are one entry.
type secWireField struct {
	Num   protowire.Number
	Type  protowire.Type
	Count int
}

// secWireFields lists a protobuf message's top-level fields in wire order.
// Nested messages are not entered — without the schema they cannot be told
// apart from strings.
func secWireFields(body []byte) ([]secWireField, error) {
	var seen []secWireField

	for len(body) > 0 {
		num, typ, n := protowire.ConsumeTag(body)
		if n < 0 {
			return nil, protowire.ParseError(n)
		}
		body = body[n:]
		n = protowire.ConsumeFieldValue(num, typ, body)
		if n < 0 {
			return nil, protowire.ParseError(n)
		}
		body = body[n:]

		if len(seen) > 0 && seen[len(seen)-1].Num == num {
			seen[len(seen)-1].Count++
			continue
		}
		seen = append(seen, secWireField{num, typ, 1})
	}
	return seen, nil
}

// secWireLayoutString renders a layout as "<number>:<wire type>[xN]" entries.
func secWireLayoutString(layout []secWireField) string {
	parts := make([]string, 0, len(layout))
	for _, f := range layout {
		part := fmt.Sprintf("%d:%s", f.Num, secWireTypeName(f.Type))
		if f.Count > 1 {
			part += fmt.Sprintf("x%d", f.Count)
		}
		parts = append(parts, part)
	}
	return strings.Join(parts, " ")
}

func secWireTypeName(t protowire.Type) string {
	switch t {
	case protowire.VarintType:
		return "varint"
	case protowire.Fixed32Type:
		return "fixed32"
	case protowire.Fixed64Type:
		return "fixed64"
	case protowire.BytesType:
		return "bytes"
	default:
		return fmt.Sprintf("type%d", t)
	}
}

// secString returns a raw JSON string value, or "" when it is missing or not
// a string.
func secString(raw json.RawMessage) string {
	var s string
	if json.Unmarshal(raw, &s) != nil {
		return ""
	}
	return s
}

// secKeys returns the object's keys, sorted and comma-joined.
func secKeys(m map[string]json.RawMessage) string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return strings.Join(keys, ", ")
}
