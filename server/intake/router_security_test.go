package intake

import (
	"bytes"
	"encoding/json"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/cws/dumpsv1"
	"github.com/DataDog/agent-payload/v5/cyclonedx_v1_4"
	"github.com/DataDog/agent-payload/v5/sbom"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"google.golang.org/protobuf/proto"
)

func strp(s string) *string { return &s }

// ---------------------------------------------------------------------------
// Conversion function tests: cwsDumpRow / cwsFlattenTree
// ---------------------------------------------------------------------------

// threeLevelCWSTree builds init -> bash(-c "cat file", matched by r1) -> cat,
// the shape a real activity dump's process tree has: one root, one child
// carrying a rule match and args, one grandchild.
func threeLevelCWSTree() []*dumpsv1.ProcessActivityNode {
	grandchild := &dumpsv1.ProcessActivityNode{
		Process: &dumpsv1.ProcessInfo{
			Pid: 3, Ppid: 2, Comm: "cat",
			File: &dumpsv1.FileInfo{Path: "/bin/cat"},
		},
	}
	child := &dumpsv1.ProcessActivityNode{
		Process: &dumpsv1.ProcessInfo{
			Pid: 2, Ppid: 1, Comm: "bash", Args: []string{"-c", "cat file"},
		},
		MatchedRules: []*dumpsv1.MatchedRule{{RuleId: "r1", PolicyName: "default"}},
		Children:     []*dumpsv1.ProcessActivityNode{grandchild},
	}
	root := &dumpsv1.ProcessActivityNode{
		Process:  &dumpsv1.ProcessInfo{Pid: 1, Ppid: 0, Comm: "init"},
		Children: []*dumpsv1.ProcessActivityNode{child},
	}
	return []*dumpsv1.ProcessActivityNode{root}
}

// threeLevelCWSTreeWithNilSibling is threeLevelCWSTree with an extra nil
// entry in the root's Children — a shape that can occur in memory (a Go
// []*T literal with a nil element) but never survives a real proto wire
// round trip, since protobuf serializes a nil message in a repeated field as
// an EMPTY message, not as absence. Used only for the in-process walk test
// below; the handler test that goes through an actual Marshal/Unmarshal uses
// the nil-free tree above.
func threeLevelCWSTreeWithNilSibling() []*dumpsv1.ProcessActivityNode {
	tree := threeLevelCWSTree()
	tree[0].Children = append(tree[0].Children, nil)
	return tree
}

// Pins the flattening shape a real 3-level dump produces: one row per node,
// depth-first, with NodePath/ParentPath/Depth reconstructing the tree and
// the hot columns (comm, args, matched rule ids) pulled off the right node.
func TestCWSFlattenTreeThreeLevels(t *testing.T) {
	tenant, now, dumpID := "default", time.Date(2026, 9, 23, 10, 0, 0, 0, time.UTC), mustUUID(t)

	rows := cwsFlattenTree(tenant, now, dumpID, threeLevelCWSTreeWithNilSibling())

	// The nil sibling is skipped, not counted or panicked on: 3 real nodes.
	if len(rows) != 3 {
		t.Fatalf("rows: got %d, want 3 (nil sibling must be skipped, not stored)", len(rows))
	}

	root, child, grandchild := rows[0], rows[1], rows[2]

	if len(root.NodePath) != 1 || root.NodePath[0] != 0 || root.Depth != 1 {
		t.Errorf("root path/depth: got %v/%d, want [0]/1", root.NodePath, root.Depth)
	}
	if root.Comm != "init" || root.PID != 1 {
		t.Errorf("root comm/pid: got %q/%d, want init/1", root.Comm, root.PID)
	}

	wantChildPath := []uint32{0, 0}
	if !uint32SliceEqual(child.NodePath, wantChildPath) || child.Depth != 2 {
		t.Errorf("child path/depth: got %v/%d, want %v/2", child.NodePath, child.Depth, wantChildPath)
	}
	if !uint32SliceEqual(child.ParentPath, root.NodePath) {
		t.Errorf("child parent_path: got %v, want %v (the root's own path)", child.ParentPath, root.NodePath)
	}
	if child.Comm != "bash" || len(child.Args) != 2 || child.Args[1] != "cat file" {
		t.Errorf("child comm/args: got %q/%v", child.Comm, child.Args)
	}
	if len(child.MatchedRuleIDs) != 1 || child.MatchedRuleIDs[0] != "r1" {
		t.Errorf("child matched_rule_ids: got %v, want [r1]", child.MatchedRuleIDs)
	}

	wantGrandchildPath := []uint32{0, 0, 0}
	if !uint32SliceEqual(grandchild.NodePath, wantGrandchildPath) || grandchild.Depth != 3 {
		t.Errorf("grandchild path/depth: got %v/%d, want %v/3", grandchild.NodePath, grandchild.Depth, wantGrandchildPath)
	}
	if grandchild.FilePath != "/bin/cat" {
		t.Errorf("grandchild file_path: got %q, want /bin/cat", grandchild.FilePath)
	}
	// Node holds protojson of the node WITHOUT its children — cheapest check
	// that it isn't empty and isn't obviously carrying the subtree twice.
	if grandchild.Node == "" {
		t.Errorf("grandchild node: got empty protojson")
	}
}

// A dump whose Metadata is genuinely absent must not read as "every counter
// was 0" — Start/End/Size stay nil, never a present zero.
func TestCWSDumpRowNilMetadata(t *testing.T) {
	tenant, now, dumpID := "default", time.Now().UTC(), mustUUID(t)
	dump := &dumpsv1.SecDump{Host: "h1", Tree: threeLevelCWSTree()}

	row := cwsDumpRow(tenant, now, dumpID, nil, dump, []byte{0x08, 0x01})

	if row.Start != nil || row.End != nil || row.Size != nil {
		t.Errorf("Start/End/Size: got %v/%v/%v, want all nil — Metadata was absent", row.Start, row.End, row.Size)
	}
	if row.TreeNodeCount != 3 {
		t.Errorf("TreeNodeCount: got %d, want 3 (every depth, not just roots)", row.TreeNodeCount)
	}
	if row.DumpHost != "h1" {
		t.Errorf("DumpHost: got %q, want h1", row.DumpHost)
	}
}

// A present Metadata with Start=0 is a real kernel-boot-relative reading,
// not "absent" — the pointer must be non-nil.
func TestCWSDumpRowPresentZeroMetadataCounter(t *testing.T) {
	dump := &dumpsv1.SecDump{
		Metadata: &dumpsv1.Metadata{AgentVersion: "7.58.2", Start: 0, End: 500},
	}
	row := cwsDumpRow("default", time.Now(), mustUUID(t), nil, dump, nil)

	if row.Start == nil || *row.Start != 0 {
		t.Errorf("Start: got %v, want a present pointer to 0", row.Start)
	}
	if row.End == nil || *row.End != 500 {
		t.Errorf("End: got %v, want a present pointer to 500", row.End)
	}
	if row.AgentVersion != "7.58.2" {
		t.Errorf("AgentVersion: got %q", row.AgentVersion)
	}
}

// The header's ddtags is a multiset like every other Datadog tag list, and
// any key beyond the five documented ones survives in HeaderExtra rather
// than being dropped.
func TestCWSDumpRowHeaderTagsAndExtra(t *testing.T) {
	header := map[string]json.RawMessage{
		"host":       json.RawMessage(`"h1"`),
		"service":    json.RawMessage(`"cws-agent"`),
		"ddsource":   json.RawMessage(`"security-agent"`),
		"ddtags":     json.RawMessage(`["env:prod","kube_service:a","kube_service:b"]`),
		"dns_names":  json.RawMessage(`["example.com"]`),
		"unexpected": json.RawMessage(`{"nested":true}`),
	}
	row := cwsDumpRow("default", time.Now(), mustUUID(t), header, nil, nil)

	if got := row.HeaderTags["kube_service"]; len(got) != 2 || got[0] != "a" || got[1] != "b" {
		t.Errorf("HeaderTags[kube_service]: got %v, want [a b] — tags are a multiset", got)
	}
	if row.DNSNames != `["example.com"]` {
		t.Errorf("DNSNames: got %q, want the raw JSON array text", row.DNSNames)
	}
	if row.HeaderExtra["unexpected"] != `{"nested":true}` {
		t.Errorf("HeaderExtra[unexpected]: got %q, want the raw JSON text kept, not dropped", row.HeaderExtra["unexpected"])
	}
}

// ---------------------------------------------------------------------------
// Conversion function tests: secEventRows
// ---------------------------------------------------------------------------

// A secruntime batch with one undecodable message: the envelope with the
// bad "message" must still produce a row (MessageDecoded=0, Message=""),
// never disappear from the batch.
func TestSecEventRowsOneUndecodableMessage(t *testing.T) {
	body := []byte(`[
		{"hostname":"h1","service":"cws-agent","ddsource":"security-agent","status":"info",
		 "timestamp":1700000000000,"ddtags":["env:prod","team:sec"],
		 "message":"{\"agent\":{\"rule_id\":\"r1\",\"policy_name\":\"default\"},\"evt\":{\"name\":\"exec\",\"category\":\"process\"}}"},
		{"hostname":"h2","service":"cws-agent","ddsource":"security-agent","status":"info","message":"not json"}
	]`)

	var envelopes []map[string]json.RawMessage
	if err := json.Unmarshal(body, &envelopes); err != nil {
		t.Fatalf("test fixture: %v", err)
	}
	events := make([]map[string]json.RawMessage, len(envelopes))
	for i, e := range envelopes {
		inner, err := secInnerMessage(e["message"])
		if err == nil {
			events[i] = inner
		}
	}

	rows := secEventRows("default", time.Date(2026, 9, 23, 0, 0, 0, 0, time.UTC), "secruntime", envelopes, events)
	if len(rows) != 2 {
		t.Fatalf("rows: got %d, want 2 — the undecodable envelope must still produce a row", len(rows))
	}

	ok, bad := rows[0], rows[1]
	if ok.MessageDecoded != 1 {
		t.Errorf("ok.MessageDecoded: got %d, want 1", ok.MessageDecoded)
	}
	if ok.RuleID != "r1" || ok.PolicyName != "default" {
		t.Errorf("ok rule_id/policy_name: got %q/%q, want r1/default", ok.RuleID, ok.PolicyName)
	}
	if ok.EvtName != "exec" || ok.EvtCategory != "process" {
		t.Errorf("ok evt_name/evt_category: got %q/%q, want exec/process", ok.EvtName, ok.EvtCategory)
	}
	if got := ok.DDTags["team"]; len(got) != 1 || got[0] != "sec" {
		t.Errorf("ok ddtags[team]: got %v, want [sec]", got)
	}
	if ok.Timestamp == nil || !ok.Timestamp.Equal(time.UnixMilli(1700000000000).UTC()) {
		t.Errorf("ok timestamp: got %v, want the parsed ms epoch", ok.Timestamp)
	}
	if ok.SeqInBatch != 0 || bad.SeqInBatch != 1 {
		t.Errorf("seq_in_batch: got %d/%d, want 0/1 — arrival order must survive", ok.SeqInBatch, bad.SeqInBatch)
	}

	if bad.MessageDecoded != 0 {
		t.Errorf("bad.MessageDecoded: got %d, want 0 — \"not json\" must not decode", bad.MessageDecoded)
	}
	if bad.Message != "" {
		t.Errorf("bad.Message: got %q, want empty", bad.Message)
	}
	if bad.MessageRaw != `"not json"` {
		t.Errorf("bad.MessageRaw: got %q, want the raw JSON text kept regardless of decode success", bad.MessageRaw)
	}
}

// ---------------------------------------------------------------------------
// Conversion function tests: sbomRows
// ---------------------------------------------------------------------------

// An SBOM entity with two components (one nested under the other) and one
// vulnerability: the shape the brief asks to be pinned.
func TestSBOMRowsComponentsAndVulnerability(t *testing.T) {
	sub := &cyclonedx_v1_4.Component{
		BomRef: strp("pkg:c1-sub"), Name: "libbar", Version: "2.0",
		Type: cyclonedx_v1_4.Classification_CLASSIFICATION_LIBRARY,
	}
	top := &cyclonedx_v1_4.Component{
		BomRef: strp("pkg:c1"), Name: "libfoo", Version: "1.0",
		Type: cyclonedx_v1_4.Classification_CLASSIFICATION_LIBRARY,
		Licenses: []*cyclonedx_v1_4.LicenseChoice{
			{Choice: &cyclonedx_v1_4.LicenseChoice_License{
				License: &cyclonedx_v1_4.License{License: &cyclonedx_v1_4.License_Id{Id: "MIT"}},
			}},
		},
		Hashes:     []*cyclonedx_v1_4.Hash{{Alg: cyclonedx_v1_4.HashAlg_HASH_ALG_SHA_256, Value: "abcd"}},
		Components: []*cyclonedx_v1_4.Component{sub},
	}
	vuln := &cyclonedx_v1_4.Vulnerability{
		BomRef: strp("pkg:c1"), Id: strp("CVE-2024-1234"),
		Source: &cyclonedx_v1_4.Source{Name: strp("NVD")},
		Cwes:   []int32{79},
		Affects: []*cyclonedx_v1_4.VulnerabilityAffects{
			{Ref: "pkg:c1"},
		},
	}
	bom := &cyclonedx_v1_4.Bom{
		SpecVersion:     "1.4",
		Components:      []*cyclonedx_v1_4.Component{top},
		Vulnerabilities: []*cyclonedx_v1_4.Vulnerability{vuln},
	}
	payload := &sbom.SBOMPayload{
		Version: 1, Host: "h1",
		Entities: []*sbom.SBOMEntity{
			{
				Type: sbom.SBOMSourceType_CONTAINER_IMAGE_LAYERS, Id: "e1",
				Sbom: &sbom.SBOMEntity_Cyclonedx{Cyclonedx: bom},
			},
		},
	}

	entities, components, vulns := sbomRows("default", time.Now(), payload)

	if len(entities) != 1 {
		t.Fatalf("entities: got %d, want 1", len(entities))
	}
	e := entities[0]
	if e.ComponentCount != 2 {
		t.Errorf("ComponentCount: got %d, want 2 (top + nested)", e.ComponentCount)
	}
	if e.VulnerabilityCount != 1 {
		t.Errorf("VulnerabilityCount: got %d, want 1", e.VulnerabilityCount)
	}
	if e.Bom == "" {
		t.Errorf("Bom: got empty, want the protojson of the whole BOM")
	}

	if len(components) != 2 {
		t.Fatalf("components: got %d, want 2", len(components))
	}
	parent, child := components[0], components[1]
	if parent.BomRef != "pkg:c1" || parent.Depth != 0 || parent.ParentBomRef != "" {
		t.Errorf("parent: got bom_ref=%q depth=%d parent_bom_ref=%q, want pkg:c1/0/\"\"",
			parent.BomRef, parent.Depth, parent.ParentBomRef)
	}
	if len(parent.Licenses) != 1 || parent.Licenses[0] != "MIT" {
		t.Errorf("parent licenses: got %v, want [MIT]", parent.Licenses)
	}
	if parent.Hashes["HASH_ALG_SHA_256"] != "abcd" {
		t.Errorf("parent hashes: got %v", parent.Hashes)
	}
	if child.BomRef != "pkg:c1-sub" || child.Depth != 1 || child.ParentBomRef != "pkg:c1" {
		t.Errorf("child: got bom_ref=%q depth=%d parent_bom_ref=%q, want pkg:c1-sub/1/pkg:c1",
			child.BomRef, child.Depth, child.ParentBomRef)
	}
	// entity_id must be the SAME generated id across the entity row and
	// every component/vulnerability it produced — that is the join.
	if parent.EntityID != e.EntityID || child.EntityID != e.EntityID {
		t.Errorf("component entity_id must match the entity row's generated id")
	}

	if len(vulns) != 1 {
		t.Fatalf("vulnerabilities: got %d, want 1", len(vulns))
	}
	v := vulns[0]
	if v.ID != "CVE-2024-1234" || v.SourceName != "NVD" {
		t.Errorf("vuln id/source_name: got %q/%q, want CVE-2024-1234/NVD", v.ID, v.SourceName)
	}
	if len(v.Cwes) != 1 || v.Cwes[0] != 79 {
		t.Errorf("vuln cwes: got %v, want [79]", v.Cwes)
	}
	if len(v.AffectsRefs) != 1 || v.AffectsRefs[0] != "pkg:c1" {
		t.Errorf("vuln affects_refs: got %v, want [pkg:c1]", v.AffectsRefs)
	}
	if v.EntityID != e.EntityID {
		t.Errorf("vuln entity_id must match the entity row's generated id")
	}
}

// The oneof's other arm: a failed generation carries Error, no Bom, and no
// components/vulnerabilities to flatten.
func TestSBOMRowsErrorArm(t *testing.T) {
	payload := &sbom.SBOMPayload{
		Host: "h1",
		Entities: []*sbom.SBOMEntity{
			{Id: "e1", Sbom: &sbom.SBOMEntity_Error{Error: "generation timed out"}},
		},
	}
	entities, components, vulns := sbomRows("default", time.Now(), payload)

	if len(entities) != 1 {
		t.Fatalf("entities: got %d, want 1", len(entities))
	}
	if entities[0].Error != "generation timed out" {
		t.Errorf("Error: got %q", entities[0].Error)
	}
	if entities[0].Bom != "" {
		t.Errorf("Bom: got %q, want empty — the oneof was Error, not Cyclonedx", entities[0].Bom)
	}
	if len(components) != 0 || len(vulns) != 0 {
		t.Errorf("components/vulns: got %d/%d, want 0/0", len(components), len(vulns))
	}
}

// ---------------------------------------------------------------------------
// Handler tests: real requests through the real engine, real rows in storage
// ---------------------------------------------------------------------------

// The worked example this file follows (see testserver_test.go's
// TestHandleLogsStoresEveryItem): a secdump request through the real engine,
// asserting on the rows that reached both cws_activity_dumps and
// cws_dump_nodes, and that the tenant landed on every one.
func TestHandleSecDumpStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCWS)

	headerJSON, _ := json.Marshal(map[string]any{
		"host": "h1", "service": "cws-agent", "ddsource": "security-agent",
		"ddtags": []string{"env:prod"},
	})
	dumpBytes, err := proto.Marshal(&dumpsv1.SecDump{
		Host: "h1", Service: "cws-agent",
		Metadata: &dumpsv1.Metadata{AgentVersion: "7.58.2"},
		Tree:     threeLevelCWSTree(),
	})
	if err != nil {
		t.Fatalf("marshal test dump: %v", err)
	}

	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)
	part, _ := mw.CreateFormField("event")
	part.Write(headerJSON)
	part, _ = mw.CreateFormFile("dump", "dump")
	part.Write(dumpBytes)
	mw.Close()

	req := httptest.NewRequest(http.MethodPost, "/api/v2/secdump", &buf)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", mw.FormDataContentType())
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/secdump: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	dumps := Rows[storage.CWSActivityDumpRow](node)
	if len(dumps) != 1 {
		t.Fatalf("cws_activity_dumps rows: got %d, want 1", len(dumps))
	}
	if dumps[0].TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", dumps[0].TenantID, testTenant)
	}
	if dumps[0].HeaderHost != "h1" || dumps[0].AgentVersion != "7.58.2" {
		t.Errorf("header_host/agent_version: got %q/%q", dumps[0].HeaderHost, dumps[0].AgentVersion)
	}
	if dumps[0].TreeNodeCount != 3 {
		t.Errorf("tree_node_count: got %d, want 3", dumps[0].TreeNodeCount)
	}

	nodes := Rows[storage.CWSDumpNodeRow](node)
	if len(nodes) != 3 {
		t.Fatalf("cws_dump_nodes rows: got %d, want 3", len(nodes))
	}
	for _, n := range nodes {
		if n.DumpID != dumps[0].DumpID {
			t.Errorf("node dump_id: got %v, want %v (must link to the dump row)", n.DumpID, dumps[0].DumpID)
		}
	}
}

// A secruntime batch through the real engine: both the decodable and the
// undecodable envelope must reach security_events.
func TestHandleSecLogsTrackStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeRuntimeSecurity)

	body := []byte(`[
		{"hostname":"h1","service":"cws-agent","ddsource":"security-agent","status":"info",
		 "message":"{\"agent\":{\"rule_id\":\"r1\"}}"},
		{"hostname":"h2","service":"cws-agent","ddsource":"security-agent","status":"info","message":"not json"}
	]`)

	if w := post(t, e, "/api/v2/secruntime", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/secruntime: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.SecurityEventRow](node)
	if len(rows) != 2 {
		t.Fatalf("security_events rows: got %d, want 2", len(rows))
	}
	for _, r := range rows {
		if r.TenantID != testTenant || r.Track != "secruntime" {
			t.Errorf("tenant/track: got %q/%q, want %q/secruntime", r.TenantID, r.Track, testTenant)
		}
	}
	if rows[0].RuleID != "r1" {
		t.Errorf("rows[0].RuleID: got %q, want r1", rows[0].RuleID)
	}
	if rows[1].MessageDecoded != 0 {
		t.Errorf("rows[1].MessageDecoded: got %d, want 0", rows[1].MessageDecoded)
	}
}

// An SBOM batch through the real engine: entities, components and
// vulnerabilities must all reach storage from one request.
func TestHandleSBOMStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSBOM)

	bom := &cyclonedx_v1_4.Bom{
		SpecVersion: "1.4",
		Components: []*cyclonedx_v1_4.Component{
			{BomRef: strp("pkg:c1"), Name: "libfoo", Version: "1.0"},
		},
		Vulnerabilities: []*cyclonedx_v1_4.Vulnerability{
			{BomRef: strp("pkg:c1"), Id: strp("CVE-2024-1234")},
		},
	}
	payload := &sbom.SBOMPayload{
		Version: 1, Host: "h1",
		Entities: []*sbom.SBOMEntity{
			{Id: "e1", Type: sbom.SBOMSourceType_CONTAINER_IMAGE_LAYERS,
				Sbom: &sbom.SBOMEntity_Cyclonedx{Cyclonedx: bom}},
		},
	}
	body, err := proto.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal test payload: %v", err)
	}

	req := httptest.NewRequest(http.MethodPost, "/api/v2/sbom", bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/x-protobuf")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/sbom: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	entities := Rows[storage.SBOMEntityRow](node)
	if len(entities) != 1 || entities[0].TenantID != testTenant {
		t.Fatalf("sbom_entities: got %d rows (tenant %q), want 1 (%q)", len(entities), entities[0].TenantID, testTenant)
	}
	if len(Rows[storage.SBOMComponentRow](node)) != 1 {
		t.Errorf("sbom_components: got %d rows, want 1", len(Rows[storage.SBOMComponentRow](node)))
	}
	if len(Rows[storage.SBOMVulnerabilityRow](node)) != 1 {
		t.Errorf("sbom_vulnerabilities: got %d rows, want 1", len(Rows[storage.SBOMVulnerabilityRow](node)))
	}
}

// sdsresult has no published Go type: every request, decodable-looking or
// not, must land in raw_payloads with reason "no_schema" — never dropped,
// never stored as a typed row.
func TestHandleSDSResultAlwaysStoresRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSDS)

	// A tiny hand-built protobuf-shaped body: field 1, varint, value 1.
	body := []byte{0x08, 0x01}

	if w := post(t, e, "/api/v2/sdsresult", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/sdsresult: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(rows))
	}
	if rows[0].Intake != "sds" || rows[0].Reason != "no_schema" {
		t.Errorf("intake/reason: got %q/%q, want sds/no_schema", rows[0].Intake, rows[0].Reason)
	}
	if rows[0].Body != string(body) {
		t.Errorf("body: got %q, want the raw bytes kept verbatim", rows[0].Body)
	}
}

// ---------------------------------------------------------------------------
// test helpers local to this file
// ---------------------------------------------------------------------------

func mustUUID(t *testing.T) uuid.UUID {
	t.Helper()
	// A fixed, non-zero id is enough for these tests — nothing here asserts
	// on the value itself, only that it is threaded through consistently.
	var id uuid.UUID
	for i := range id {
		id[i] = byte(i + 1)
	}
	return id
}

func uint32SliceEqual(a, b []uint32) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
