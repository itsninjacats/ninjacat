package intake

import (
	"encoding/json"
	"strings"
	"testing"
)

// Seven of twelve checks in a captured batch from a real Kubernetes cluster
// carried no `tags` key at all — containerd.health and the kubelet checks
// among them. datadogV1.ServiceCheck marks the field required because
// Datadog's OpenAPI spec does, so decoding the batch as a unit failed and
// took every well-formed check down with it.
func TestCheckRunsSurviveMissingTags(t *testing.T) {
	body := []byte(`[
	  {"check":"with.tags","host_name":"h1","timestamp":1790002960,"status":0,"message":"","tags":["a:1"]},
	  {"check":"containerd.health","host_name":"h1","timestamp":1790002960,"status":0,"message":"","tags":null},
	  {"check":"kubernetes.kubelet.check.ping","host_name":"h1","timestamp":1790002960,"status":1,"message":"slow"}
	]`)

	runs, originals, undecodable, err := parseCheckRuns(body)
	if err != nil {
		t.Fatalf("batch rejected: %v", err)
	}
	if len(originals) != len(runs) {
		t.Errorf("originals: got %d for %d checks — they are indexed together", len(originals), len(runs))
	}
	if len(undecodable) != 0 {
		t.Errorf("%d checks reported undecodable, want 0", len(undecodable))
	}
	if len(runs) != 3 {
		t.Fatalf("got %d checks, want 3 — a check without tags must not discard its neighbours", len(runs))
	}
	if runs[0].Check != "with.tags" || len(runs[0].Tags) != 1 {
		t.Errorf("tagged check mangled: %+v", runs[0])
	}
	// The agent sends "tags": null rather than omitting the key, so presence
	// alone is not the test — the value has to be inspected.
	if runs[1].Check != "containerd.health" || len(runs[1].Tags) != 0 {
		t.Errorf("check with tags:null should decode with an empty tag set, got %+v", runs[1])
	}
	if runs[2].Status != 1 {
		t.Errorf("status lost: got %v, want 1", runs[2].Status)
	}
}

// A body that is plainly an array must report why the ARRAY decode failed.
// Reporting the single-object fallback's error instead yields "cannot
// unmarshal array into Go value of type map[string]interface {}", which
// describes the payload correctly and explains nothing.
func TestDecodeJSONListReportsTheArrayError(t *testing.T) {
	type item struct {
		N int `json:"n"`
	}
	_, err := decodeJSONList[item]([]byte(`[{"n":"not-a-number"}]`))
	if err == nil {
		t.Fatal("expected an error")
	}
	if strings.Contains(err.Error(), "map[string]interface") {
		t.Errorf("reported the fallback error instead of the array error: %v", err)
	}
}

// An entry that is not an object at all — a client that sent a list of check
// NAMES, say — used to be counted and dropped inside parseCheckRuns, which put
// the only copy of it out of reach of the handler that could have kept it. It
// now comes back so the bytes can reach raw_payloads.
//
// Note the contrast with a merely odd object: datadogV1.ServiceCheck's
// generated UnmarshalJSON swallows that into UnparsedObject and returns no
// error, so it arrives as a decoded run and the handler stores it raw itself.
func TestCheckRunsReturnTheItemsItCouldNotDecode(t *testing.T) {
	body := []byte(`[
	  {"check":"ok.check","host_name":"h1","status":0,"tags":["a:1"]},
	  "containerd.health"
	]`)

	runs, originals, undecodable, err := parseCheckRuns(body)
	if err != nil {
		t.Fatalf("batch rejected: %v", err)
	}
	// The skipped item is exactly what would misalign a re-split of the body
	// done later: originals[0] must be the check that survived, not the
	// string that did not.
	if len(originals) != 1 || !strings.Contains(string(originals[0]), "ok.check") {
		t.Errorf("originals drifted from the decoded checks: %v", originals)
	}
	if len(runs) != 1 || runs[0].Check != "ok.check" {
		t.Fatalf("well-formed check lost: %+v", runs)
	}
	if len(undecodable) != 1 {
		t.Fatalf("undecodable items: got %d, want 1", len(undecodable))
	}
	if !strings.Contains(string(undecodable[0]), "containerd.health") {
		t.Errorf("the item came back changed: %s", undecodable[0])
	}
}

// The browser SDK posts NDJSON, small clients post a bare object and the agent
// posts an array — all three to the same route, with no header telling them
// apart. Reading only the array shape meant a browser's logs decoded as one
// item at best and as nothing at worst.
func TestParseLogItemsAcceptsEveryFraming(t *testing.T) {
	cases := []struct {
		name string
		body string
		want []string
	}{
		{
			name: "array, what the agent sends",
			body: `[{"message":"a"},{"message":"b"}]`,
			want: []string{"a", "b"},
		},
		{
			name: "bare object, what small clients send",
			body: `{"message":"a"}`,
			want: []string{"a"},
		},
		{
			name: "NDJSON, what the browser SDK sends",
			body: "{\"message\":\"a\"}\n{\"message\":\"b\"}\n",
			want: []string{"a", "b"},
		},
		{
			name: "NDJSON with no trailing newline",
			body: "{\"message\":\"a\"}\n{\"message\":\"b\"}",
			want: []string{"a", "b"},
		},
		{
			name: "leading whitespace before an array",
			body: "  \n\t[{\"message\":\"a\"}]",
			want: []string{"a"},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			items, err := parseLogItems([]byte(tc.body))
			if err != nil {
				t.Fatalf("rejected: %v", err)
			}
			if len(items) != len(tc.want) {
				t.Fatalf("items: got %d, want %d (%v)", len(items), len(tc.want), items)
			}
			for i, raw := range items {
				var item struct {
					Message string `json:"message"`
				}
				if err := json.Unmarshal(raw, &item); err != nil {
					t.Fatalf("item %d is not an object: %v", i, err)
				}
				if item.Message != tc.want[i] {
					t.Errorf("item %d message: got %q, want %q — order must survive", i, item.Message, tc.want[i])
				}
			}
		})
	}
}

// A broken NDJSON line has to name WHICH line broke: the whole point of the
// framing is that one bad line is not the whole batch.
func TestParseLogItemsNamesTheBrokenLine(t *testing.T) {
	_, err := parseLogItems([]byte("{\"message\":\"a\"}\n{\"message\":\n"))
	if err == nil {
		t.Fatal("expected an error")
	}
	if !strings.Contains(err.Error(), "item 1") {
		t.Errorf("error does not say which item failed: %v", err)
	}
}

// The batch framings a handler has to re-split to keep an element's original
// bytes. An array yields its elements in order (whitespace and all, so the
// digits of a number nobody decoded are still there), a bare object yields
// itself — the shape datadogpy posts — and anything else yields nothing, which
// is the caller's signal to fall back to the decoded copy.
func TestJSONElementsMirrorsTheFramingsWeAccept(t *testing.T) {
	cases := []struct {
		name string
		body string
		want []string
	}{
		{"array", `[{"a":1}, {"b":12345678901234567891}]`, []string{`{"a":1}`, `{"b":12345678901234567891}`}},
		{"bare object", ` {"a":1} `, []string{`{"a":1}`}},
		{"empty array", `[]`, nil},
		{"scalar", `"containerd.health"`, nil},
		{"truncated", `[{"a":1}`, nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := jsonElements([]byte(tc.body))
			if len(got) != len(tc.want) {
				t.Fatalf("got %d elements, want %d: %v", len(got), len(tc.want), got)
			}
			for i := range tc.want {
				if string(got[i]) != tc.want[i] {
					t.Errorf("element %d: got %s, want %s", i, got[i], tc.want[i])
				}
			}
		})
	}
}

// The same, one level down: the series of a metrics or distribution_points
// payload. A `series` that is not a list produced no decoded series either, so
// there is nothing to align with and nothing to return.
func TestJSONFieldElementsOnlySplitsAList(t *testing.T) {
	cases := []struct {
		name string
		body string
		want int
	}{
		{"list", `{"series":[{"metric":"a"},{"metric":"b"}]}`, 2},
		{"dict", `{"series":{"a":[]}}`, 0},
		{"absent", `{"other":[]}`, 0},
		{"not an object", `[1,2]`, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := jsonFieldElements([]byte(tc.body), "series"); len(got) != tc.want {
				t.Fatalf("got %d elements, want %d: %v", len(got), tc.want, got)
			}
		})
	}
}
