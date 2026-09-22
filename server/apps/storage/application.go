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
//
// The NAMES stay here because the intake handlers address them and a constant
// two packages reference belongs somewhere neutral. Each writer's CONFIG —
// its table, INSERT and batching — lives in the rows_*.go file that owns the
// table and registers it (registry.go).
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

// App owns writing to ClickHouse.
//
//	storage (application)
//	  └── storage_sup (supervisor, one_for_one)
//	        ├── storage_checks             rows_checks.go
//	        ├── storage_container_events   rows_containers.go
//	        ├── storage_container_images   rows_containers.go
//	        ├── storage_events             rows_events.go
//	        ├── storage_hosts              rows_hosts.go
//	        ├── storage_k8s_resources      rows_k8s.go
//	        ├── storage_k8s_manifests      rows_k8s.go
//	        ├── storage_k8s_cluster        rows_k8s.go
//	        ├── storage_k8s_actions        rows_k8s.go
//	        ├── storage_logs               rows_logs.go
//	        ├── storage_metrics            rows_metrics.go
//	        ├── storage_sketches           rows_metrics.go
//	        ├── storage_processes          rows_process.go
//	        └── storage_raw_payloads       rows_raw.go
//
// There is no list of children anywhere: the supervisor asks Writers(), which
// is built by the init() in each of those files. The order above is therefore
// filename order, and it is cosmetic — one_for_one children neither start in a
// meaningful sequence nor depend on each other.
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
	// Writers() is whatever the rows_*.go files registered from init() — see
	// registry.go. A new table gets a writer here without this function being
	// touched, and without any shared list to conflict over.
	specs := Writers()
	children := make([]act.SupervisorChildSpec, 0, len(specs))
	for _, w := range specs {
		children = append(children, act.SupervisorChildSpec{
			Name:    w.Name,
			Factory: func() gen.ProcessBehavior { return &BatchWriter{} },
			Args:    []any{w.Config, sup.app.conn},
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
