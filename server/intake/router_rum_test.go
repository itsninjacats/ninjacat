package intake

import (
	"bytes"
	"compress/zlib"
	"encoding/json"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The RUM intake is the one surface whose clients are not agents but web
// pages and phones, and almost every failure it can have is invisible from
// the outside: a browser that loses a batch to CORS reports success, an
// Android app that gets a 200 drops the batch and logs it only on the device,
// and a mobile batch can legitimately carry events from eighteen hours ago.
// These tests pin the behaviours that make those failures impossible rather
// than merely unlikely.

// ---------------------------------------------------------------------------
// Conversion functions
// ---------------------------------------------------------------------------

// The view sample Datadog ships with rum-events-format, which is also what a
// real browser sends: Web Vitals in the modern view.performance.* block, the
// deprecated flat fields beside them, counters per kind, and _dd.document_version.
func TestRumViewRowFromUpstreamSample(t *testing.T) {
	raw := readSample(t, "rum-events/view.json")
	o, err := rumDecodeObject(raw)
	if err != nil {
		t.Fatalf("decode sample: %v", err)
	}

	row := rumViewRow(storage.RumRequest{TenantID: "t"}, "view", o, raw)

	if row.ApplicationID != "ac8218cf-498b-4d33-bd44-151095959547" {
		t.Errorf("application id: got %q", row.ApplicationID)
	}
	if row.ViewID != "623d50fd-75cf-4025-97d2-e51ff94171f6" {
		t.Errorf("view id: got %q", row.ViewID)
	}
	if row.DocumentVersion != 9 {
		t.Errorf("document_version: got %d, want 9 — this is the ReplacingMergeTree version", row.DocumentVersion)
	}
	// 1591283924940 ms, not 1591283924 s and not arrival time.
	if want := time.UnixMilli(1591283924940).UTC(); !row.Date.Equal(want) {
		t.Errorf("date: got %s, want %s — the SDK's date, in milliseconds", row.Date, want)
	}
	if row.ViewLoadingType != "initial_load" {
		t.Errorf("loading_type: got %q — averaging loading_time across initial_load and route_change is meaningless, so the type must survive", row.ViewLoadingType)
	}
	if row.ViewTimeSpent != 245512755000 {
		t.Errorf("time_spent: got %d", row.ViewTimeSpent)
	}
	// The modern block wins over the deprecated flat field.
	if row.LCP == nil || *row.LCP != 20000000 {
		t.Errorf("lcp: got %v, want view.performance.lcp.timestamp", row.LCP)
	}
	if row.CLS == nil || *row.CLS != 0.1 {
		t.Errorf("cls: got %v, want view.performance.cls.score", row.CLS)
	}
	if row.FCP == nil || *row.FCP != 420725000 {
		t.Errorf("fcp: got %v", row.FCP)
	}
	if row.ErrorCount == nil || *row.ErrorCount != 2 {
		t.Errorf("error count: got %v, want 2", row.ErrorCount)
	}
	if row.FrustrationCount == nil || *row.FrustrationCount != 9 {
		t.Errorf("frustration count: got %v, want 9", row.FrustrationCount)
	}
	// This sample sends no crash sub-object at all. That is not zero crashes;
	// it is a platform with no crashes to report, and a 0 would be averaged.
	if row.CrashCount != nil {
		t.Errorf("crash count: got %v, want nil — absent is not zero", *row.CrashCount)
	}
	if row.DeviceBrand != "Apple" || row.OSName != "iOS" {
		t.Errorf("device/os: got %q/%q", row.DeviceBrand, row.OSName)
	}
	if !strings.Contains(row.FeatureFlags, "feature_one") {
		t.Errorf("feature_flags: got %q, want the whole map as JSON", row.FeatureFlags)
	}
	if row.Event != string(raw) {
		t.Error("event column is not the line as it arrived — the typed columns are an index over it, it is the record")
	}
}

// view_update is the newer wire spelling of the same update and must land in
// the same row, or one view's history splits across two rows whose counters
// disagree with each other.
func TestRumViewUpdateUsesTheSameIdentity(t *testing.T) {
	raw := readSample(t, "rum-events/view_update.json")
	o, err := rumDecodeObject(raw)
	if err != nil {
		t.Fatalf("decode sample: %v", err)
	}
	row := rumViewRow(storage.RumRequest{TenantID: "t"}, "view_update", o, raw)

	if row.EventType != "view_update" {
		t.Errorf("event_type: got %q", row.EventType)
	}
	if row.ApplicationID == "" || row.SessionID == "" || row.ViewID == "" {
		t.Errorf("identity: application=%q session=%q view=%q — all three are the upsert key",
			row.ApplicationID, row.SessionID, row.ViewID)
	}
	if row.DocumentVersion == 0 {
		t.Error("document_version is 0 — without it a view_update can never win a merge")
	}
}

// Per-kind projection. Each case is a shape one of the three SDKs actually
// sends; the columns of the other kinds must stay empty rather than pick up a
// neighbour's value.
func TestRumEventRowPerKind(t *testing.T) {
	cases := []struct {
		name  string
		kind  string
		body  string
		check func(t *testing.T, r storage.RumEventRow)
	}{
		{
			name: "action carries its label from target.name and every frustration type",
			kind: "action",
			body: `{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
				"action":{"id":"a1","type":"click","target":{"name":"Buy now"},
				          "frustration":{"type":["rage_click","dead_click"]}}}`,
			check: func(t *testing.T, r storage.RumEventRow) {
				if r.ActionName != "Buy now" {
					t.Errorf("action name: got %q, want the target's name", r.ActionName)
				}
				want := []string{"rage_click", "dead_click"}
				if !reflect.DeepEqual(r.ActionFrustrationTypes, want) {
					t.Errorf("frustration types: got %v, want %v — one click can be classified twice", r.ActionFrustrationTypes, want)
				}
				if r.ErrorMessage != "" || r.ResourceURL != "" {
					t.Error("an action filled another kind's columns")
				}
			},
		},
		{
			name: "a crash report is an ordinary error with is_crash set",
			kind: "error",
			body: `{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
				"error":{"id":"e1","message":"boom","source":"source","is_crash":true,
				         "handling":"unhandled","fingerprint":"fp","stack":"at x"}}`,
			check: func(t *testing.T, r storage.RumEventRow) {
				if r.ErrorIsCrash == nil || !*r.ErrorIsCrash {
					t.Errorf("is_crash: got %v, want true", r.ErrorIsCrash)
				}
				if r.ErrorMessage != "boom" || r.ErrorFingerprint != "fp" {
					t.Errorf("error fields: %q / %q", r.ErrorMessage, r.ErrorFingerprint)
				}
			},
		},
		{
			name: "an error that never mentions is_crash is unknown, not false",
			kind: "error",
			body: `{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
				"error":{"id":"e1","message":"boom","source":"source"}}`,
			check: func(t *testing.T, r storage.RumEventRow) {
				if r.ErrorIsCrash != nil {
					t.Errorf("is_crash: got %v, want nil", *r.ErrorIsCrash)
				}
			},
		},
		{
			name: "a resource that returned 0 bytes is not a resource with no size",
			kind: "resource",
			body: `{"type":"resource","date":1,"application":{"id":"app"},"session":{"id":"s"},
				"resource":{"id":"r1","type":"xhr","url":"https://x/y","method":"GET",
				            "status_code":204,"duration":1200,"size":0}}`,
			check: func(t *testing.T, r storage.RumEventRow) {
				if r.ResourceSize == nil || *r.ResourceSize != 0 {
					t.Errorf("size: got %v, want a stored 0", r.ResourceSize)
				}
				if r.ResourceStatusCode == nil || *r.ResourceStatusCode != 204 {
					t.Errorf("status_code: got %v", r.ResourceStatusCode)
				}
			},
		},
		{
			name: "a vital keeps the sub-discriminator that picked its variant",
			kind: "vital",
			body: `{"type":"vital","date":1,"application":{"id":"app"},"session":{"id":"s"},
				"vital":{"id":"v1","type":"duration","name":"checkout","duration":900}}`,
			check: func(t *testing.T, r storage.RumEventRow) {
				if r.VitalType != "duration" || r.VitalName != "checkout" {
					t.Errorf("vital: %q / %q", r.VitalType, r.VitalName)
				}
				if r.VitalDuration == nil || *r.VitalDuration != 900 {
					t.Errorf("vital duration: got %v", r.VitalDuration)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			o, err := rumDecodeObject([]byte(tc.body))
			if err != nil {
				t.Fatalf("decode: %v", err)
			}
			r := rumEventRow(storage.RumRequest{TenantID: "t"}, tc.kind, o, []byte(tc.body))
			if r.EventType != tc.kind {
				t.Fatalf("event_type: got %q, want %q", r.EventType, tc.kind)
			}
			tc.check(t, r)
		})
	}
}

// Tags are a multiset: nothing forbids two tags sharing a key, and every SDK
// lets an app add its own. A map[string]string here would keep the last one
// and drop the rest, which is exactly the damage documented in
// docs/decisions/0001-tags-are-a-multiset.md.
func TestRumEventRowKeepsRepeatedTags(t *testing.T) {
	body := []byte(`{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
		"ddtags":"env:prod,team:web,team:payments,beta"}`)
	o, err := rumDecodeObject(body)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	r := rumEventRow(storage.RumRequest{TenantID: "t"}, "action", o, body)

	want := map[string][]string{
		"env":  {"prod"},
		"team": {"web", "payments"},
		"beta": {""},
	}
	if !reflect.DeepEqual(r.Tags, want) {
		t.Errorf("tags: got %v, want %v", r.Tags, want)
	}
}

// An SDK ships against a schema snapshot months older than ours and adds keys
// between releases. A key no struct declares must still reach a column —
// here, the context map and the event itself.
func TestRumEventRowKeepsUndeclaredKeys(t *testing.T) {
	body := []byte(`{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
		"action":{"id":"a1","type":"click","brand_new_field":42},
		"context":{"cart_value":9007199254740993,"nested":{"a":[1,"b"]}},
		"unknown_top_level":{"x":1}}`)
	o, err := rumDecodeObject(body)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	r := rumEventRow(storage.RumRequest{TenantID: "t"}, "action", o, body)

	// 2^53+1: a float64 anywhere on this path would come back as ...992.
	if !strings.Contains(r.Context, "9007199254740993") {
		t.Errorf("context: got %q — a 64-bit integer went through float64", r.Context)
	}
	if !strings.Contains(r.Event, "unknown_top_level") || !strings.Contains(r.Event, "brand_new_field") {
		t.Error("the event column lost a key the schema does not declare")
	}
}

func TestRumTelemetryRowFromUpstreamSamples(t *testing.T) {
	cases := []struct {
		file          string
		wantStatus    string
		wantType      string
		wantMessage   string
		wantFeature   string
		wantConfigSub string
	}{
		{file: "telemetry-events/error.json", wantStatus: "error", wantMessage: "XHR error POST https://app.datadoghq.com/api/v1/logs-analytics/aggregate?type=rum"},
		{file: "telemetry-events/configuration.json", wantType: "configuration", wantConfigSub: "session_sample_rate"},
		{file: "telemetry-events/usage.json", wantType: "usage", wantFeature: "set-tracking-consent"},
	}

	for _, tc := range cases {
		t.Run(tc.file, func(t *testing.T) {
			raw := readSample(t, tc.file)
			o, err := rumDecodeObject(raw)
			if err != nil {
				t.Fatalf("decode: %v", err)
			}
			r := rumTelemetryRow(storage.RumRequest{TenantID: "t"}, o, raw)

			if r.Type != "telemetry" {
				t.Errorf("type: got %q", r.Type)
			}
			if tc.wantStatus != "" && r.Status != tc.wantStatus {
				t.Errorf("status: got %q, want %q", r.Status, tc.wantStatus)
			}
			if tc.wantType != "" && r.TelemetryType != tc.wantType {
				t.Errorf("telemetry type: got %q, want %q", r.TelemetryType, tc.wantType)
			}
			if tc.wantMessage != "" && r.Message != tc.wantMessage {
				t.Errorf("message: got %q", r.Message)
			}
			if tc.wantFeature != "" && r.UsageFeature != tc.wantFeature {
				t.Errorf("usage feature: got %q, want %q", r.UsageFeature, tc.wantFeature)
			}
			// The configuration object is the answer to "why is this app
			// sending nothing?", so it is kept whole rather than sampled.
			if tc.wantConfigSub != "" && !strings.Contains(r.Configuration, tc.wantConfigSub) {
				t.Errorf("configuration: %q does not contain %q", r.Configuration, tc.wantConfigSub)
			}
			if r.ApplicationID == "" || r.SessionID == "" {
				t.Errorf("identity: application=%q session=%q", r.ApplicationID, r.SessionID)
			}
		})
	}
}

// timeseries is mobile-only and its start/end are NANOSECONDS while `date` is
// milliseconds. Mixing the two would put a sample run a million times too far
// from the event that produced it.
func TestRumTimeseriesRowKeepsArraysAndNanoseconds(t *testing.T) {
	body := []byte(`{"type":"timeseries","date":1591283924940,"application":{"id":"app"},
		"session":{"id":"s"},"source":"ios",
		"timeseries":{"id":"ts1","name":"memory","schema":"object-v2",
			"start":1591283924940000000,"end":1591283925940000000,
			"data":{"timestamps":[1591283924940000000,1591283925440000000],
			        "values":{"memory_footprint":[100,200]}}}}`)
	o, err := rumDecodeObject(body)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	r := rumTimeseriesRow(storage.RumRequest{TenantID: "t"}, o, body)

	if r.Name != "memory" || r.ID != "ts1" || r.Schema != "object-v2" {
		t.Errorf("discriminators: %q %q %q", r.Name, r.ID, r.Schema)
	}
	if want := time.Unix(0, 1591283924940000000).UTC(); !r.Start.Equal(want) {
		t.Errorf("start: got %s, want %s (nanoseconds)", r.Start, want)
	}
	if want := time.UnixMilli(1591283924940).UTC(); !r.Date.Equal(want) {
		t.Errorf("date: got %s, want %s (milliseconds)", r.Date, want)
	}
	want := []int64{1591283924940000000, 1591283925440000000}
	if !reflect.DeepEqual(r.Timestamps, want) {
		t.Errorf("timestamps: got %v, want %v — order is the pairing with values", r.Timestamps, want)
	}
	if !strings.Contains(r.Values, "memory_footprint") {
		t.Errorf("values: got %q", r.Values)
	}
}

// The span format is hand-rolled per SDK with no published schema: ids are
// text of 64- and 128-bit numbers, meta hides two nested objects among flat
// keys, and metrics are numbers.
func TestRumSpanRowFlattensMetaAndKeepsIDsAsText(t *testing.T) {
	span := []byte(`{"trace_id":"9007199254740993","span_id":"2","parent_id":"0",
		"name":"urlsession.request","service":"app","resource":"GET /x","type":"custom",
		"start":1591283924940000000,"duration":1500000,"error":1,
		"meta":{"_dd.source":"mobile","device":{"brand":"Apple","model":"iPhone"},
		        "os":{"name":"iOS"},"tracer.version":"2.0.0","nulled":null},
		"metrics":{"_top_level":1,"_sampling_priority_v1":2}}`)
	o, err := rumDecodeObject(span)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	r := rumSpanRow(storage.RumRequest{TenantID: "t"}, "prod", "", o, span)

	if r.TraceID != "9007199254740993" {
		t.Errorf("trace_id: got %q — ids must stay text, never a float64", r.TraceID)
	}
	if r.Env != "prod" {
		t.Errorf("env: got %q, want the envelope's value", r.Env)
	}
	if want := time.Unix(0, 1591283924940000000).UTC(); !r.Start.Equal(want) {
		t.Errorf("start: got %s, want %s", r.Start, want)
	}
	if r.DurationNS != 1500000 || r.Error != 1 {
		t.Errorf("duration/error: %d / %d", r.DurationNS, r.Error)
	}
	for k, want := range map[string]string{
		"device.brand":   "Apple",
		"device.model":   "iPhone",
		"os.name":        "iOS",
		"_dd.source":     "mobile",
		"tracer.version": "2.0.0",
		"nulled":         "",
	} {
		if got := r.Meta[k]; got != want {
			t.Errorf("meta[%q]: got %q, want %q — nested objects are flattened, not dropped", k, got, want)
		}
	}
	if r.Metrics["_sampling_priority_v1"] != 2 {
		t.Errorf("metrics: got %v", r.Metrics)
	}
	if r.Span != string(span) {
		t.Error("span column is not the object as it arrived")
	}
}

// The browser puts its whole identity in the query string because its fetch
// sets no headers at all; iOS and Android put theirs in headers. One request
// block has to read both, and a parameter with no column of its own must not
// vanish — except the credential, which must never become durable.
func TestRumRequestInfoReadsQueryAndHeaders(t *testing.T) {
	cases := []struct {
		name  string
		url   string
		hdr   map[string]string
		check func(t *testing.T, r storage.RumRequest)
	}{
		{
			name: "browser: everything in the query string, first attempt",
			url: "/api/v2/rum?ddsource=browser&dd-api-key=k&dd-evp-origin=browser" +
				"&dd-evp-origin-version=5.23.0&dd-request-id=req-1&batch_time=1591283924940&_dd.api=beacon",
			check: func(t *testing.T, r storage.RumRequest) {
				if r.DDSource != "browser" || r.EVPOrigin != "browser" || r.EVPOriginVersion != "5.23.0" {
					t.Errorf("origin block: %q %q %q", r.DDSource, r.EVPOrigin, r.EVPOriginVersion)
				}
				if r.RequestID != "req-1" || r.DDAPI != "beacon" {
					t.Errorf("request id / api: %q %q", r.RequestID, r.DDAPI)
				}
				if r.BatchTime == nil || !r.BatchTime.Equal(time.UnixMilli(1591283924940).UTC()) {
					t.Errorf("batch_time: got %v", r.BatchTime)
				}
				// A first attempt has no retry count. "Retried zero times" is
				// the same number and a different fact.
				if r.RetryCount != nil || r.RetryAfter != nil {
					t.Errorf("retry: got %v / %v, want nil on a first attempt", r.RetryCount, r.RetryAfter)
				}
				if strings.Contains(r.QueryExtra, "dd-api-key") || strings.Contains(r.QueryExtra, "\"k\"") {
					t.Errorf("query_extra leaked the credential: %q", r.QueryExtra)
				}
			},
		},
		{
			name: "browser retry: _dd.retry_count and _dd.retry_after",
			url:  "/api/v2/rum?ddsource=browser&_dd.retry_count=2&_dd.retry_after=1000",
			check: func(t *testing.T, r storage.RumRequest) {
				if r.RetryCount == nil || *r.RetryCount != 2 {
					t.Errorf("retry_count: got %v", r.RetryCount)
				}
				if r.RetryAfter == nil || *r.RetryAfter != 1000 {
					t.Errorf("retry_after: got %v", r.RetryAfter)
				}
			},
		},
		{
			name: "mobile retry: the same two facts spelled inside ddtags",
			url:  "/api/v2/rum?ddsource=ios&ddtags=retry_count:3,retry_after:503",
			hdr: map[string]string{
				"Dd-Evp-Origin":         "ios",
				"Dd-Evp-Origin-Version": "3.17.0",
				"Dd-Request-Id":         "req-2",
				"Dd-Idempotency-Key":    "sha1-of-body",
			},
			check: func(t *testing.T, r storage.RumRequest) {
				if r.EVPOrigin != "ios" || r.EVPOriginVersion != "3.17.0" || r.RequestID != "req-2" {
					t.Errorf("headers: %q %q %q", r.EVPOrigin, r.EVPOriginVersion, r.RequestID)
				}
				if r.IdempotencyKey != "sha1-of-body" {
					t.Errorf("idempotency key: %q — it is what tells a retry from a new batch", r.IdempotencyKey)
				}
				if r.RetryCount == nil || *r.RetryCount != 3 {
					t.Errorf("retry_count: got %v", r.RetryCount)
				}
				if r.RetryAfter == nil || *r.RetryAfter != 503 {
					t.Errorf("retry_after: got %v, want the HTTP status the mobile SDKs send", r.RetryAfter)
				}
			},
		},
		{
			name: "a parameter with no column is kept, not counted",
			url:  "/api/v2/rum?ddsource=browser&brand_new=1&brand_new=2",
			check: func(t *testing.T, r storage.RumRequest) {
				var extra map[string][]string
				if err := json.Unmarshal([]byte(r.QueryExtra), &extra); err != nil {
					t.Fatalf("query_extra %q: %v", r.QueryExtra, err)
				}
				if !reflect.DeepEqual(extra["brand_new"], []string{"1", "2"}) {
					t.Errorf("query_extra: got %v — a parameter may legally repeat", extra)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c, _ := ginContext(http.MethodPost, tc.url, nil, tc.hdr)
			tc.check(t, rumRequestInfo(c))
		})
	}
}

// `proxy` as a string sends the real path and query inside ?ddforward=, so
// without unwrapping the payload lands on NoRoute with the key attached.
func TestRumUnwrapForward(t *testing.T) {
	cases := []struct {
		name      string
		in        string
		wantPath  string
		wantQuery map[string]string
	}{
		{
			name:     "the forwarded path and query become the request",
			in:       "/rum-proxy?ddforward=" + url.QueryEscape("/api/v2/rum?ddsource=browser&dd-api-key=k&dd-request-id=r1"),
			wantPath: "/api/v2/rum",
			wantQuery: map[string]string{
				"ddsource":      "browser",
				"dd-api-key":    "k",
				"dd-request-id": "r1",
			},
		},
		{
			name:      "an outer parameter the proxy added survives the merge",
			in:        "/rum-proxy?tenant=acme&ddforward=" + url.QueryEscape("/api/v2/logs?ddsource=browser"),
			wantPath:  "/api/v2/logs",
			wantQuery: map[string]string{"ddsource": "browser", "tenant": "acme"},
		},
		{
			name:      "no ddforward changes nothing",
			in:        "/api/v2/rum?ddsource=browser",
			wantPath:  "/api/v2/rum",
			wantQuery: map[string]string{"ddsource": "browser"},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			r := httptest.NewRequest(http.MethodPost, tc.in, nil)
			rumUnwrapForward(r)
			if r.URL.Path != tc.wantPath {
				t.Errorf("path: got %q, want %q", r.URL.Path, tc.wantPath)
			}
			q := r.URL.Query()
			if q.Get("ddforward") != "" {
				t.Error("ddforward survived the unwrap and would be stored as an unknown parameter")
			}
			for k, want := range tc.wantQuery {
				if got := q.Get(k); got != want {
					t.Errorf("query %q: got %q, want %q", k, got, want)
				}
			}
		})
	}
}

func TestRumReplayVariant(t *testing.T) {
	cases := []struct {
		name, part, metaType, want string
	}{
		{"browser segment", "segment", "", "segment"},
		{"mobile segment", "file0", "", "segment"},
		{"browser canvas resource", "image", "resource", "resource"},
		{"mobile canvas resource, repeated part name", "image", "resource", "resource"},
		{"a part named something else, classified by the metadata", "blob", "resource", "resource"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			meta := rumObj{}
			if tc.metaType != "" {
				meta["type"] = tc.metaType
			}
			if got := rumReplayVariant(tc.part, meta); got != tc.want {
				t.Errorf("got %q, want %q", got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

// The worked example: a real NDJSON batch through the real engine, with the
// mix of variants a browser sends in one request — a view, an action, an
// error and an SDK telemetry event — plus one line from an SDK newer than
// this build. Every line must land somewhere.
func TestHandleRUMStoresEveryVariant(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeRUM)

	lines := [][]byte{
		readSample(t, "rum-events/view.json"),
		readSample(t, "rum-events/action.json"),
		readSample(t, "rum-events/error.json"),
		readSample(t, "telemetry-events/configuration.json"),
		[]byte(`{"type":"timeseries","date":1,"application":{"id":"app"},"session":{"id":"s"},` +
			`"timeseries":{"id":"ts","name":"cpu","schema":"object-v2","start":1,"end":2,` +
			`"data":{"timestamps":[1],"values":{"cpu_ticks_count":[1]}}}}`),
		// An SDK built against a newer schema than ours. The batch must not
		// die with it.
		[]byte(`{"type":"teleportation","date":1,"application":{"id":"app"}}`),
	}
	body := bytes.Join(compact(t, lines), []byte("\n"))

	w := post(t, e, "/api/v2/rum?ddsource=browser", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/rum: got %d, want 202 — Android drops a batch on 200 (%s)", w.Code, w.Body.String())
	}
	if w.Body.String() != "{}" {
		t.Errorf("body: got %q, want {}", w.Body.String())
	}

	if n := len(Rows[storage.RumViewRow](node)); n != 1 {
		t.Errorf("rum_views rows: got %d, want 1", n)
	}
	events := Rows[storage.RumEventRow](node)
	if len(events) != 2 {
		t.Fatalf("rum_events rows: got %d, want 2 (action, error)", len(events))
	}
	if events[0].EventType != "action" || events[1].EventType != "error" {
		t.Errorf("event order: got %q, %q — NDJSON order must survive", events[0].EventType, events[1].EventType)
	}
	if n := len(Rows[storage.RumTelemetryRow](node)); n != 1 {
		t.Errorf("rum_telemetry rows: got %d, want 1", n)
	}
	if n := len(Rows[storage.RumTimeseriesRow](node)); n != 1 {
		t.Errorf("rum_timeseries rows: got %d, want 1", n)
	}

	// The unknown line is kept verbatim, with the type that failed to match.
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1 — an unknown variant is kept, never counted", len(raws))
	}
	if raws[0].Reason != "unknown_event" || raws[0].Note != "teleportation" {
		t.Errorf("raw payload: reason=%q note=%q, want unknown_event/teleportation", raws[0].Reason, raws[0].Note)
	}
	if raws[0].Intake != "rum" {
		t.Errorf("raw payload intake: got %q, want rum", raws[0].Intake)
	}

	for _, r := range events {
		if r.Req.TenantID != testTenant {
			t.Errorf("tenant: got %q, want %q — tenant_id is the first ORDER BY column everywhere", r.Req.TenantID, testTenant)
		}
	}
}

// The browser cannot set a header on a beacon, so its key is in the query
// string; iOS and Android always use the header. One engine must take both.
func TestRUMEngineAcceptsBothKeyLocations(t *testing.T) {
	a, node := newTestServer(t)
	e := a.rumEngine()

	body := []byte(`{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},"action":{"id":"a","type":"click"}}`)

	// Browser: ?dd-api-key=
	w := serve(e, request(http.MethodPost, "/api/v2/rum?ddsource=browser&dd-api-key="+testAPIKey, body, nil))
	if w.Code != http.StatusAccepted {
		t.Fatalf("query-string key: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	// Mobile: DD-API-KEY header.
	w = serve(e, request(http.MethodPost, "/api/v2/rum?ddsource=ios", body,
		map[string]string{"DD-API-KEY": testAPIKey}))
	if w.Code != http.StatusAccepted {
		t.Fatalf("header key: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	if n := len(Rows[storage.RumEventRow](node)); n != 2 {
		t.Errorf("stored rows: got %d, want 2 — one per accepted request", n)
	}

	// A rejected request must still carry the CORS header, or the browser
	// reports success and drops the batch: the 403 becomes invisible.
	w = serve(e, request(http.MethodPost, "/api/v2/rum?ddsource=browser&dd-api-key=wrong", body,
		map[string]string{"Origin": "https://shop.example"}))
	if w.Code != http.StatusForbidden {
		t.Fatalf("bad key: got %d, want 403", w.Code)
	}
	if got := w.Header().Get("Access-Control-Allow-Origin"); got != "https://shop.example" {
		t.Errorf("403 Access-Control-Allow-Origin: got %q, want the origin — without it the SDK reads status 0 as success and drops the batch silently", got)
	}
}

func TestRUMPreflight(t *testing.T) {
	a, _ := newTestServer(t)
	e := a.rumEngine()

	w := serve(e, request(http.MethodOptions, "/api/v2/rum", nil,
		map[string]string{
			"Origin":                         "https://shop.example",
			"Access-Control-Request-Method":  "POST",
			"Access-Control-Request-Headers": "content-type",
		}))

	if w.Code != http.StatusNoContent {
		t.Fatalf("preflight: got %d, want 204", w.Code)
	}
	h := w.Header()
	if h.Get("Access-Control-Allow-Origin") != "https://shop.example" {
		t.Errorf("allow-origin: got %q", h.Get("Access-Control-Allow-Origin"))
	}
	if !strings.Contains(h.Get("Access-Control-Allow-Methods"), "POST") {
		t.Errorf("allow-methods: got %q", h.Get("Access-Control-Allow-Methods"))
	}
	if !strings.Contains(h.Get("Access-Control-Allow-Headers"), "DD-API-KEY") {
		t.Errorf("allow-headers: got %q", h.Get("Access-Control-Allow-Headers"))
	}
	if h.Get("Access-Control-Max-Age") == "" {
		t.Error("no Access-Control-Max-Age: every request would preflight again")
	}
}

// An allowlist is an operator's choice and must be honoured exactly: an
// origin on it is allowed, one that is not gets no header at all.
func TestRUMAllowedOriginsFromEnv(t *testing.T) {
	t.Setenv("NINJACAT_RUM_ALLOWED_ORIGINS", "https://shop.example, https://admin.example")
	a, _ := newTestServer(t)
	e := a.rumEngine()

	for origin, want := range map[string]string{
		"https://shop.example":  "https://shop.example",
		"https://admin.example": "https://admin.example",
		"https://evil.example":  "",
	} {
		w := serve(e, request(http.MethodOptions, "/api/v2/rum", nil, map[string]string{"Origin": origin}))
		if got := w.Header().Get("Access-Control-Allow-Origin"); got != want {
			t.Errorf("origin %q: allow-origin %q, want %q", origin, got, want)
		}
	}
}

// The browser sends no Content-Encoding header — the only signal that a body
// is deflated is ?dd-evp-encoding=deflate, which Decompress() cannot see.
func TestHandleRUMInflatesDeflatedBody(t *testing.T) {
	a, node := newTestServer(t)
	e := a.rumEngine()

	plain := []byte(`{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},"action":{"id":"a","type":"click"}}`)
	var buf bytes.Buffer
	zw := zlib.NewWriter(&buf)
	if _, err := zw.Write(plain); err != nil {
		t.Fatal(err)
	}
	zw.Close()

	w := serve(e, request(http.MethodPost,
		"/api/v2/rum?ddsource=browser&dd-evp-encoding=deflate&dd-api-key="+testAPIKey,
		buf.Bytes(), nil))
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RumEventRow](node)
	if len(rows) != 1 {
		t.Fatalf("stored rows: got %d, want 1 — the body was never inflated", len(rows))
	}
	if rows[0].ActionID != "a" {
		t.Errorf("action id: got %q", rows[0].ActionID)
	}
}

// The proxy-as-a-string form, end to end: the SDK posts to its own path and
// everything that matters is inside ddforward.
func TestRUMEngineRoutesThroughDdforward(t *testing.T) {
	a, node := newTestServer(t)
	e := a.rumEngine()

	body := []byte(`{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},"action":{"id":"a","type":"click"}}`)
	fwd := url.QueryEscape("/api/v2/rum?ddsource=browser&dd-api-key=" + testAPIKey + "&dd-evp-origin=browser")

	w := serve(e, request(http.MethodPost, "/my-rum-proxy?ddforward="+fwd, body, nil))
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	rows := Rows[storage.RumEventRow](node)
	if len(rows) != 1 {
		t.Fatalf("stored rows: got %d, want 1", len(rows))
	}
	if rows[0].Req.DDSource != "browser" || rows[0].Req.EVPOrigin != "browser" {
		t.Errorf("request block: ddsource=%q origin=%q — the unwrapped query must reach the row",
			rows[0].Req.DDSource, rows[0].Req.EVPOrigin)
	}
}

// A mobile replay upload: an on-disk batch spanning two views ships two
// segments and an ARRAY of metadata, entry i describing part i.
func TestHandleRUMReplayMobileMultipart(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeRUM)

	seg0 := []byte("zlib-bytes-of-view-0")
	seg1 := []byte("zlib-bytes-of-view-1")
	meta := `[
		{"application":{"id":"app"},"session":{"id":"s"},"view":{"id":"v0"},"source":"android",
		 "start":1591283924940,"end":1591283925940,"records_count":12,"has_full_snapshot":true,
		 "index_in_view":null,"raw_segment_size":4096,"compressed_segment_size":512},
		{"application":{"id":"app"},"session":{"id":"s"},"view":{"id":"v1"},"source":"android",
		 "start":1591283926940,"end":1591283927940,"records_count":3,"has_full_snapshot":false,
		 "raw_segment_size":1024,"compressed_segment_size":128}
	]`

	body, contentType := multipartBody(t, []multipartPart{
		{name: "file0", filename: "file0", data: seg0},
		{name: "file1", filename: "file1", data: seg1},
		{name: "event", filename: "blob", data: []byte(meta)},
	})

	req := request(http.MethodPost, "/api/v2/replay?ddsource=android", body,
		map[string]string{"Content-Type": contentType, "Dd-Api-Key": testAPIKey})
	w := serve(e, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RumReplaySegmentRow](node)
	if len(rows) != 2 {
		t.Fatalf("replay rows: got %d, want 2 — one row per blob, never one per request", len(rows))
	}
	if rows[0].ViewID != "v0" || rows[1].ViewID != "v1" {
		t.Errorf("metadata pairing is by position: got %q, %q", rows[0].ViewID, rows[1].ViewID)
	}
	// The bytes are stored exactly as received: Datadog's own intake keeps
	// segments unprocessed, and the SDK builds streams meant to be
	// concatenated later.
	if rows[0].Segment != string(seg0) || rows[1].Segment != string(seg1) {
		t.Error("segment bytes were modified in flight")
	}
	if rows[0].Variant != "segment" || rows[0].PartName != "file0" {
		t.Errorf("variant/part: %q / %q", rows[0].Variant, rows[0].PartName)
	}
	if rows[0].RecordsCount == nil || *rows[0].RecordsCount != 12 {
		t.Errorf("records_count: got %v", rows[0].RecordsCount)
	}
	if rows[0].HasFullSnapshot == nil || !*rows[0].HasFullSnapshot {
		t.Errorf("has_full_snapshot: got %v", rows[0].HasFullSnapshot)
	}
	// Android sends an explicit null, iOS omits the key, the browser sends a
	// number. All three must stay distinguishable from 0.
	if rows[0].IndexInView != nil || rows[1].IndexInView != nil {
		t.Errorf("index_in_view: got %v / %v, want nil for both", rows[0].IndexInView, rows[1].IndexInView)
	}
	if rows[0].Start == nil || !rows[0].Start.Equal(time.UnixMilli(1591283924940).UTC()) {
		t.Errorf("start: got %v", rows[0].Start)
	}
	if rows[0].Event == "" {
		t.Error("the metadata object for this segment was not stored")
	}
}

// The browser's canvas-resource variant on the same path: an image part and a
// one-object metadata part that says type:"resource".
func TestHandleRUMReplayResourceVariant(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeRUM)

	img := []byte{0x89, 'P', 'N', 'G', 0x0d}
	body, contentType := multipartBody(t, []multipartPart{
		{name: "image", filename: "canvas", data: img},
		{name: "event", data: []byte(`{"application":{"id":"app"},"type":"resource"}`)},
	})

	w := serve(e, request(http.MethodPost, "/api/v2/replay?ddsource=browser", body,
		map[string]string{"Content-Type": contentType, "Dd-Api-Key": testAPIKey}))
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RumReplaySegmentRow](node)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1", len(rows))
	}
	if rows[0].Variant != "resource" {
		t.Errorf("variant: got %q, want resource", rows[0].Variant)
	}
	if rows[0].Image != string(img) || rows[0].Segment != "" {
		t.Error("an image went into the segment column, or was altered")
	}
}

// One span per envelope is what both mobile SDKs produce today, but the field
// is an array and the envelope carries the env beside it.
func TestHandleRUMSpansEnvelope(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeRUM)

	body := []byte(`{"spans":[{"trace_id":"1","span_id":"2","parent_id":"0","name":"op","service":"app","resource":"GET /x","type":"custom","start":1591283924940000000,"duration":5,"error":0,"meta":{"os":{"name":"iOS"}},"metrics":{"_top_level":1}}],"env":"prod"}
{"spans":[{"trace_id":"3","span_id":"4","parent_id":"2","name":"op2","service":"app","start":1591283924950000000,"duration":6,"error":0}],"env":"prod","new_envelope_key":{"x":1}}
not json at all`)

	w := post(t, e, "/api/v2/spans?ddsource=ios", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RumSpanRow](node)
	if len(rows) != 2 {
		t.Fatalf("span rows: got %d, want 2", len(rows))
	}
	if rows[0].TraceID != "1" || rows[1].ParentID != "2" {
		t.Errorf("ids: %q / %q", rows[0].TraceID, rows[1].ParentID)
	}
	if rows[0].Env != "prod" {
		t.Errorf("env: got %q, want the envelope's value", rows[0].Env)
	}
	if rows[0].Meta["os.name"] != "iOS" {
		t.Errorf("meta: got %v", rows[0].Meta)
	}
	// A key a future SDK adds to the envelope is stored, not counted.
	if !strings.Contains(rows[1].EnvelopeExtra, "new_envelope_key") {
		t.Errorf("envelope_extra: got %q", rows[1].EnvelopeExtra)
	}
	if rows[0].EnvelopeExtra != "" {
		t.Errorf("envelope_extra on a plain envelope: got %q, want empty", rows[0].EnvelopeExtra)
	}

	// The line that is not JSON is kept, and the two that are still stored.
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Reason != "decode_error" {
		t.Fatalf("raw payloads: got %d rows (%v)", len(raws), raws)
	}
}

// The probes are what an operator curls to see whether the host answers at
// all, and they must not need a credential.
func TestRUMEngineKeepsProbes(t *testing.T) {
	a, _ := newTestServer(t)
	e := a.rumEngine()

	for _, path := range []string{"/ping", "/_health"} {
		if w := serve(e, request(http.MethodGet, path, nil, nil)); w.Code != http.StatusOK {
			t.Errorf("GET %s: got %d, want 200", path, w.Code)
		}
	}
}

// ---------------------------------------------------------------------------
// ClickHouse round trips
// ---------------------------------------------------------------------------

// A view is upserted, not appended: the SDK re-sends the same view.id with a
// growing _dd.document_version and updated counters, and all three SDKs keep
// only the highest version per id inside one batch. FINAL must return the
// newer row — and it only can if both versions land in the same partition,
// which is why rum_views partitions by month rather than by day.
func TestRumViewsFinalYieldsTheNewerDocumentVersion(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// A RECENT date, and that is not incidental: these tables carry a TTL on
	// the SDK's own date, and ClickHouse evaluates it at insert time — a row
	// from 2020 is dropped before it is ever visible. The 18-hour staleness a
	// mobile batch can legitimately have stays comfortably inside it; a fixed
	// timestamp in a test does not.
	date := time.Now().UTC().Add(-time.Hour).Truncate(time.Millisecond)
	base := storage.RumViewRow{
		Req: storage.RumRequest{
			TenantID: "t", ReceivedAt: time.Now().UTC(),
			DDSource: "browser", EVPOrigin: "browser", EVPOriginVersion: "5.23.0",
			RequestID: "r1", RemoteAddr: "203.0.113.7", UserAgent: "Mozilla/5.0",
		},
		Date:          date,
		ApplicationID: "app",
		SessionID:     "sess",
		ViewID:        "view",
		EventType:     "view",
		Source:        "browser",
		Tags:          map[string][]string{"env": {"prod"}, "team": {"web", "payments"}},
	}

	first := base
	first.DocumentVersion = 1
	first.ViewTimeSpent = 1000
	first.ErrorCount = ptr[int64](0)

	second := base
	second.DocumentVersion = 2
	second.EventType = "view_update"
	second.ViewTimeSpent = 9000
	second.ErrorCount = ptr[int64](3)
	// The later version carries a crash object the first one had not; the
	// first must still be NULL rather than 0.
	second.CrashCount = ptr[int64](1)

	// Two separate batches, because that is what the wire does: an active
	// page re-sends its view every few seconds, one HTTP request each. (One
	// batch would not prove as much — ClickHouse already collapses duplicates
	// inside a single inserted block.)
	storagetest.Insert(t, conn, storage.RumViewsWriter, []storage.Row{first})
	storagetest.Insert(t, conn, storage.RumViewsWriter, []storage.Row{second})

	if n := storagetest.Count(t, conn, "rum_views"); n != 2 {
		t.Fatalf("rows before merge: got %d, want 2 — ReplacingMergeTree collapses on merge, not on insert", n)
	}

	got := storagetest.QueryRow(t, conn,
		"SELECT document_version, event_type, view_time_spent, error_count, crash_count, ddtags['team'] "+
			"FROM rum_views FINAL WHERE tenant_id = 't' AND view_id = 'view'")
	if v, ok := got[0].(uint64); !ok || v != 2 {
		t.Errorf("document_version: got %v, want 2 — FINAL returned the older version", got[0])
	}
	if s, _ := got[1].(string); s != "view_update" {
		t.Errorf("event_type: got %v, want view_update", got[1])
	}
	if v, _ := got[2].(int64); v != 9000 {
		t.Errorf("view_time_spent: got %v, want 9000", got[2])
	}
	if !reflect.DeepEqual(got[5], []string{"web", "payments"}) {
		t.Errorf("ddtags['team']: got %v, want both values — tags are a multiset", got[5])
	}
}

// Every RUM table inserted once, with the awkward values: a NULL beside a
// stored zero, a repeated tag key, a nanosecond timestamp, arrays kept in
// order, and binary bytes in a String column. This is what proves the
// migration, the INSERT column lists and AppendTo agree with ClickHouse's own
// types — the three can only disagree at runtime otherwise.
func TestRumTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	req := storage.RumRequest{
		TenantID: "t", ReceivedAt: time.Now().UTC(),
		DDSource: "ios", EVPOrigin: "ios", EVPOriginVersion: "3.17.0",
		RequestID: "r1", IdempotencyKey: "sha1", DDAPI: "fetch",
		RemoteAddr: "203.0.113.7", UserAgent: "App/1.0 CFNetwork",
		QueryExtra: `{"brand_new":["1"]}`,
	}
	// Recent, for the same TTL reason as the test above.
	date := time.Now().UTC().Add(-time.Hour).Truncate(time.Millisecond)
	nanos := date.Add(123 * time.Nanosecond)

	storagetest.Insert(t, conn, storage.RumEventsWriter, []storage.Row{
		storage.RumEventRow{
			Req: req, Date: date, ApplicationID: "app", SessionID: "sess", ViewID: "v",
			EventType: "resource", Source: "ios",
			ResourceURL: "https://x/y", ResourceStatusCode: ptr[int64](204),
			ResourceSize: ptr[int64](0), // a stored zero
			Tags:         map[string][]string{"team": {"web", "payments"}},
			Event:        `{"type":"resource"}`,
		},
	})
	storagetest.Insert(t, conn, storage.RumTelemetryWriter, []storage.Row{
		storage.RumTelemetryRow{
			Req: req, Date: date, ApplicationID: "app", SessionID: "sess",
			Type: "telemetry", Status: "debug", TelemetryType: "configuration",
			Configuration: `{"session_sample_rate":100}`, ExperimentalFeatures: []string{"foo"},
			Event: `{"type":"telemetry"}`,
		},
	})
	storagetest.Insert(t, conn, storage.RumTimeseriesWriter, []storage.Row{
		storage.RumTimeseriesRow{
			Req: req, Date: date, ApplicationID: "app", SessionID: "sess",
			Name: "memory", ID: "ts", Schema: "object-v2",
			Start: nanos, End: nanos.Add(time.Second),
			Timestamps: []int64{nanos.UnixNano(), nanos.Add(500 * time.Millisecond).UnixNano()},
			Values:     `{"memory_footprint":[1,2]}`,
			Event:      `{"type":"timeseries"}`,
		},
	})
	storagetest.Insert(t, conn, storage.RumReplaySegmentsWriter, []storage.Row{
		storage.RumReplaySegmentRow{
			Req: req, ApplicationID: "app", SessionID: "sess", ViewID: "v", Source: "ios",
			Variant: "segment", PartName: "file0", PartFilename: "file0",
			Start: &date, RecordsCount: ptr[int64](12), HasFullSnapshot: ptr(true),
			Segment: string([]byte{0x78, 0x9c, 0x00, 0xff}), // zlib header bytes, verbatim
			Event:   `{"records_count":12}`,
		},
	})
	storagetest.Insert(t, conn, storage.RumSpansWriter, []storage.Row{
		storage.RumSpanRow{
			Req: req, TraceID: "9007199254740993", SpanID: "2", ParentID: "0",
			Name: "op", Service: "app", Resource: "GET /x", Type: "custom", Env: "prod",
			Start: nanos, DurationNS: 1500000, Error: 1,
			Meta:    map[string]string{"device.brand": "Apple"},
			Metrics: map[string]float64{"_top_level": 1},
			Span:    `{"name":"op"}`,
		},
	})

	for table, want := range map[string]uint64{
		"rum_events": 1, "rum_telemetry": 1, "rum_timeseries": 1,
		"rum_replay_segments": 1, "rum_spans": 1,
	} {
		if n := storagetest.Count(t, conn, table); n != want {
			t.Errorf("%s: got %d rows, want %d", table, n, want)
		}
	}

	got := storagetest.QueryRow(t, conn,
		"SELECT resource_size, long_task_duration, ddtags['team'], dd_api, query_extra FROM rum_events")
	// A Nullable column scans as a pointer, which is the whole point: a
	// stored 0 and an absent value are two different results here, not one.
	if v, ok := got[0].(*int64); !ok || v == nil || *v != 0 {
		t.Errorf("resource_size: got %#v, want a stored 0", got[0])
	}
	if v, ok := got[1].(*int64); !ok || v != nil {
		t.Errorf("long_task_duration: got %#v, want NULL — this row is a resource and never measured one", got[1])
	}
	if !reflect.DeepEqual(got[2], []string{"web", "payments"}) {
		t.Errorf("ddtags['team']: got %v", got[2])
	}
	if s, _ := got[3].(string); s != "fetch" {
		t.Errorf("dd_api: got %v", got[3])
	}
	if s, _ := got[4].(string); s != `{"brand_new":["1"]}` {
		t.Errorf("query_extra: got %v", got[4])
	}

	// Nanoseconds must survive the DateTime64(9) columns, and the segment
	// bytes must come back byte for byte.
	got = storagetest.QueryRow(t, conn, "SELECT start, timestamps FROM rum_timeseries")
	if ts, ok := got[0].(time.Time); !ok || !ts.Equal(nanos) {
		t.Errorf("timeseries start: got %#v, want %s", got[0], nanos)
	}
	if want := []int64{nanos.UnixNano(), nanos.Add(500 * time.Millisecond).UnixNano()}; !reflect.DeepEqual(got[1], want) {
		t.Errorf("timestamps: got %v, want %v", got[1], want)
	}

	got = storagetest.QueryRow(t, conn, "SELECT segment, index_in_view FROM rum_replay_segments")
	if s, _ := got[0].(string); s != string([]byte{0x78, 0x9c, 0x00, 0xff}) {
		t.Errorf("segment: got %q — the blob must come back exactly as it arrived", s)
	}
	// Android sends an explicit null here and iOS omits the key; only the
	// browser sends a number. NULL is the honest answer for the other two.
	if v, ok := got[1].(*int64); !ok || v != nil {
		t.Errorf("index_in_view: got %#v, want NULL", got[1])
	}

	got = storagetest.QueryRow(t, conn, "SELECT trace_id, start, meta['device.brand'] FROM rum_spans")
	if s, _ := got[0].(string); s != "9007199254740993" {
		t.Errorf("trace_id: got %v — 2^53+1 did not survive", got[0])
	}
	if ts, ok := got[1].(time.Time); !ok || !ts.Equal(nanos) {
		t.Errorf("span start: got %#v, want %s", got[1], nanos)
	}
	if s, _ := got[2].(string); s != "Apple" {
		t.Errorf("meta['device.brand']: got %v", got[2])
	}
}

// Every writer this file registers must be reachable under the name handlers
// address, and its INSERT must agree with its AppendTo. storagetest's own
// suite walks the whole registry, but a failure there names a table without
// naming the branch that added it.
func TestRumWritersArity(t *testing.T) {
	cases := []struct {
		name   gen.Atom
		sample storage.Row
	}{
		{storage.RumViewsWriter, storage.RumViewRow{}},
		{storage.RumEventsWriter, storage.RumEventRow{}},
		{storage.RumTelemetryWriter, storage.RumTelemetryRow{}},
		{storage.RumTimeseriesWriter, storage.RumTimeseriesRow{}},
		{storage.RumReplaySegmentsWriter, storage.RumReplaySegmentRow{}},
		{storage.RumSpansWriter, storage.RumSpanRow{}},
	}
	for _, tc := range cases {
		t.Run(string(tc.name), func(t *testing.T) {
			storagetest.AssertArity(t, tc.name, tc.sample)
		})
	}
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

type multipartPart struct {
	name     string
	filename string
	data     []byte
}

func multipartBody(t *testing.T, parts []multipartPart) ([]byte, string) {
	t.Helper()
	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)
	for _, p := range parts {
		var (
			w   interface{ Write([]byte) (int, error) }
			err error
		)
		if p.filename != "" {
			w, err = mw.CreateFormFile(p.name, p.filename)
		} else {
			w, err = mw.CreateFormField(p.name)
		}
		if err != nil {
			t.Fatalf("multipart part %q: %v", p.name, err)
		}
		if _, err := w.Write(p.data); err != nil {
			t.Fatalf("multipart write %q: %v", p.name, err)
		}
	}
	if err := mw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes(), mw.FormDataContentType()
}

func request(method, target string, body []byte, headers map[string]string) *http.Request {
	var r *http.Request
	if body == nil {
		r = httptest.NewRequest(method, target, nil)
	} else {
		r = httptest.NewRequest(method, target, bytes.NewReader(body))
	}
	for k, v := range headers {
		r.Header.Set(k, v)
	}
	return r
}

func serve(h http.Handler, r *http.Request) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}

// readSample loads one of the upstream event samples shipped with the
// generated types, so the conversion tests run against Datadog's own JSON
// rather than something written here to match the code.
func readSample(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile("../rumevents/testdata/samples/" + name)
	if err != nil {
		t.Fatalf("read sample %s: %v", name, err)
	}
	return data
}

// compact squeezes a pretty-printed sample onto one line, which is what
// NDJSON framing requires.
func compact(t *testing.T, lines [][]byte) [][]byte {
	t.Helper()
	out := make([][]byte, 0, len(lines))
	for _, l := range lines {
		var buf bytes.Buffer
		if err := json.Compact(&buf, l); err != nil {
			t.Fatalf("compact: %v", err)
		}
		out = append(out, buf.Bytes())
	}
	return out
}

// ginContext builds the context a middleware or converter sees, without a
// route or an engine around it.
func ginContext(method, target string, body []byte, headers map[string]string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = request(method, target, body, headers)
	return c, w
}

func ptr[T any](v T) *T { return &v }
