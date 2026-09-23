package intake

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
	"time"

	"github.com/DataDog/datadog-api-client-go/v2/api/datadogV2"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// arrival is the fixed "now" the timestamp tests fall back to. A literal, so a
// test can tell "we used arrival time" from "we parsed something" — with
// time.Now() on both sides the two are indistinguishable.
var arrival = time.Date(2026, 9, 23, 12, 0, 0, 0, time.UTC)

// decodeLogItem builds the model the handler works on, the way the wire does:
// through the generated UnmarshalJSON, which is what puts `status`,
// `timestamp` and every custom attribute into AdditionalProperties as
// json.Number rather than float64.
func decodeLogItem(t *testing.T, body string) datadogV2.HTTPLogItem {
	t.Helper()
	var e datadogV2.HTTPLogItem
	if err := json.Unmarshal([]byte(body), &e); err != nil {
		t.Fatalf("decode %s: %v", body, err)
	}
	return e
}

// Four wire forms carry the same idea and the handler used to read exactly
// one of them: `timestamp` as a millisecond number. A sender using any of the
// other three got ARRIVAL TIME silently — not a missing value but a wrong one,
// and one that passes every sanity check because it always looks plausible.
//
// The browser SDK sends `date`; several clients send RFC 3339 strings.
func TestLogTimestampResolutionOrder(t *testing.T) {
	ms := time.Date(2026, 9, 23, 8, 15, 30, 0, time.UTC)

	cases := []struct {
		name       string
		body       string
		wantTime   time.Time
		wantSource string
		// wantKept is an attribute that must still be in `attributes` after
		// the row is built, because it was NOT the one used.
		wantKept string
	}{
		{
			name:       "timestamp in milliseconds, what the agent sends",
			body:       `{"message":"m","timestamp":1790151330000}`,
			wantTime:   ms,
			wantSource: "timestamp_ms",
		},
		{
			name:       "timestamp as RFC 3339, which Datadog's intake also takes",
			body:       `{"message":"m","timestamp":"2026-09-23T08:15:30Z"}`,
			wantTime:   ms,
			wantSource: "timestamp_string",
		},
		{
			name:       "date in milliseconds",
			body:       `{"message":"m","date":1790151330000}`,
			wantTime:   ms,
			wantSource: "date_ms",
		},
		{
			name:       "date as an ISO-8601 string, what the browser SDK sends",
			body:       `{"message":"m","date":"2026-09-23T08:15:30Z"}`,
			wantTime:   ms,
			wantSource: "date_string",
		},
		{
			name:       "timestamp wins over date when both are present",
			body:       `{"message":"m","timestamp":1790151330000,"date":1000000000000}`,
			wantTime:   ms,
			wantSource: "timestamp_ms",
			wantKept:   "date",
		},
		{
			name:       "no timestamp at all: HTTPLogItem has no field for one",
			body:       `{"message":"m"}`,
			wantTime:   arrival,
			wantSource: "arrival",
		},
		{
			name: "an unparseable timestamp is arrival time AND stays readable",
			// The row says "arrival", so nobody reads the value as the
			// sender's clock, and the value itself is still in attributes,
			// so somebody can see what was actually sent.
			body:       `{"message":"m","timestamp":"yesterday"}`,
			wantTime:   arrival,
			wantSource: "arrival",
			wantKept:   "timestamp",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row, err := logRow(testTenant, decodeLogItem(t, tc.body), arrival, "", "", "", nil)
			if err != nil {
				t.Fatalf("logRow: %v", err)
			}
			if !row.Timestamp.Equal(tc.wantTime) {
				t.Errorf("timestamp: got %s, want %s", row.Timestamp, tc.wantTime)
			}
			if row.TimestampSource != tc.wantSource {
				t.Errorf("timestamp_source: got %q, want %q", row.TimestampSource, tc.wantSource)
			}
			if tc.wantKept != "" {
				var attrs map[string]any
				if row.Attributes != "" {
					if err := json.Unmarshal([]byte(row.Attributes), &attrs); err != nil {
						t.Fatalf("attributes is not JSON: %v", err)
					}
				}
				if _, ok := attrs[tc.wantKept]; !ok {
					t.Errorf("%q was dropped by both the column and the attributes map: %s",
						tc.wantKept, row.Attributes)
				}
			}
		})
	}
}

// Every attribute that did not become a column has to survive. dd.trace_id is
// the reason: it is the one key that joins a log line to its trace, and this
// handler used to COUNT it and throw it away.
func TestLogAttributesKeepEverythingWithoutAColumn(t *testing.T) {
	e := decodeLogItem(t, `{
		"message":"boom","hostname":"h1","service":"api","ddsource":"go","ddtags":"env:prod",
		"status":"error","timestamp":1790151330000,
		"dd.trace_id":"7277407061953423024","usr":{"id":"u-1","name":"Ada"},"http":{"status_code":500}
	}`)

	row, err := logRow(testTenant, e, arrival, "", "", "", nil)
	if err != nil {
		t.Fatalf("logRow: %v", err)
	}

	var attrs map[string]any
	if err := json.Unmarshal([]byte(row.Attributes), &attrs); err != nil {
		t.Fatalf("attributes is not JSON: %v", err)
	}

	// A 64-bit trace id must come out with every digit: through float64 it
	// would come back as 7277407061953423000 and join nothing.
	if got := attrs["dd.trace_id"]; got != "7277407061953423024" {
		t.Errorf("dd.trace_id: got %v, want the string exactly as sent", got)
	}
	if usr, ok := attrs["usr"].(map[string]any); !ok || usr["id"] != "u-1" {
		t.Errorf("nested usr object lost its shape: %v", attrs["usr"])
	}
	if _, ok := attrs["http"]; !ok {
		t.Errorf("http attributes dropped: %s", row.Attributes)
	}

	// And the keys that DID become columns are gone from here: a value in two
	// places is a value that can disagree with itself.
	for _, lifted := range []string{"status", "timestamp"} {
		if _, ok := attrs[lifted]; ok {
			t.Errorf("%q is both a column and an attribute", lifted)
		}
	}
	if row.Status != "error" {
		t.Errorf("status: got %q, want error", row.Status)
	}
}

// Datadog's intake accepts ddsource, ddtags, hostname/host and service as
// QUERY PARAMETERS, which is how a sender that cannot shape its body still
// says what it is. Reading only the body lost them with no trace at all — not
// even a count, since they are not part of any decoded item.
func TestLogRowTakesQueryParameterDefaults(t *testing.T) {
	cases := []struct {
		name         string
		body         string
		qSource      string
		qService     string
		qHost        string
		qTags        []string
		wantHost     string
		wantService  string
		wantSource   string
		wantTagCount int
	}{
		{
			name:     "the body wins for a scalar",
			body:     `{"message":"m","hostname":"body-host","service":"body-svc","ddsource":"body-src"}`,
			qSource:  "q-src",
			qService: "q-svc",
			qHost:    "q-host",
			// The item said it; the query was a default for items that did not.
			wantHost: "body-host", wantService: "body-svc", wantSource: "body-src",
		},
		{
			name:     "the query fills what the body left out",
			body:     `{"message":"m"}`,
			qSource:  "q-src",
			qService: "q-svc",
			qHost:    "q-host",
			wantHost: "q-host", wantService: "q-svc", wantSource: "q-src",
		},
		{
			name: "host is an accepted spelling of hostname",
			// It is not a declared field, so it arrives as an ordinary
			// attribute and used to leave the host column empty.
			body:     `{"message":"m","host":"attr-host"}`,
			qHost:    "q-host",
			wantHost: "attr-host",
		},
		{
			name:         "ddtags MERGE rather than override",
			body:         `{"message":"m","ddtags":"env:prod,service:api"}`,
			qTags:        []string{"team:core"},
			wantTagCount: 3,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row, err := logRow(testTenant, decodeLogItem(t, tc.body), arrival,
				tc.qSource, tc.qService, tc.qHost, tc.qTags)
			if err != nil {
				t.Fatalf("logRow: %v", err)
			}
			if tc.wantHost != "" && row.Host != tc.wantHost {
				t.Errorf("host: got %q, want %q", row.Host, tc.wantHost)
			}
			if tc.wantService != "" && row.Service != tc.wantService {
				t.Errorf("service: got %q, want %q", row.Service, tc.wantService)
			}
			if tc.wantSource != "" && row.Source != tc.wantSource {
				t.Errorf("source: got %q, want %q", row.Source, tc.wantSource)
			}
			if tc.wantTagCount > 0 {
				n := 0
				for _, v := range row.Tags {
					n += len(v)
				}
				if n != tc.wantTagCount {
					t.Errorf("tags: got %d values (%v), want %d — the query tags a batch, the item tags itself",
						n, row.Tags, tc.wantTagCount)
				}
			}
			// The `host` attribute is removed only when it was USED.
			if tc.name == "host is an accepted spelling of hostname" {
				if row.Attributes != "" {
					t.Errorf("host was used as a column and should not repeat in attributes: %s", row.Attributes)
				}
			}
		})
	}
}

// A browser posting NDJSON with an ISO date and a query-parameter ddsource is
// the exact request that used to lose both its timestamps and its source. This
// runs it through the real engine.
func TestHandleLogsAcceptsNDJSONWithQueryDefaults(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	body := []byte("{\"message\":\"first\",\"date\":\"2026-09-23T08:15:30Z\",\"status\":\"info\"}\n" +
		"{\"message\":\"second\",\"date\":\"2026-09-23T08:15:31Z\"}\n")

	w := post(t, e, "/api/v2/logs?ddsource=browser&ddtags=env:prod&service=shop", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	// Datadog's logs intake answers with an empty object, not the error array
	// the metrics endpoints use. Clients check for it.
	if got := w.Body.String(); got != `{}` {
		t.Errorf("body: got %s, want {}", got)
	}

	rows := Rows[storage.LogRow](node)
	if len(rows) != 2 {
		t.Fatalf("rows: got %d, want 2", len(rows))
	}
	want := time.Date(2026, 9, 23, 8, 15, 30, 0, time.UTC)
	if !rows[0].Timestamp.Equal(want) {
		t.Errorf("timestamp: got %s, want %s — an ISO date must not fall back to arrival", rows[0].Timestamp, want)
	}
	if rows[0].TimestampSource != "date_string" {
		t.Errorf("timestamp_source: got %q, want date_string", rows[0].TimestampSource)
	}
	if rows[0].Source != "browser" || rows[0].Service != "shop" {
		t.Errorf("query defaults lost: source=%q service=%q", rows[0].Source, rows[0].Service)
	}
	if got := rows[0].Tags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("query ddtags lost: %v", rows[0].Tags)
	}
	if rows[0].Status != "info" {
		t.Errorf("status: got %q, want info", rows[0].Status)
	}
}

// An item that decoded as JSON but not as a log has none of the declared
// fields populated, so a row built from it would be empty strings wearing a
// timestamp. The bytes go to raw_payloads instead — and its well-formed
// neighbour still becomes a row.
func TestHandleLogsKeepsAnUnexpectedItemRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	body := []byte(`[{"message":"good"},{"message":{"nested":"object"}}]`)

	if w := post(t, e, "/api/v2/logs", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	logs := Rows[storage.LogRow](node)
	if len(logs) != 1 || logs[0].Message != "good" {
		t.Fatalf("log rows: got %+v, want only the well-formed item", logs)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw rows: got %d, want 1 — a shape we cannot read must not vanish", len(raws))
	}
	if raws[0].Intake != "logs" || raws[0].Reason != "unexpected_shape" {
		t.Errorf("raw row labelled %q/%q, want logs/unexpected_shape", raws[0].Intake, raws[0].Reason)
	}
	if raws[0].Body == "" {
		t.Error("raw row carries no body")
	}
}

// A body that is not JSON at all is kept whole: this is the only copy.
func TestHandleLogsKeepsAnUndecodableBodyRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	if w := post(t, e, "/api/v2/logs", []byte("not json at all")); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 — a bad body must not change the contract", w.Code)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Reason != "decode_error" {
		t.Fatalf("raw rows: %+v, want one decode_error", raws)
	}
}

// The agent's own connectivity sweep carries no body and must stay free: it
// runs at startup against every host and a row per sweep is noise.
func TestHandleLogsIgnoresTheDiagnoseSweep(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	req := httptest.NewRequest(http.MethodPost, "/api/v2/logs", nil)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("X-Requested-With", "datadog-agent-diagnose")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Errorf("got %d, want 202", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for the diagnose sweep, want 0", n)
	}
}

// logAttributes returns "" for "no attributes", and AppendTo turns that into
// "{}" — a JSON column rejects an empty string. Pinned here because the two
// halves live in different packages and only fail together, at insert time.
func TestLogAttributesEmptyStaysEmpty(t *testing.T) {
	row, err := logRow(testTenant, decodeLogItem(t, `{"message":"m"}`), arrival, "", "", "", nil)
	if err != nil {
		t.Fatalf("logRow: %v", err)
	}
	if row.Attributes != "" {
		t.Errorf("attributes: got %q, want empty", row.Attributes)
	}
	b := &storagetest.CaptureBatch{}
	if err := row.AppendTo(b); err != nil {
		t.Fatalf("AppendTo: %v", err)
	}
	if got := b.Args[len(b.Args)-2]; got != "{}" {
		t.Errorf("attributes column: got %#v, want \"{}\" — a JSON column rejects an empty string", got)
	}
}

// AppendTo against the real INSERT column list, for both tables this router
// feeds. The compiler cannot see this agreement.
func TestLogsRowArity(t *testing.T) {
	storagetest.AssertArity(t, storage.LogsWriter, storage.LogRow{})
	storagetest.AssertArity(t, storage.RawPayloadsWriter, storage.RawPayloadRow{})
}

// The full path for the logs table: scratch database, every migration applied
// from empty, a real batch through the registered INSERT, the values read
// back. Only ClickHouse can prove AppendTo and the column TYPES agree — a
// string landing in a JSON column, or a Nullable that is not one, passes every
// in-process check and fails here.
func TestLogRowRoundTripThroughClickHouse(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

	storagetest.Insert(t, conn, storage.LogsWriter, []storage.Row{
		storage.LogRow{
			TenantID: testTenant, Timestamp: now, Host: "h1", Service: "api",
			Source: "go", Status: "error", Message: "boom",
			Tags:            map[string][]string{"kube_service": {"a", "b"}},
			Attributes:      `{"dd.trace_id":"7277407061953423024","usr":{"id":"u-1"}}`,
			TimestampSource: "date_string",
		},
		// A row with nothing optional set: the JSON column must still take it.
		storage.LogRow{TenantID: testTenant, Timestamp: now, Message: "bare"},
	})

	if got := storagetest.Count(t, conn, "logs"); got != 2 {
		t.Fatalf("logs: %d rows, want 2", got)
	}

	row := storagetest.QueryRow(t, conn,
		"SELECT timestamp_source, toString(attributes.`dd.trace_id`), tags['kube_service'] FROM logs WHERE message = 'boom'")
	if row[0] != "date_string" {
		t.Errorf("timestamp_source: got %v", row[0])
	}
	// Read back by attribute: the point of a JSON column over a String one.
	if got, want := row[1], "7277407061953423024"; got != want {
		t.Errorf("attributes.dd.trace_id: got %v, want %v", got, want)
	}
	// Two tags sharing a key, still two after the round trip.
	if got, ok := row[2].([]string); !ok || !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Errorf("tags[kube_service]: got %#v, want [a b]", row[2])
	}
}
