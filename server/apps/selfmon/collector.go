package selfmon

import (
	"fmt"
	"os"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/gen"
	"github.com/itsninjacats/server/apps/storage"
)

// Collect tells the collector to sample the node right now.
//
// The timer sends it on every tick, but it is exported so an operator (or the
// panel) can force a sample without waiting for the interval — the same shape
// as apikeys.Refresh.
type Collect struct{}

// Collector samples the node and ships the result as metrics.
//
// It is an ordinary actor with ordinary state: the previous counter sample,
// needed to turn the node's lifetime totals into per-interval deltas. That
// state is exactly why this is an actor and not a goroutine with a ticker —
// it is owned by one process, mutated only in HandleMessage, and restored
// from scratch if the supervisor has to restart it.
//
// It never calls ClickHouse. It sends a message to the metrics writer, which
// already owns batching, flushing and backpressure. Self-monitoring therefore
// travels the exact same path as an agent's metrics, and gets the same
// guarantees — including being dropped under overload rather than blocking.
type Collector struct {
	act.Actor

	cfg  Config
	prev *counters
	stop gen.CancelFunc
}

func (c *Collector) Init(args ...any) error {
	if len(args) == 0 {
		return fmt.Errorf("selfmon: missing Config in args")
	}
	cfg, ok := args[0].(Config)
	if !ok {
		return fmt.Errorf("selfmon: first arg must be Config, got %T", args[0])
	}
	c.cfg = cfg

	if c.cfg.Host == "" {
		// Falling back to the node name keeps the series identifiable even
		// when the OS hostname is unavailable, which it is in some containers.
		if h, err := os.Hostname(); err == nil && h != "" {
			c.cfg.Host = h
		} else {
			c.cfg.Host = string(c.Node().Name())
		}
	}

	stop, err := c.SendEvery(c.PID(), Collect{}, c.cfg.Interval)
	if err != nil {
		return fmt.Errorf("selfmon: timer: %w", err)
	}
	c.stop = stop

	c.Log().Info("selfmon started, sampling node every %s as %q on host %q",
		c.cfg.Interval, prefix+"*", c.cfg.Host)
	return nil
}

func (c *Collector) HandleMessage(from gen.PID, message any) error {
	switch message.(type) {
	case Collect:
		c.sample()
	default:
		c.Log().Warning("selfmon: unexpected message %T from %s", message, from)
	}
	return nil
}

func (c *Collector) sample() {
	info, err := c.Node().ShortInfo()
	if err != nil {
		// The node is shutting down. Nothing to report and nowhere to report
		// it to, so this is not an error worth restarting the actor over.
		c.Log().Debug("selfmon: node info unavailable: %s", err)
		return
	}

	points, now := build(info, c.prev, c.cfg.TenantID, c.cfg.Host, c.cfg.Interval, time.Now().UTC())
	c.prev = &now

	if len(points) == 0 {
		return
	}

	// Send, not Call: we do not want to know whether the writer got it, and
	// we must never block the collector on a busy writer. A lost self-metric
	// is a missing point on a chart; a blocked collector would be a process
	// stuck for as long as ClickHouse is slow.
	if err := c.Send(storage.MetricsWriter, storage.WriteMetrics{Points: points}); err != nil {
		c.Log().Warning("selfmon: cannot reach metrics writer: %s", err)
	}
}

func (c *Collector) Terminate(reason error) {
	if c.stop != nil {
		c.stop()
	}
}
