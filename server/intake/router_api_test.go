package intake

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/gogen"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// ---------------------------------------------------------------------------
// metrics
// ---------------------------------------------------------------------------

// v1 has no resource list; the agent's own encoder sends `device` as an
// undeclared key, and the handler used to tally it into a log line and drop it.
// It now lands in the same resources map v2's [type, name] list fills, so one
// query answers "which device?" across both wire versions.
func TestSeriesV1KeepsDeviceAndUndeclaredKeys(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"series":[{
		"metric":"system.disk.used","host":"h1","type":"gauge",
		"points":[[1790151330,42]],"tags":["env:prod"],
		"device":"/dev/sda1","source_type_name":"System","unit":"byte",
		"an_agent_we_have_not_seen_yet":{"nested":1}
	}]}`)

	w := post(t, e, "/api/v1/series", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	// The agent reads this body; an empty object would be an API error to it.
	if got := w.Body.String(); got != `{"errors":[]}` {
		t.Errorf("body: got %s, want {\"errors\":[]}", got)
	}

	points := Rows[storage.MetricPoint](node)
	if len(points) != 1 {
		t.Fatalf("points: got %d, want 1", len(points))
	}
	p := points[0]
	if p.Resources["device"] != "/dev/sda1" {
		t.Errorf("device: got %q, want /dev/sda1 — v1's device is v2's device resource", p.Resources["device"])
	}
	if p.SourceType != "System" || p.Unit != "byte" {
		t.Errorf("source_type_name/unit lost: %q/%q", p.SourceType, p.Unit)
	}
	if got := p.Extra["an_agent_we_have_not_seen_yet"]; got != `{"nested":1}` {
		t.Errorf("undeclared key: got %q, want the value JSON-encoded", got)
	}
	// v1 carries no origin at all, and zero is what "not sent" means there.
	if p.OriginProduct != 0 || p.OriginCategory != 0 || p.OriginService != 0 {
		t.Errorf("origin: got %d/%d/%d, want all zero", p.OriginProduct, p.OriginCategory, p.OriginService)
	}
}

// A point missing its timestamp or its value is not a point, and the series it
// came from is stored through its other points — so the pair itself is the
// smallest thing worth keeping, rather than a silent `continue`.
func TestSeriesV1KeepsBrokenPointsRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"series":[{"metric":"m","points":[[1790151330,1],[null,2]]}]}`)
	if w := post(t, e, "/api/v1/series", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	if n := len(Rows[storage.MetricPoint](node)); n != 1 {
		t.Errorf("points: got %d, want 1 — the good point must survive its neighbour", n)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "series" {
		t.Fatalf("raw rows: %+v, want one for the broken pair", raws)
	}
	if !strings.Contains(raws[0].Body, "null") {
		t.Errorf("the pair was not kept as it arrived: %s", raws[0].Body)
	}
}

// Origin is Datadog's own provenance signal — which product and which
// integration produced a metric. It rode in as one opaque "10/11/42" log
// string; the question people ask of it ("show me everything from dogstatsd")
// is a filter, so it needs columns.
func TestSeriesV2StoresOriginAndNonHostResources(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	payload := gogen.MetricPayload{Series: []*gogen.MetricPayload_MetricSeries{{
		Metric: "system.cpu.idle",
		Resources: []*gogen.MetricPayload_Resource{
			{Type: "host", Name: "h1"},
			{Type: "device", Name: "eth0"},
		},
		Tags:   []string{"env:prod", "env:canary"},
		Points: []*gogen.MetricPayload_MetricPoint{{Timestamp: 1790151330, Value: 0.5}},
		Metadata: &gogen.Metadata{Origin: &gogen.Origin{
			OriginProduct: 10, OriginCategory: 11, OriginService: 42,
		}},
	}}}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}

	if w := post(t, e, "/api/v2/series", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	points := Rows[storage.MetricPoint](node)
	if len(points) != 1 {
		t.Fatalf("points: got %d, want 1", len(points))
	}
	p := points[0]
	if p.Host != "h1" {
		t.Errorf("host resource: got %q", p.Host)
	}
	if p.Resources["device"] != "eth0" {
		t.Errorf("device resource: got %q, want eth0", p.Resources["device"])
	}
	if p.OriginProduct != 10 || p.OriginCategory != 11 || p.OriginService != 42 {
		t.Errorf("origin: got %d/%d/%d, want 10/11/42", p.OriginProduct, p.OriginCategory, p.OriginService)
	}
	// Two tags sharing a key: the multiset shape, end to end.
	if got := p.Tags["env"]; !reflect.DeepEqual(got, []string{"prod", "canary"}) {
		t.Errorf("tags[env]: got %v, want [prod canary]", got)
	}
}

// ---------------------------------------------------------------------------
// sketches
// ---------------------------------------------------------------------------

// CommonMetadata describes the SENDER, not the metric: agent version,
// timezone, its clock and both of its IPs. It was logged once per batch and
// dropped, which is exactly the data that explains a gap on a chart.
//
// The legacy GK Distribution in the same payload cannot become a bucket row
// without inventing numbers, so it goes to raw_payloads whole.
func TestSketchesStoreOriginSenderIdentityAndLegacyDistributions(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	payload := gogen.SketchPayload{
		Sketches: []gogen.SketchPayload_Sketch{{
			Metric: "http.request.duration",
			Host:   "h1",
			Tags:   []string{"env:prod"},
			Dogsketches: []gogen.SketchPayload_Sketch_Dogsketch{{
				Ts: 1790151330, Cnt: 3, Min: 1, Max: 9, Avg: 4, Sum: 12,
				K: []int32{1, 2}, N: []uint32{2, 1},
			}},
			Distributions: []gogen.SketchPayload_Sketch_Distribution{{
				Ts: 1790151330, Cnt: 2, V: []float64{1, 2}, G: []uint32{1, 1},
			}},
			Metadata: &gogen.Metadata{Origin: &gogen.Origin{
				OriginProduct: 10, OriginCategory: 10, OriginService: 7,
			}},
		}},
		Metadata: gogen.CommonMetadata{
			AgentVersion: "7.58.2", Timezone: "UTC", CurrentEpoch: 1790151330,
			InternalIp: "10.0.0.4", PublicIp: "203.0.113.7",
			ApiKey: "must-never-be-stored",
		},
	}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}

	if w := post(t, e, "/api/beta/sketches", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	sketches := Rows[storage.SketchRow](node)
	if len(sketches) != 1 {
		t.Fatalf("sketch rows: got %d, want 1 (one per dogsketch)", len(sketches))
	}
	if s := sketches[0]; s.OriginProduct != 10 || s.OriginService != 7 {
		t.Errorf("origin: got %d/%d/%d", s.OriginProduct, s.OriginCategory, s.OriginService)
	}

	batches := Rows[storage.AgentBatchMetadataRow](node)
	if len(batches) != 1 {
		t.Fatalf("agent_batch_metadata rows: got %d, want 1 per request", len(batches))
	}
	b := batches[0]
	if b.AgentVersion != "7.58.2" || b.Timezone != "UTC" || b.InternalIP != "10.0.0.4" {
		t.Errorf("sender identity lost: %+v", b)
	}
	if b.CurrentEpoch != 1790151330 {
		t.Errorf("current_epoch: got %v — it is the clock-skew measurement", b.CurrentEpoch)
	}
	// The api_key is a field of CommonMetadata and must reach no column: the
	// request already authenticated, and a credential does not belong in a
	// table with a TTL.
	if strings.Contains(strings.Join([]string{b.AgentVersion, b.Timezone, b.InternalIP, b.PublicIP, b.Intake}, " "),
		"must-never-be-stored") {
		t.Error("the api_key from CommonMetadata reached a column")
	}

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "sketches" || raws[0].Reason != "no_schema" {
		t.Fatalf("raw rows: %+v, want one sketches/no_schema for the legacy Distribution", raws)
	}
	if !strings.Contains(raws[0].Body, "distributions") {
		t.Errorf("the legacy distribution was not kept: %s", raws[0].Body)
	}
}

// ---------------------------------------------------------------------------
// check_run
// ---------------------------------------------------------------------------

// datadogpy posts a BARE OBJECT here, not an array, and a check with an
// undeclared key means a newer agent or a third-party client grew a field —
// which is precisely when its value is worth more than a count in a log.
func TestCheckRunTakesABareObjectAndKeepsUndeclaredKeys(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"check":"app.ok","host_name":"h1","timestamp":1790151330,
		"status":1,"message":"slow","tags":["env:prod"],"my_own_field":"hello"}`)

	if w := post(t, e, "/api/v1/check_run", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	runs := Rows[storage.CheckRunRow](node)
	if len(runs) != 1 {
		t.Fatalf("check rows: got %d, want 1", len(runs))
	}
	r := runs[0]
	if r.CheckName != "app.ok" || r.Status != "WARNING" || r.Message != "slow" {
		t.Errorf("row: %+v", r)
	}
	if got := r.Extra["my_own_field"]; got != `"hello"` {
		t.Errorf("extra[my_own_field]: got %q, want the value JSON-encoded", got)
	}
}

// ---------------------------------------------------------------------------
// /intake/
// ---------------------------------------------------------------------------

// The agent's own events rode in on /intake/ and were logged, eight of them,
// and dropped — while the identical shape sent to /api/v1/events was stored.
// They now feed the same table.
func TestIntakeEventsBecomeEventRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{
		"apiKey":"",
		"internalHostname":"agent-host",
		"events":{"System":[{
			"msg_title":"docker restarted","msg_text":"the runtime came back",
			"timestamp":1790151330,"host":"h1","alert_type":"success",
			"priority":"low","aggregation_key":"docker","event_type":"docker",
			"source_type_name":"docker","tags":["env:prod","env:canary"],
			"something_new":42
		}]}
	}`)

	w := post(t, e, "/intake/", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}
	if got := w.Body.String(); got != `{"status":"ok"}` {
		t.Errorf("body: got %s, want {\"status\":\"ok\"}", got)
	}

	events := Rows[storage.EventRow](node)
	if len(events) != 1 {
		t.Fatalf("event rows: got %d, want 1", len(events))
	}
	ev := events[0]
	if ev.Title != "docker restarted" || ev.Text != "the runtime came back" {
		t.Errorf("msg_title/msg_text are the agent's names for title/text: %+v", ev)
	}
	if !ev.Timestamp.Equal(time.Unix(1790151330, 0).UTC()) {
		t.Errorf("timestamp: got %s", ev.Timestamp)
	}
	if ev.EventType != "docker" {
		t.Errorf("event_type: got %q — it exists only on this source", ev.EventType)
	}
	if ev.AlertType != "success" || ev.Priority != "low" {
		t.Errorf("alert_type/priority: %q/%q", ev.AlertType, ev.Priority)
	}
	if got := ev.Tags["env"]; !reflect.DeepEqual(got, []string{"prod", "canary"}) {
		t.Errorf("tags[env]: got %v, want [prod canary]", got)
	}
	if got := ev.Extra["something_new"]; got != "42" {
		t.Errorf("extra[something_new]: got %q, want 42", got)
	}
	if ev.EventIDNum == 0 {
		t.Error("event_id_num is 0 — clients key links on it")
	}
}

// The mapping itself, as a table: the agent's field names differ from the
// public API's for the same three things, and the envelope stands in for what
// an event left out.
func TestIntakeEventRowMapping(t *testing.T) {
	cases := []struct {
		name     string
		ev       map[string]any
		source   string
		host     string
		wantHost string
		wantSrc  string
		wantType string
		wantPrio string
		wantRaw  string
	}{
		{
			name:     "an event without a host belongs to the agent that sent it",
			ev:       map[string]any{"msg_title": "t"},
			source:   "System",
			host:     "agent-host",
			wantHost: "agent-host",
			wantSrc:  "System",
			wantType: "info",
			wantPrio: "normal",
		},
		{
			name:     "the event's own host and source win",
			ev:       map[string]any{"msg_title": "t", "host": "h9", "source_type_name": "kubernetes"},
			source:   "System",
			host:     "agent-host",
			wantHost: "h9",
			wantSrc:  "kubernetes",
			wantType: "info",
			wantPrio: "normal",
		},
		{
			name: "an invented alert type is coerced AND kept",
			// Dropping the event would lose the one thing we were asked to
			// remember; coercing it silently would hide what was sent.
			ev:       map[string]any{"msg_title": "t", "alert_type": "catastrophe"},
			wantHost: "",
			wantSrc:  "",
			wantType: "info",
			wantPrio: "normal",
			wantRaw:  "catastrophe",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row := intakeEventRow(testTenant, tc.source, tc.host, tc.ev)
			if row.Host != tc.wantHost {
				t.Errorf("host: got %q, want %q", row.Host, tc.wantHost)
			}
			if row.SourceTypeName != tc.wantSrc {
				t.Errorf("source_type_name: got %q, want %q", row.SourceTypeName, tc.wantSrc)
			}
			if row.AlertType != tc.wantType {
				t.Errorf("alert_type: got %q, want %q", row.AlertType, tc.wantType)
			}
			if row.Priority != tc.wantPrio {
				t.Errorf("priority: got %q, want %q", row.Priority, tc.wantPrio)
			}
			if row.AlertTypeRaw != tc.wantRaw {
				t.Errorf("alert_type_raw: got %q, want %q", row.AlertTypeRaw, tc.wantRaw)
			}
			// No timestamp on the wire means arrival time, never 1970.
			if row.Timestamp.Year() < 2020 {
				t.Errorf("timestamp fell back to the epoch: %s", row.Timestamp)
			}
		})
	}
}

// The V5 collector sends check statuses as POSITIONAL arrays with no key names
// anywhere, and external_host_tags for hosts the agent is not running on. Both
// were logged and dropped.
func TestIntakeAgentChecksAndExternalHostTags(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{
		"apiKey":"deadbeef","internalHostname":"agent-host","agentVersion":"7.58.2",
		"uuid":"4f1a","meta":{"socket-hostname":"agent-host"},
		"agent_checks":[
			["docker","docker","docker:1",0,""],
			["kubelet","kubernetes","kubelet:2",2,"connection refused","a sixth element"]
		],
		"external_host_tags":[
			["vm-1",{"vsphere":["cluster:prod","cluster:eu"],"aws":["region:eu-west-1"]}]
		]
	}`)

	if w := post(t, e, "/intake/", body); w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200", w.Code)
	}

	checks := Rows[storage.AgentCheckRow](node)
	if len(checks) != 2 {
		t.Fatalf("agent_checks rows: got %d, want 2", len(checks))
	}
	if checks[0].CheckName != "docker" || checks[0].SourceType != "docker" || checks[0].InstanceID != "docker:1" {
		t.Errorf("positions 0-2: %+v", checks[0])
	}
	if checks[0].Status == nil || *checks[0].Status != 0 {
		t.Errorf("status 0 means OK and must survive as 0, not as NULL: %v", checks[0].Status)
	}
	if checks[1].Message != "connection refused" {
		t.Errorf("position 4: %q", checks[1].Message)
	}
	if !strings.Contains(checks[1].PositionalExtra, "a sixth element") {
		t.Errorf("a sixth element is what this column exists for: %q", checks[1].PositionalExtra)
	}
	if !strings.Contains(checks[0].Meta, "socket-hostname") {
		t.Errorf("meta: %q", checks[0].Meta)
	}
	if checks[0].AgentVersion != "7.58.2" || checks[0].Hostname != "agent-host" {
		t.Errorf("sender identity: %+v", checks[0])
	}

	tags := Rows[storage.ExternalHostTagsRow](node)
	if len(tags) != 2 {
		t.Fatalf("external_host_tags rows: got %d, want one per SOURCE", len(tags))
	}
	for _, r := range tags {
		if r.Host != "vm-1" {
			t.Errorf("host: got %q — it is the host DESCRIBED, not the sender", r.Host)
		}
	}
	// Sorted by source, so a replay of the same payload produces the same rows.
	if tags[0].Source != "aws" || tags[1].Source != "vsphere" {
		t.Errorf("sources: got %q, %q", tags[0].Source, tags[1].Source)
	}
	if got := tags[1].Tags["cluster"]; !reflect.DeepEqual(got, []string{"prod", "eu"}) {
		t.Errorf("tags[cluster]: got %v, want [prod eu] — tags are a multiset", got)
	}
}

// The host payload had nine columns and about twenty keys. What was missing is
// precisely what a fleet inventory is asked: agent flavour, Python, FIPS,
// install method, OTLP, interfaces, filesystems, and which tag SOURCES a host
// carries.
func TestIntakeHostRowKeepsTheWholeEnvelope(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	// gohai arrives as a STRING holding JSON — hostimpl serialises it that way.
	gohai := `{"cpu":{"cpu_cores":"2"},"memory":{"total":"8Gb"},` +
		`"platform":{"os":"linux"},"filesystem":[{"name":"/dev/sda1"}],` +
		`"network":{"ipaddress":"10.0.0.4"},"gpu":{"vendor":"nvidia"}}`
	envelope := map[string]any{
		"apiKey":             "deadbeef",
		"internalHostname":   "h1",
		"agentVersion":       "7.58.2",
		"os":                 "linux",
		"uuid":               "4f1a",
		"agent-flavor":       "agent",
		"python":             "3.12.6",
		"gohai":              gohai,
		"host-tags":          map[string][]string{"system": {"env:prod"}, "google_tags": {"zone:eu-west1-b"}},
		"systemStats":        map[string]any{"cpuCores": 2, "machine": "x86_64"},
		"meta":               map[string]any{"socket-hostname": "h1"},
		"network":            map[string]any{"ipaddress": "10.0.0.4"},
		"logs":               map[string]any{"transport": "HTTP"},
		"install-method":     map[string]any{"tool": "helm"},
		"proxy-info":         map[string]any{"no-proxy-nonexact-match": false},
		"otlp":               map[string]any{"enabled": true},
		"container-meta":     map[string]any{"cluster_name": "prod"},
		"fips_mode":          false,
		"fips_proxy_enabled": true,
		"resources":          map[string]any{"processes": map[string]any{"snaps": []any{}}},
		"a_key_from_7_60":    "surprise",
	}
	body, err := json.Marshal(envelope)
	if err != nil {
		t.Fatal(err)
	}

	if w := post(t, e, "/intake/", body); w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200", w.Code)
	}

	hosts := Rows[storage.HostRow](node)
	if len(hosts) != 1 {
		t.Fatalf("host rows: got %d, want 1", len(hosts))
	}
	h := hosts[0]

	if h.UUID != "4f1a" || h.AgentFlavor != "agent" || h.PythonVersion != "3.12.6" {
		t.Errorf("agent identity: %+v", h)
	}
	if h.CPU["cpu_cores"] != "2" || h.Platform["os"] != "linux" {
		t.Errorf("gohai sections: cpu=%v platform=%v", h.CPU, h.Platform)
	}
	if !strings.Contains(h.Filesystem, "/dev/sda1") {
		t.Errorf("gohai.filesystem: %q", h.Filesystem)
	}
	// The envelope's network (the host's addresses) owns the column; gohai's
	// section of the same name is a different thing and keeps its name in
	// gohai_extra rather than overwriting it.
	if !strings.Contains(h.Network, "10.0.0.4") {
		t.Errorf("network: %q", h.Network)
	}
	if _, ok := h.GohaiExtra["network"]; !ok {
		t.Errorf("gohai.network was dropped: %v", h.GohaiExtra)
	}
	if _, ok := h.GohaiExtra["gpu"]; !ok {
		t.Errorf("an unknown gohai section was dropped: %v", h.GohaiExtra)
	}

	if h.InstallMethod["tool"] != `"helm"` {
		t.Errorf("install-method: got %v, values are JSON-encoded", h.InstallMethod)
	}
	if h.SystemStats["cpuCores"] != "2" {
		t.Errorf("systemStats: got %v", h.SystemStats)
	}
	if h.ContainerMeta["cluster_name"] != `"prod"` {
		t.Errorf("container-meta: got %v", h.ContainerMeta)
	}
	if !strings.Contains(h.OTLP, "true") || !strings.Contains(h.Logs, "HTTP") {
		t.Errorf("otlp/logs: %q / %q", h.OTLP, h.Logs)
	}

	// Absent is not zero, and "FIPS is off" is not "this agent does not know
	// about FIPS".
	if h.FIPSMode == nil || *h.FIPSMode != 0 {
		t.Errorf("fips_mode: got %v, want 0 (sent as false)", h.FIPSMode)
	}
	if h.FIPSProxyEnabled == nil || *h.FIPSProxyEnabled != 1 {
		t.Errorf("fips_proxy_enabled: got %v, want 1", h.FIPSProxyEnabled)
	}

	// Every tag source, not only "system" — on a cloud host the provider's
	// tags used to be discarded outright.
	if got := h.HostTags["google_tags"]; !reflect.DeepEqual(got, []string{"zone:eu-west1-b"}) {
		t.Errorf("host_tags[google_tags]: got %v", got)
	}
	if got := h.Tags["env"]; !reflect.DeepEqual(got, []string{"prod"}) {
		t.Errorf("tags still come from the system source: %v", h.Tags)
	}

	if h.Resources == "" {
		t.Error("the legacy process snapshot was dropped")
	}
	if got := h.IntakeExtra["a_key_from_7_60"]; got != `"surprise"` {
		t.Errorf("intake_extra: got %q", got)
	}
	// The API key is in the envelope and must reach no column.
	for k, v := range h.IntakeExtra {
		if strings.Contains(v, "deadbeef") {
			t.Errorf("the api key reached intake_extra[%s]", k)
		}
	}
}

// An absent fips key is a NULL, not a false: the three-way distinction is the
// reason the column is Nullable.
func TestHostRowLeavesAbsentFIPSNull(t *testing.T) {
	row := hostRow(testTenant, "h1", "7.58.2", "linux", time.Now().UTC(),
		map[string]json.RawMessage{}, map[string]json.RawMessage{}, nil)
	if row.FIPSMode != nil || row.FIPSProxyEnabled != nil {
		t.Errorf("absent fips keys became %v/%v, want NULL", row.FIPSMode, row.FIPSProxyEnabled)
	}
	if row.Meta != "" || row.Network != "" {
		t.Errorf("an absent section must be the empty string, not \"{}\": meta=%q network=%q", row.Meta, row.Network)
	}
}

// A shape no branch recognises is the whole reason raw_payloads exists: the
// bytes that would tell us what a fifth producer sends arrive once.
func TestIntakeUnknownShapeGoesToRawPayloads(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	if w := post(t, e, "/intake/", []byte(`{"somethingElse":{"a":1}}`)); w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 — the contract does not change for an unknown shape", w.Code)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Reason != "unexpected_shape" {
		t.Fatalf("raw rows: %+v", raws)
	}
	if !strings.Contains(raws[0].Note, "somethingElse") {
		t.Errorf("the note should name the keys we saw: %q", raws[0].Note)
	}
}

// ---------------------------------------------------------------------------
// /api/v1/metadata
// ---------------------------------------------------------------------------

// Eleven producers share this path and every one of them was decoded and
// discarded. One table, one row per variant key, the variant's own body kept
// as the JSON it arrived as.
func TestMetadataStoresTheVariantAndItsEnvelope(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{
		"hostname":"h1","timestamp":1790151330,"uuid":"4f1a",
		"check_metadata":{"docker":[{"version.raw":"1.2.3","config.hash":"abc"}]},
		"a_sibling_key":{"x":1}
	}`)

	w := post(t, e, "/api/v1/metadata", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}
	if got := w.Body.String(); got != `{"status":"ok"}` {
		t.Errorf("body: got %s", got)
	}

	rows := Rows[storage.AgentMetadataRow](node)
	if len(rows) != 1 {
		t.Fatalf("metadata rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Variant != "check_metadata" {
		t.Errorf("variant: got %q", r.Variant)
	}
	if r.Hostname != "h1" || r.UUID != "4f1a" {
		t.Errorf("envelope: %+v", r)
	}
	if r.Timestamp == nil || !r.Timestamp.Equal(time.Unix(1790151330, 0).UTC()) {
		t.Errorf("timestamp: got %v", r.Timestamp)
	}
	if !strings.Contains(r.Payload, "version.raw") {
		t.Errorf("payload: %q — the variant's body is kept verbatim", r.Payload)
	}
	if _, ok := r.EnvelopeExtra["a_sibling_key"]; !ok {
		t.Errorf("envelope_extra: %v", r.EnvelopeExtra)
	}
}

// An absent envelope timestamp is NULL, not 1970 — which reads like a real
// value and sorts before everything.
func TestMetadataWithoutTimestampStoresNull(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	post(t, e, "/api/v1/metadata", []byte(`{"hostname":"h1","ha_agent_metadata":{"enabled":true}}`))

	rows := Rows[storage.AgentMetadataRow](node)
	if len(rows) != 1 {
		t.Fatalf("metadata rows: got %d, want 1", len(rows))
	}
	if rows[0].Timestamp != nil {
		t.Errorf("timestamp: got %v, want NULL", rows[0].Timestamp)
	}
}

// A variant key this switch does not know is a producer we have never seen.
func TestMetadataUnknownVariantGoesToRawPayloads(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	post(t, e, "/api/v1/metadata", []byte(`{"hostname":"h1","brand_new_metadata":{"a":1}}`))

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "metadata" || raws[0].Reason != "no_schema" {
		t.Fatalf("raw rows: %+v, want one metadata/no_schema", raws)
	}
}

// ---------------------------------------------------------------------------
// /api/v1/events
// ---------------------------------------------------------------------------

// This endpoint MUST return a body: the client deserialises the reply into
// EventCreateResponse and keeps the id. The contract is pinned here alongside
// the new columns, because the columns are the part that changed.
func TestEventsKeepTheirParentAndTheirOriginalWords(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"title":"deploy","text":"v2 is out","date_happened":1790151330,
		"host":"h1","alert_type":"warning","priority":"low",
		"related_event_id":8675309,"tags":["env:prod"],"team":"core"}`)

	w := post(t, e, "/api/v1/events", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	var reply struct {
		Status string `json:"status"`
		Event  struct {
			ID    uint64 `json:"id"`
			IDStr string `json:"id_str"`
			Title string `json:"title"`
		} `json:"event"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &reply); err != nil {
		t.Fatalf("the reply must deserialise into EventCreateResponse: %v (%s)", err, w.Body)
	}
	if reply.Status != "ok" || reply.Event.ID == 0 || reply.Event.Title != "deploy" {
		t.Errorf("reply: %+v", reply)
	}

	rows := Rows[storage.EventRow](node)
	if len(rows) != 1 {
		t.Fatalf("event rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.RelatedEventID == nil || *r.RelatedEventID != 8675309 {
		t.Errorf("related_event_id: got %v — it is the link to the parent event", r.RelatedEventID)
	}
	// What the sender wrote survives beside the coerced value. On this path
	// the two agree, because a value outside the enum never reaches the row at
	// all (see TestEventsWithAnInventedAlertTypeAreKeptRaw); on the /intake/
	// path, where the fields are free strings, they diverge.
	if r.AlertType != "warning" || r.AlertTypeRaw != "warning" {
		t.Errorf("alert_type/raw: %q/%q", r.AlertType, r.AlertTypeRaw)
	}
	if r.Priority != "low" || r.PriorityRaw != "low" {
		t.Errorf("priority/raw: %q/%q", r.Priority, r.PriorityRaw)
	}
	if got := r.Extra["team"]; got != `"core"` {
		t.Errorf("extra[team]: got %q", got)
	}
	// Nothing sent related_event_id on the agent path, so nothing invents one.
	if r.EventType != "" {
		t.Errorf("event_type: got %q, want empty on the public API path", r.EventType)
	}
}

// datadogV1.EventCreateRequest's alert_type is a generated ENUM, so a value
// outside Datadog's vocabulary makes the whole event unparseable — none of the
// declared fields is populated. A row built from that would be empty strings
// wearing an event id, so the object goes to raw_payloads instead, and the
// response contract does not move.
func TestEventsWithAnInventedAlertTypeAreKeptRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	w := post(t, e, "/api/v1/events",
		[]byte(`{"title":"deploy","text":"v2 is out","alert_type":"catastrophe"}`))
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	if n := len(Rows[storage.EventRow](node)); n != 0 {
		t.Errorf("event rows: got %d, want 0 — an unparsed event is not an event", n)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "events" || raws[0].Reason != "unexpected_shape" {
		t.Fatalf("raw rows: %+v, want one events/unexpected_shape", raws)
	}
	if !strings.Contains(raws[0].Body, "catastrophe") {
		t.Errorf("the object we could not fit was not kept: %s", raws[0].Body)
	}
}

// An event with no parent must arrive as NULL: Datadog's id space starts at 1
// and a 0 would read as a real event.
func TestEventsWithoutARelatedIDStoreNull(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	post(t, e, "/api/v1/events", []byte(`{"title":"t","text":"x"}`))
	rows := Rows[storage.EventRow](node)
	if len(rows) != 1 {
		t.Fatalf("event rows: got %d, want 1", len(rows))
	}
	if rows[0].RelatedEventID != nil {
		t.Errorf("related_event_id: got %v, want NULL", *rows[0].RelatedEventID)
	}
}

// ---------------------------------------------------------------------------
// distribution_points
// ---------------------------------------------------------------------------

// The raw values are deliberately not kept — see docs/tables/api.md — but a
// pair that could not be split at all still holds numbers nobody else has.
func TestDistributionPointsKeepAnUnsplittablePairRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"series":[
		{"metric":"latency","points":[[1790151330,[1,2,3]]],"tags":["env:prod"],"team":"core"},
		{"metric":"broken","points":[[1790151330]]}
	]}`)

	if w := post(t, e, "/api/v1/distribution_points", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	sketches := Rows[storage.SketchRow](node)
	if len(sketches) != 1 {
		t.Fatalf("sketch rows: got %d, want 1", len(sketches))
	}
	if sketches[0].Count != 3 || sketches[0].Sum != 6 {
		t.Errorf("the sketch was built from the values: %+v", sketches[0])
	}
	if got := sketches[0].Extra["team"]; got != `"core"` {
		t.Errorf("series extra: got %q", got)
	}

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "distribution_points" {
		t.Fatalf("raw rows: %+v, want one for the pair with no values", raws)
	}
}

// ---------------------------------------------------------------------------
// endpoints the agent reads from
// ---------------------------------------------------------------------------

// The proof IS the credential, so the table keeps its SHA-256 and never the
// proof. The reply is what the agent parses: an empty api_key stalls every
// product configured with delegated_auth.
func TestIntakeKeyStoresAFingerprintAndNeverTheProof(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	req := httptest.NewRequest(http.MethodPost, "/api/v2/intake-key", bytes.NewReader(nil))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Authorization", "Delegated super-secret-proof")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200", w.Code)
	}
	var reply struct {
		Data struct {
			Attributes struct {
				APIKey string `json:"api_key"`
			} `json:"attributes"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &reply); err != nil {
		t.Fatalf("reply: %v (%s)", err, w.Body)
	}
	if reply.Data.Attributes.APIKey != testAPIKey {
		t.Errorf("api_key: got %q — an empty one is an error on the agent's side", reply.Data.Attributes.APIKey)
	}

	rows := Rows[storage.DelegatedAuthRow](node)
	if len(rows) != 1 {
		t.Fatalf("delegated_auth rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Scheme != "Delegated" {
		t.Errorf("scheme: got %q", r.Scheme)
	}
	if r.ProofFingerprint == "" || len(r.ProofFingerprint) != 64 {
		t.Errorf("proof_fingerprint: got %q, want a full SHA-256 hex", r.ProofFingerprint)
	}
	if strings.Contains(r.ProofFingerprint, "super-secret-proof") {
		t.Error("the proof itself reached the table")
	}
	if r.APIKeyID != "test" {
		t.Errorf("api_key_id: got %q — the key by its id, never the key", r.APIKeyID)
	}
}

// The uploader asks which build ids we hold; we answer "none", and the
// question is what a future srcmap handler diffs against. The reply must stay
// a JSON:API array or the uploader's decode fails and no symbol is ever sent.
func TestSymbolsQueryStoresTheBuildIDsItWasAskedAbout(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"data":{"type":"symbolsQuery","id":"1","attributes":{
		"arch":"x86_64","buildIds":["aaaa","bbbb","cccc"]}}}`)

	w := post(t, e, "/api/v2/profiles/symbols/query", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200", w.Code)
	}
	if got := w.Body.String(); got != `{"data":[]}` {
		t.Errorf("body: got %s, want {\"data\":[]}", got)
	}

	rows := Rows[storage.SymbolQueryRow](node)
	if len(rows) != 1 {
		t.Fatalf("symbol_queries rows: got %d, want 1", len(rows))
	}
	if rows[0].Arch != "x86_64" {
		t.Errorf("arch: got %q", rows[0].Arch)
	}
	// Order matters: the uploader's reply is positional against this list.
	if !reflect.DeepEqual(rows[0].BuildIDs, []string{"aaaa", "bbbb", "cccc"}) {
		t.Errorf("build_ids: got %v", rows[0].BuildIDs)
	}
}

// ---------------------------------------------------------------------------
// Private Action Runner
// ---------------------------------------------------------------------------

// Enrollment hands out the identity every later PAR request is signed with,
// and nothing stored it — so nothing could ever verify one of those
// signatures. The reply's shape is equally load-bearing: a status other than
// 200, or a type other than "createRunnerResponse", and the runner retries
// forever.
func TestRunnerEnrollStoresTheIdentityItHandsOut(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	pem := "-----BEGIN PUBLIC KEY-----\nMFkwEwYH\n-----END PUBLIC KEY-----\n"
	body, err := json.Marshal(map[string]any{"data": map[string]any{
		"type": "createRunnerRequest",
		"attributes": map[string]any{
			"runner_name":                 "runner-1",
			"runner_modes":                []string{"workflow_automation", "app_builder"},
			"runner_host":                 "runner-host",
			"public_key_pem":              pem,
			"agent_hostname":              "agent-host",
			"orch_cluster_id":             "cluster-9",
			"agent_flavor":                "agent",
			"an_attribute_we_do_not_name": "kept",
		},
	}})
	if err != nil {
		t.Fatal(err)
	}

	w := post(t, e, "/api/unstable/on_prem_runners/api_key_only", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want exactly 200 — anything else is retried forever", w.Code)
	}
	var reply struct {
		Data struct {
			Type       string `json:"type"`
			ID         string `json:"id"`
			Attributes struct {
				RunnerID string `json:"runner_id"`
				OrgID    int64  `json:"org_id"`
			} `json:"attributes"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &reply); err != nil {
		t.Fatalf("reply: %v (%s)", err, w.Body)
	}
	if reply.Data.Type != "createRunnerResponse" {
		t.Errorf("type: got %q — jsonapi.Unmarshal rejects anything else", reply.Data.Type)
	}
	// The runner builds urn:dd:apps:on-prem-runner:<region>:<org>:<runner_id>
	// and splits it on ":".
	if strings.Contains(reply.Data.Attributes.RunnerID, ":") {
		t.Errorf("runner_id contains a colon: %q", reply.Data.Attributes.RunnerID)
	}

	rows := Rows[storage.RunnerEnrollmentRow](node)
	if len(rows) != 1 {
		t.Fatalf("enrollment rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.RunnerID != reply.Data.Attributes.RunnerID {
		t.Errorf("the stored runner_id is not the one we answered with: %q vs %q",
			r.RunnerID, reply.Data.Attributes.RunnerID)
	}
	if r.OrgID != reply.Data.Attributes.OrgID {
		t.Errorf("org_id: stored %d, answered %d — the derivation is ours and only this row remembers it",
			r.OrgID, reply.Data.Attributes.OrgID)
	}
	// The whole PEM, not the fingerprint the log prints: without it no OPMS
	// JWT can ever be checked.
	if r.PublicKeyPEM != pem {
		t.Errorf("public_key_pem: got %q", r.PublicKeyPEM)
	}
	if !reflect.DeepEqual(r.Modes, []string{"workflow_automation", "app_builder"}) {
		t.Errorf("modes: got %v", r.Modes)
	}
	if !strings.Contains(r.Attributes, "an_attribute_we_do_not_name") {
		t.Errorf("attributes JSON: %q — an unnamed attribute must stay queryable", r.Attributes)
	}
}

// A task outcome carries the action's own result, which used to be logged as a
// byte count. error_code is a numeric enum and a succeeded task sends none.
func TestRunnerTaskUpdateStoresOutcomeAndOutputs(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"data":{"type":"publishTaskUpdate","id":"fail_task","attributes":{
		"task_id":"t-1","action_fqn":"com.datadoghq.http.request","job_id":"j-1",
		"client":{"version":"1.2.3"},
		"payload":{"branch":"error","outputs":{"body":"nope"},
			"error_code":7,"error_details":"connection refused","api_error":"",
			"an_unnamed_payload_key":1},
		"an_unnamed_attribute":2
	}}}`)

	if w := post(t, e, "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RunnerTaskUpdateRow](node)
	if len(rows) != 1 {
		t.Fatalf("task update rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Outcome != "fail_task" || r.TaskID != "t-1" || r.JobID != "j-1" {
		t.Errorf("row: %+v", r)
	}
	if r.ErrorCode == nil || *r.ErrorCode != 7 {
		t.Errorf("error_code: got %v, want 7", r.ErrorCode)
	}
	if !strings.Contains(r.Outputs, "nope") {
		t.Errorf("outputs: got %q — the result, not its size", r.Outputs)
	}
	if !strings.Contains(r.Client, "1.2.3") {
		t.Errorf("client: got %q", r.Client)
	}
	if r.Extra["an_unnamed_attribute"] != "2" {
		t.Errorf("top-level extra: %v", r.Extra)
	}
	if r.Extra["payload.an_unnamed_payload_key"] != "1" {
		t.Errorf("nested extra must keep its level: %v", r.Extra)
	}
}

// A succeeded task sends no error code, and 0 is a real enum value — so NULL.
func TestRunnerTaskUpdateWithoutAnErrorCodeStoresNull(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	post(t, e, "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update",
		[]byte(`{"data":{"type":"x","id":"succeed_task","attributes":{"task_id":"t-1","payload":{"branch":"main"}}}}`))

	rows := Rows[storage.RunnerTaskUpdateRow](node)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1", len(rows))
	}
	if rows[0].ErrorCode != nil {
		t.Errorf("error_code: got %v, want NULL", *rows[0].ErrorCode)
	}
}

// The dequeue is the only liveness signal a runner with no work produces, and
// its answer must be a bare 200 with an empty body — a body would be read as a
// task and would need a signed envelope.
func TestRunnerDequeueStoresThePollAndAnswersBare200(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	req := httptest.NewRequest(http.MethodPost,
		"/api/v2/on-prem-management-service/workflow-tasks/dequeue",
		bytes.NewReader([]byte(`{"data":{"type":"dequeue","id":"1","attributes":{
			"runner_started_at":"2026-09-23 08:00:00","last_task_received_at":"never"}}}`)))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Datadog-OnPrem-Version", "1.4.0")
	req.Header.Set("X-Datadog-OnPrem-Modes", "workflow_automation")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 — the runner reads any other status as an error", w.Code)
	}
	if w.Body.Len() != 0 {
		t.Errorf("body: got %q, want empty — a body is parsed as a task", w.Body.String())
	}

	rows := Rows[storage.RunnerDequeueRow](node)
	if len(rows) != 1 {
		t.Fatalf("dequeue rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	// Kept as STRINGS: the format is undocumented and an unparseable value
	// must not become 1970.
	if r.RunnerStartedAt != "2026-09-23 08:00:00" || r.LastTaskReceivedAt != "never" {
		t.Errorf("timestamps: %q / %q", r.RunnerStartedAt, r.LastTaskReceivedAt)
	}
	if r.Version != "1.4.0" || r.Modes != "workflow_automation" {
		t.Errorf("headers: %q / %q", r.Version, r.Modes)
	}
}

// credentials are SECRETS and are stored on purpose: a connection without them
// cannot be used. What must never happen is the reverse of the logging rule —
// the table keeps them, the log keeps only their key names.
func TestActionConnectionsStoreCredentials(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"data":{"type":"connection","id":"c-1","attributes":{
		"name":"prod jira","runner_id":"r-1","tags":["env:prod","env:eu"],
		"integration":{"type":"jira","credentials":{"token":"s3cr3t"},"an_unnamed_key":1},
		"an_unnamed_attribute":2
	}}}`)

	if w := post(t, e, "/api/v2/actions/connections", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.ActionConnectionRow](node)
	if len(rows) != 1 {
		t.Fatalf("connection rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Name != "prod jira" || r.RunnerID != "r-1" || r.IntegrationType != "jira" {
		t.Errorf("row: %+v", r)
	}
	if !strings.Contains(r.Credentials, "s3cr3t") {
		t.Errorf("credentials: got %q — a connection without them cannot be used", r.Credentials)
	}
	if got := r.Tags["env"]; !reflect.DeepEqual(got, []string{"prod", "eu"}) {
		t.Errorf("tags: got %v", got)
	}
	if r.Extra["integration.an_unnamed_key"] != "1" || r.Extra["an_unnamed_attribute"] != "2" {
		t.Errorf("extra: %v", r.Extra)
	}
}

// A heartbeat must be answered with exactly 200: a 404 tells the runner the
// job is gone and it stops heartbeating.
func TestRunnerHeartbeatStoresAndAnswers200(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	body := []byte(`{"data":{"type":"heartbeat","id":"1","attributes":{
		"task_id":"t-1","action_fqn":"com.datadoghq.http.request","job_id":"j-1",
		"client":{"version":"1.2.3"}}}}`)

	w := post(t, e, "/api/v2/on-prem-management-service/workflow-tasks/heartbeat", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want exactly 200", w.Code)
	}

	rows := Rows[storage.RunnerHeartbeatRow](node)
	if len(rows) != 1 {
		t.Fatalf("heartbeat rows: got %d, want 1", len(rows))
	}
	if rows[0].TaskID != "t-1" || rows[0].JobID != "j-1" {
		t.Errorf("row: %+v", rows[0])
	}
}

// A PAR body that is not a JSON:API document at all keeps its bytes, and the
// status contract does not move.
func TestRunnerEnrollWithABadBodyAnswers400AndKeepsTheBytes(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAPI)

	if w := post(t, e, "/api/unstable/on_prem_runners", []byte("not json")); w.Code != http.StatusBadRequest {
		t.Fatalf("got %d, want 400", w.Code)
	}
	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 || raws[0].Intake != "par" {
		t.Fatalf("raw rows: %+v", raws)
	}
}

// ---------------------------------------------------------------------------
// storage agreement
// ---------------------------------------------------------------------------

// AppendTo against each writer's real INSERT column list, plus the nil-map
// guard on a zero-value row. This is the one thing the compiler cannot see.
func TestAPIRowArity(t *testing.T) {
	storagetest.AssertArity(t, storage.MetricsWriter, storage.MetricPoint{})
	storagetest.AssertArity(t, storage.SketchesWriter, storage.SketchRow{})
	storagetest.AssertArity(t, storage.ChecksWriter, storage.CheckRunRow{})
	storagetest.AssertArity(t, storage.EventsWriter, storage.EventRow{})
	storagetest.AssertArity(t, storage.HostsWriter, storage.HostRow{})
	storagetest.AssertArity(t, storage.AgentBatchMetadataWriter, storage.AgentBatchMetadataRow{})
	storagetest.AssertArity(t, storage.AgentChecksWriter, storage.AgentCheckRow{})
	storagetest.AssertArity(t, storage.ExternalHostTagsWriter, storage.ExternalHostTagsRow{})
	storagetest.AssertArity(t, storage.AgentMetadataWriter, storage.AgentMetadataRow{})
	storagetest.AssertArity(t, storage.DelegatedAuthWriter, storage.DelegatedAuthRow{})
	storagetest.AssertArity(t, storage.SymbolQueriesWriter, storage.SymbolQueryRow{})
	storagetest.AssertArity(t, storage.RunnerEnrollmentsWriter, storage.RunnerEnrollmentRow{})
	storagetest.AssertArity(t, storage.RunnerTaskUpdatesWriter, storage.RunnerTaskUpdateRow{})
	storagetest.AssertArity(t, storage.RunnerHeartbeatsWriter, storage.RunnerHeartbeatRow{})
	storagetest.AssertArity(t, storage.RunnerDequeuesWriter, storage.RunnerDequeueRow{})
	storagetest.AssertArity(t, storage.ActionConnectionsWriter, storage.ActionConnectionRow{})
}

// The whole path for every table this router feeds: a scratch database, all
// migrations applied from empty, real rows through the registered INSERTs, and
// the values read back. Only ClickHouse can prove AppendTo and the column
// TYPES agree — a Nullable that is not one, a Map whose value shape differs, a
// JSON column handed an empty string. Each of those passes every in-process
// check and fails exactly here.
func TestAPIRowsRoundTripThroughClickHouse(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	status := int64(0)
	errCode := int64(7)
	fips := uint8(1)

	storagetest.Insert(t, conn, storage.MetricsWriter, []storage.Row{
		storage.MetricPoint{TenantID: testTenant, Timestamp: now, Metric: "m", Host: "h1",
			Value: 1, OriginProduct: 10, OriginCategory: 11, OriginService: 42,
			Resources: map[string]string{"device": "eth0"},
			Extra:     map[string]string{"k": `"v"`}},
	})
	storagetest.Insert(t, conn, storage.SketchesWriter, []storage.Row{
		storage.SketchRow{TenantID: testTenant, Timestamp: now, Metric: "m", Host: "h1",
			BucketKeys: []int32{1}, BucketCounts: []uint32{2}, OriginProduct: 10},
	})
	storagetest.Insert(t, conn, storage.ChecksWriter, []storage.Row{
		storage.CheckRunRow{TenantID: testTenant, Timestamp: now, CheckName: "c", Host: "h1",
			Status: "OK", Extra: map[string]string{"k": "1"}},
	})
	storagetest.Insert(t, conn, storage.EventsWriter, []storage.Row{
		storage.EventRow{TenantID: testTenant, Timestamp: now, Title: "deploy",
			AlertType: "info", Priority: "normal", EventType: "docker",
			RelatedEventID: &errCode, AlertTypeRaw: "catastrophe", PriorityRaw: "urgent",
			Extra: map[string]string{"team": `"core"`}},
		// And one with the Nullable left NULL.
		storage.EventRow{TenantID: testTenant, Timestamp: now, Title: "plain",
			AlertType: "info", Priority: "normal"},
	})
	storagetest.Insert(t, conn, storage.HostsWriter, []storage.Row{
		storage.HostRow{TenantID: testTenant, Host: "h1", SeenAt: now,
			AgentVersion: "7.58.2", OS: "linux", UUID: "4f1a", AgentFlavor: "agent",
			PythonVersion: "3.12.6", Meta: `{"socket-hostname":"h1"}`,
			SystemStats: map[string]string{"cpuCores": "2"},
			Network:     `{"ipaddress":"10.0.0.4"}`, Filesystem: `[{"name":"/dev/sda1"}]`,
			InstallMethod:    map[string]string{"tool": `"helm"`},
			FIPSProxyEnabled: &fips,
			HostTags:         map[string][]string{"google_tags": {"zone:eu-west1-b"}},
			GohaiExtra:       map[string]string{"gpu": `{"vendor":"nvidia"}`},
			IntakeExtra:      map[string]string{"a_key_from_7_60": `"surprise"`},
			Resources:        `{"processes":{}}`},
	})
	storagetest.Insert(t, conn, storage.AgentBatchMetadataWriter, []storage.Row{
		storage.AgentBatchMetadataRow{TenantID: testTenant, ReceivedAt: now, Intake: "sketches",
			AgentVersion: "7.58.2", Timezone: "UTC", CurrentEpoch: 1790151330,
			InternalIP: "10.0.0.4", PublicIP: "203.0.113.7"},
	})
	storagetest.Insert(t, conn, storage.AgentChecksWriter, []storage.Row{
		storage.AgentCheckRow{TenantID: testTenant, ReceivedAt: now, Hostname: "h1",
			AgentVersion: "7.58.2", CheckName: "docker", SourceType: "docker",
			InstanceID: "docker:1", Status: &status, Message: "",
			PositionalExtra: `["a sixth element"]`, Meta: `{"socket-hostname":"h1"}`},
		storage.AgentCheckRow{TenantID: testTenant, ReceivedAt: now, Hostname: "h1",
			CheckName: "truncated"},
	})
	storagetest.Insert(t, conn, storage.ExternalHostTagsWriter, []storage.Row{
		storage.ExternalHostTagsRow{TenantID: testTenant, ReceivedAt: now, Host: "vm-1",
			Source: "vsphere", Tags: map[string][]string{"cluster": {"prod", "eu"}}},
	})
	ts := now
	storagetest.Insert(t, conn, storage.AgentMetadataWriter, []storage.Row{
		storage.AgentMetadataRow{TenantID: testTenant, ReceivedAt: now, Variant: "check_metadata",
			Hostname: "h1", Timestamp: &ts, UUID: "4f1a",
			Payload:       `{"docker":[{"version.raw":"1.2.3"}]}`,
			EnvelopeExtra: map[string]string{"a_sibling_key": `{"x":1}`}},
		storage.AgentMetadataRow{TenantID: testTenant, ReceivedAt: now, Variant: "ha_agent_metadata",
			Hostname: "h1", Payload: `{"enabled":true}`},
	})
	storagetest.Insert(t, conn, storage.DelegatedAuthWriter, []storage.Row{
		storage.DelegatedAuthRow{TenantID: testTenant, At: now, Scheme: "Delegated",
			ProofFingerprint: strings.Repeat("a", 64), APIKeyID: "test"},
	})
	storagetest.Insert(t, conn, storage.SymbolQueriesWriter, []storage.Row{
		storage.SymbolQueryRow{TenantID: testTenant, At: now, Arch: "x86_64",
			BuildIDs: []string{"aaaa", "bbbb"}, Resource: `{"data":{}}`},
	})
	storagetest.Insert(t, conn, storage.RunnerEnrollmentsWriter, []storage.Row{
		storage.RunnerEnrollmentRow{TenantID: testTenant, RunnerID: "r-1", EnrolledAt: now,
			Name: "runner-1", Modes: []string{"workflow_automation"}, Host: "runner-host",
			PublicKeyPEM: "-----BEGIN PUBLIC KEY-----", AgentHostname: "agent-host",
			OrchClusterID: "cluster-9", AgentFlavor: "agent", OrgID: 12345,
			Attributes: `{"runner_name":"runner-1"}`},
	})
	storagetest.Insert(t, conn, storage.RunnerTaskUpdatesWriter, []storage.Row{
		storage.RunnerTaskUpdateRow{TenantID: testTenant, At: now, Outcome: "fail_task",
			TaskID: "t-1", ActionFQN: "com.datadoghq.http.request", JobID: "j-1",
			Client: `{"version":"1.2.3"}`, Branch: "error", Outputs: `{"body":"nope"}`,
			ErrorCode: &errCode, ErrorDetails: "connection refused",
			Extra: map[string]string{"payload.k": "1"}},
		storage.RunnerTaskUpdateRow{TenantID: testTenant, At: now, Outcome: "succeed_task"},
	})
	storagetest.Insert(t, conn, storage.RunnerHeartbeatsWriter, []storage.Row{
		storage.RunnerHeartbeatRow{TenantID: testTenant, At: now, TaskID: "t-1", JobID: "j-1"},
	})
	storagetest.Insert(t, conn, storage.RunnerDequeuesWriter, []storage.Row{
		storage.RunnerDequeueRow{TenantID: testTenant, At: now,
			RunnerStartedAt: "2026-09-23 08:00:00", LastTaskReceivedAt: "never",
			Version: "1.4.0", Modes: "workflow_automation"},
	})
	storagetest.Insert(t, conn, storage.ActionConnectionsWriter, []storage.Row{
		storage.ActionConnectionRow{TenantID: testTenant, At: now, Name: "prod jira",
			RunnerID: "r-1", Tags: map[string][]string{"env": {"prod", "eu"}},
			IntegrationType: "jira", Credentials: `{"token":"s3cr3t"}`,
			Extra: map[string]string{"integration.k": "1"}},
	})

	for table, want := range map[string]uint64{
		"metrics": 1, "sketches": 1, "check_runs": 1, "events": 2, "hosts": 1,
		"agent_batch_metadata": 1, "agent_checks": 2, "external_host_tags": 1,
		"agent_metadata": 2, "delegated_auth_requests": 1, "symbol_queries": 1,
		"runner_enrollments": 1, "runner_task_updates": 2, "runner_heartbeats": 1,
		"runner_dequeues": 1, "action_connections": 1,
	} {
		if got := storagetest.Count(t, conn, table); got != want {
			t.Errorf("%s: %d rows, want %d", table, got, want)
		}
	}

	// A few values read back by column, to prove the types agree and not only
	// the counts.
	row := storagetest.QueryRow(t, conn,
		"SELECT origin_product, resources['device'], extra['k'] FROM metrics")
	if row[0] != uint32(10) || row[1] != "eth0" || row[2] != `"v"` {
		t.Errorf("metrics: got %v", row)
	}

	row = storagetest.QueryRow(t, conn,
		"SELECT agent_flavor, host_tags['google_tags'], fips_mode IS NULL, fips_proxy_enabled FROM hosts")
	if row[0] != "agent" {
		t.Errorf("hosts.agent_flavor: got %v", row[0])
	}
	if got, ok := row[1].([]string); !ok || !reflect.DeepEqual(got, []string{"zone:eu-west1-b"}) {
		t.Errorf("hosts.host_tags: got %#v", row[1])
	}
	// An absent FIPS key stays NULL rather than becoming "off".
	if row[2] != uint8(1) {
		t.Errorf("hosts.fips_mode should be NULL, got %v", row[2])
	}
	if p, ok := row[3].(*uint8); !ok || p == nil || *p != 1 {
		t.Errorf("hosts.fips_proxy_enabled: got %#v", row[3])
	}

	row = storagetest.QueryRow(t, conn,
		"SELECT status FROM agent_checks WHERE check_name = 'truncated'")
	if p, ok := row[0].(*int64); !ok || p != nil {
		t.Errorf("a truncated positional array must leave status NULL, got %#v — otherwise every short entry reads as OK", row[0])
	}

	row = storagetest.QueryRow(t, conn,
		"SELECT toString(attributes.runner_name), org_id FROM runner_enrollments FINAL")
	if row[0] != "runner-1" {
		t.Errorf("runner_enrollments.attributes: got %v", row[0])
	}
	if row[1] != int64(12345) {
		t.Errorf("runner_enrollments.org_id: got %v", row[1])
	}
}
