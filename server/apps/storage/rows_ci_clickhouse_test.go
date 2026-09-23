package storage_test

import (
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// A real round trip for every table migration 0017 adds.
//
// The arity tests next door prove AppendTo and the INSERT agree about COUNT.
// Only ClickHouse proves they agree about TYPES, and this family of tables
// has more ways to get that wrong than most: eight UInt64 ids that must not
// arrive as strings, four Nullable(UInt8) flags fed from *uint8, a
// Nullable(UInt64) span id, two parallel Array(String) columns, a
// Map(k, Array(k)) tag multiset next to three plain maps and one
// Map(k, Float64), and a DateTime64(9) that has to keep nanoseconds.
//
// Skips when ClickHouse is not running (OpenScratchDB);
// NINJACAT_TEST_REQUIRE_CLICKHOUSE=1 turns that skip into a failure.

// ciTestTime is a real timestamp, not time.Time{}. Measured and documented in
// storagetest's own round trip: a zero-value time inserts successfully, lands
// outside DateTime64's range, and is then dropped on the spot by the table's
// TTL — so a test built on zero times asserts on rows that never existed.
var ciTestTime = time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

func u8(v uint8) *uint8    { return &v }
func i64(v int64) *int64   { return &v }
func u64(v uint64) *uint64 { return &v }
func str(v string) *string { return &v }

// ci_test_events is the widest row in the tree, and the one where a type
// mismatch is most likely: the ids and the two source-line columns look like
// numbers on the wire and have to stay numbers in the column.
func TestCITestEventRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// 2^53 + 1: the value that proves the id never went through a float64.
	const bigSession = uint64(9007199254740993)

	storagetest.Insert(t, conn, storage.CITestEventsWriter, []storage.Row{
		storage.CITestEventRow{
			TenantID:       "default",
			ReceivedAt:     ciTestTime,
			EventType:      "test",
			EventVersion:   2,
			PayloadVersion: 1,
			SessionID:      bigSession,
			ModuleID:       234567890,
			SuiteID:        123123123,
			TraceID:        1111111111111111111,
			SpanID:         2222222222222222222,
			ParentID:       0,
			Service:        "my-service",
			Env:            "ci",
			Name:           "go.test",
			Resource:       "pkg.TestThing",
			SpanType:       "test",
			// Nanosecond precision has to survive DateTime64(9) — two tests
			// in the same millisecond are otherwise unorderable.
			Start:               time.Date(2026, 9, 23, 10, 30, 0, 123456789, time.UTC),
			DurationNs:          796143,
			Error:               0,
			TestName:            "TestThing",
			TestSuite:           "thing_test.go",
			TestStatus:          "pass",
			TestSourceStart:     i64(120),
			TestSourceEnd:       i64(180),
			TestIsRetry:         u8(1),
			TestIsNew:           u8(0),
			CodeCoverageEnabled: nil, // the tracer never said
			GitRepositoryURL:    "https://github.com/itsninjacats/ninjacat",
			CIProviderName:      "github",
			OSPlatform:          "linux",
			Language:            "go",
			LibraryVersion:      "2.4.0",
			EVPSubdomain:        "citestcycle-intake",
			AgentVersion:        "7.83.0",
			Meta:                map[string]string{"test.name": "TestThing"},
			Metrics:             map[string]float64{"test.source.start": 120},
			Metadata:            map[string]string{"language": "go"},
			Content:             `{"name":"go.test"}`,
		},
	})

	if got := storagetest.Count(t, conn, "ci_test_events"); got != 1 {
		t.Fatalf("ci_test_events: %d rows, want 1", got)
	}

	row := storagetest.QueryRow(t, conn,
		`SELECT session_id, span_id, test_source_start, code_coverage_enabled,
		        test_is_retry, toUnixTimestamp64Nano(start), meta['test.name'],
		        metrics['test.source.start'], content
		 FROM ci_test_events`)
	if len(row) != 9 {
		t.Fatalf("SELECT returned %d columns, want 9", len(row))
	}
	if row[0] != bigSession {
		t.Errorf("session_id: got %v (%T), want %d — an id above 2^53 must survive the column too",
			row[0], row[0], bigSession)
	}
	if row[1] != uint64(2222222222222222222) {
		t.Errorf("span_id: got %v", row[1])
	}
	// Nullable(Int64) comes back as *int64.
	if p, ok := row[2].(*int64); !ok || p == nil || *p != 120 {
		t.Errorf("test_source_start: got %v (%T), want *int64(120)", row[2], row[2])
	}
	// A flag the tracer never set stays NULL rather than becoming false.
	if p, ok := row[3].(*uint8); !ok || p != nil {
		t.Errorf("code_coverage_enabled: got %v (%T), want a nil *uint8 — absent is not false", row[3], row[3])
	}
	if p, ok := row[4].(*uint8); !ok || p == nil || *p != 1 {
		t.Errorf("test_is_retry: got %v (%T), want *uint8(1)", row[4], row[4])
	}
	if row[5] != int64(1790159400123456789) {
		t.Errorf("start: got %v ns, want 1790159400123456789 — DateTime64(9) must keep nanoseconds", row[5])
	}
	if row[6] != "TestThing" {
		t.Errorf("meta['test.name']: got %v", row[6])
	}
	if row[7] != float64(120) {
		t.Errorf("metrics['test.source.start']: got %v (%T)", row[7], row[7])
	}
	if row[8] != `{"name":"go.test"}` {
		t.Errorf("content: got %v", row[8])
	}
}

// ci_coverage pins the two things unique to it: a Nullable(UInt64) span id
// that is genuinely absent in suite-skipping mode, and the two parallel
// arrays that must stay the same length.
func TestCICoverageRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	storagetest.Insert(t, conn, storage.CICoverageWriter, []storage.Row{
		storage.CICoverageRow{
			TenantID:       "default",
			ReceivedAt:     ciTestTime,
			PayloadVersion: 2,
			SessionID:      123456789,
			SuiteID:        123123123,
			SpanID:         u64(2222222222222222222),
			FilesFilename:  []string{"intake/router_civisibility.go", "intake/raw.go"},
			// The second file was reported without line-level data; its slot
			// stays empty rather than shifting the arrays out of step.
			FilesBitmap: []string{"\x01\x02\x03", ""},
			Raw:         "\x82\xa1a\x01",
			RawFormat:   "msgpack",
			Event:       `{"dummy": true}`,
			Extra:       map[string]string{"new_key": "1"},
		},
		storage.CICoverageRow{
			TenantID:       "default",
			ReceivedAt:     ciTestTime,
			PayloadVersion: 2,
			SessionID:      123456789,
			SuiteID:        999,
			// Suite-level coverage has no span to attribute to, and 0 is a
			// legal span id — so NULL.
			SpanID:        nil,
			FilesFilename: []string{"apps/storage/rows_ci.go"},
			FilesBitmap:   []string{""},
			RawFormat:     "msgpack",
		},
	})

	if got := storagetest.Count(t, conn, "ci_coverage"); got != 2 {
		t.Fatalf("ci_coverage: %d rows, want 2", got)
	}

	row := storagetest.QueryRow(t, conn,
		`SELECT span_id, files_filename, files_bitmap, raw_format, extra['new_key']
		 FROM ci_coverage WHERE suite_id = 123123123`)
	if len(row) != 5 {
		t.Fatalf("SELECT returned %d columns, want 5", len(row))
	}
	if p, ok := row[0].(*uint64); !ok || p == nil || *p != 2222222222222222222 {
		t.Errorf("span_id: got %v (%T)", row[0], row[0])
	}
	names, _ := row[1].([]string)
	bitmaps, _ := row[2].([]string)
	if len(names) != 2 || len(bitmaps) != 2 {
		t.Fatalf("parallel arrays came back as %d filenames and %d bitmaps", len(names), len(bitmaps))
	}
	if names[0] != "intake/router_civisibility.go" || bitmaps[1] != "" {
		t.Errorf("arrays: %v / %q", names, bitmaps)
	}
	if bitmaps[0] != "\x01\x02\x03" {
		t.Errorf("bitmap bytes must survive a String column verbatim, got %q", bitmaps[0])
	}
	if row[3] != "msgpack" || row[4] != "1" {
		t.Errorf("raw_format/extra: %v / %v", row[3], row[4])
	}

	nullSpan := storagetest.QueryRow(t, conn, "SELECT span_id FROM ci_coverage WHERE suite_id = 999")
	if p, ok := nullSpan[0].(*uint64); !ok || p != nil {
		t.Errorf("span_id: got %v (%T), want a nil *uint64", nullSpan[0], nullSpan[0])
	}
}

// git_commits is a ReplacingMergeTree: the same sha seen twice must collapse
// to one row, and the later sighting — the one that carries a packfile id —
// must be the one that wins.
func TestGitCommitRoundTripReplaces(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	packfileID := "3b1f" // short on purpose: the column is a String, not a hash type
	storagetest.Insert(t, conn, storage.GitCommitsWriter, []storage.Row{
		storage.GitCommitRow{
			TenantID:      "default",
			RepositoryURL: "https://github.com/itsninjacats/ninjacat",
			SHA:           "aaa111",
			SeenAt:        ciTestTime,
			// A search_commits candidate is a QUESTION about a sha, so no
			// packfile id: claiming one would say we hold objects we have
			// never seen.
			PackfileID: nil,
			Source:     "search_commits",
		},
	})
	storagetest.Insert(t, conn, storage.GitCommitsWriter, []storage.Row{
		storage.GitCommitRow{
			TenantID:      "default",
			RepositoryURL: "https://github.com/itsninjacats/ninjacat",
			SHA:           "aaa111",
			SeenAt:        ciTestTime.Add(time.Minute),
			PackfileID:    str(packfileID),
			Source:        "packfile",
		},
	})

	// FINAL, because ReplacingMergeTree only collapses on merge — the same
	// caveat the hosts table documents.
	row := storagetest.QueryRow(t, conn,
		"SELECT sha, packfile_id, source FROM git_commits FINAL")
	if len(row) != 3 {
		t.Fatalf("SELECT returned %d columns, want 3", len(row))
	}
	if row[0] != "aaa111" {
		t.Errorf("sha: got %v", row[0])
	}
	if p, ok := row[1].(*string); !ok || p == nil || *p != packfileID {
		t.Errorf("packfile_id: got %v (%T), want the later sighting to win", row[1], row[1])
	}
	if row[2] != "packfile" {
		t.Errorf("source: got %v, want packfile", row[2])
	}
}

func TestGitPackfileRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	pack := "PACK\x00\x00\x00\x02\x00\x00\x00\x01binary"
	storagetest.Insert(t, conn, storage.GitPackfilesWriter, []storage.Row{
		storage.GitPackfileRow{
			TenantID:      "default",
			ReceivedAt:    ciTestTime,
			RepositoryURL: "https://github.com/itsninjacats/ninjacat",
			PushedSHA:     "headsha123",
			PackfileID:    "3b1f",
			Filename:      "pack-abc.pack",
			Packfile:      pack,
			SizeBytes:     uint64(len(pack)),
			OtherParts:    map[string]string{"unexpected": "part"},
		},
	})

	row := storagetest.QueryRow(t, conn,
		"SELECT pushed_sha, packfile, size_bytes, other_parts['unexpected'] FROM git_packfiles")
	if len(row) != 4 {
		t.Fatalf("SELECT returned %d columns, want 4", len(row))
	}
	if row[0] != "headsha123" {
		t.Errorf("pushed_sha: got %v", row[0])
	}
	// A pack is binary and must come back byte for byte — a String column
	// holds bytes, not text, and nothing here may re-encode it.
	if row[1] != pack {
		t.Errorf("packfile: got %q, want the bytes verbatim", row[1])
	}
	if row[2] != uint64(len(pack)) {
		t.Errorf("size_bytes: got %v (%T)", row[2], row[2])
	}
	if row[3] != "part" {
		t.Errorf("other_parts: got %v", row[3])
	}
}

func TestCISettingsRequestRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	storagetest.Insert(t, conn, storage.CISettingsRequestsWriter, []storage.Row{
		storage.CISettingsRequestRow{
			TenantID:       "default",
			ReceivedAt:     ciTestTime,
			Endpoint:       "settings",
			RequestID:      "req-1",
			RequestType:    "ci_app_test_service_libraries_settings",
			Service:        "my-service",
			Env:            "ci",
			RepositoryURL:  "https://github.com/itsninjacats/ninjacat",
			Branch:         "feat/write-layer",
			SHA:            "0f4c2a1b",
			TestLevel:      "test",
			Configurations: `{"os.platform":"linux"}`,
			RequestBody:    `{"data":{}}`,
			ResponseBody:   `{"data":{"attributes":{"itr_enabled":false}}}`,
		},
	})

	row := storagetest.QueryRow(t, conn,
		"SELECT endpoint, request_id, service, configurations, response_body FROM ci_settings_requests")
	if len(row) != 5 {
		t.Fatalf("SELECT returned %d columns, want 5", len(row))
	}
	if row[0] != "settings" || row[1] != "req-1" || row[2] != "my-service" {
		t.Errorf("identity columns: %v / %v / %v", row[0], row[1], row[2])
	}
	if row[3] != `{"os.platform":"linux"}` {
		t.Errorf("configurations: got %v", row[3])
	}
	// The answer we gave is stored because it is a contract we invented:
	// the day it changes, what a tracer was told is the difference between
	// a bug report and a mystery.
	if row[4] != `{"data":{"attributes":{"itr_enabled":false}}}` {
		t.Errorf("response_body: got %v", row[4])
	}
}

// ci_webhook_events carries the one genuine tag multiset in this family,
// next to three plain maps — exactly the mix a Map(k, Array(k)) type error
// hides in.
func TestCIWebhookEventRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	start := ciTestTime
	end := ciTestTime.Add(6*time.Minute + 30*time.Second)

	storagetest.Insert(t, conn, storage.CIWebhookEventsWriter, []storage.Row{
		storage.CIWebhookEventRow{
			TenantID:         "default",
			ReceivedAt:       ciTestTime,
			Provider:         "jenkins",
			Level:            "pipeline",
			Service:          "jenkins-prod",
			PayloadVersion:   i64(1),
			PartialRetry:     u8(0),
			IsManual:         u8(1),
			TraceID:          "5678901234567890123",
			SpanID:           "1234567890123456789",
			UniqueID:         "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9",
			PipelineID:       "42",
			PipelineUniqueID: "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9",
			Name:             "ninjacat",
			URL:              "https://jenkins.example.test/job/ninjacat/42/",
			Status:           "success",
			StartRaw:         "2026-09-23T10:30:00.000Z",
			StartParsed:      &start,
			EndRaw:           "2026-09-23T10:36:30.000Z",
			EndParsed:        &end,
			QueueTimeMs:      i64(1500),
			NodeName:         "built-in",
			NodeLabels:       []string{"linux", "docker"},
			GitBranch:        "feat/write-layer",
			UserName:         "michal",
			Parameters:       map[string]string{"BRANCH": "feat/write-layer"},
			// Two values under one key: the shape a plain Map would silently
			// halve. See docs/decisions/0001-tags-are-a-multiset.md.
			Tags:       map[string][]string{"kube_service": {"a", "b"}, "bare_tag": {""}},
			DeliveryID: "d6f1c0a2",
			Headers:    map[string]string{"User-Agent": "Jenkins/2.4"},
			Extra:      map[string]string{"something_new": `{"later":true}`},
			Body:       `{"level":"pipeline"}`,
		},
	})

	row := storagetest.QueryRow(t, conn,
		`SELECT provider, level, tags['kube_service'], tags['bare_tag'], node_labels,
		        payload_version, is_manual, queue_time_ms, start_parsed, parameters['BRANCH']
		 FROM ci_webhook_events`)
	if len(row) != 10 {
		t.Fatalf("SELECT returned %d columns, want 10", len(row))
	}
	if row[0] != "jenkins" || row[1] != "pipeline" {
		t.Errorf("provider/level: %v / %v", row[0], row[1])
	}
	kube, _ := row[2].([]string)
	if len(kube) != 2 || kube[0] != "a" || kube[1] != "b" {
		t.Errorf("tags['kube_service']: got %v, want both values — a repeated tag key must not lose one", row[2])
	}
	bare, _ := row[3].([]string)
	if len(bare) != 1 || bare[0] != "" {
		t.Errorf("tags['bare_tag']: got %v, want one empty value", row[3])
	}
	labels, _ := row[4].([]string)
	if len(labels) != 2 {
		t.Errorf("node_labels: got %v", row[4])
	}
	if p, ok := row[5].(*int64); !ok || p == nil || *p != 1 {
		t.Errorf("payload_version: got %v (%T)", row[5], row[5])
	}
	if p, ok := row[6].(*uint8); !ok || p == nil || *p != 1 {
		t.Errorf("is_manual: got %v (%T)", row[6], row[6])
	}
	if p, ok := row[7].(*int64); !ok || p == nil || *p != 1500 {
		t.Errorf("queue_time_ms: got %v (%T)", row[7], row[7])
	}
	if p, ok := row[8].(*time.Time); !ok || p == nil || !p.Equal(start) {
		t.Errorf("start_parsed: got %v (%T), want %v", row[8], row[8], start)
	}
	if row[9] != "feat/write-layer" {
		t.Errorf("parameters['BRANCH']: got %v", row[9])
	}
}

func TestCIPipelineEventRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	storagetest.Insert(t, conn, storage.CIPipelineEventsWriter, []storage.Row{
		storage.CIPipelineEventRow{
			TenantID:   "default",
			ReceivedAt: ciTestTime,
			Kind:       "measure",
			DataType:   "ci_custom_metric",
			Provider:   "gitlab",
			CILevel:    i64(3),
			// ci_env is the join key back to whatever pipeline a webhook or a
			// tracer already reported, which is why it is a real map and not
			// just part of the raw body.
			CIEnv:      map[string]string{"CI_JOB_ID": "77"},
			Tags:       map[string]string{"team": "backend"},
			Metrics:    map[string]float64{"build.seconds": 42.5},
			Attributes: `{"metrics":{"build.seconds":42.5}}`,
			Body:       `{"data":{"type":"ci_custom_metric"}}`,
			Extra:      map[string]string{"unknown": "1"},
		},
	})

	row := storagetest.QueryRow(t, conn,
		`SELECT kind, data_type, ci_level, ci_env['CI_JOB_ID'], tags['team'],
		        metrics['build.seconds'], attributes
		 FROM ci_pipeline_events`)
	if len(row) != 7 {
		t.Fatalf("SELECT returned %d columns, want 7", len(row))
	}
	if row[0] != "measure" || row[1] != "ci_custom_metric" {
		t.Errorf("kind/data_type: %v / %v", row[0], row[1])
	}
	if p, ok := row[2].(*int64); !ok || p == nil || *p != 3 {
		t.Errorf("ci_level: got %v (%T), want *int64(3) — a custom span sends none at all", row[2], row[2])
	}
	if row[3] != "77" || row[4] != "backend" {
		t.Errorf("ci_env/tags: %v / %v", row[3], row[4])
	}
	if row[5] != 42.5 {
		t.Errorf("metrics: got %v (%T)", row[5], row[5])
	}
	if row[6] != `{"metrics":{"build.seconds":42.5}}` {
		t.Errorf("attributes: got %v", row[6])
	}
}

// synthetics_test_configs has no producer yet — the agent's poller READS it
// and a future panel writes it. The round trip is what proves the table and
// its writer exist and agree, so the day something does write to it, the
// only new thing is the caller.
func TestSyntheticsTestConfigRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	storagetest.Insert(t, conn, storage.SyntheticsTestConfigsWriter, []storage.Row{
		storage.SyntheticsTestConfigRow{
			TenantID:  "default",
			UpdatedAt: ciTestTime,
			PublicID:  "abc-def-ghi",
			Version:   1,
			Type:      "api",
			// Only UDP, TCP and ICMP are valid: the agent's custom
			// UnmarshalJSON switches on this field and an unrecognised value
			// fails the WHOLE poll response, not just this test.
			Subtype:    "TCP",
			TickEvery:  60,
			OrgID:      2,
			MainDC:     "us1.prod",
			ResultID:   "res-1",
			RunType:    "scheduled",
			Assertions: `[{"operator":"lessThan","property":"avg","target":"100","type":"latency"}]`,
			Request:    `{"host":"example.test","port":443,"tcp_method":"syn"}`,
			Enabled:    1,
		},
	})

	// FINAL: ReplacingMergeTree, same caveat as git_commits.
	row := storagetest.QueryRow(t, conn,
		`SELECT public_id, subtype, tick_every, org_id, run_type, assertions, request, enabled
		 FROM synthetics_test_configs FINAL`)
	if len(row) != 8 {
		t.Fatalf("SELECT returned %d columns, want 8", len(row))
	}
	if row[0] != "abc-def-ghi" || row[1] != "TCP" {
		t.Errorf("public_id/subtype: %v / %v", row[0], row[1])
	}
	if row[2] != int32(60) {
		t.Errorf("tick_every: got %v (%T), want int32(60)", row[2], row[2])
	}
	if row[3] != int64(2) {
		t.Errorf("org_id: got %v (%T)", row[3], row[3])
	}
	if row[4] != "scheduled" {
		t.Errorf("run_type: got %v", row[4])
	}
	if row[5] == "" || row[6] == "" {
		t.Errorf("assertions/request must round-trip as JSON text: %v / %v", row[5], row[6])
	}
	if row[7] != uint8(1) {
		t.Errorf("enabled: got %v (%T)", row[7], row[7])
	}
}
