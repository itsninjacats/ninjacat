package rumevents

// The files under testdata/samples are copied verbatim from
// DataDog/rum-events-format (samples/rum-events, samples/telemetry-events),
// Apache-2.0, at the commit named in the *_gen.go headers. Datadog validates
// them against the schemas in CI, so they are the reference wire shapes.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// sampleTypes maps every sample to the variant Decode must select.
var sampleTypes = map[string]string{
	"rum-events/action.json":                                    "*rumevents.RumActionEvent",
	"rum-events/app_start.json":                                 "*rumevents.RumActionEvent",
	"rum-events/error.json":                                     "*rumevents.RumErrorEvent",
	"rum-events/extended_error.json":                            "*rumevents.RumErrorEvent",
	"rum-events/wasm-error.json":                                "*rumevents.RumErrorEvent",
	"rum-events/long_task.json":                                 "*rumevents.RumLongTaskEvent",
	"rum-events/long_animation_frame.json":                      "*rumevents.RumLongTaskEvent",
	"rum-events/resource.json":                                  "*rumevents.RumResourceEvent",
	"rum-events/resource-graphql.json":                          "*rumevents.RumResourceEvent",
	"rum-events/view.json":                                      "*rumevents.RumViewEvent",
	"rum-events/view-with-tab.json":                             "*rumevents.RumViewEvent",
	"rum-events/stream.json":                                    "*rumevents.RumViewEvent",
	"rum-events/view_update.json":                               "*rumevents.RumViewUpdateEvent",
	"rum-events/transition.json":                                "*rumevents.RumTransitionEvent",
	"rum-events/vital.json":                                     "*rumevents.RumVitalDurationEvent",
	"telemetry-events/error.json":                               "*rumevents.TelemetryErrorEvent",
	"telemetry-events/debug.json":                               "*rumevents.TelemetryDebugEvent",
	"telemetry-events/configuration.json":                       "*rumevents.TelemetryConfigurationEvent",
	"telemetry-events/usage.json":                               "*rumevents.TelemetryUsageEvent",
	"telemetry-events/usage-add-view-loading-time-browser.json": "*rumevents.TelemetryUsageEvent",
	"telemetry-events/usage-add-view-loading-time-mobile.json":  "*rumevents.TelemetryUsageEvent",
}

func readSample(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "samples", filepath.FromSlash(name)))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// jsonValue decodes JSON into a generic value with numbers kept as
// json.Number, so that two documents can be compared without float rounding.
func jsonValue(t *testing.T, data []byte) any {
	t.Helper()
	var v any
	if err := jsonx.UnmarshalNumber(data, &v); err != nil {
		t.Fatalf("invalid JSON: %v\n%s", err, data)
	}
	return v
}

// assertRoundTrip decodes, re-encodes and checks that not a single key or
// value changed, at any depth.
func assertRoundTrip(t *testing.T, input []byte) Event {
	t.Helper()
	ev, err := Decode(input)
	if err != nil {
		t.Fatalf("Decode: %v", err)
	}
	output, err := json.Marshal(ev)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	want, got := jsonValue(t, input), jsonValue(t, output)
	if !reflect.DeepEqual(want, got) {
		t.Fatalf("round trip changed the document\n input: %s\noutput: %s", input, output)
	}
	return ev
}

// setPath sets a value inside a JSON document by object path (array indexes
// are written as numbers) and returns the re-encoded document.
func setPath(t *testing.T, doc []byte, value any, path ...string) []byte {
	t.Helper()
	root := jsonValue(t, doc)
	cur := root
	for i, seg := range path[:len(path)-1] {
		switch c := cur.(type) {
		case map[string]any:
			next, ok := c[seg]
			if !ok {
				next = map[string]any{}
				c[seg] = next
			}
			cur = next
		case []any:
			var idx int
			fmt.Sscan(seg, &idx)
			cur = c[idx]
		default:
			t.Fatalf("setPath: %s is not a container", strings.Join(path[:i+1], "."))
		}
	}
	cur.(map[string]any)[path[len(path)-1]] = value
	out, err := json.Marshal(root)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func TestDecodeSamples(t *testing.T) {
	seen := map[string]bool{}
	for _, dir := range []string{"rum-events", "telemetry-events"} {
		entries, err := os.ReadDir(filepath.Join("testdata", "samples", dir))
		if err != nil {
			t.Fatal(err)
		}
		for _, e := range entries {
			name := dir + "/" + e.Name()
			want, ok := sampleTypes[name]
			if !ok {
				t.Errorf("%s: sample without an expected type in sampleTypes", name)
				continue
			}
			seen[name] = true
			t.Run(name, func(t *testing.T) {
				ev := assertRoundTrip(t, readSample(t, name))
				if got := fmt.Sprintf("%T", ev); got != want {
					t.Fatalf("decoded as %s, want %s", got, want)
				}
			})
		}
	}
	for name := range sampleTypes {
		if !seen[name] {
			t.Errorf("%s listed in sampleTypes but missing from testdata", name)
		}
	}
}

func TestEveryVariantHasASample(t *testing.T) {
	covered := map[string]bool{}
	for _, typ := range sampleTypes {
		covered[strings.TrimPrefix(typ, "*rumevents.")] = true
	}
	// Mobile-only variants that have no sample upstream are exercised by
	// TestDiscriminatorSelectsVariant instead.
	synthetic := map[string]bool{
		"RumVitalOperationStepEvent": true, "RumVitalAppLaunchEvent": true,
		"RumTimeseriesMemoryEvent": true, "RumTimeseriesCpuEvent": true,
	}
	for _, v := range eventVariants {
		if !covered[v.Name] && !synthetic[v.Name] {
			t.Errorf("variant %s has neither a sample nor a synthetic test", v.Name)
		}
	}
}

func TestUnknownFieldsSurviveAtEveryDepth(t *testing.T) {
	doc := readSample(t, "rum-events/view.json")
	doc = setPath(t, doc, map[string]any{"a": json.Number("1")}, "zz_top")
	doc = setPath(t, doc, "nested", "view", "zz_view")
	doc = setPath(t, doc, true, "session", "zz_session")
	doc = setPath(t, doc, json.Number("42"), "_dd", "zz_dd")
	doc = setPath(t, doc, "item", "view", "in_foreground_periods", "0", "zz_item")
	doc = setPath(t, doc, "usr-extra", "usr", "zz_usr")
	doc = setPath(t, doc, json.Number("7"), "view", "performance", "cls", "zz_cls")

	ev := assertRoundTrip(t, doc).(*RumViewEvent)

	check := func(what string, extra map[string]any, key string, want any) {
		t.Helper()
		got, ok := extra[key]
		if !ok {
			t.Errorf("%s: unknown key %q was dropped", what, key)
			return
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s: %q = %#v, want %#v", what, key, got, want)
		}
	}
	check("event", ev.AdditionalProperties, "zz_top", map[string]any{"a": json.Number("1")})
	check("view", ev.View.AdditionalProperties, "zz_view", "nested")
	check("session", ev.Session.AdditionalProperties, "zz_session", true)
	check("_dd", ev.DD.AdditionalProperties, "zz_dd", json.Number("42"))
	check("in_foreground_periods[0]", ev.View.InForegroundPeriods[0].AdditionalProperties, "zz_item", "item")
	check("usr", ev.Usr.AdditionalProperties, "zz_usr", "usr-extra")
	check("view.performance.cls", ev.View.Performance.CLS.AdditionalProperties, "zz_cls", json.Number("7"))

	// Declared fields next to the unknown ones are still decoded.
	if ev.View.ID != "623d50fd-75cf-4025-97d2-e51ff94171f6" || ev.Session.ID != "cacbf45c-3a05-48ce-b066-d76349460599" {
		t.Errorf("declared fields lost: view.id=%q session.id=%q", ev.View.ID, ev.Session.ID)
	}
}

func TestLargeIntegersKeepPrecision(t *testing.T) {
	const big = "9007199254740993" // 2^53 + 1: not representable in float64
	const max = "9223372036854775807"

	doc := readSample(t, "rum-events/view.json")
	doc = setPath(t, doc, json.Number(big), "date")
	doc = setPath(t, doc, json.Number(max), "view", "custom_timings", "huge")
	doc = setPath(t, doc, json.Number(big+".5"), "view", "performance", "cls", "score")
	doc = setPath(t, doc, json.Number(max), "context", "big")
	doc = setPath(t, doc, json.Number(big), "zz_unknown")

	ev := assertRoundTrip(t, doc).(*RumViewEvent)
	if ev.Date != 9007199254740993 {
		t.Errorf("date = %d, want %s", ev.Date, big)
	}
	if got := ev.View.CustomTimings["huge"]; got != 9223372036854775807 {
		t.Errorf("custom_timings.huge = %d, want %s", got, max)
	}
	if got := ev.View.Performance.CLS.Score.String(); got != big+".5" {
		t.Errorf("cls.score = %s, want %s.5", got, big)
	}
	if got := ev.Context["big"]; got != json.Number(max) {
		t.Errorf("context.big = %#v, want json.Number(%s)", got, max)
	}
	if got := ev.AdditionalProperties["zz_unknown"]; got != json.Number(big) {
		t.Errorf("zz_unknown = %#v, want json.Number(%s)", got, big)
	}

	vital := setPath(t, readSample(t, "rum-events/vital.json"), json.Number(big), "vital", "duration")
	vev := assertRoundTrip(t, vital).(*RumVitalDurationEvent)
	if got := vev.Vital.Duration.String(); got != big {
		t.Errorf("vital.duration = %s, want %s", got, big)
	}
	out, _ := json.Marshal(vev)
	if !bytes.Contains(out, []byte(`"duration":`+big)) {
		t.Errorf("re-encoded duration lost precision: %s", out)
	}
}

func TestEmptyArrayIsNotDropped(t *testing.T) {
	doc := readSample(t, "telemetry-events/usage-add-view-loading-time-browser.json")
	ev := assertRoundTrip(t, doc).(*TelemetryUsageEvent)
	if ev.ExperimentalFeatures == nil || len(ev.ExperimentalFeatures) != 0 {
		t.Fatalf("experimental_features = %#v, want empty non-nil slice", ev.ExperimentalFeatures)
	}
	out, _ := json.Marshal(ev)
	if !bytes.Contains(out, []byte(`"experimental_features":[]`)) {
		t.Errorf("empty array dropped on encode: %s", out)
	}
	if ev.Telemetry.Usage.Feature != "addViewLoadingTime" {
		t.Errorf("flattened usage.feature = %q", ev.Telemetry.Usage.Feature)
	}
}

func TestDiscriminatorSelectsVariant(t *testing.T) {
	vital := readSample(t, "rum-events/vital.json")
	telemetryErr := readSample(t, "telemetry-events/error.json")
	action := readSample(t, "rum-events/action.json")
	view := readSample(t, "rum-events/view.json")

	timeseries := func(name string) []byte {
		return []byte(`{"type":"timeseries","date":1,"application":{"id":"a"},"session":{"id":"s","type":"user"},
			"_dd":{"format_version":2},"timeseries":{"id":"t","name":"` + name + `","schema":"object-v2","start":1,"end":2,"data":{"timestamps":[1],"values":{}}}}`)
	}

	cases := []struct {
		name string
		doc  []byte
		want string
	}{
		{"vital duration", vital, "*rumevents.RumVitalDurationEvent"},
		{"vital operation_step", setPath(t, setPath(t, vital, "start", "vital", "step_type"), "operation_step", "vital", "type"), "*rumevents.RumVitalOperationStepEvent"},
		{"vital app_launch", setPath(t, setPath(t, vital, "ttid", "vital", "app_launch_metric"), "app_launch", "vital", "type"), "*rumevents.RumVitalAppLaunchEvent"},
		{"telemetry error without telemetry.type", telemetryErr, "*rumevents.TelemetryErrorEvent"},
		{"telemetry error with telemetry.type=log", setPath(t, telemetryErr, "log", "telemetry", "type"), "*rumevents.TelemetryErrorEvent"},
		{"telemetry status=debug", setPath(t, telemetryErr, "debug", "telemetry", "status"), "*rumevents.TelemetryDebugEvent"},
		{"telemetry.type wins over status", setPath(t, telemetryErr, "usage", "telemetry", "type"), "*rumevents.TelemetryUsageEvent"},
		{"timeseries cpu", timeseries("cpu"), "*rumevents.RumTimeseriesCpuEvent"},
		{"timeseries memory", timeseries("memory"), "*rumevents.RumTimeseriesMemoryEvent"},
		// Shape must not matter: an action-shaped document labelled "error"
		// is an error event, and a view-shaped one labelled "resource" is a
		// resource event. Nothing is lost either way.
		{"action shape, type=error", setPath(t, action, "error", "type"), "*rumevents.RumErrorEvent"},
		{"view shape, type=resource", setPath(t, view, "resource", "type"), "*rumevents.RumResourceEvent"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			ev := assertRoundTrip(t, c.doc)
			if got := fmt.Sprintf("%T", ev); got != c.want {
				t.Fatalf("decoded as %s, want %s", got, c.want)
			}
		})
	}
}

func TestDecodeRejectsUnknownAndInvalid(t *testing.T) {
	var unknown *UnknownEventError

	_, err := Decode([]byte(`{"type":"hologram","date":1}`))
	if !errors.As(err, &unknown) || unknown.Type != "hologram" {
		t.Errorf("unknown type: got %v", err)
	}

	_, err = Decode(setPath(t, readSample(t, "rum-events/vital.json"), "zzz", "vital", "type"))
	if !errors.As(err, &unknown) || unknown.Type != "vital" {
		t.Errorf("unknown vital.type: got %v", err)
	}

	_, err = Decode(setPath(t, readSample(t, "telemetry-events/error.json"), "fatal", "telemetry", "status"))
	if !errors.As(err, &unknown) || unknown.Type != "telemetry" {
		t.Errorf("unknown telemetry.status: got %v", err)
	}

	_, err = Decode([]byte(`{"date":1}`))
	if !errors.As(err, &unknown) || unknown.Type != "" {
		t.Errorf("missing type: got %v", err)
	}

	_, err = Decode([]byte(`{"type":7}`))
	if !errors.As(err, &unknown) {
		t.Errorf("non-string type: got %v", err)
	}

	for _, bad := range []string{`[]`, `null`, `"view"`, `{"type":"view"} trailing`, `{"type":"view","date":"not a number"}`} {
		if _, err := Decode([]byte(bad)); err == nil {
			t.Errorf("%s: expected an error", bad)
		}
	}
}

func TestEventTypeAndInterfaces(t *testing.T) {
	ev := assertRoundTrip(t, readSample(t, "rum-events/vital.json"))
	if ev.EventType() != "vital" {
		t.Errorf("EventType = %q", ev.EventType())
	}
	if _, ok := ev.(RumVitalEvent); !ok {
		t.Error("vital duration event does not implement RumVitalEvent")
	}
	if _, ok := ev.(RumEvent); !ok {
		t.Error("vital duration event does not implement RumEvent")
	}
	if _, ok := ev.(TelemetryEvent); ok {
		t.Error("vital duration event must not implement TelemetryEvent")
	}
}

// TestSchemaRoots pins the browser/mobile split that matters for storage:
// transition is browser-only, timeseries and app_launch are mobile-only.
func TestSchemaRoots(t *testing.T) {
	const (
		generic  = "rum-events-schema.json"
		browser  = "rum-events-browser-schema.json"
		mobile   = "rum-events-mobile-schema.json"
		electron = "rum-events-electron-schema.json"
	)
	want := map[string][]string{
		"RumActionEvent":              {generic, browser, mobile},
		"RumTransitionEvent":          {generic, browser},
		"RumErrorEvent":               {generic, browser, mobile, electron},
		"RumLongTaskEvent":            {generic, browser, mobile},
		"RumResourceEvent":            {generic, browser, mobile, electron},
		"RumViewEvent":                {generic, browser, mobile, electron},
		"RumViewUpdateEvent":          {generic, browser, mobile, electron},
		"RumVitalDurationEvent":       {generic, browser, mobile, electron},
		"RumVitalOperationStepEvent":  {generic, browser, mobile, electron},
		"RumVitalAppLaunchEvent":      {generic, mobile},
		"TelemetryErrorEvent":         {mobile},
		"TelemetryDebugEvent":         {mobile},
		"TelemetryConfigurationEvent": {mobile},
		"TelemetryUsageEvent":         {mobile},
		"RumTimeseriesMemoryEvent":    {mobile},
		"RumTimeseriesCpuEvent":       {mobile},
	}
	for _, v := range eventVariants {
		got := v.New().(Event).SchemaRoots()
		if !reflect.DeepEqual(got, want[v.Name]) {
			t.Errorf("%s.SchemaRoots() = %v, want %v", v.Name, got, want[v.Name])
		}
	}
	if len(want) != len(eventVariants) {
		t.Errorf("expected %d variants, generated %d", len(want), len(eventVariants))
	}
}

// TestGeneratedCodeIsUpToDate regenerates the package into a temporary
// directory and compares it with the checked-in files. It needs a checkout of
// DataDog/rum-events-format in RUM_EVENTS_FORMAT and is skipped otherwise.
func TestGeneratedCodeIsUpToDate(t *testing.T) {
	checkout := os.Getenv("RUM_EVENTS_FORMAT")
	if checkout == "" {
		t.Skip("RUM_EVENTS_FORMAT not set")
	}
	tmp := t.TempDir()
	cmd := exec.Command("go", "run", "./internal/gen", "-preset", "rum", "-schemas", filepath.Join(checkout, "schemas"), "-out", tmp)
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("generator failed: %v\n%s", err, out)
	}
	for _, name := range []string{"event_gen.go", "rum_gen.go", "telemetry_gen.go"} {
		want, err := os.ReadFile(filepath.Join(tmp, name))
		if err != nil {
			t.Fatal(err)
		}
		got, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(want, got) {
			t.Errorf("%s is stale: run `RUM_EVENTS_FORMAT=%s go generate ./rumevents/`", name, checkout)
		}
	}
}
