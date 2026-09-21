package intake

import (
	"testing"

	"github.com/DataDog/agent-payload/v5/contlcycle"
	"google.golang.org/protobuf/proto"
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
