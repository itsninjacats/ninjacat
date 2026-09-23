package intake

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/agentdiscovery"
	"github.com/DataDog/agent-payload/v5/healthplatform"
	"github.com/google/uuid"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// ---------------------------------------------------------------------------
// evpParseTime — the "absent vs zero" rule every String+Nullable pair in
// this file leans on.

func TestEvpParseTime(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want bool // whether a non-nil *time.Time comes back
	}{
		{"empty string is absent, not epoch", "", false},
		{"garbage is absent, not epoch", "not-a-timestamp", false},
		{"RFC3339", "2026-09-23T10:30:00Z", true},
		{"RFC3339Nano", "2026-09-23T10:30:00.123456789Z", true},
		{"unix seconds as a bare number is not RFC3339", "1758622200", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := evpParseTime(tc.in)
			if (got != nil) != tc.want {
				t.Errorf("evpParseTime(%q) = %v, want non-nil=%v", tc.in, got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// agentdiscovery-intake.<site>

// A batch whose HostId repeats onto every payload row, whose config files
// travel as index-aligned parallel arrays, and whose env var VALUES survive
// even though the log line beside this code only prints their names.
func TestAgentDiscoveryRows(t *testing.T) {
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	ingested := timestamppb.New(now.Add(-time.Minute))

	batch := &agentdiscovery.AgentDiscoveryPayloadBatch{
		HostId: "host-1",
		Payloads: []*agentdiscovery.AgentDiscoveryPayload{
			nil, // a batch with a nil payload element must not panic
			{
				Integration:        "postgres",
				Runtime:            "python",
				RuntimeId:          "rt-1",
				IngestionTimestamp: ingested,
				ConfigFiles: []*agentdiscovery.AgentDiscoveryConfigFile{
					nil, // ditto for a nil config file element
					{
						Path: "/etc/datadog-agent/conf.d/postgres.yaml",
						Content: []byte(
							"instances:\n  - host: db\n    password: hunter2\n"),
						Truncated:     false,
						PayloadFormat: agentdiscovery.AgentDiscoveryConfigFilePayloadFormat_PAYLOAD_FORMAT_YAML,
					},
					{Path: "/etc/x.conf", Content: []byte("..."), Truncated: true,
						PayloadFormat: agentdiscovery.AgentDiscoveryConfigFilePayloadFormat_PAYLOAD_FORMAT_UNKNOWN},
				},
				EnvVars: []*agentdiscovery.AgentDiscoveryEnvVar{
					{Name: "PGPASSWORD", Value: "hunter2"}, // credential-shaped, kept on purpose
					{Name: "PGHOST", Value: "db"},
				},
			},
			{
				// No IngestionTimestamp at all: must stay nil, not epoch.
				Integration: "redis", Runtime: "go", RuntimeId: "rt-2",
			},
		},
	}

	rows := agentDiscoveryRows("acme", now, batch)
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2 (nil payload skipped)", len(rows))
	}

	pg := rows[0]
	if pg.TenantID != "acme" || pg.HostID != "host-1" {
		t.Errorf("tenant/host_id: got %q/%q, want acme/host-1", pg.TenantID, pg.HostID)
	}
	if pg.Integration != "postgres" || pg.RuntimeID != "rt-1" {
		t.Errorf("integration/runtime_id: got %q/%q", pg.Integration, pg.RuntimeID)
	}
	if pg.IngestionTimestamp == nil || !pg.IngestionTimestamp.Equal(ingested.AsTime()) {
		t.Errorf("ingestion_timestamp: got %v, want %v", pg.IngestionTimestamp, ingested.AsTime())
	}
	if len(pg.ConfigPaths) != 2 || len(pg.ConfigContents) != 2 ||
		len(pg.ConfigTruncated) != 2 || len(pg.ConfigFormats) != 2 {
		t.Fatalf("config arrays: got %d/%d/%d/%d, want 2 each (nil element skipped)",
			len(pg.ConfigPaths), len(pg.ConfigContents), len(pg.ConfigTruncated), len(pg.ConfigFormats))
	}
	if pg.ConfigPaths[0] != "/etc/datadog-agent/conf.d/postgres.yaml" {
		t.Errorf("config_paths[0]: got %q", pg.ConfigPaths[0])
	}
	if pg.ConfigContents[0] != "instances:\n  - host: db\n    password: hunter2\n" {
		t.Errorf("config_contents[0] was not kept verbatim: got %q", pg.ConfigContents[0])
	}
	if pg.ConfigTruncated[0] != 0 || pg.ConfigTruncated[1] != 1 {
		t.Errorf("config_truncated: got %v, want [0 1]", pg.ConfigTruncated)
	}
	if pg.ConfigFormats[0] != "PAYLOAD_FORMAT_YAML" || pg.ConfigFormats[1] != "PAYLOAD_FORMAT_UNKNOWN" {
		t.Errorf("config_formats: got %v", pg.ConfigFormats)
	}
	if len(pg.EnvVarNames) != 2 || pg.EnvVarNames[0] != "PGPASSWORD" || pg.EnvVarNames[1] != "PGHOST" {
		t.Errorf("env_var_names: got %v, want [PGPASSWORD PGHOST] in wire order", pg.EnvVarNames)
	}
	if len(pg.EnvVarValues) != 2 || pg.EnvVarValues[0] != "hunter2" || pg.EnvVarValues[1] != "db" {
		t.Errorf("env_var_values: got %v, want values kept, not just names", pg.EnvVarValues)
	}

	redis := rows[1]
	if redis.IngestionTimestamp != nil {
		t.Errorf("ingestion_timestamp with no wire value: got %v, want nil (not epoch)", redis.IngestionTimestamp)
	}
	if redis.EnvVarNames != nil || redis.EnvVarValues != nil {
		t.Errorf("env_vars with none sent: got names=%v values=%v, want nil", redis.EnvVarNames, redis.EnvVarValues)
	}
}

// EnvVars is a REPEATED field on the wire, not a map: a duplicate name must
// not collapse to its last value, and the original order must survive —
// exactly what a map[string]string representation would silently lose.
func TestAgentDiscoveryRowsPreservesDuplicateEnvVarNames(t *testing.T) {
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	batch := &agentdiscovery.AgentDiscoveryPayloadBatch{
		HostId: "host-1",
		Payloads: []*agentdiscovery.AgentDiscoveryPayload{
			{
				Integration: "postgres", Runtime: "python",
				EnvVars: []*agentdiscovery.AgentDiscoveryEnvVar{
					{Name: "PATH", Value: "/usr/bin"},
					{Name: "PATH", Value: "/opt/datadog/bin"}, // duplicate name, later value
				},
			},
		},
	}

	rows := agentDiscoveryRows("acme", now, batch)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	row := rows[0]
	if len(row.EnvVarNames) != 2 || len(row.EnvVarValues) != 2 {
		t.Fatalf("env_vars: got %d names / %d values, want 2 each — a map would have collapsed the duplicate",
			len(row.EnvVarNames), len(row.EnvVarValues))
	}
	if row.EnvVarNames[0] != "PATH" || row.EnvVarValues[0] != "/usr/bin" {
		t.Errorf("env_vars[0]: got %s=%s, want PATH=/usr/bin (wire order preserved)", row.EnvVarNames[0], row.EnvVarValues[0])
	}
	if row.EnvVarNames[1] != "PATH" || row.EnvVarValues[1] != "/opt/datadog/bin" {
		t.Errorf("env_vars[1]: got %s=%s, want PATH=/opt/datadog/bin (both values kept)", row.EnvVarNames[1], row.EnvVarValues[1])
	}
}

// ---------------------------------------------------------------------------
// agenthealth-intake.<site>

// One report with two issues: pins the sorted-key order (Go map iteration is
// random, but two ingests of the same report must produce the same rows),
// the nil-Host and nil-PersistedIssue cases, and the ResolvedAt ptr-optional
// distinction (unresolved vs resolved-with-empty-string would be the same
// bug this column exists to prevent).
func TestHealthReportRows(t *testing.T) {
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	reportID := uuid.MustParse("11111111-1111-1111-1111-111111111111")
	resolvedAt := "2026-09-23T09:00:00Z"

	extra, err := structpb.NewStruct(map[string]any{"probe": "tcp", "attempts": 3.0})
	if err != nil {
		t.Fatalf("structpb.NewStruct: %v", err)
	}

	report := &healthplatform.HealthReport{
		SchemaVersion: "1.0",
		EventType:     "health_report",
		EmittedAt:     "2026-09-23T10:29:00Z",
		Service:       "logs-agent",
		Host:          &healthplatform.HostInfo{Hostname: "web-1", ParIds: []string{"par-1"}},
		Issues: map[string]*healthplatform.Issue{
			"z-issue": {
				Id: "z-issue", IssueName: "disk_full", Title: "Disk full",
				Severity: healthplatform.IssueSeverity_ISSUE_SEVERITY_HIGH,
				Tags:     []string{"env:prod", "env:staging"}, // multi-valued: a multiset, not a property
				Extra:    extra,
				PersistedIssue: &healthplatform.PersistedIssue{
					State: healthplatform.IssueState_ISSUE_STATE_RESOLVED,
					// ResolvedAt IS set: distinct from the other issue below.
					ResolvedAt: &resolvedAt,
				},
			},
			"a-issue": {
				Id: "a-issue", IssueName: "cert_expiring",
				Severity: healthplatform.IssueSeverity_ISSUE_SEVERITY_MEDIUM,
				// No PersistedIssue at all — must not panic, and ResolvedAt
				// must stay nil (never "", which would claim "resolved with
				// no timestamp").
			},
		},
	}

	reportRow, issueRows := healthReportRows("acme", now, reportID, report, nil)

	if reportRow.Host != "web-1" || len(reportRow.ParIDs) != 1 || reportRow.ParIDs[0] != "par-1" {
		t.Errorf("host/par_ids: got %q/%v", reportRow.Host, reportRow.ParIDs)
	}
	if reportRow.AgentVersion != nil {
		t.Errorf("agent_version with none sent: got %v, want nil", reportRow.AgentVersion)
	}
	if reportRow.IssueCount != 2 {
		t.Errorf("issue_count: got %d, want 2", reportRow.IssueCount)
	}
	if reportRow.EmittedAtParsed == nil {
		t.Errorf("emitted_at_parsed: got nil, want a parsed RFC3339 value")
	}

	if len(issueRows) != 2 {
		t.Fatalf("got %d issue rows, want 2", len(issueRows))
	}
	// Sorted by issue key: "a-issue" before "z-issue", regardless of Go's
	// random map iteration order over report.Issues.
	if issueRows[0].IssueKey != "a-issue" || issueRows[1].IssueKey != "z-issue" {
		t.Fatalf("issue order: got %q, %q, want a-issue, z-issue (sorted)",
			issueRows[0].IssueKey, issueRows[1].IssueKey)
	}

	noLifecycle, withLifecycle := issueRows[0], issueRows[1]
	if noLifecycle.ResolvedAt != nil {
		t.Errorf("a-issue (no PersistedIssue) resolved_at: got %v, want nil", noLifecycle.ResolvedAt)
	}
	if noLifecycle.PersistedState != "" {
		t.Errorf("a-issue persisted_state: got %q, want empty (no lifecycle reported)", noLifecycle.PersistedState)
	}

	if withLifecycle.ResolvedAt == nil || *withLifecycle.ResolvedAt != resolvedAt {
		t.Errorf("z-issue resolved_at: got %v, want %q", withLifecycle.ResolvedAt, resolvedAt)
	}
	if withLifecycle.PersistedState != "ISSUE_STATE_RESOLVED" {
		t.Errorf("z-issue persisted_state: got %q", withLifecycle.PersistedState)
	}
	if withLifecycle.Severity != "ISSUE_SEVERITY_HIGH" {
		t.Errorf("z-issue severity: got %q", withLifecycle.Severity)
	}
	wantTags := map[string][]string{"env": {"prod", "staging"}}
	if len(withLifecycle.Tags["env"]) != 2 || withLifecycle.Tags["env"][0] != "prod" || withLifecycle.Tags["env"][1] != "staging" {
		t.Errorf("tags: got %v, want %v (multi-valued, order preserved)", withLifecycle.Tags, wantTags)
	}
	if withLifecycle.Extra == "" || !contains(withLifecycle.Extra, "probe") {
		t.Errorf("extra (structpb as JSON): got %q, want it to contain the Struct's fields", withLifecycle.Extra)
	}
	for i := range issueRows {
		if issueRows[i].ReportID != reportID {
			t.Errorf("row %d report_id: got %v, want %v — the join key must repeat onto every issue", i, issueRows[i].ReportID, reportID)
		}
	}
}

// Issue.Extra is a structpb.Struct on the wire, and structpb.Value's own
// generated type stores every JSON number as float64 — a rounding that
// happens inside structpb's UnmarshalJSON, before this intake's own
// UseNumber decoder ever runs. A probe id above 2^53 must still survive
// exactly, via the parallel raw decode healthReportRows is given.
func TestHealthReportRowsExtraPrecision(t *testing.T) {
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	reportID := uuid.MustParse("22222222-2222-2222-2222-222222222222")

	body := []byte(`{
		"issues": {
			"conn-refused": {
				"id": "conn-refused",
				"extra": {"probe": "tcp", "probe_pid": 9007199254740993}
			}
		}
	}`)

	var report healthplatform.HealthReport
	if err := json.Unmarshal(body, &report); err != nil {
		t.Fatalf("json.Unmarshal: %v", err)
	}

	_, issueRows := healthReportRows("acme", now, reportID, &report, evpHealthIssuesRaw(body))
	if len(issueRows) != 1 {
		t.Fatalf("got %d issue rows, want 1", len(issueRows))
	}

	// A float64 round trip would render this as 9007199254740992 (or
	// scientific notation) — only the exact literal proves UseNumber, not
	// structpb, produced this string.
	if !contains(issueRows[0].Extra, "9007199254740993") {
		t.Errorf("extra: got %q, want it to contain the exact literal 9007199254740993, not a float64-rounded value",
			issueRows[0].Extra)
	}
}

func contains(haystack, needle string) bool {
	return len(haystack) >= len(needle) && (haystack == needle || len(needle) == 0 ||
		indexOf(haystack, needle) >= 0)
}

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

// ---------------------------------------------------------------------------
// event-management-intake.<site>

// Pins three things at once: the doubly-nested "attributes" key (outer
// envelope attribute vs the producer's own inner object) must not collide,
// the hyphenated "system-notable-events" map key must survive verbatim, and
// an attribute this table has no column for lands in OuterExtra as JSON.
func TestEventManagementRow(t *testing.T) {
	body := []byte(`{
		"data": {
			"type": "event",
			"id": "evt-123",
			"attributes": {
				"host": "web-1",
				"title": "High CPU",
				"category": "performance",
				"integration_id": "system",
				"message": "cpu above 90%",
				"timestamp": "2026-09-23T10:30:00Z",
				"tags": ["env:prod", "env:staging"],
				"aggregation_key": "cpu-web-1",
				"system-notable-events": {"event_type": "anomaly"},
				"attributes": {"status": "warn", "priority": "high"},
				"unknown_field": {"nested": true}
			}
		},
		"meta": {"page": 1}
	}`)

	m, ok := evpObject("test", body)
	if !ok {
		t.Fatalf("evpObject failed to decode the fixture")
	}

	row, ok := eventManagementRow(m)
	if !ok {
		t.Fatalf("eventManagementRow returned false on a well-formed envelope")
	}

	if row.DataType != "event" || row.Host != "web-1" || row.Title != "High CPU" {
		t.Errorf("data_type/host/title: got %q/%q/%q", row.DataType, row.Host, row.Title)
	}
	if row.NotableEventType != "anomaly" {
		t.Errorf("notable_event_type: got %q, want anomaly (from the hyphenated key)", row.NotableEventType)
	}
	if row.Attributes != `{"priority":"high","status":"warn"}` {
		t.Errorf("attributes (inner, kept nested): got %q", row.Attributes)
	}
	if len(row.Tags["env"]) != 2 {
		t.Errorf("tags: got %v, want env with two values", row.Tags)
	}
	if row.OuterExtra["unknown_field"] != `{"nested":true}` {
		t.Errorf("outer_extra: got %v, want unknown_field to hold its JSON", row.OuterExtra)
	}
	if _, stillThere := row.OuterExtra["host"]; stillThere {
		t.Errorf("outer_extra leaked a known key: %v", row.OuterExtra)
	}
	// data.id (a sibling of "attributes", not inside it) and the envelope's
	// own "meta" (a sibling of "data") must both still reach OuterExtra,
	// prefixed so neither can be confused with an attributes-level key.
	if row.OuterExtra["data.id"] != `"evt-123"` {
		t.Errorf("outer_extra[data.id]: got %v, want the JSON:API data.id to survive", row.OuterExtra)
	}
	if row.OuterExtra["top.meta"] != `{"page":1}` {
		t.Errorf("outer_extra[top.meta]: got %v, want the envelope-level meta to survive", row.OuterExtra)
	}

	// data.attributes missing entirely — the shape this track's envelope
	// requires and does not have.
	if _, ok := eventManagementRow(map[string]any{"data": map[string]any{"type": "event"}}); ok {
		t.Errorf("eventManagementRow with no data.attributes: got ok=true, want false")
	}
}

// ---------------------------------------------------------------------------
// softinv-intake.<site>

// One payload, two entries: a fully-typed one and one with an undeclared key
// and an absent is_64_bit — the Nullable(UInt8) case a `false` default would
// silently misreport as 32-bit.
func TestHostSoftwareRows(t *testing.T) {
	body := []byte(`{
		"hostname": "web-1",
		"extra_payload_field": "seen",
		"host_software": {
			"collected_at": "2026-09-23T10:00:00Z",
			"software": [
				{"software_type": "package", "name": "curl", "version": "7.88.1",
				 "publisher": "curl", "deployment_status": "installed",
				 "deployment_time": "2026-09-20T00:00:00Z", "product_code": "curl",
				 "is_64_bit": true, "install_paths": ["/usr/bin/curl"]},
				{"software_type": "package", "name": "mystery",
				 "vendor_specific_field": "x"},
				"not-an-object"
			]
		}
	}`)

	m, ok := evpObject("test", body)
	if !ok {
		t.Fatalf("evpObject failed to decode the fixture")
	}

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	rows := hostSoftwareRows("acme", now, m)
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2 (the non-object entry is dropped)", len(rows))
	}

	curl, mystery := rows[0], rows[1]
	if curl.Name != "curl" || curl.Hostname != "web-1" {
		t.Errorf("curl name/hostname: got %q/%q", curl.Name, curl.Hostname)
	}
	if curl.Is64Bit == nil || *curl.Is64Bit != 1 {
		t.Errorf("curl is_64_bit: got %v, want 1", curl.Is64Bit)
	}
	if curl.DeploymentTimeParsed == nil {
		t.Errorf("curl deployment_time_parsed: got nil, want parsed")
	}
	if len(curl.InstallPaths) != 1 || curl.InstallPaths[0] != "/usr/bin/curl" {
		t.Errorf("curl install_paths: got %v", curl.InstallPaths)
	}
	if curl.PayloadExtra["extra_payload_field"] != `"seen"` {
		t.Errorf("payload_extra: got %v", curl.PayloadExtra)
	}
	// A key on host_software itself (a sibling of "software", one level
	// deeper than the payload's own hostname/host_software) must still reach
	// PayloadExtra, prefixed so it cannot be confused with a payload-level key.
	if curl.PayloadExtra["host_software.collected_at"] != `"2026-09-23T10:00:00Z"` {
		t.Errorf("payload_extra[host_software.collected_at]: got %v", curl.PayloadExtra)
	}

	if mystery.Is64Bit != nil {
		t.Errorf("mystery is_64_bit (never reported): got %v, want nil, not false", mystery.Is64Bit)
	}
	if mystery.Extra["vendor_specific_field"] != `"x"` {
		t.Errorf("mystery extra: got %v", mystery.Extra)
	}
	// payload_extra is the SAME map, repeated onto every row of this payload.
	if mystery.PayloadExtra["extra_payload_field"] != `"seen"` {
		t.Errorf("mystery payload_extra: got %v", mystery.PayloadExtra)
	}
}

// ---------------------------------------------------------------------------
// http-synthetics.<site>

// Pins assertion order/count (the handler used to only tally valid==true),
// the failure-object-presence distinction, and that a numeric netstat value
// travels through json.Number rather than float64.
func TestSyntheticsResultRow(t *testing.T) {
	body := []byte(`{
		"test": {"id": "t-1", "name": "check google", "type": "api", "subType": "http", "version": "1"},
		"location": {"id": "loc-1", "name": "aws:eu-west-1", "displayName": "EU West"},
		"result": {
			"id": "r-1", "initialId": "r-1", "status": "passed", "runType": "scheduled",
			"duration": 123.45,
			"testStartedAt": "2026-09-23T10:30:00Z",
			"testFinishedAt": "2026-09-23T10:30:01Z",
			"assertions": [
				{"type": "statusCode", "operator": "is", "expected": 200, "actual": 200, "valid": true},
				{"type": "responseTime", "operator": "lessThan", "expected": 1000, "actual": 1500, "valid": false}
			],
			"netstats": {"packetsSent": 9007199254740993, "packetsReceived": 10, "packetLossPercentage": 0.5,
				"jitter": 1.2, "latency": 42.5, "hops": 3}
		},
		"_dd": {"origin": "synthetics"},
		"v": 5
	}`)

	m, ok := evpObject("test", body)
	if !ok {
		t.Fatalf("evpObject failed to decode the fixture")
	}

	row := syntheticsResultRow(m)

	if row.TestID != "t-1" || row.LocationID != "loc-1" || row.Status != "passed" {
		t.Errorf("test_id/location_id/status: got %q/%q/%q", row.TestID, row.LocationID, row.Status)
	}
	if len(row.AssertionType) != 2 || row.AssertionValid[0] != 1 || row.AssertionValid[1] != 0 {
		t.Fatalf("assertions: got type=%v valid=%v, want 2 entries, [1 0] valid — order and count both matter",
			row.AssertionType, row.AssertionValid)
	}
	if row.AssertionExpected[1] != "1000" {
		t.Errorf("assertion_expected[1]: got %q, want the raw JSON number 1000", row.AssertionExpected[1])
	}
	if row.FailureCode != nil || row.FailureMessage != nil {
		t.Errorf("failure fields with no failure object: got code=%v message=%v, want nil (not empty string)",
			row.FailureCode, row.FailureMessage)
	}
	// A value one bit above float64's exact-integer range (2^53): only
	// survives if it went through json.Number, never float64.
	if row.NetstatsPacketsSent == nil || *row.NetstatsPacketsSent != 9007199254740993 {
		t.Errorf("netstats_packets_sent: got %v, want 9007199254740993 exactly (not rounded via float64)",
			row.NetstatsPacketsSent)
	}
	if row.NetstatsPacketLossPercentage == nil || *row.NetstatsPacketLossPercentage != 0.5 {
		t.Errorf("netstats_packet_loss_percentage: got %v, want 0.5", row.NetstatsPacketLossPercentage)
	}
	if row.DD != `{"origin":"synthetics"}` {
		t.Errorf("dd: got %q", row.DD)
	}
	if row.V != "5" {
		t.Errorf("v: got %q, want the literal 5 as sent", row.V)
	}
	if row.Extra != nil {
		t.Errorf("extra: got %v, want nil — every top-level key here is documented", row.Extra)
	}

	// A result WITH a failure object: presence itself is the signal.
	withFailure, ok := evpObject("test", []byte(`{"result":{"failure":{"code":"TIMEOUT","message":"no response"}}}`))
	if !ok {
		t.Fatalf("evpObject failed to decode the failure fixture")
	}
	fr := syntheticsResultRow(withFailure)
	if fr.FailureCode == nil || *fr.FailureCode != "TIMEOUT" {
		t.Errorf("failure_code: got %v, want TIMEOUT", fr.FailureCode)
	}
}

// An unknown key directly on the result object — a sibling of id/status/
// duration/assertions/config/..., not inside any of them — must still reach
// Extra rather than disappearing; before this, only top-level keys (test,
// location, result, _dd, enrichment, v) had a catch-all at all.
func TestSyntheticsResultRowUnknownResultKey(t *testing.T) {
	m, ok := evpObject("test", []byte(`{"result":{"status":"passed","region":"eu-west-1"}}`))
	if !ok {
		t.Fatalf("evpObject failed to decode the fixture")
	}
	row := syntheticsResultRow(m)
	if row.Extra["result.region"] != `"eu-west-1"` {
		t.Errorf("extra: got %v, want result.region to hold the undeclared key", row.Extra)
	}
}

// ---------------------------------------------------------------------------
// data-obs-intake.<site> — /api/v1/lineage

// Facets are OpenLineage's own extension point — genuinely open-ended, kept
// as opaque JSON — and inputs/outputs are independent lists with their own
// index alignment, not a shared one.
func TestOpenLineageEventRow(t *testing.T) {
	body := []byte(`{
		"eventType": "COMPLETE",
		"eventTime": "2026-09-23T10:30:00Z",
		"producer": "https://example.com/spark",
		"schemaURL": "https://openlineage.io/spec/1-0-5/OpenLineage.json",
		"run": {"runId": "run-1", "facets": {"parent": {"run": {"runId": "run-0"}}}},
		"job": {"namespace": "etl", "name": "load_orders", "facets": {"sql": {"query": "SELECT 1"}}},
		"inputs": [{"namespace": "warehouse", "name": "raw_orders", "facets": {"schema": {"fields": []}}}],
		"outputs": [
			{"namespace": "warehouse", "name": "orders"},
			{"namespace": "warehouse", "name": "orders_summary"}
		],
		"custom_extension_field": "seen"
	}`)

	m, ok := evpObject("test", body)
	if !ok {
		t.Fatalf("evpObject failed to decode the fixture")
	}

	row := openLineageEventRow(m, "2", "trace-agent/7")

	if row.RunID != "run-1" || row.EventType != "COMPLETE" {
		t.Errorf("run_id/event_type: got %q/%q", row.RunID, row.EventType)
	}
	if row.RunFacets != `{"parent":{"run":{"runId":"run-0"}}}` {
		t.Errorf("run_facets (opaque JSON): got %q", row.RunFacets)
	}
	if len(row.InputNamespace) != 1 || row.InputName[0] != "raw_orders" {
		t.Errorf("inputs: got namespace=%v name=%v", row.InputNamespace, row.InputName)
	}
	if len(row.OutputName) != 2 || row.OutputName[0] != "orders" || row.OutputName[1] != "orders_summary" {
		t.Fatalf("outputs: got %v, want [orders orders_summary] in order", row.OutputName)
	}
	if row.APIVersion != "2" || row.Via != "trace-agent/7" {
		t.Errorf("api_version/via: got %q/%q", row.APIVersion, row.Via)
	}
	if row.Extra["custom_extension_field"] != `"seen"` {
		t.Errorf("extra: got %v", row.Extra)
	}
}

// ---------------------------------------------------------------------------
// Handler-level tests: real engine, real tenant, real storage capture.

// A protobuf batch through the real engine must reach agent_discovery with
// the tenant from the API key attached to every row.
func TestHandleAgentDiscoveryStoresEveryPayload(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAgentDiscovery)

	batch := &agentdiscovery.AgentDiscoveryPayloadBatch{
		HostId: "host-1",
		Payloads: []*agentdiscovery.AgentDiscoveryPayload{
			{Integration: "postgres", Runtime: "python", RuntimeId: "rt-1"},
		},
	}
	body, err := proto.Marshal(batch)
	if err != nil {
		t.Fatalf("proto.Marshal: %v", err)
	}

	if w := post(t, e, "/api/v2/agentdiscovery", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/agentdiscovery: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.AgentDiscoveryRow](node)
	if len(rows) != 1 {
		t.Fatalf("stored rows: got %d, want 1", len(rows))
	}
	if rows[0].TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", rows[0].TenantID, testTenant)
	}
	if rows[0].Integration != "postgres" {
		t.Errorf("integration: got %q", rows[0].Integration)
	}
}

// softinv is the batched-list track: one bad item must not lose the good
// ones, and the bad item's raw bytes must reach raw_payloads rather than
// vanishing.
func TestHandleSoftwareInventoryStoresRowsAndRawOnBadItem(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSoftwareInventory)

	body := []byte(`[
		{"hostname": "web-1", "host_software": {"software": [
			{"software_type": "package", "name": "curl", "version": "7.88.1"}
		]}},
		"not-an-object"
	]`)

	if w := post(t, e, "/api/v2/softinv", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/softinv: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.HostSoftwareRow](node)
	if len(rows) != 1 {
		t.Fatalf("host_software rows: got %d, want 1", len(rows))
	}
	if rows[0].TenantID != testTenant || rows[0].Name != "curl" {
		t.Errorf("row: got tenant=%q name=%q", rows[0].TenantID, rows[0].Name)
	}

	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1 (the malformed item)", len(raw))
	}
	if raw[0].Intake != "softinv" || raw[0].Reason != "decode_error" {
		t.Errorf("raw payload intake/reason: got %q/%q", raw[0].Intake, raw[0].Reason)
	}
	if raw[0].Body != `"not-an-object"` {
		t.Errorf("raw payload body: got %q, want the item verbatim", raw[0].Body)
	}
}

// An envelope with no data.attributes is the one shape event-management
// cannot store as a row — it must go to raw_payloads, not disappear.
func TestHandleEventManagementUnexpectedShapeGoesToRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeEventManagement)

	body := []byte(`{"data": {"type": "event"}}`)

	if w := post(t, e, "/api/v2/events", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/events: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	if rows := Rows[storage.EventManagementEventRow](node); len(rows) != 0 {
		t.Errorf("event_management_events rows: got %d, want 0", len(rows))
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "unexpected_shape" {
		t.Fatalf("raw_payloads: got %+v, want one row with reason unexpected_shape", raw)
	}
}

// query-actions has no Go type at all — the handler must still capture the
// exact bytes as JSON and the caller's DD-EVP-ORIGIN headers.
func TestHandleQueryActionsStoresResultVerbatim(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDataObs)

	body := []byte(`[{"whatever_the_integration_sent": 42, "nested": {"a": 1}}]`)
	req := httptest.NewRequest(http.MethodPost, "/api/v2/query-actions", bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Dd-Evp-Origin", "python-integration")
	req.Header.Set("Dd-Evp-Origin-Version", "1.2.3")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/query-actions: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.QueryActionResultRow](node)
	if len(rows) != 1 {
		t.Fatalf("query_action_results rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q", r.TenantID)
	}
	if r.Result != `{"nested":{"a":1},"whatever_the_integration_sent":42}` {
		t.Errorf("result: got %q", r.Result)
	}
	if len(r.Keys) != 2 || r.Keys[0] != "nested" || r.Keys[1] != "whatever_the_integration_sent" {
		t.Errorf("keys: got %v, want sorted top-level keys", r.Keys)
	}
	if r.DDEVPOrigin != "python-integration" || r.DDEVPOriginVersion != "1.2.3" {
		t.Errorf("dd_evp_origin/version: got %q/%q", r.DDEVPOrigin, r.DDEVPOriginVersion)
	}
}

// ---------------------------------------------------------------------------
// Integration: proves the migration, the INSERT and AppendTo agree with
// ClickHouse's own types for the write-heavy end of this file's tables.
func TestEVPTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	reportID := uuid.New()

	storagetest.Insert(t, conn, storage.AgentDiscoveryWriter, []storage.Row{
		storage.AgentDiscoveryRow{
			TenantID: "default", ReceivedAt: now,
			HostID: "host-1", Integration: "postgres", Runtime: "python", RuntimeID: "rt-1",
			ConfigPaths: []string{"/etc/x.yaml"}, ConfigContents: []string{"a: 1"},
			ConfigTruncated: []uint8{0}, ConfigFormats: []string{"PAYLOAD_FORMAT_YAML"},
			EnvVarNames: []string{"PGPASSWORD"}, EnvVarValues: []string{"hunter2"},
		},
	})
	storagetest.Insert(t, conn, storage.AgentHealthReportsWriter, []storage.Row{
		storage.AgentHealthReportRow{
			TenantID: "default", ReceivedAt: now, ReportID: reportID,
			SchemaVersion: "1.0", EventType: "health_report", Host: "web-1", IssueCount: 1,
		},
	})
	storagetest.Insert(t, conn, storage.AgentHealthIssuesWriter, []storage.Row{
		storage.AgentHealthIssueRow{
			TenantID: "default", ReceivedAt: now, ReportID: reportID,
			IssueKey: "disk_full", ID: "disk_full", Severity: "ISSUE_SEVERITY_HIGH",
			Tags: map[string][]string{"env": {"prod", "staging"}},
		},
	})
	storagetest.Insert(t, conn, storage.EventManagementWriter, []storage.Row{
		storage.EventManagementEventRow{
			TenantID: "default", ReceivedAt: now, DataType: "event", Host: "web-1",
			Tags: map[string][]string{"env": {"prod"}},
		},
	})
	storagetest.Insert(t, conn, storage.HostSoftwareWriter, []storage.Row{
		storage.HostSoftwareRow{TenantID: "default", ReceivedAt: now, Hostname: "web-1", Name: "curl"},
	})
	storagetest.Insert(t, conn, storage.SyntheticsResultsWriter, []storage.Row{
		storage.SyntheticsResultRow{TenantID: "default", ReceivedAt: now, TestID: "t-1", Status: "passed"},
	})
	storagetest.Insert(t, conn, storage.OpenLineageWriter, []storage.Row{
		storage.OpenLineageEventRow{TenantID: "default", ReceivedAt: now, EventType: "COMPLETE", RunID: "run-1"},
	})
	storagetest.Insert(t, conn, storage.QueryActionResultsWriter, []storage.Row{
		storage.QueryActionResultRow{TenantID: "default", ReceivedAt: now, Result: `{"a":1}`, Keys: []string{"a"}},
	})

	for _, table := range []string{
		"agent_discovery", "agent_health_reports", "agent_health_issues",
		"event_management_events", "host_software", "synthetics_results",
		"openlineage_events", "query_action_results",
	} {
		if got := storagetest.Count(t, conn, table); got != 1 {
			t.Errorf("%s: %d rows, want 1", table, got)
		}
	}

	// Values, not just counts: the multiset tag map and the sensitive env
	// var value both need to survive the round trip intact.
	row := storagetest.QueryRow(t, conn, "SELECT integration, env_var_names[1], env_var_values[1] FROM agent_discovery")
	if row[0] != "postgres" || row[1] != "PGPASSWORD" || row[2] != "hunter2" {
		t.Errorf("agent_discovery: got integration=%v env_var_name=%v env_var_value=%v", row[0], row[1], row[2])
	}

	row = storagetest.QueryRow(t, conn, "SELECT has(tags['env'], 'staging') FROM agent_health_issues")
	if row[0] != uint8(1) {
		t.Errorf("agent_health_issues tags: got %v, want has(tags['env'],'staging')=1", row[0])
	}
}
