package selfmon

import (
	"os"
	"strconv"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
)

// Names under which these processes register in the node.
//
// CollectorName is a registered name, not a PID, so anything on the node —
// or on another node once roles are split — can reach it by address without
// having been handed a reference first.
const (
	AppName        = gen.Atom("selfmon")
	SupervisorName = gen.Atom("selfmon_sup")
	CollectorName  = gen.Atom("selfmon_collector")
)

// defaultInterval matches what a Datadog agent uses for its own checks.
// Short enough to see a leak developing, long enough not to be noise.
const defaultInterval = 15 * time.Second

// Config is what the collector needs to know.
type Config struct {
	// Interval between samples.
	Interval time.Duration

	// TenantID the metrics are written under.
	//
	// Self-monitoring belongs to whoever runs ninjacat, not to a customer,
	// so in a real multi-tenant deployment this should point at an operator
	// tenant rather than a shared one. It defaults to "default" because that
	// is the only tenant that exists today.
	TenantID string

	// Host reported with the metrics. Empty means "use the OS hostname".
	Host string
}

// App is ninjacat monitoring itself.
//
// Tree:
//
//	selfmon (application)
//	  └── selfmon_sup (supervisor, one_for_one)
//	        └── selfmon_collector (actor — timer + node sampling)
//
// It is a separate application rather than a process inside storage or
// httpapi on purpose: self-monitoring is not required for ingest to work, so
// it must be possible to lose it, restart it, or leave it out of a node's
// application list entirely without touching anything else.
type App struct {
	app.Application
	cfg Config
}

// CreateApp builds the application. Defaults come from the environment so a
// deployment can retune sampling without a rebuild.
func CreateApp(cfg Config) gen.ApplicationBehavior {
	if cfg.Interval <= 0 {
		cfg.Interval = envDuration("NINJACAT_SELFMON_INTERVAL", defaultInterval)
	}
	if cfg.TenantID == "" {
		cfg.TenantID = envOr("NINJACAT_SELFMON_TENANT", "default")
	}
	if cfg.Host == "" {
		cfg.Host = os.Getenv("NINJACAT_SELFMON_HOST")
	}
	return &App{cfg: cfg}
}

func (a *App) Load(args ...any) (gen.ApplicationSpec, error) {
	spec := gen.ApplicationSpec{
		Name:        AppName,
		Description: "ninjacat node self-monitoring",
		Mode:        gen.ApplicationModeTemporary,
		Group: []gen.ApplicationMemberSpec{
			{
				Name:    SupervisorName,
				Factory: func() gen.ProcessBehavior { return &Supervisor{cfg: a.cfg} },
			},
		},
	}
	return a.Tune(spec, args...)
}

func (a *App) Tune(spec gen.ApplicationSpec, args ...any) (gen.ApplicationSpec, error) {
	return spec, nil
}

// Supervisor watches the collector.
//
// Transient, not permanent, and this is the one real design decision here:
// if sampling the node keeps crashing, the node itself must stay up. Ingest
// does not depend on telemetry about ingest. The supervisor retries a few
// times and then gives up quietly, leaving the rest of the tree alone.
type Supervisor struct {
	act.Supervisor
	cfg Config
}

func (sup *Supervisor) Init(args ...any) (act.SupervisorSpec, error) {
	spec := act.SupervisorSpec{
		Type: act.SupervisorTypeOneForOne,
		Restart: act.SupervisorRestart{
			Strategy:  act.SupervisorStrategyTransient,
			Intensity: 3,
			Period:    30,
		},
		Children: []act.SupervisorChildSpec{
			{
				Name:    CollectorName,
				Factory: func() gen.ProcessBehavior { return &Collector{} },
				Args:    []any{sup.cfg},
			},
		},
	}
	return spec, nil
}

func (sup *Supervisor) HandleMessage(from gen.PID, message any) error { return nil }
func (sup *Supervisor) Terminate(reason error)                        {}

func envOr(name, fallback string) string {
	if v := os.Getenv(name); v != "" {
		return v
	}
	return fallback
}

func envDuration(name string, fallback time.Duration) time.Duration {
	v := os.Getenv(name)
	if v == "" {
		return fallback
	}
	if d, err := time.ParseDuration(v); err == nil && d > 0 {
		return d
	}
	// Bare number means seconds, which is what people type.
	if n, err := strconv.Atoi(v); err == nil && n > 0 {
		return time.Duration(n) * time.Second
	}
	return fallback
}
