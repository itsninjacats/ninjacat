package query

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// Names under which these processes register in the node.
//
// PoolName is the address the panel API sends its questions to. It is a
// registered name and not a PID, so once ingest and querying live on separate
// nodes the caller does not change — only where the name resolves does.
const (
	AppName        = gen.Atom("query")
	SupervisorName = gen.Atom("query_sup")
	PoolName       = gen.Atom("query_pool")
)

// defaultPoolSize is how many queries can run at once.
//
// Four is chosen against the panel, not against ClickHouse: a dashboard opens
// a handful of panels at once and each is one query. Raising it lets more run
// concurrently; it does not make any single query faster.
const defaultPoolSize = 4

// App answers questions about stored telemetry.
//
//	query (application)
//	  └── query_sup (supervisor, one_for_one)
//	        └── query_pool (pool of Workers)
//
// Separate from storage on purpose, even though both talk to ClickHouse.
// Writing and reading fail differently and scale differently: a slow query
// must never be able to stall ingest, and the two will eventually want to run
// on different nodes. Sharing an application would make that split a rewrite.
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
		Description: "Telemetry queries against ClickHouse",
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

// Init opens one ClickHouse connection for every worker to share.
//
// The driver pools underneath and is safe for concurrent use, so one handle
// serves the whole pool. MaxOpenConns is set to the pool size for a reason:
// without it, four workers could open far more sockets than there are workers
// to use them.
func (a *App) Init(ref gen.Ref, mode gen.ApplicationMode) error {
	addr := envOr("CLICKHOUSE_ADDR", "localhost:9000")

	conn, err := clickhouse.Open(&clickhouse.Options{
		Addr: strings.Split(addr, ","),
		Auth: clickhouse.Auth{
			Database: envOr("CLICKHOUSE_DB", "ninjacat"),
			Username: envOr("CLICKHOUSE_USER", "ninjacat"),
			Password: envOr("CLICKHOUSE_PASSWORD", "ninjacat"),
		},
		Compression:  &clickhouse.Compression{Method: clickhouse.CompressionLZ4},
		MaxOpenConns: poolSize(),
	})
	if err != nil {
		return fmt.Errorf("query: clickhouse: %w", err)
	}
	a.conn = conn
	return nil
}

func (a *App) Terminate(reason error) {
	if a.conn != nil {
		_ = a.conn.Close()
	}
}

// Supervisor watches the pool.
//
// Permanent: without it the panel shows nothing at all. If the pool cannot
// stay up, that is worth escalating rather than papering over.
type Supervisor struct {
	act.Supervisor
	app *App
}

func (sup *Supervisor) Init(args ...any) (act.SupervisorSpec, error) {
	return act.SupervisorSpec{
		Type: act.SupervisorTypeOneForOne,
		Restart: act.SupervisorRestart{
			Strategy:  act.SupervisorStrategyPermanent,
			Intensity: 3,
			Period:    10,
		},
		Children: []act.SupervisorChildSpec{
			{
				Name:    PoolName,
				Factory: func() gen.ProcessBehavior { return &Pool{conn: sup.app.conn} },
			},
		},
	}, nil
}

func (sup *Supervisor) HandleMessage(from gen.PID, message any) error { return nil }
func (sup *Supervisor) Terminate(reason error)                        {}

// Pool distributes incoming queries across Workers.
//
// act.Pool forwards both sends and calls round-robin, skipping workers whose
// mailbox is full and respawning any that died. For a Call that matters: the
// worker replies straight to the original caller, so the pool is never in the
// path of the answer.
//
// If every mailbox is full the request is dropped and the caller's Call times
// out. That is the right failure for a query — better a panel that reports a
// timeout than one that waits forever.
type Pool struct {
	act.Pool
	conn driver.Conn
}

func (p *Pool) Init(args ...any) (act.PoolOptions, error) {
	if p.conn == nil {
		return act.PoolOptions{}, fmt.Errorf("query pool: no ClickHouse connection")
	}
	p.Log().Info("query pool started with %d workers", poolSize())
	return act.PoolOptions{
		PoolSize:          int64(poolSize()),
		WorkerFactory:     func() gen.ProcessBehavior { return &Worker{} },
		WorkerArgs:        []any{p.conn},
		WorkerMailboxSize: 32,
	}, nil
}

func poolSize() int {
	if v := os.Getenv("NINJACAT_QUERY_POOL"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 && n <= 64 {
			return n
		}
	}
	return defaultPoolSize
}

func envOr(name, fallback string) string {
	if v := os.Getenv(name); v != "" {
		return v
	}
	return fallback
}
