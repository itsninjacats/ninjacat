package intake

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"mime"
	"mime/multipart"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/cws/dumpsv1"
	"github.com/DataDog/agent-payload/v5/cyclonedx_v1_4"
	"github.com/DataDog/agent-payload/v5/sbom"
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"google.golang.org/protobuf/encoding/protojson"
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
// Stored:
//
//	secdump                cws_activity_dumps (one row per request) + cws_dump_nodes
//	                        (one row per ProcessActivityNode, flattened depth-first)
//	secruntime/secinfo/compliance  security_events (one row per envelope)
//	sbom                    sbom_entities + sbom_components + sbom_vulnerabilities
//	sdsresult               raw_payloads, always — no published Go type exists yet
//	                        (see HandleSDSResult), so this track never leaves
//	                        "no_schema".
//
// See schema/migrations/0009_security.sql for the tables and
// docs/tables/security.md for the column-to-source-field mapping.

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
// vice versa. The whole request body is read up front (rather than streamed
// straight into the multipart reader) so a body that turns out not to be
// multipart at all, or that decodes to neither part, still has bytes to hand
// storeRaw — see the two early-return branches below.
func (a *Server) HandleSecDump(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[secdump] cannot read body: %v", err)
		return
	}

	_, params, err := mime.ParseMediaType(c.GetHeader("Content-Type"))
	if err != nil || params["boundary"] == "" {
		log.Printf("[secdump] not multipart: content-type=%q err=%v", c.GetHeader("Content-Type"), err)
		a.storeRaw(c, "secdump", "decode_error", "content-type is not multipart/form-data", body)
		return
	}

	// header: the "event" part, profile.ActivityDumpHeader as a JSON object,
	// top-level values kept as raw JSON (not importable, see secDecodeDumpHeader).
	// dump: the "dump" part, the activity dump itself. dumpBytes are its raw
	// protobuf bytes, kept alongside for lossless storage.
	// Either header or dump is nil when its part was missing or did not decode.
	var (
		header      map[string]json.RawMessage
		headerBytes []byte
		dump        *dumpsv1.SecDump
		dumpBytes   []byte
	)

	mr := multipart.NewReader(bytes.NewReader(body), params["boundary"])
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
			headerBytes = data
		case "dump":
			dump = secDecodeDump(data)
			dumpBytes = data
		default:
			log.Printf("[secdump] unexpected part %q (%d bytes)", part.FormName(), len(data))
		}
	}

	if header == nil && dump == nil {
		a.storeRaw(c, "secdump", "decode_error", "neither multipart part decoded", body)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	receivedAt := time.Now().UTC()
	dumpID := uuid.New()

	row := cwsDumpRow(tenant, receivedAt, dumpID, header, headerBytes, dump, dumpBytes)
	a.store(storage.CWSActivityDumpsWriter, storage.WriteCWSActivityDumps{Dumps: []storage.CWSActivityDumpRow{row}}, 1)

	if dump != nil && len(dump.GetTree()) > 0 {
		nodes := cwsFlattenTree(tenant, receivedAt, dumpID, dump.GetTree())
		a.store(storage.CWSDumpNodesWriter, storage.WriteCWSDumpNodes{Nodes: nodes}, len(nodes))
	}
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
	// proto.Unmarshal keeps it as an unknown field, and cwsDumpRow stores the
	// raw bytes below so nothing is lost.
	md := dump.GetMetadata()
	log.Printf("[secdump] host=%s service=%s source=%s name=%s container=%s agent=%s — %d tree roots, tags=%v",
		dump.GetHost(), dump.GetService(), dump.GetSource(), md.GetName(), md.GetContainerId(),
		md.GetAgentVersion(), len(dump.GetTree()), dump.GetTags())
	return dump
}

// secKnownHeaderKeys are the "event" part's documented top-level keys.
// Anything else lands in CWSActivityDumpRow.HeaderExtra.
var secKnownHeaderKeys = map[string]bool{
	"host": true, "service": true, "ddsource": true, "ddtags": true, "dns_names": true,
}

// cwsDumpRow builds the cws_activity_dumps row from whichever of header/dump
// decoded. Both may be present, either may be nil (never both, callers check
// that before calling this).
//
// headerBytes is the "event" part's raw bytes, kept in HeaderRaw regardless
// of whether it parsed as a JSON object — mirroring dumpBytes/Dump below, so
// a malformed "event" part paired with a good "dump" part still loses
// nothing (see the migration's comment on header_raw).
func cwsDumpRow(tenant string, receivedAt time.Time, dumpID uuid.UUID,
	header map[string]json.RawMessage, headerBytes []byte, dump *dumpsv1.SecDump, dumpBytes []byte) storage.CWSActivityDumpRow {

	row := storage.CWSActivityDumpRow{
		TenantID:   tenant,
		ReceivedAt: receivedAt,
		DumpID:     dumpID,
		HeaderRaw:  string(headerBytes),
		Dump:       string(dumpBytes),
	}

	if header != nil {
		row.HeaderHost = secString(header["host"])
		row.HeaderService = secString(header["service"])
		row.HeaderSource = secString(header["ddsource"])
		row.HeaderTags = tagsToMultiMap(secStringSlice(header["ddtags"]))
		if raw, ok := header["dns_names"]; ok {
			row.DNSNames = string(raw)
		}
		extra := make(map[string]string)
		for k, v := range header {
			if secKnownHeaderKeys[k] {
				continue
			}
			extra[k] = string(v)
		}
		row.HeaderExtra = extra
	}

	if dump != nil {
		row.DumpHost = dump.GetHost()
		row.DumpService = dump.GetService()
		row.DumpSource = dump.GetSource()
		row.DumpTags = tagsToMultiMap(dump.GetTags())
		row.TreeNodeCount = uint32(cwsCountTree(dump.GetTree()))

		if md := dump.GetMetadata(); md != nil {
			row.AgentVersion = md.GetAgentVersion()
			row.AgentCommit = md.GetAgentCommit()
			row.KernelVersion = md.GetKernelVersion()
			row.LinuxDistribution = md.GetLinuxDistribution()
			row.Arch = md.GetArch()
			row.MetadataName = md.GetName()
			row.ProtobufVersion = md.GetProtobufVersion()
			if md.GetDifferentiateArgs() {
				row.DifferentiateArgs = 1
			}
			row.Comm = md.GetComm()
			row.ContainerID = md.GetContainerId()
			start, end, size := md.GetStart(), md.GetEnd(), md.GetSize()
			row.Start, row.End, row.Size = &start, &end, &size
			row.Serialization = md.GetSerialization()
			row.CgroupID = md.GetCgroupId()
			row.CgroupManager = md.GetCgroupManager()
		}
	}

	return row
}

// cwsCountTree counts every node in the tree, at every depth — not just the
// roots — for CWSActivityDumpRow.TreeNodeCount.
func cwsCountTree(nodes []*dumpsv1.ProcessActivityNode) int {
	n := 0
	for _, node := range nodes {
		if node == nil {
			continue
		}
		n++
		n += cwsCountTree(node.GetChildren())
	}
	return n
}

// cwsFlattenTree walks the dump's process tree depth-first and returns one
// row per node, with NodePath/ParentPath/Depth recording where each node
// sat.
func cwsFlattenTree(tenant string, receivedAt time.Time, dumpID uuid.UUID, tree []*dumpsv1.ProcessActivityNode) []storage.CWSDumpNodeRow {
	var rows []storage.CWSDumpNodeRow
	var walk func(nodes []*dumpsv1.ProcessActivityNode, path []uint32)
	walk = func(nodes []*dumpsv1.ProcessActivityNode, path []uint32) {
		for i, n := range nodes {
			if n == nil {
				continue
			}
			nodePath := make([]uint32, len(path)+1)
			copy(nodePath, path)
			nodePath[len(path)] = uint32(i)

			rows = append(rows, cwsNodeRow(tenant, receivedAt, dumpID, n, nodePath, path))
			walk(n.GetChildren(), nodePath)
		}
	}
	walk(tree, nil)
	return rows
}

// cwsNodeRow builds one cws_dump_nodes row. The Node column holds protojson
// of n with Children cleared — proto.Clone first, so the walk in
// cwsFlattenTree still owns the original tree.
func cwsNodeRow(tenant string, receivedAt time.Time, dumpID uuid.UUID,
	n *dumpsv1.ProcessActivityNode, nodePath, parentPath []uint32) storage.CWSDumpNodeRow {

	shallow, _ := proto.Clone(n).(*dumpsv1.ProcessActivityNode)
	if shallow != nil {
		shallow.Children = nil
	}
	nodeJSON, err := protojson.Marshal(shallow)
	if err != nil {
		log.Printf("[secdump] protojson node at %v: %v", nodePath, err)
	}

	var ruleIDs []string
	for _, r := range n.GetMatchedRules() {
		ruleIDs = append(ruleIDs, r.GetRuleId())
	}

	proc := n.GetProcess()
	var filePath string
	if f := proc.GetFile(); f != nil {
		filePath = f.GetPath()
	}

	return storage.CWSDumpNodeRow{
		TenantID:   tenant,
		ReceivedAt: receivedAt,
		DumpID:     dumpID,

		NodePath:   nodePath,
		Depth:      uint16(len(nodePath)),
		ParentPath: parentPath,

		Node: string(nodeJSON),

		PID:            proc.GetPid(),
		PPID:           proc.GetPpid(),
		Comm:           proc.GetComm(),
		ContainerID:    proc.GetContainerId(),
		FilePath:       filePath,
		Args:           proc.GetArgs(),
		ImageTags:      n.GetImageTags(),
		MatchedRuleIDs: ruleIDs,
		GenerationType: n.GetGenerationType().String(),

		FilesCount:    uint32(len(n.GetFiles())),
		DNSCount:      uint32(len(n.GetDnsNames())),
		SocketsCount:  uint32(len(n.GetSockets())),
		SyscallsCount: uint32(len(n.GetSyscalls())),
	}
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
// One handler serves three tracks; each still gets its own row, tagged by
// track, in the shared security_events table.
func (a *Server) handleSecLogsTrack(label string) gin.HandlerFunc {
	return func(c *gin.Context) {
		defer c.JSON(http.StatusAccepted, gin.H{})

		body, err := c.GetRawData()
		if err != nil {
			log.Printf("[%s] cannot read body: %v", label, err)
			return
		}

		// envelopes[i]: the logs-pipeline envelope, top-level values as raw
		// JSON. "message" stays in it as the original escaped string.
		var envelopes []map[string]json.RawMessage
		if err := json.Unmarshal(body, &envelopes); err != nil {
			log.Printf("[%s] not a JSON array: %v", label, err)
			describe(label, c.GetHeader("Content-Type"), body)
			a.storeRaw(c, label, "unexpected_shape", fmt.Sprintf("top-level body is not a JSON array: %v", err), body)
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
		case "secruntime", "secinfo", "compliance":
			tenant := TenantFromContext(c)
			if tenant == "" {
				return
			}
			rows := secEventRows(tenant, time.Now().UTC(), label, envelopes, events)
			a.store(storage.SecurityEventsWriter, storage.WriteSecurityEvents{Events: rows}, len(rows))
		default:
			log.Printf("[%s] no hand-off for this track; %d entries dropped", label, len(envelopes))
		}
	}
}

// secKnownEnvelopeKeys are the logs-pipeline envelope's documented top-level
// keys. Anything else lands in SecurityEventRow.Extra.
var secKnownEnvelopeKeys = map[string]bool{
	"message": true, "status": true, "timestamp": true,
	"hostname": true, "service": true, "ddsource": true, "ddtags": true,
}

// secEventRows builds one SecurityEventRow per envelope, index-aligned with
// events (nil events[i] means the inner "message" did not decode — the row
// still gets built, with MessageDecoded left at 0).
func secEventRows(tenant string, receivedAt time.Time, track string,
	envelopes []map[string]json.RawMessage, events []map[string]json.RawMessage) []storage.SecurityEventRow {

	rows := make([]storage.SecurityEventRow, 0, len(envelopes))
	for i, e := range envelopes {
		row := storage.SecurityEventRow{
			TenantID:   tenant,
			ReceivedAt: receivedAt,
			Track:      track,
			SeqInBatch: uint32(i),
			Timestamp:  secParseTimestamp(e["timestamp"]),
			Hostname:   secString(e["hostname"]),
			Service:    secString(e["service"]),
			DDSource:   secString(e["ddsource"]),
			Status:     secString(e["status"]),
			DDTags:     tagsToMultiMap(secStringSlice(e["ddtags"])),
		}
		if raw, ok := e["message"]; ok {
			row.MessageRaw = string(raw)
		}

		extra := make(map[string]string)
		for k, v := range e {
			if secKnownEnvelopeKeys[k] {
				continue
			}
			extra[k] = string(v)
		}
		row.Extra = extra

		if ev := events[i]; ev != nil {
			row.MessageDecoded = 1
			if b, err := json.Marshal(ev); err == nil {
				row.Message = string(b)
			}
			row.RuleID = secNestedString(ev, "agent", "rule_id")
			row.PolicyName = secNestedString(ev, "agent", "policy_name")
			row.EvtName = secNestedString(ev, "evt", "name")
			row.EvtCategory = secNestedString(ev, "evt", "category")
			row.Title = secNestedString(ev, "title")
			row.EventKind = secNestedString(ev, "kind")
		}

		rows = append(rows, row)
	}
	return rows
}

// secParseTimestamp reads the envelope's "timestamp" field, which travels as
// either a millisecond epoch number or an RFC3339 string depending on
// producer. Returns nil — never a zero time.Time — when the field is absent
// or does not parse as either shape.
func secParseTimestamp(raw json.RawMessage) *time.Time {
	if len(raw) == 0 {
		return nil
	}

	var s string
	if json.Unmarshal(raw, &s) == nil {
		if t, err := time.Parse(time.RFC3339, s); err == nil {
			t = t.UTC()
			return &t
		}
		if ms, err := strconv.ParseInt(s, 10, 64); err == nil {
			t := time.UnixMilli(ms).UTC()
			return &t
		}
		return nil
	}

	var ms int64
	if json.Unmarshal(raw, &ms) == nil {
		t := time.UnixMilli(ms).UTC()
		return &t
	}
	return nil
}

// secStringSlice reads a JSON array of strings, or a single comma-separated
// string (some producers send ddtags that way) — best-effort, returns nil on
// anything else rather than guessing further.
func secStringSlice(raw json.RawMessage) []string {
	if len(raw) == 0 {
		return nil
	}
	var arr []string
	if json.Unmarshal(raw, &arr) == nil {
		return arr
	}
	var s string
	if json.Unmarshal(raw, &s) == nil && s != "" {
		return strings.Split(s, ",")
	}
	return nil
}

// secNestedString walks a chain of nested JSON objects (map[string]json.RawMessage
// at every level but the last) and returns the string at the end, or "" if
// any step is missing, not an object, or not a string.
func secNestedString(m map[string]json.RawMessage, path ...string) string {
	cur := m
	for i, p := range path {
		raw, ok := cur[p]
		if !ok {
			return ""
		}
		if i == len(path)-1 {
			return secString(raw)
		}
		var next map[string]json.RawMessage
		if json.Unmarshal(raw, &next) != nil {
			return ""
		}
		cur = next
	}
	return ""
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

	payload := &sbom.SBOMPayload{}
	if err := proto.Unmarshal(body, payload); err != nil {
		log.Printf("[sbom] protobuf: %v (%d bytes)", err, len(body))
		a.storeRaw(c, "sbom", "decode_error", err.Error(), body)
		return
	}

	if len(payload.GetEntities()) == 0 {
		// proto.Unmarshal fails open — see HandleContainerLifecycle.
		log.Printf("[sbom] decoded to zero entities (%d bytes) — wrong payload type?", len(body))
		a.storeRaw(c, "sbom", "unexpected_shape", "decoded to zero entities", body)
		return
	}

	log.Printf("[sbom] host=%s source=%s version=%d — %d entities",
		payload.GetHost(), payload.GetSource(), payload.GetVersion(), len(payload.GetEntities()))

	for _, e := range payload.GetEntities() {
		log.Printf("   %-16s id=%s status=%s inUse=%t heartbeat=%t tags=%v",
			e.GetType(), e.GetId(), e.GetStatus(), e.GetInUse(), e.GetHeartbeat(), e.GetRepoTags())
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	receivedAt := time.Now().UTC()

	entities, components, vulns := sbomRows(tenant, receivedAt, payload)
	a.store(storage.SBOMEntitiesWriter, storage.WriteSBOMEntities{Entities: entities}, len(entities))
	if len(components) > 0 {
		a.store(storage.SBOMComponentsWriter, storage.WriteSBOMComponents{Components: components}, len(components))
	}
	if len(vulns) > 0 {
		a.store(storage.SBOMVulnerabilitiesWriter, storage.WriteSBOMVulnerabilities{Vulnerabilities: vulns}, len(vulns))
	}
}

// sbomRows builds sbom_entities rows plus every sbom_components /
// sbom_vulnerabilities row their CycloneDX BOMs unpack into. Nil entities
// are skipped (proto repeated fields of pointers can carry them).
func sbomRows(tenant string, receivedAt time.Time, payload *sbom.SBOMPayload) (
	entities []storage.SBOMEntityRow, components []storage.SBOMComponentRow, vulns []storage.SBOMVulnerabilityRow) {

	for _, e := range payload.GetEntities() {
		if e == nil {
			continue
		}
		entityID := uuid.New()

		row := storage.SBOMEntityRow{
			TenantID:        tenant,
			ReceivedAt:      receivedAt,
			EntityID:        entityID,
			PayloadVersion:  payload.GetVersion(),
			Host:            payload.GetHost(),
			Type:            e.GetType().String(),
			ID:              e.GetId(),
			RepoTags:        e.GetRepoTags(),
			RepoDigests:     e.GetRepoDigests(),
			DDTags:          tagsToMultiMap(e.GetDdTags()),
			Hash:            e.GetHash(),
			Status:          e.GetStatus().String(),
			KernelVersion:   e.GetKernelVersion(),
			CPUArchitecture: e.GetCpuArchitecture(),
		}
		if e.GetInUse() {
			row.InUse = 1
		}
		if e.GetHeartbeat() {
			row.Heartbeat = 1
		}
		// Source/DdEnv are proto3 optional (pointer) fields on the PAYLOAD,
		// shared by every entity in it — nil means the agent did not set
		// them, not "".
		if payload.Source != nil {
			s := payload.GetSource()
			row.Source = &s
		}
		if payload.DdEnv != nil {
			s := payload.GetDdEnv()
			row.DdEnv = &s
		}
		if ts := e.GetGeneratedAt(); ts != nil {
			t := ts.AsTime()
			row.GeneratedAt = &t
		}
		if d := e.GetGenerationDuration(); d != nil {
			ms := d.AsDuration().Milliseconds()
			row.GenerationDurationMs = &ms
		}

		switch v := e.GetSbom().(type) {
		case *sbom.SBOMEntity_Error:
			row.Error = v.Error
		case *sbom.SBOMEntity_Cyclonedx:
			if bomJSON, err := protojson.Marshal(v.Cyclonedx); err == nil {
				row.Bom = string(bomJSON)
			} else {
				// protojson requires every string field to be valid UTF-8;
				// proto.Marshal does not, so it survives inputs protojson
				// rejects (e.g. non-UTF-8 bytes in scanned package metadata).
				// This is the entity's only backstop once that happens — the
				// flattened component/vulnerability rows below are built off
				// the already-decoded Go structs and are unaffected either way.
				log.Printf("[sbom] protojson bom for entity %s: %v — falling back to raw protobuf bytes", e.GetId(), err)
				if raw, mErr := proto.Marshal(v.Cyclonedx); mErr == nil {
					row.BomRaw = base64.StdEncoding.EncodeToString(raw)
				} else {
					log.Printf("[sbom] protobuf marshal also failed for entity %s bom: %v", e.GetId(), mErr)
				}
			}
			comps := sbomFlattenComponents(tenant, receivedAt, entityID, v.Cyclonedx.GetComponents(), "", 0)
			components = append(components, comps...)
			row.ComponentCount = uint32(len(comps))

			vs := sbomVulnerabilityRows(tenant, receivedAt, entityID, v.Cyclonedx.GetVulnerabilities())
			vulns = append(vulns, vs...)
			row.VulnerabilityCount = uint32(len(vs))
		}

		entities = append(entities, row)
	}
	return entities, components, vulns
}

// sbomFlattenComponents walks a CycloneDX component list recursively — a
// component's own sub-components get rows too, linked by ParentBomRef.
func sbomFlattenComponents(tenant string, receivedAt time.Time, entityID uuid.UUID,
	comps []*cyclonedx_v1_4.Component, parentBomRef string, depth uint16) []storage.SBOMComponentRow {

	var out []storage.SBOMComponentRow
	for _, c := range comps {
		if c == nil {
			continue
		}

		var licenses []string
		for _, lc := range c.GetLicenses() {
			switch {
			case lc.GetLicense() != nil && lc.GetLicense().GetId() != "":
				licenses = append(licenses, lc.GetLicense().GetId())
			case lc.GetLicense() != nil:
				licenses = append(licenses, lc.GetLicense().GetName())
			case lc.GetExpression() != "":
				licenses = append(licenses, lc.GetExpression())
			}
		}

		hashes := make(map[string]string, len(c.GetHashes()))
		for _, h := range c.GetHashes() {
			hashes[h.GetAlg().String()] = h.GetValue()
		}

		properties := sbomPropertiesMultiMap(c.GetProperties())

		out = append(out, storage.SBOMComponentRow{
			TenantID:     tenant,
			ReceivedAt:   receivedAt,
			EntityID:     entityID,
			BomRef:       c.GetBomRef(),
			ParentBomRef: parentBomRef,
			Depth:        depth,
			Type:         c.GetType().String(),
			Name:         c.GetName(),
			Version:      c.GetVersion(),
			// Purl/Cpe/Group/Publisher/Author/Description are all proto3
			// optional on the wire — sbomOptString keeps "never set" distinct
			// from "set to empty string" instead of collapsing both to "".
			Purl:        sbomOptString(c.Purl),
			Cpe:         sbomOptString(c.Cpe),
			Group:       sbomOptString(c.Group),
			Publisher:   sbomOptString(c.Publisher),
			Author:      sbomOptString(c.Author),
			Description: sbomOptString(c.Description),
			Scope:       c.GetScope().String(),

			Licenses:   licenses,
			Hashes:     hashes,
			Properties: properties,

			ExternalReferences: protojsonArray(c.GetExternalReferences()),
			Evidence:           protojsonArray(c.GetEvidence()),
		})

		out = append(out, sbomFlattenComponents(tenant, receivedAt, entityID, c.GetComponents(), c.GetBomRef(), depth+1)...)
	}
	return out
}

// sbomVulnerabilityRows builds one row per cyclonedx_v1_4.Vulnerability.
func sbomVulnerabilityRows(tenant string, receivedAt time.Time, entityID uuid.UUID,
	vulns []*cyclonedx_v1_4.Vulnerability) []storage.SBOMVulnerabilityRow {

	var out []storage.SBOMVulnerabilityRow
	for _, v := range vulns {
		if v == nil {
			continue
		}

		row := storage.SBOMVulnerabilityRow{
			TenantID:   tenant,
			ReceivedAt: receivedAt,
			EntityID:   entityID,
			BomRef:     v.GetBomRef(),
			ID:         v.GetId(),
			// Description/Detail/Recommendation are proto3 optional — see the
			// same note on sbomFlattenComponents above.
			Description:    sbomOptString(v.Description),
			Detail:         sbomOptString(v.Detail),
			Recommendation: sbomOptString(v.Recommendation),
			Ratings:        protojsonArray(v.GetRatings()),
			Cwes:           v.GetCwes(),
			Advisories:     protojsonArray(v.GetAdvisories()),
			Properties:     sbomPropertiesMultiMap(v.GetProperties()),
			Affects:        protojsonArray(v.GetAffects()),
		}
		if s := v.GetSource(); s != nil {
			row.SourceName = sbomOptString(s.Name)
			row.SourceURL = sbomOptString(s.Url)
		}
		if ts := v.GetCreated(); ts != nil {
			t := ts.AsTime()
			row.Created = &t
		}
		if ts := v.GetPublished(); ts != nil {
			t := ts.AsTime()
			row.Published = &t
		}
		if ts := v.GetUpdated(); ts != nil {
			t := ts.AsTime()
			row.Updated = &t
		}
		if an := v.GetAnalysis(); an != nil {
			row.AnalysisState = an.GetState().String()
			row.AnalysisJustification = an.GetJustification().String()
			row.AnalysisDetail = an.GetDetail()
			for _, r := range an.GetResponse() {
				row.AnalysisResponse = append(row.AnalysisResponse, r.String())
			}
		}
		for _, af := range v.GetAffects() {
			row.AffectsRefs = append(row.AffectsRefs, af.GetRef())
		}

		out = append(out, row)
	}
	return out
}

// sbomOptString copies a CycloneDX proto3-optional string field: nil stays
// nil ("never set" — the column must read NULL, not ""), a set field —
// even an explicitly empty one, which the CycloneDX spec allows for e.g.
// version — copies through. The copy avoids the returned pointer aliasing
// the proto message's own memory.
func sbomOptString(v *string) *string {
	if v == nil {
		return nil
	}
	s := *v
	return &s
}

// sbomPropertiesMultiMap turns a CycloneDX Properties list into a multiset,
// same shape and same reason as tagsToMultiMap: the spec explicitly allows
// repeated Property entries sharing a name (real scanners emit them, e.g.
// syft/trivy custom metadata), and a plain map[string]string here silently
// dropped every duplicate but the last with no column to recover it from —
// see docs/decisions/0001-tags-are-a-multiset.md for why that class of bug
// is taken seriously in this repo.
func sbomPropertiesMultiMap(props []*cyclonedx_v1_4.Property) map[string][]string {
	if len(props) == 0 {
		return nil
	}
	m := make(map[string][]string, len(props))
	for _, p := range props {
		m[p.GetName()] = append(m[p.GetName()], p.GetValue())
	}
	return m
}

// protojsonArray renders a repeated proto field as a JSON array of protojson
// objects — the lossless-but-untyped shape for the CycloneDX sub-messages
// (external references, evidence, ratings, advisories, affects) that get a
// String column instead of their own table.
//
// An element that fails protojson (non-UTF-8 string content — see the BOM
// fallback in sbomRows) is never just skipped: it still gets an array entry,
// wrapping its raw protobuf bytes instead of its JSON shape, so the array's
// length always matches the wire's repeated-field count.
func protojsonArray[T proto.Message](items []T) string {
	if len(items) == 0 {
		return ""
	}
	parts := make([]json.RawMessage, 0, len(items))
	for _, it := range items {
		b, err := protojson.Marshal(it)
		if err == nil {
			parts = append(parts, b)
			continue
		}
		log.Printf("[sbom] protojson array element: %v — falling back to raw protobuf bytes", err)
		raw, mErr := proto.Marshal(it)
		if mErr != nil {
			log.Printf("[sbom] protobuf marshal also failed for array element: %v", mErr)
			continue
		}
		wrapped, mErr := json.Marshal(map[string]string{"raw_protobuf_base64": base64.StdEncoding.EncodeToString(raw)})
		if mErr != nil {
			continue
		}
		parts = append(parts, wrapped)
	}
	out, err := json.Marshal(parts)
	if err != nil {
		return ""
	}
	return string(out)
}

// HandleSDSResult accepts Sensitive Data Scanner results.
//
// The schema is datadog/sds/sds_result.proto (SdsResultPayload), but the
// generated package pkg/proto/pbgo/sds exists only on datadog-agent main —
// no published pkg/proto version through v0.84.0-devel ships it (checked:
// pbgo/ in both v0.83.2 and v0.84.0-devel has core, dogstatsdhttp,
// languagedetection, mocks, privateactionrunner, process, procmgr, sbom,
// trace — no sds). Until it does, this track has no schema at all: every
// request goes to raw_payloads with reason "no_schema", note set to the
// top-level wire layout when it could be read.
//
// Top-level fields per the .proto: 1 scan_source, 2 timestamp, 3 resource,
// 4 scan_results (repeated), 5 scan_stats, 6 scanner_metadata, 7 rules (map),
// 8 scanning_source, 9 rule_ids (repeated).
//
// TODO(ninjacat): once pkg/proto publishes pbgo/sds, replace secWireFields
// with proto.Unmarshal(body, &sds.SdsResultPayload{}) and store real rows
// instead of raw_payloads. Note for then: timestamps and counters in that
// schema are int64 — keep them int64, and treat 0 as not provided.
func (a *Server) HandleSDSResult(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[sdsresult] cannot read body: %v", err)
		return
	}

	note := "protobuf did not parse as a sequence of top-level fields"
	layout, err := secWireFields(body)
	switch {
	case err != nil:
		log.Printf("[sdsresult] protobuf: %v (%d bytes)", err, len(body))
	case len(layout) == 0:
		note = "no fields (empty or wrong payload type?)"
		log.Printf("[sdsresult] %s (%d bytes)", note, len(body))
	default:
		note = secWireLayoutString(layout)
		log.Printf("[sdsresult] %d bytes, fields: %s", len(body), note)
	}

	a.storeRaw(c, "sds", "no_schema", note, body)
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
