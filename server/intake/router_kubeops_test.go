package intake

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
)

// The orchestrator converter is one reflection-based function for ~27 message
// types, so the tests pin the contract it must keep for all of them: the kind
// comes from the element's type name, identity comes from Metadata, and the
// envelope (cluster, group, agent version) lands on every row.

func TestOrchResourceRowsIdentity(t *testing.T) {
	now := time.Now().UTC()
	meta := func(ns, name, uid string, owner *process.OwnerReference) *process.Metadata {
		md := &process.Metadata{Namespace: ns, Name: name, Uid: uid, ResourceVersion: "42"}
		if owner != nil {
			md.OwnerReferences = []*process.OwnerReference{owner}
		}
		return md
	}

	cases := []struct {
		name string
		body process.MessageBody
		want struct {
			kind, ns, objName, uid, ownerKind, ownerName string
		}
	}{
		{
			name: "pod",
			body: &process.CollectorPod{
				ClusterName: "prod", ClusterId: "cid-1", GroupId: 3, GroupSize: 7,
				Pods: []*process.Pod{{
					Metadata: meta("default", "web-abc12", "uid-pod", &process.OwnerReference{Kind: "ReplicaSet", Name: "web"}),
				}},
			},
			want: struct{ kind, ns, objName, uid, ownerKind, ownerName string }{
				"Pod", "default", "web-abc12", "uid-pod", "ReplicaSet", "web",
			},
		},
		{
			name: "deployment",
			body: &process.CollectorDeployment{
				ClusterName: "prod", ClusterId: "cid-1", GroupId: 3, GroupSize: 7,
				Deployments: []*process.Deployment{{
					Metadata: meta("shop", "checkout", "uid-dep", nil),
				}},
			},
			want: struct{ kind, ns, objName, uid, ownerKind, ownerName string }{
				"Deployment", "shop", "checkout", "uid-dep", "", "",
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rows := orchResourceRows("tenant-1", now, tc.body)
			if len(rows) != 1 {
				t.Fatalf("got %d rows, want 1", len(rows))
			}
			r := rows[0]
			if r.Kind != tc.want.kind {
				t.Errorf("Kind = %q, want %q", r.Kind, tc.want.kind)
			}
			if r.Namespace != tc.want.ns || r.Name != tc.want.objName || r.UID != tc.want.uid {
				t.Errorf("identity = %s/%s uid=%s, want %s/%s uid=%s",
					r.Namespace, r.Name, r.UID, tc.want.ns, tc.want.objName, tc.want.uid)
			}
			if r.OwnerKind != tc.want.ownerKind || r.OwnerName != tc.want.ownerName {
				t.Errorf("owner = %s/%s, want %s/%s", r.OwnerKind, r.OwnerName, tc.want.ownerKind, tc.want.ownerName)
			}
			if r.ResourceVersion != "42" {
				t.Errorf("ResourceVersion = %q, want 42", r.ResourceVersion)
			}
			// The envelope must land on every row, whatever the kind.
			if r.TenantID != "tenant-1" || r.ClusterName != "prod" || r.ClusterID != "cid-1" {
				t.Errorf("envelope = tenant=%q cluster=%q (%s)", r.TenantID, r.ClusterName, r.ClusterID)
			}
			if r.GroupID != 3 || r.GroupSize != 7 {
				t.Errorf("group = %d/%d, want 3/7", r.GroupID, r.GroupSize)
			}
			if !r.CollectedAt.Equal(now) {
				t.Errorf("CollectedAt = %v, want %v", r.CollectedAt, now)
			}
		})
	}
}

// Pod numbers are sums over the container statuses, the same arithmetic the
// log line uses: ready containers counted, restarts added up across all of
// them, total taken from the slice length.
func TestOrchResourceRowsPodCounts(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), &process.CollectorPod{
		Pods: []*process.Pod{{
			Metadata: &process.Metadata{Namespace: "ns", Name: "p", Uid: "u"},
			NodeName: "node-1",
			Phase:    "Running",
			Status:   "CrashLoopBackOff",
			ContainerStatuses: []*process.ContainerStatus{
				{Name: "app", Ready: true, RestartCount: 4},
				{Name: "sidecar", Ready: true, RestartCount: 1},
				{Name: "init-ish", Ready: false, RestartCount: 0},
			},
		}},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.NodeName != "node-1" || r.Phase != "Running" || r.Status != "CrashLoopBackOff" {
		t.Errorf("node/phase/status = %q/%q/%q", r.NodeName, r.Phase, r.Status)
	}
	if r.Ready != 2 || r.Desired != 3 {
		t.Errorf("ready/desired = %d/%d, want 2/3", r.Ready, r.Desired)
	}
	want := map[string]int64{"restarts": 5, "ready_containers": 2, "total_containers": 3}
	if !reflect.DeepEqual(r.Counts, want) {
		t.Errorf("Counts = %v, want %v", r.Counts, want)
	}
}

// Deployment numbers come from dedicated fields; the stable snake_case keys
// are part of the storage contract, so they are pinned here.
func TestOrchResourceRowsDeploymentCounts(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), &process.CollectorDeployment{
		Deployments: []*process.Deployment{{
			Metadata:            &process.Metadata{Namespace: "ns", Name: "d", Uid: "u"},
			ReplicasDesired:     5,
			ReadyReplicas:       3,
			UpdatedReplicas:     4,
			AvailableReplicas:   3,
			UnavailableReplicas: 2,
		}},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.Ready != 3 || r.Desired != 5 || r.Available != 3 {
		t.Errorf("ready/desired/available = %d/%d/%d, want 3/5/3", r.Ready, r.Desired, r.Available)
	}
	want := map[string]int64{"ready": 3, "desired": 5, "updated": 4, "available": 3, "unavailable": 2}
	if !reflect.DeepEqual(r.Counts, want) {
		t.Errorf("Counts = %v, want %v", r.Counts, want)
	}
}

// A nil element in the collection and an object without Metadata must neither
// panic nor produce a row: without namespace/name/uid there is nothing to
// store the object under.
func TestOrchResourceRowsToleratesNils(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), &process.CollectorPod{
		Pods: []*process.Pod{
			nil,
			{}, // Metadata omitted on the wire
			{Metadata: &process.Metadata{Namespace: "ns", Name: "ok", Uid: "u"}},
		},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1 (nil element and nil Metadata skipped)", len(rows))
	}
	if rows[0].Name != "ok" {
		t.Errorf("surviving row is %q, want ok", rows[0].Name)
	}
}

// The CRD and CR frames wrap an inner *process.CollectorManifest that the
// agent may leave nil; the converter must treat that as an empty collection.
// A nil element inside a populated envelope gets the same treatment.
func TestOrchManifestRowsNilEnvelope(t *testing.T) {
	now := time.Now().UTC()

	if rows := orchManifestRows("t", now, nil); len(rows) != 0 {
		t.Errorf("nil envelope: got %d rows, want 0", len(rows))
	}
	// The exact path the handler takes for an empty CRD frame.
	if rows := orchManifestRows("t", now, (&process.CollectorManifestCRD{}).GetManifest()); len(rows) != 0 {
		t.Errorf("empty CRD frame: got %d rows, want 0", len(rows))
	}

	rows := orchManifestRows("t", now, &process.CollectorManifest{
		ClusterName: "prod", ClusterId: "cid",
		Manifests: []*process.Manifest{
			nil,
			{Uid: "u1", Kind: "ConfigMap", ApiVersion: "v1", ResourceVersion: "7",
				Content: []byte("kind: ConfigMap"), ContentType: "yaml", IsTerminated: true},
		},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.UID != "u1" || r.Kind != "ConfigMap" || r.Content != "kind: ConfigMap" || r.IsTerminated != 1 {
		t.Errorf("row = %+v", r)
	}
}

// An empty (or unparseable) timestamp means "not provided" and must become
// arrival time — the wireTime rule — never 1970-01-01.
func TestKubeActionRowTimestamp(t *testing.T) {
	arrival := time.Date(2026, 9, 21, 12, 0, 0, 0, time.UTC)

	cases := []struct {
		name string
		ts   string
		want time.Time
	}{
		{"empty falls back to arrival", "", arrival},
		{"unparseable falls back to arrival", "yesterday-ish", arrival},
		{"valid RFC 3339 is kept", "2026-09-20T08:30:00Z", time.Date(2026, 9, 20, 8, 30, 0, 0, time.UTC)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row := kubeActionRow("t", arrival, kubeActionResultEvent{Timestamp: tc.ts})
			if !row.Timestamp.Equal(tc.want) {
				t.Errorf("Timestamp = %v, want %v", row.Timestamp, tc.want)
			}
			if row.Timestamp.Year() == 1970 {
				t.Errorf("Timestamp collapsed to the epoch")
			}
		})
	}
}

// Keys the struct does not declare must surface, sorted, in ExtraKeys — the
// whole trip: raw JSON through kubeActionDecode into the row.
func TestKubeActionRowExtraKeys(t *testing.T) {
	raw := json.RawMessage(`{
		"action_id": "a-1",
		"event_type": "action_executed",
		"status": "success",
		"zeta_field": 1,
		"alpha_field": {"nested": true}
	}`)
	ev, err := kubeActionDecode(raw)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	row := kubeActionRow("t", time.Now().UTC(), ev)
	if row.ActionID != "a-1" || row.EventType != "action_executed" || row.Status != "success" {
		t.Errorf("declared fields = %q/%q/%q", row.ActionID, row.EventType, row.Status)
	}
	want := []string{"alpha_field", "zeta_field"}
	if !reflect.DeepEqual(row.ExtraKeys, want) {
		t.Errorf("ExtraKeys = %v, want %v", row.ExtraKeys, want)
	}
}
