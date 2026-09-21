package storage

import (
	"context"
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/itsninjacats/server/schema"
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

	K8sResourcesWriter    = gen.Atom("storage_k8s_resources")
	K8sManifestsWriter    = gen.Atom("storage_k8s_manifests")
	K8sClusterWriter      = gen.Atom("storage_k8s_cluster")
	K8sActionsWriter      = gen.Atom("storage_k8s_actions")
	ContainerEventsWriter = gen.Atom("storage_container_events")
	ContainerImagesWriter = gen.Atom("storage_container_images")
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
	{K8sResourcesWriter, WriterConfig{
		Name: "k8s_resources",
		Insert: `INSERT INTO k8s_resources
			(tenant_id, collected_at, cluster_id, cluster_name, kind, namespace, name,
			 uid, resource_version, owner_kind, owner_name, node_name, phase, status,
			 ready, desired, available, counts, labels, annotations, tags,
			 agent_version, group_id, group_size)`,
		// The cluster agent sends whole collection passes at once: thousands of
		// objects in a burst every ~10s, then silence. MaxRows keeps a burst
		// from becoming one giant insert, the buffer holds a few full passes
		// while ClickHouse hiccups, and a second in-flight flush lets the next
		// pass start draining before the previous insert lands. Rows carry
		// label and annotation maps, so 50k of them is real memory — hence an
		// explicit ceiling instead of the 200k default.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}},
	{K8sManifestsWriter, WriterConfig{
		Name: "k8s_manifests",
		Insert: `INSERT INTO k8s_manifests
			(tenant_id, collected_at, cluster_id, cluster_name, uid, kind, api_version,
			 resource_version, content, content_type, is_terminated)`,
		// Every row is a whole YAML document, so these row counts stand for
		// megabytes: 200 rows can be 2 MB of insert and 5000 buffered can be
		// tens of MB. The tight ceilings bound memory, and a single in-flight
		// flush is plenty for what is bulk, not urgency.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}},
	{K8sClusterWriter, WriterConfig{
		Name: "k8s_cluster",
		Insert: `INSERT INTO k8s_cluster
			(tenant_id, collected_at, cluster_id, cluster_name, node_count,
			 pod_capacity, pod_allocatable, cpu_capacity, cpu_allocatable,
			 memory_capacity, memory_allocatable, kubelet_versions, apiserver_versions)`,
		// One row per cluster per pass — the quietest table here. The timer
		// does all the flushing; the thresholds only exist so a stall cannot
		// grow the buffer unbounded.
		MaxRows: 50, FlushInterval: 15 * time.Second,
		BufferLimit: 1_000, MaxInFlight: 1,
	}},
	{K8sActionsWriter, WriterConfig{
		Name: "k8s_actions",
		Insert: `INSERT INTO k8s_actions
			(tenant_id, timestamp, action_id, org_id, event_type, status, action_type,
			 cluster_id, cluster_name, resource_id, resource_kind, resource_name,
			 resource_namespace, requested_by, message, extra_keys)`,
		// Human scale, like the events table: an operator triggers an action
		// and expects to see its result — a 10s timer is about as long as
		// that wait should get. Small everything, one flush at a time.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 10_000, MaxInFlight: 1,
	}},
	{ContainerEventsWriter, WriterConfig{
		Name: "container_events",
		Insert: `INSERT INTO container_events
			(tenant_id, timestamp, host, cluster_id, object_kind, event_type,
			 container_id, container_name, pod_uid, task_arn, source,
			 exit_code, created_at, exited_at, owner_type, owner_uid,
			 old_state, new_state, transition_at)`,
		// Every node agent reports every restart and OOM in the fleet, and a
		// bad rollout turns that into a storm — the second in-flight flush is
		// for exactly that day. This is also the table that must not drop
		// rows lightly (each one is a crash somebody will look for), so the
		// buffer is the deepest of the new set.
		MaxRows: 1000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}},
	{ContainerImagesWriter, WriterConfig{
		Name: "container_images",
		Insert: `INSERT INTO container_images
			(tenant_id, collected_at, host, image_key, identity_source,
			 image_id, digest, name, short_name,
			 registry, repo_tags, repo_digests, size_bytes, os_name, os_version,
			 architecture, layer_count, layer_bytes, built_at, published_at, dd_tags)`,
		// Periodic full inventories: every node re-announces every image it
		// holds, so arrivals are bursty and repetitive. ReplacingMergeTree
		// absorbs the repetition; here we just batch the bursts, with a second
		// flush in flight for when many nodes report at once.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
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
//	        ├── storage_events
//	        ├── storage_hosts
//	        ├── storage_k8s_resources
//	        ├── storage_k8s_manifests
//	        ├── storage_k8s_cluster
//	        ├── storage_k8s_actions
//	        ├── storage_container_events
//	        └── storage_container_images
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

// Connect opens a ClickHouse connection from the CLICKHOUSE_* environment
// and verifies it with a Ping. Shared between the application's Init and the
// `ninjacat migrate` subcommand, so both dial exactly the same way.
func Connect() (driver.Conn, error) {
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
		return nil, fmt.Errorf("storage: connect: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := conn.Ping(ctx); err != nil {
		conn.Close()
		return nil, fmt.Errorf("storage: clickhouse unreachable at %s: %w", addr, err)
	}
	return conn, nil
}

// Init opens the ClickHouse connection once for the whole application and
// brings the schema up to date.
//
// The driver pools connections internally and is safe for concurrent use, so
// all writers share it. Opening one per writer would multiply connections for
// no benefit.
//
// Failing here aborts application start, which is what we want: without a
// database there is nothing to write to, and without the schema every insert
// would fail anyway.
func (a *App) Init(ref gen.Ref, mode gen.ApplicationMode) error {
	conn, err := Connect()
	if err != nil {
		return err
	}

	// Migrate on startup unless explicitly disabled.
	//
	// HAZARD, stated honestly: with more than one server replica, every
	// replica races to apply migrations at boot — there is no lock. Today
	// that is harmless: we run a single instance, and every statement is
	// CREATE ... IF NOT EXISTS, so the losers of the race no-op. Under
	// Kubernetes with replicas > 1 it stops being fine: set
	// NINJACAT_AUTO_MIGRATE=false in the deployment and run
	// `ninjacat migrate` as a pre-upgrade Job instead — the same shape the
	// frontend already uses for Postgres (lab/k8s/manifests/22-migrate-job.yaml,
	// `node migrate.js` from the serving image).
	if envOr("NINJACAT_AUTO_MIGRATE", "true") != "false" {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
		defer cancel()
		n, err := schema.Apply(ctx, conn)
		if err != nil {
			conn.Close()
			return fmt.Errorf("storage: %w", err)
		}
		if n > 0 {
			log.Printf("storage: applied %d schema migration(s)", n)
		}
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
