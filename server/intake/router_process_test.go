package intake

import (
	"testing"

	"github.com/DataDog/agent-payload/v5/process"
)

// The agent feeds this reply into CheckRunner.UpdateRTStatus, which indexes the
// statuses it collected without checking that it got any. A nil Status
// therefore does not degrade gracefully — it segfaults the agent and
// crash-loops its container. Observed for real before this was fixed.
func TestResCollectorCarriesStatus(t *testing.T) {
	raw := resCollectorResponse()
	if len(raw) == 0 {
		t.Fatal("empty response")
	}

	msg, err := process.DecodeMessage(raw)
	if err != nil {
		t.Fatalf("the agent could not decode our own reply: %v", err)
	}

	res, ok := msg.Body.(*process.ResCollector)
	if !ok {
		t.Fatalf("body is %T, want *process.ResCollector", msg.Body)
	}
	if res.GetStatus() == nil {
		t.Fatal("Status is nil — this is the shape that panics the agent")
	}
	if got := res.GetStatus().GetInterval(); got != processCheckInterval {
		t.Errorf("interval: got %d, want %d", got, processCheckInterval)
	}
	if got := res.GetStatus().GetActiveClients(); got != 0 {
		t.Errorf("active clients: got %d, want 0 — nothing is watching a live view", got)
	}
	if res.GetHeader() == nil {
		t.Error("nested Header missing; older agents look for it")
	}
}
