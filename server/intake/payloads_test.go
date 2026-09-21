package intake

import (
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

	runs, err := parseCheckRuns(body)
	if err != nil {
		t.Fatalf("batch rejected: %v", err)
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
