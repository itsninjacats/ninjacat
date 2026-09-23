package intake

import (
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/contimage"
	"github.com/DataDog/agent-payload/v5/contlcycle"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/itsninjacats/server/apps/storage"
)

// The exit code is a proto3 oneof, so "no code reported" and "exit 0" are
// different wire states that decode to the same GetExitCode() value. The
// intake must tell them apart: a Delete without a code is the runtime not
// knowing, not a clean exit.

func TestLifecycleExitCodeAbsentVsZero(t *testing.T) {
	absent := &contlcycle.ContainerEvent{ContainerID: "a"}
	zero := &contlcycle.ContainerEvent{
		ContainerID:      "b",
		OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 0},
	}
	oom := &contlcycle.ContainerEvent{
		ContainerID:      "c",
		OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 137},
	}

	if _, ok := lcExitCode(absent); ok {
		t.Errorf("absent exit code reported as present")
	}
	if code, ok := lcExitCode(zero); !ok || code != 0 {
		t.Errorf("exit 0: got (%d, %v), want (0, true)", code, ok)
	}
	if code, ok := lcExitCode(oom); !ok || code != 137 {
		t.Errorf("exit 137: got (%d, %v), want (137, true)", code, ok)
	}
	if _, ok := lcExitCode(nil); ok {
		t.Errorf("nil event reported as having an exit code")
	}

	if got := lcExitCodeString(absent); got != "-" {
		t.Errorf("absent formatted as %q, want -", got)
	}
	if got := lcExitCodeString(zero); got != "0" {
		t.Errorf("exit 0 formatted as %q, want 0", got)
	}
	if got := lcExitCodeString(oom); got != "137(signal 9)" {
		t.Errorf("exit 137 formatted as %q, want 137(signal 9)", got)
	}
}

// The distinction has to survive the wire, not just the Go struct: an
// explicit exit 0 must still be present after Marshal/Unmarshal.
func TestLifecycleExitCodeZeroSurvivesWire(t *testing.T) {
	in := &contlcycle.EventsPayload{
		Host:       "node1",
		ObjectKind: contlcycle.ObjectKind_Container,
		Events: []*contlcycle.Event{
			{EventType: contlcycle.Event_Delete, TypedEvent: &contlcycle.Event_Container{
				Container: &contlcycle.ContainerEvent{
					ContainerID:      "clean",
					OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 0},
				},
			}},
			{EventType: contlcycle.Event_Delete, TypedEvent: &contlcycle.Event_Container{
				Container: &contlcycle.ContainerEvent{ContainerID: "unknown"},
			}},
		},
	}
	body, err := proto.Marshal(in)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var out contlcycle.EventsPayload
	if err := proto.Unmarshal(body, &out); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(out.GetEvents()) != 2 {
		t.Fatalf("got %d events, want 2", len(out.GetEvents()))
	}
	if _, ok := lcExitCode(out.GetEvents()[0].GetContainer()); !ok {
		t.Errorf("explicit exit 0 lost on the wire")
	}
	if _, ok := lcExitCode(out.GetEvents()[1].GetContainer()); ok {
		t.Errorf("absent exit code became present on the wire")
	}
}

// Nil nested messages must not panic: the getters absorb them.
func TestLifecycleLogToleratesNils(t *testing.T) {
	p := &contlcycle.EventsPayload{Events: []*contlcycle.Event{
		{TypedEvent: &contlcycle.Event_Container{Container: &contlcycle.ContainerEvent{
			Transition: &contlcycle.ContainerStateTransition{},
		}}},
		{TypedEvent: &contlcycle.Event_Pod{Pod: &contlcycle.PodEvent{
			Transition: &contlcycle.PodStateTransition{},
		}}},
		{TypedEvent: &contlcycle.Event_Task{Task: &contlcycle.TaskEvent{}}},
		{TypedEvent: &contlcycle.Event_Container{}},
		{},
	}}
	lcLogPayload(p)
}

// ---------------------------------------------------------------------------
// Conversion to storage rows
// ---------------------------------------------------------------------------

// testNow is a fixed conversion time so a test can tell "the sender's value"
// from "the receive time" — the two things an absent timestamp must never
// silently become.
var testNow = time.Date(2026, 9, 21, 12, 0, 0, 0, time.UTC)

func i32ptr(v int32) *int32 { return &v }

// The conversion-level counterpart of TestLifecycleExitCodeAbsentVsZero: the
// oneof distinction must reach the row as nil versus a real pointer, because
// the Nullable(Int32) column is the last place it can survive.
func TestLifecycleRowExitCode(t *testing.T) {
	cases := []struct {
		name string
		ev   *contlcycle.ContainerEvent
		want *int32
	}{
		{"absent stays NULL", &contlcycle.ContainerEvent{ContainerID: "a"}, nil},
		{"explicit zero is zero", &contlcycle.ContainerEvent{
			ContainerID:      "b",
			OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 0},
		}, i32ptr(0)},
		{"OOM kill is 137", &contlcycle.ContainerEvent{
			ContainerID:      "c",
			OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 137},
		}, i32ptr(137)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			p := &contlcycle.EventsPayload{
				Host: "node1",
				Events: []*contlcycle.Event{{
					EventType:  contlcycle.Event_Delete,
					TypedEvent: &contlcycle.Event_Container{Container: tc.ev},
				}},
			}
			rows := lcRows(p, "t1", testNow)
			if len(rows) != 1 {
				t.Fatalf("got %d rows, want 1", len(rows))
			}
			got := rows[0].ExitCode
			switch {
			case tc.want == nil && got != nil:
				t.Errorf("exit code: got %d, want nil", *got)
			case tc.want != nil && got == nil:
				t.Errorf("exit code: got nil, want %d", *tc.want)
			case tc.want != nil && *got != *tc.want:
				t.Errorf("exit code: got %d, want %d", *got, *tc.want)
			}
		})
	}
}

// Absent timestamps must convert to nil — never 1970, never the receive time.
// Presence is the oneof/pointer being non-nil, not the value being non-zero:
// a transition stamped 0 is "the agent did not say when", not the epoch.
func TestLifecycleRowTimestampsAbsent(t *testing.T) {
	p := &contlcycle.EventsPayload{Events: []*contlcycle.Event{{
		EventType: contlcycle.Event_Transition,
		TypedEvent: &contlcycle.Event_Container{Container: &contlcycle.ContainerEvent{
			ContainerID: "a",
			Transition:  &contlcycle.ContainerStateTransition{}, // TransitionTimestamp zero
		}},
	}}}
	rows := lcRows(p, "t1", testNow)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	for name, got := range map[string]*time.Time{
		"CreatedAt": r.CreatedAt, "ExitedAt": r.ExitedAt, "TransitionAt": r.TransitionAt,
	} {
		if got != nil {
			t.Errorf("%s: got %v, want nil for an absent timestamp", name, *got)
		}
	}
}

func TestLifecycleRowTimestampsPresent(t *testing.T) {
	const created, exited, transitioned = 1700000000, 1700000600, 1700000300
	p := &contlcycle.EventsPayload{Events: []*contlcycle.Event{{
		EventType: contlcycle.Event_Delete,
		TypedEvent: &contlcycle.Event_Container{Container: &contlcycle.ContainerEvent{
			ContainerID:               "a",
			OptionalCreationTimestamp: &contlcycle.ContainerEvent_CreationTimestamp{CreationTimestamp: created},
			OptionalExitTimestamp:     &contlcycle.ContainerEvent_ExitTimestamp{ExitTimestamp: exited},
			Transition: &contlcycle.ContainerStateTransition{
				TransitionTimestamp: transitioned,
			},
		}},
	}}}
	rows := lcRows(p, "t1", testNow)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	for name, tc := range map[string]struct {
		got  *time.Time
		want int64
	}{
		"CreatedAt":    {r.CreatedAt, created},
		"ExitedAt":     {r.ExitedAt, exited},
		"TransitionAt": {r.TransitionAt, transitioned},
	} {
		if tc.got == nil {
			t.Errorf("%s: got nil, want %v", name, time.Unix(tc.want, 0).UTC())
			continue
		}
		if !tc.got.Equal(time.Unix(tc.want, 0)) {
			t.Errorf("%s: got %v, want %v", name, *tc.got, time.Unix(tc.want, 0).UTC())
		}
	}
}

// Each oneof variant fills only its own identity column; the others stay
// empty rather than borrowing a value.
func TestLifecycleRowIdentityByVariant(t *testing.T) {
	exitTS := int64(1700000600)
	p := &contlcycle.EventsPayload{
		Host:       "node1",
		ClusterId:  "cl-1",
		ObjectKind: contlcycle.ObjectKind_Pod,
		Events: []*contlcycle.Event{
			{EventType: contlcycle.Event_Delete, TypedEvent: &contlcycle.Event_Pod{
				Pod: &contlcycle.PodEvent{PodUID: "pod-1", Source: "kubelet", ExitTimestamp: &exitTS},
			}},
			{EventType: contlcycle.Event_Delete, TypedEvent: &contlcycle.Event_Task{
				Task: &contlcycle.TaskEvent{TaskARN: "arn:aws:ecs:task/1", Source: "ecs", ExitTimestamp: &exitTS},
			}},
		},
	}
	rows := lcRows(p, "t1", testNow)
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2", len(rows))
	}

	pod := rows[0]
	if pod.PodUID != "pod-1" || pod.Source != "kubelet" {
		t.Errorf("pod row: got PodUID=%q Source=%q", pod.PodUID, pod.Source)
	}
	if pod.ContainerID != "" || pod.ContainerName != "" || pod.TaskARN != "" {
		t.Errorf("pod row leaked foreign identity: container=%q/%q task=%q",
			pod.ContainerID, pod.ContainerName, pod.TaskARN)
	}
	if pod.Host != "node1" || pod.ClusterID != "cl-1" || pod.ObjectKind != "Pod" || pod.EventType != "Delete" {
		t.Errorf("pod row envelope: host=%q cluster=%q kind=%q type=%q",
			pod.Host, pod.ClusterID, pod.ObjectKind, pod.EventType)
	}
	if pod.ExitedAt == nil || !pod.ExitedAt.Equal(time.Unix(exitTS, 0)) {
		t.Errorf("pod row ExitedAt: got %v, want %v", pod.ExitedAt, time.Unix(exitTS, 0).UTC())
	}

	task := rows[1]
	if task.TaskARN != "arn:aws:ecs:task/1" || task.Source != "ecs" {
		t.Errorf("task row: got TaskARN=%q Source=%q", task.TaskARN, task.Source)
	}
	if task.ContainerID != "" || task.PodUID != "" {
		t.Errorf("task row leaked foreign identity: container=%q pod=%q", task.ContainerID, task.PodUID)
	}
}

// A nil oneof still yields a row: the EventType is real information even when
// the detail is missing (or newer than this schema).
func TestLifecycleRowNilOneof(t *testing.T) {
	p := &contlcycle.EventsPayload{
		Host:   "node1",
		Events: []*contlcycle.Event{{EventType: contlcycle.Event_Create}},
	}
	rows := lcRows(p, "t1", testNow)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.EventType != "Create" || r.Host != "node1" || r.TenantID != "t1" {
		t.Errorf("got EventType=%q Host=%q TenantID=%q", r.EventType, r.Host, r.TenantID)
	}
	if r.ContainerID != "" || r.PodUID != "" || r.TaskARN != "" || r.ExitCode != nil {
		t.Errorf("detail-less event grew detail: container=%q pod=%q task=%q exit=%v",
			r.ContainerID, r.PodUID, r.TaskARN, r.ExitCode)
	}
}

// A container event's owner and transition states reach the row through the
// same formatters as the log, so the two views cannot drift.
func TestLifecycleRowOwnerAndTransition(t *testing.T) {
	reason := "OOMKilled"
	p := &contlcycle.EventsPayload{Events: []*contlcycle.Event{{
		EventType: contlcycle.Event_Transition,
		TypedEvent: &contlcycle.Event_Container{Container: &contlcycle.ContainerEvent{
			ContainerID: "a",
			Owner:       &contlcycle.ContainerEvent_Owner{OwnerType: contlcycle.ObjectKind_Pod, OwnerUID: "pod-9"},
			Transition: &contlcycle.ContainerStateTransition{
				LastObservedState: &contlcycle.ContainerStateValue{Kind: contlcycle.ContainerStateKind_CONTAINER_STATE_KIND_RUNNING},
				NewState: &contlcycle.ContainerStateValue{
					Kind:   contlcycle.ContainerStateKind_CONTAINER_STATE_KIND_TERMINATED,
					Reason: &reason,
				},
				TransitionTimestamp: 1700000300,
			},
		}},
	}}}
	rows := lcRows(p, "t1", testNow)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.OwnerType != "Pod" || r.OwnerUID != "pod-9" {
		t.Errorf("owner: got %q/%q, want Pod/pod-9", r.OwnerType, r.OwnerUID)
	}
	if want := lcContainerState(&contlcycle.ContainerStateValue{Kind: contlcycle.ContainerStateKind_CONTAINER_STATE_KIND_RUNNING}); r.OldState != want {
		t.Errorf("OldState: got %q, want %q", r.OldState, want)
	}
	if r.NewState == "" || !strings.Contains(r.NewState, "OOMKilled") {
		t.Errorf("NewState %q does not carry the reason", r.NewState)
	}
	if r.TransitionAt == nil || !r.TransitionAt.Equal(time.Unix(1700000300, 0)) {
		t.Errorf("TransitionAt: got %v, want %v", r.TransitionAt, time.Unix(1700000300, 0).UTC())
	}
}

// Identity is the registry digest when there is one. An image carrying
// neither a digest nor an id has nothing to key on and is skipped rather than
// stored under an empty key, where every such image would collapse into one
// row. Layers are summed the same way the log sums them.
func TestImageRows(t *testing.T) {
	p := &contimage.ContainerImagePayload{
		Host: "node1",
		Images: []*contimage.ContainerImage{
			{Name: "no-digest"}, // skipped
			{
				Id:        "img-1",
				Name:      "registry.example.com/app",
				ShortName: "app",
				Registry:  "registry.example.com",
				Digest:    "sha256:abc",
				Size:      1234,
				RepoTags:  []string{"app:1.0"},
				Os:        &contimage.ContainerImage_OperatingSystem{Name: "linux", Architecture: "amd64"},
				Layers: []*contimage.ContainerImage_ContainerImageLayer{
					{Size: 100},
					{Size: 250},
				},
				BuiltAt: timestamppb.New(time.Unix(1700000000, 0)),
				// PublishedAt deliberately absent.
			},
		},
	}
	rows, skipped := ciRows(p, "t1", testNow)
	if skipped != 1 {
		t.Errorf("skipped: got %d, want 1", skipped)
	}
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.Digest != "sha256:abc" || r.Host != "node1" || r.TenantID != "t1" {
		t.Errorf("identity: digest=%q host=%q tenant=%q", r.Digest, r.Host, r.TenantID)
	}
	if r.LayerCount != 2 || r.LayerBytes != 350 {
		t.Errorf("layers: got count=%d bytes=%d, want 2/350", r.LayerCount, r.LayerBytes)
	}
	if r.SizeBytes != 1234 {
		t.Errorf("SizeBytes: got %d, want 1234", r.SizeBytes)
	}
	if r.OSName != "linux" || r.Architecture != "amd64" {
		t.Errorf("platform: got %q/%q", r.OSName, r.Architecture)
	}
	if r.BuiltAt == nil || !r.BuiltAt.Equal(time.Unix(1700000000, 0)) {
		t.Errorf("BuiltAt: got %v, want %v", r.BuiltAt, time.Unix(1700000000, 0).UTC())
	}
	if r.PublishedAt != nil {
		t.Errorf("PublishedAt: got %v, want nil for an absent timestamp", *r.PublishedAt)
	}
}

// Nil payloads and empty lists convert to nothing, quietly — the getters
// absorb the nils, and zero rows is a valid answer, not an error.
func TestContainerRowsTolerateNilAndEmpty(t *testing.T) {
	if rows := lcRows(nil, "t1", testNow); len(rows) != 0 {
		t.Errorf("nil lifecycle payload: got %d rows, want 0", len(rows))
	}
	if rows := lcRows(&contlcycle.EventsPayload{}, "t1", testNow); len(rows) != 0 {
		t.Errorf("empty event list: got %d rows, want 0", len(rows))
	}
	if rows, skipped := ciRows(nil, "t1", testNow); len(rows) != 0 || skipped != 0 {
		t.Errorf("nil image payload: got %d rows, %d skipped, want 0/0", len(rows), skipped)
	}
	if rows, skipped := ciRows(&contimage.ContainerImagePayload{}, "t1", testNow); len(rows) != 0 || skipped != 0 {
		t.Errorf("empty image list: got %d rows, %d skipped, want 0/0", len(rows), skipped)
	}
}

// A missing digest is a normal state — locally built images, tarball loads and
// freshly pulled images all lack one — so the image id stands in rather than
// the image being dropped. What was stored under which identity has to stay
// visible, because an image with no registry digest is exactly the one an SBOM
// will never join to.
func TestImageRowsIdentityFallback(t *testing.T) {
	p := &contimage.ContainerImagePayload{
		Host: "node1",
		Images: []*contimage.ContainerImage{
			{Id: "img-a", Name: "pushed", Digest: "sha256:abc"},
			{Id: "img-b", Name: "built-locally"}, // no digest, has an id
			{Name: "nothing-at-all"},             // neither: unidentifiable
		},
	}

	rows, skipped := ciRows(p, "t1", testNow)
	if skipped != 1 {
		t.Errorf("skipped: got %d, want 1 (only the image with no identity at all)", skipped)
	}
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2", len(rows))
	}

	want := []struct {
		key    string
		source string
		digest string
		id     string
	}{
		{key: "sha256:abc", source: "digest", digest: "sha256:abc", id: "img-a"},
		{key: "img-b", source: "image_id", digest: "", id: "img-b"},
	}
	for i, w := range want {
		r := rows[i]
		if r.ImageKey != w.key {
			t.Errorf("row %d ImageKey: got %q, want %q", i, r.ImageKey, w.key)
		}
		if r.IdentitySource != w.source {
			t.Errorf("row %d IdentitySource: got %q, want %q", i, r.IdentitySource, w.source)
		}
		// The raw columns stay truthful: the fallback fills ImageKey, it does
		// not forge a digest.
		if r.Digest != w.digest {
			t.Errorf("row %d Digest: got %q, want %q", i, r.Digest, w.digest)
		}
		if r.ImageID != w.id {
			t.Errorf("row %d ImageID: got %q, want %q", i, r.ImageID, w.id)
		}
	}
}

// The self-metric is the only thing that makes a silently incomplete inventory
// visible, so its shape matters: a COUNT under the intake prefix, carrying the
// tenant and host whose payload produced the skip.
//
// Interval must stay 0. selfmon reports a sampling window because it samples;
// this is event-driven, and a fabricated interval would be divided by later.
func TestSelfCountPointShape(t *testing.T) {
	now := time.Unix(1700000000, 0).UTC()
	p := selfCountPoint("tenant-7", "node-9", SelfImagesSkipped, 3, now,
		map[string][]string{"reason": {"no_identity"}})

	if p.Metric != "ninjacat.intake.images.skipped" {
		t.Errorf("metric name: got %q", p.Metric)
	}
	if p.MetricType != "COUNT" {
		t.Errorf("metric type: got %q, want COUNT", p.MetricType)
	}
	if p.Interval != 0 {
		t.Errorf("interval: got %d, want 0 — this is event-driven, not sampled", p.Interval)
	}
	if p.TenantID != "tenant-7" || p.Host != "node-9" {
		t.Errorf("identity: tenant=%q host=%q", p.TenantID, p.Host)
	}
	if p.Value != 3 {
		t.Errorf("value: got %v, want 3", p.Value)
	}
	if p.SourceType != "ninjacat" {
		t.Errorf("source type: got %q", p.SourceType)
	}
	if len(p.Tags["reason"]) != 1 || p.Tags["reason"][0] != "no_identity" {
		t.Errorf("tags: got %v", p.Tags)
	}
	if !p.Timestamp.Equal(now) {
		t.Errorf("timestamp: got %v, want %v", p.Timestamp, now)
	}
}

// ---------------------------------------------------------------------------
// Fidelity: what the transition formatter used to flatten away
// ---------------------------------------------------------------------------

// A container killed by the OOM killer, which is what this intake exists for.
// The flattened OldState/NewState strings stay — a dashboard wants a label —
// but "every OOM kill with signal 9" is a question about columns, and the
// reason, exit code and signal had none.
func TestLifecycleRowStructuredContainerTransition(t *testing.T) {
	reason := "OOMKilled"
	exit := int32(137)
	signal := int32(9)

	rows := lcRows(&contlcycle.EventsPayload{
		Version: "v1", Host: "node-1", ClusterId: "cid",
		Events: []*contlcycle.Event{{
			EventType: contlcycle.Event_Transition,
			TypedEvent: &contlcycle.Event_Container{Container: &contlcycle.ContainerEvent{
				ContainerID: "containerd://abc",
				Transition: &contlcycle.ContainerStateTransition{
					ContainerKind:       contlcycle.ContainerKind_CONTAINER_KIND_REGULAR,
					Precision:           contlcycle.Precision_PRECISION_EXACT,
					MissedIntermediate:  contlcycle.MissedIntermediate_MISSED_INTERMEDIATE_PROVEN,
					TransitionTimestamp: 1700000000,
					LastObservedState: &contlcycle.ContainerStateValue{
						Kind: contlcycle.ContainerStateKind_CONTAINER_STATE_KIND_RUNNING,
					},
					NewState: &contlcycle.ContainerStateValue{
						Kind:     contlcycle.ContainerStateKind_CONTAINER_STATE_KIND_TERMINATED,
						Reason:   &reason,
						ExitCode: &exit,
						Signal:   &signal,
					},
				},
			}},
		}},
	}, "t", time.Now().UTC())

	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]

	if r.PayloadVersion != "v1" {
		t.Errorf("PayloadVersion = %q, want v1", r.PayloadVersion)
	}
	if r.EventVariant != "container" {
		t.Errorf("EventVariant = %q, want container", r.EventVariant)
	}
	if r.ContainerKind != "CONTAINER_KIND_REGULAR" {
		t.Errorf("ContainerKind = %q", r.ContainerKind)
	}
	if r.Precision != "PRECISION_EXACT" || r.MissedIntermediate != "MISSED_INTERMEDIATE_PROVEN" {
		t.Errorf("precision/missed = %q / %q — these are the agent saying how much to trust the row",
			r.Precision, r.MissedIntermediate)
	}
	if r.OldStateKind != "CONTAINER_STATE_KIND_RUNNING" || r.NewStateKind != "CONTAINER_STATE_KIND_TERMINATED" {
		t.Errorf("state kinds = %q -> %q", r.OldStateKind, r.NewStateKind)
	}
	if r.NewReason == nil || *r.NewReason != "OOMKilled" {
		t.Errorf("NewReason = %v, want OOMKilled", r.NewReason)
	}
	if r.NewExitCode == nil || *r.NewExitCode != 137 || r.NewSignal == nil || *r.NewSignal != 9 {
		t.Errorf("new exit/signal = %v / %v, want 137 / 9", r.NewExitCode, r.NewSignal)
	}
	// The RUNNING state carried no reason, exit code or signal — proto3
	// optionals left unset — and absent must stay absent rather than become
	// "" and 0, which would read as "terminated cleanly with no reason".
	if r.OldReason != nil || r.OldExitCode != nil || r.OldSignal != nil {
		t.Errorf("old state invented values: reason=%v exit=%v signal=%v", r.OldReason, r.OldExitCode, r.OldSignal)
	}
	// The formatted strings are still there, from the same wire message, so
	// the log and the row cannot disagree.
	if !strings.Contains(r.NewState, "OOMKilled") || !strings.Contains(r.NewState, "signal=9") {
		t.Errorf("NewState = %q, want the human label kept beside the columns", r.NewState)
	}
}

// A pod scheduling failure. The old formatter collapsed the whole status into
// one string and never read Message at all — which is the half that says what
// to do about it.
func TestLifecycleRowPodConditionTransition(t *testing.T) {
	reason := "Unschedulable"
	message := "0/3 nodes are available: insufficient memory"

	rows := lcRows(&contlcycle.EventsPayload{
		Events: []*contlcycle.Event{{
			EventType: contlcycle.Event_Transition,
			TypedEvent: &contlcycle.Event_Pod{Pod: &contlcycle.PodEvent{
				PodUID: "pod-uid-1",
				Transition: &contlcycle.PodStateTransition{
					Field: contlcycle.PodStatusField_POD_STATUS_FIELD_CONDITION,
					LastObservedState: &contlcycle.PodStatusValue{
						Value: &contlcycle.PodStatusValue_Phase{Phase: "Pending"},
					},
					NewState: &contlcycle.PodStatusValue{
						Value: &contlcycle.PodStatusValue_Condition{Condition: &contlcycle.ConditionValue{
							Type: "PodScheduled", Status: "False", Reason: &reason, Message: &message,
						}},
					},
				},
			}},
		}},
	}, "t", time.Now().UTC())
	r := rows[0]

	if r.EventVariant != "pod" || r.PodUID != "pod-uid-1" {
		t.Errorf("variant/uid = %q / %q", r.EventVariant, r.PodUID)
	}
	if r.PodStatusField != "POD_STATUS_FIELD_CONDITION" {
		t.Errorf("PodStatusField = %q — WHICH field moved was never stored", r.PodStatusField)
	}
	// The oneof discriminator: a phase row and a condition row use different
	// columns, and nothing said which arm had been populated.
	if r.OldStateVariant != "phase" || r.NewStateVariant != "condition" {
		t.Errorf("state variants = %q -> %q, want phase -> condition", r.OldStateVariant, r.NewStateVariant)
	}
	if r.OldPhase == nil || *r.OldPhase != "Pending" {
		t.Errorf("OldPhase = %v, want Pending", r.OldPhase)
	}
	if r.OldConditionType != nil || r.NewPhase != nil {
		t.Errorf("the unused arm was filled in: oldCondType=%v newPhase=%v", r.OldConditionType, r.NewPhase)
	}
	if r.NewConditionType == nil || *r.NewConditionType != "PodScheduled" ||
		r.NewConditionStatus == nil || *r.NewConditionStatus != "False" {
		t.Errorf("new condition = %v / %v", r.NewConditionType, r.NewConditionStatus)
	}
	if r.NewConditionMessage == nil || *r.NewConditionMessage != message {
		t.Errorf("NewConditionMessage = %v, want the scheduler's explanation", r.NewConditionMessage)
	}
}

// containerName is a proto3 optional, so "never set" and "set to empty" are
// different wire states. The column cannot become Nullable without rewriting
// every stored row, so presence travels beside it.
func TestLifecycleRowContainerNamePresence(t *testing.T) {
	empty := ""
	cases := []struct {
		name        string
		event       *contlcycle.ContainerEvent
		wantPresent uint8
	}{
		{"never set", &contlcycle.ContainerEvent{ContainerID: "a"}, 0},
		{"set to empty", &contlcycle.ContainerEvent{ContainerID: "b", ContainerName: &empty}, 1},
		{"set to a name", &contlcycle.ContainerEvent{ContainerID: "c", ContainerName: strPtr("app")}, 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rows := lcRows(&contlcycle.EventsPayload{
				Events: []*contlcycle.Event{{TypedEvent: &contlcycle.Event_Container{Container: tc.event}}},
			}, "t", time.Now().UTC())
			if rows[0].ContainerNamePresent != tc.wantPresent {
				t.Errorf("ContainerNamePresent = %d, want %d", rows[0].ContainerNamePresent, tc.wantPresent)
			}
		})
	}
}

// An event whose oneof is nil, or a variant newer than this schema, produced
// a row indistinguishable from a typed event whose fields all happened to be
// empty. The variant column is what tells them apart.
func TestLifecycleRowUnknownVariant(t *testing.T) {
	rows := lcRows(&contlcycle.EventsPayload{
		Events: []*contlcycle.Event{{EventType: contlcycle.Event_Delete}},
	}, "t", time.Now().UTC())
	if rows[0].EventVariant != "unknown" {
		t.Errorf("EventVariant = %q, want unknown", rows[0].EventVariant)
	}
	if rows[0].EventType != "Delete" {
		t.Errorf("EventType = %q — the envelope is still real information", rows[0].EventType)
	}
}

func strPtr(s string) *string { return &s }

// The layer list was reduced to two numbers and the build history thrown away
// whole — the Dockerfile instruction that created each layer, its author, and
// when. Layers are index-aligned arrays in wire order, because an image IS
// its ordered layer stack.
func TestImageRowsLayers(t *testing.T) {
	created := timestamppb.New(time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC))
	source := "agent"

	rows, skipped := ciRows(&contimage.ContainerImagePayload{
		Version: "v1", Host: "node-1", Source: &source,
		Images: []*contimage.ContainerImage{{
			Id: "sha256:img", Name: "shop/app", Digest: "sha256:dig", Size: 300,
			Layers: []*contimage.ContainerImage_ContainerImageLayer{
				{
					MediaType: "application/vnd.oci.image.layer.v1.tar+gzip",
					Digest:    "sha256:layer1", Size: 100,
					Urls: []string{"https://example.invalid/blob"},
					History: &contimage.ContainerImage_ContainerImageLayer_History{
						Created: created, CreatedBy: "RUN apk add curl", Author: "ci", Comment: "base",
					},
				},
				{
					Digest: "sha256:layer2", Size: 200,
					History: &contimage.ContainerImage_ContainerImageLayer_History{
						CreatedBy: "COPY . /app", EmptyLayer: true,
					},
				},
			},
		}},
	}, "t", time.Now().UTC())

	if skipped != 0 || len(rows) != 1 {
		t.Fatalf("rows=%d skipped=%d, want 1/0", len(rows), skipped)
	}
	r := rows[0]

	if r.PayloadVersion != "v1" {
		t.Errorf("PayloadVersion = %q", r.PayloadVersion)
	}
	// Which pipeline produced the inventory was decoded for the log line and
	// never stored; nil there is "the sender did not say", not "agent".
	if r.Source == nil || *r.Source != "agent" {
		t.Errorf("Source = %v, want agent", r.Source)
	}

	if !reflect.DeepEqual(r.LayerDigests, []string{"sha256:layer1", "sha256:layer2"}) {
		t.Errorf("LayerDigests = %v, want both in wire order", r.LayerDigests)
	}
	if !reflect.DeepEqual(r.LayerSizes, []int64{100, 200}) {
		t.Errorf("LayerSizes = %v", r.LayerSizes)
	}
	if r.LayerMediaTypes[0] != "application/vnd.oci.image.layer.v1.tar+gzip" {
		t.Errorf("LayerMediaTypes[0] = %q", r.LayerMediaTypes[0])
	}
	// The layer that had no urls must get an EMPTY slice: the driver rejects
	// a nil inside an Array(Array(String)) and takes the whole batch with it.
	if len(r.LayerURLs) != 2 || r.LayerURLs[1] == nil {
		t.Errorf("LayerURLs = %v, want an empty slice for the layer with none", r.LayerURLs)
	}
	if r.LayerURLs[0][0] != "https://example.invalid/blob" {
		t.Errorf("LayerURLs[0] = %v", r.LayerURLs[0])
	}

	// The build history: this is the single biggest thing the old converter
	// decoded and dropped.
	if r.LayerHistoryCreatedBy[0] != "RUN apk add curl" || r.LayerHistoryCreatedBy[1] != "COPY . /app" {
		t.Errorf("LayerHistoryCreatedBy = %v", r.LayerHistoryCreatedBy)
	}
	if r.LayerHistoryAuthor[0] != "ci" || r.LayerHistoryComment[0] != "base" {
		t.Errorf("author/comment = %q / %q", r.LayerHistoryAuthor[0], r.LayerHistoryComment[0])
	}
	if !reflect.DeepEqual(r.LayerHistoryEmptyLayer, []uint8{0, 1}) {
		t.Errorf("LayerHistoryEmptyLayer = %v", r.LayerHistoryEmptyLayer)
	}
	if r.LayerHistoryCreated[0] == nil || !r.LayerHistoryCreated[0].Equal(created.AsTime()) {
		t.Errorf("LayerHistoryCreated[0] = %v", r.LayerHistoryCreated[0])
	}
	// The second layer's history carried no Created, and a missing date must
	// not become 1970 — the arrays stay index-aligned with a NULL in the hole.
	if r.LayerHistoryCreated[1] != nil {
		t.Errorf("LayerHistoryCreated[1] = %v, want nil", r.LayerHistoryCreated[1])
	}
	if r.LayerCount != 2 || r.LayerBytes != 300 {
		t.Errorf("layer count/bytes = %d / %d", r.LayerCount, r.LayerBytes)
	}
}

// size is a signed int64 on the wire and unsigned in the table. Casting a
// negative blindly produces ~18 exabytes and poisons every SUM over the
// table, so it is clamped and the clamp is recorded rather than hidden.
func TestImageRowsNegativeSizeIsClampedAndFlagged(t *testing.T) {
	rows, _ := ciRows(&contimage.ContainerImagePayload{
		Images: []*contimage.ContainerImage{
			{Id: "a", Size: -1},
			{Id: "b", Size: 42},
		},
	}, "t", time.Now().UTC())
	if len(rows) != 2 {
		t.Fatalf("got %d rows, want 2", len(rows))
	}
	if rows[0].SizeBytes != 0 || rows[0].SizeNegative != 1 {
		t.Errorf("negative size = %d (flag %d), want 0 with the flag raised", rows[0].SizeBytes, rows[0].SizeNegative)
	}
	if rows[1].SizeBytes != 42 || rows[1].SizeNegative != 0 {
		t.Errorf("honest size = %d (flag %d)", rows[1].SizeBytes, rows[1].SizeNegative)
	}
}

// ---------------------------------------------------------------------------
// Through the real engine
// ---------------------------------------------------------------------------

// The whole path for a lifecycle batch, and the 202 with an empty object that
// every sender on this intake depends on.
func TestHandleContainerLifecycleStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeContainers)

	body, err := proto.Marshal(&contlcycle.EventsPayload{
		Version: "v1", Host: "node-1", ClusterId: "cid",
		ObjectKind: contlcycle.ObjectKind_Container,
		Events: []*contlcycle.Event{
			{EventType: contlcycle.Event_Create, TypedEvent: &contlcycle.Event_Container{
				Container: &contlcycle.ContainerEvent{ContainerID: "c1"}}},
			{EventType: contlcycle.Event_Delete, TypedEvent: &contlcycle.Event_Container{
				Container: &contlcycle.ContainerEvent{ContainerID: "c2",
					OptionalExitCode: &contlcycle.ContainerEvent_ExitCode{ExitCode: 137}}}},
		},
	})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	w := post(t, e, "/api/v2/contlcycle", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	if got := strings.TrimSpace(w.Body.String()); got != "{}" {
		t.Errorf("body = %q, want {}", got)
	}

	rows := Rows[storage.ContainerEventRow](node)
	if len(rows) != 2 {
		t.Fatalf("container_events rows: got %d, want 2", len(rows))
	}
	for i, r := range rows {
		if r.TenantID != testTenant || r.Host != "node-1" || r.PayloadVersion != "v1" {
			t.Errorf("row %d = %+v", i, r)
		}
	}
	if rows[1].ExitCode == nil || *rows[1].ExitCode != 137 {
		t.Errorf("exit code = %v, want 137", rows[1].ExitCode)
	}
}

// Protobuf fails OPEN: a payload meant for another endpoint decodes into an
// empty struct rather than erroring. The bytes are the only thing that says
// what really arrived, so they go to raw_payloads.
func TestHandleContainerPayloadsStoreRawWhenUndecodable(t *testing.T) {
	cases := []struct {
		name, path, intake, reason string
		body                       []byte
	}{
		{"lifecycle garbage", "/api/v2/contlcycle", "contlcycle", "decode_error", []byte{0xff, 0xff, 0xff, 0xff}},
		{"lifecycle empty decode", "/api/v2/contlcycle", "contlcycle", "unexpected_shape", nil},
		{"image garbage", "/api/v2/contimage", "contimage", "decode_error", []byte{0xff, 0xff, 0xff, 0xff}},
		{"image empty decode", "/api/v2/contimage", "contimage", "unexpected_shape", nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeContainers)

			if w := post(t, e, tc.path, tc.body); w.Code != http.StatusAccepted {
				t.Fatalf("got %d, want 202 — this intake never rejects", w.Code)
			}
			raw := Rows[storage.RawPayloadRow](node)
			if len(raw) != 1 {
				t.Fatalf("raw_payloads rows: got %d, want 1", len(raw))
			}
			if raw[0].Intake != tc.intake || raw[0].Reason != tc.reason {
				t.Errorf("intake/reason = %q/%q, want %q/%q", raw[0].Intake, raw[0].Reason, tc.intake, tc.reason)
			}
			if raw[0].TenantID != testTenant {
				t.Errorf("tenant = %q", raw[0].TenantID)
			}
		})
	}
}

// One layer reporting a negative size is the same trap as a negative image
// size, a level down: layer_bytes is their unsigned sum, so a bad layer
// either wraps the cast or quietly shrinks the total. A registry returning a
// -1 placeholder for a layer it cannot size must not poison SUM(layer_bytes)
// for the whole fleet, and the raw value must still be readable per layer.
func TestImageRowsNegativeLayerSizeIsClampedAndFlagged(t *testing.T) {
	rows, _ := ciRows(&contimage.ContainerImagePayload{
		Images: []*contimage.ContainerImage{
			// Sum stays positive, but one layer lied: the total is wrong and
			// only the flag says so.
			{Id: "a", Layers: []*contimage.ContainerImage_ContainerImageLayer{{Size: 500}, {Size: -1}}},
			// Sum goes negative: the cast would produce ~18 exabytes.
			{Id: "b", Layers: []*contimage.ContainerImage_ContainerImageLayer{{Size: -700}, {Size: 100}}},
			{Id: "c", Layers: []*contimage.ContainerImage_ContainerImageLayer{{Size: 10}, {Size: 20}}},
		},
	}, "t", time.Now().UTC())
	if len(rows) != 3 {
		t.Fatalf("got %d rows, want 3", len(rows))
	}
	if rows[0].LayerBytes != 499 || rows[0].LayerSizeNegative != 1 {
		t.Errorf("mixed layers = %d (flag %d), want 499 with the flag raised",
			rows[0].LayerBytes, rows[0].LayerSizeNegative)
	}
	if rows[1].LayerBytes != 0 || rows[1].LayerSizeNegative != 1 {
		t.Errorf("negative sum = %d (flag %d), want 0 with the flag raised",
			rows[1].LayerBytes, rows[1].LayerSizeNegative)
	}
	if rows[2].LayerBytes != 30 || rows[2].LayerSizeNegative != 0 {
		t.Errorf("honest layers = %d (flag %d)", rows[2].LayerBytes, rows[2].LayerSizeNegative)
	}
	// The clamp hides nothing: the signed values arrive as they were sent.
	if rows[1].LayerSizes[0] != -700 {
		t.Errorf("LayerSizes[0] = %d, want the raw -700", rows[1].LayerSizes[0])
	}
}
