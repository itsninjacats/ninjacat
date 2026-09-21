package storage

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

const (
	AppName        = gen.Atom("storage")
	SupervisorName = gen.Atom("storage_sup")
)

// Writer process names. Handlers send rows to these addresses.
//
// One writer per table, each with its own mailbox and buffer, so a burst of
// logs cannot delay metrics. See BatchWriter for the full reasoning.
const (
	MetricsWriter   = gen.Atom("storage_metrics")
	SketchesWriter  = gen.Atom("storage_sketches")
	ChecksWriter    = gen.Atom("storage_checks")
	LogsWriter      = gen.Atom("storage_logs")
	HostsWriter     = gen.Atom("storage_hosts")
	ProcessesWriter = gen.Atom("storage_processes")
	EventsWriter    = gen.Atom("storage_events")
)

// writers describes every table we write to. Thresholds differ by expected
// volume: logs arrive far more often than metrics, host metadata barely at all.
var writers = []struct {
	name gen.Atom
	cfg  WriterConfig
}{
	{MetricsWriter, WriterConfig{
		Name: "metrics",
		Insert: `INSERT INTO metrics
			(tenant_id, timestamp, metric, host, metric_type, source_type, unit, interval, value, tags)`,
		MaxRows: 5000, FlushInterval: 2 * time.Second,
	}},
	{SketchesWriter, WriterConfig{
		Name: "sketches",
		Insert: `INSERT INTO sketches
			(tenant_id, timestamp, metric, host, tags, count, min, max, avg, sum, bucket_keys, bucket_counts)`,
		MaxRows: 1000, FlushInterval: 5 * time.Second,
	}},
	{ChecksWriter, WriterConfig{
		Name: "check_runs",
		Insert: `INSERT INTO check_runs
			(tenant_id, timestamp, check_name, host, status, message, tags)`,
		// Check runs trickle in, so a small threshold and a longer timer.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}},
	{LogsWriter, WriterConfig{
		Name: "logs",
		Insert: `INSERT INTO logs
			(tenant_id, timestamp, host, service, source, status, message, tags)`,
		// Logs are the high-volume table: bigger batches, bigger safety margin.
		MaxRows: 20000, FlushInterval: time.Second, BufferLimit: 500_000,
	}},
	{ProcessesWriter, WriterConfig{
		Name: "processes",
		Insert: `INSERT INTO processes
			(tenant_id, timestamp, host, pid, ppid, user, comm, exe, cmdline,
			 rss, vms, cpu_pct, threads, open_fds, state, create_time, container_id, tags)`,
		// A few hundred rows per host per snapshot, every ~10s. Large batches,
		// moderate interval.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}},
	{EventsWriter, WriterConfig{
		Name: "events",
		Insert: `INSERT INTO events
			(tenant_id, timestamp, event_id, event_id_num, title, text, host,
			 alert_type, priority, aggregation_key, source_type_name, device_name, tags)`,
		// Human and system scale, not machine scale: tens or hundreds a day.
		// Small batches, and a timer short enough that a deploy marker shows
		// up on the chart while someone is still looking at it.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}},
	{HostsWriter, WriterConfig{
		Name: "hosts",
		Insert: `INSERT INTO hosts
			(tenant_id, host, seen_at, agent_version, os, platform, cpu, memory, tags)`,
		// One row per host every ~20s. ReplacingMergeTree collapses duplicates.
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}},
}

// App owns writing to ClickHouse.
//
//	storage (application)
//	  └── storage_sup (supervisor, one_for_one)
//	        ├── storage_metrics
//	        ├── storage_sketches
//	        ├── storage_checks
//	        ├── storage_logs
//	        ├── storage_processes
//	        └── storage_hosts
//
// one_for_one matters here: a writer that dies is restarted alone, and the
// other tables keep accepting data.
//
// A separate application because it is a separate ROLE. The goal is one binary
// that can run in different layouts — everything together on a small install,
// ingest and storage split apart when scaling out.
type App struct {
	app.Application
	conn driver.Conn
}

func CreateApp() gen.ApplicationBehavior {
	return &App{}
}

func (a *App) Load(args ...any) (gen.ApplicationSpec, error) {
	spec := gen.ApplicationSpec{
		Name:        AppName,
		Description: "Telemetry storage in ClickHouse",
		Mode:        gen.ApplicationModePermanent,
		Group: []gen.ApplicationMemberSpec{
			{
				Name:    SupervisorName,
				Factory: func() gen.ProcessBehavior { return &Supervisor{app: a} },
			},
		},
	}
	return a.Tune(spec, args...)
}

func (a *App) Tune(spec gen.ApplicationSpec, args ...any) (gen.ApplicationSpec, error) {
	return spec, nil
}

// Init opens the ClickHouse connection once for the whole application.
//
// The driver pools connections internally and is safe for concurrent use, so
// all writers share it. Opening one per writer would multiply connections for
// no benefit.
//
// Failing here aborts application start, which is what we want: without a
// database there is nothing to write to.
func (a *App) Init(ref gen.Ref, mode gen.ApplicationMode) error {
	addr := envOr("CLICKHOUSE_ADDR", "localhost:9000")

	conn, err := clickhouse.Open(&clickhouse.Options{
		Addr: strings.Split(addr, ","),
		Auth: clickhouse.Auth{
			Database: envOr("CLICKHOUSE_DB", "ninjacat"),
			Username: envOr("CLICKHOUSE_USER", "ninjacat"),
			Password: envOr("CLICKHOUSE_PASSWORD", "ninjacat"),
		},
		// Compress on the way in too: fewer bytes over the wire per insert.
		Compression: &clickhouse.Compression{Method: clickhouse.CompressionLZ4},
		DialTimeout: 5 * time.Second,
	})
	if err != nil {
		return fmt.Errorf("storage: connect: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := conn.Ping(ctx); err != nil {
		conn.Close()
		return fmt.Errorf("storage: clickhouse unreachable at %s: %w", addr, err)
	}

	a.conn = conn
	return nil
}

func (a *App) Terminate(reason error) {
	if a.conn != nil {
		a.conn.Close()
	}
}

type Supervisor struct {
	act.Supervisor
	app *App
}

func (sup *Supervisor) Init(args ...any) (act.SupervisorSpec, error) {
	children := make([]act.SupervisorChildSpec, 0, len(writers))
	for _, w := range writers {
		children = append(children, act.SupervisorChildSpec{
			Name:    w.name,
			Factory: func() gen.ProcessBehavior { return &BatchWriter{} },
			Args:    []any{w.cfg, sup.app.conn},
		})
	}

	return act.SupervisorSpec{
		Type: act.SupervisorTypeOneForOne,
		Restart: act.SupervisorRestart{
			Strategy:  act.SupervisorStrategyPermanent,
			Intensity: 3,
			Period:    10,
		},
		Children: children,
	}, nil
}

func (sup *Supervisor) HandleMessage(from gen.PID, message any) error { return nil }
func (sup *Supervisor) Terminate(reason error)                        {}

func envOr(name, fallback string) string {
	if v := os.Getenv(name); v != "" {
		return v
	}
	return fallback
}
