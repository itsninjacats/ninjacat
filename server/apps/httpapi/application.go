package httpapi

import (
	"fmt"
	"net/http"

	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
)

const (
	AppName        = gen.Atom("httpapi")
	SupervisorName = gen.Atom("httpapi_sup")
	GatewayName    = gen.Atom("httpapi_gateway")
)

// Config is everything the servers need. The caller builds the handlers, so
// this application knows nothing about Gin or about Datadog.
type Config struct {
	IntakeAddr    string
	IntakeHandler http.Handler

	InternalAddr    string
	InternalHandler http.Handler
}

// App exposes both HTTP servers as supervised meta-processes.
//
//	httpapi (application)
//	  └── httpapi_sup (supervisor, one_for_one)
//	        └── httpapi_gateway (actor)
//	              ├── meta: agent intake   (:8080)
//	              └── meta: panel API      (:8081)
type App struct {
	app.Application
	cfg Config
}

func CreateApp(cfg Config) gen.ApplicationBehavior {
	return &App{cfg: cfg}
}

func (a *App) Load(args ...any) (gen.ApplicationSpec, error) {
	spec := gen.ApplicationSpec{
		Name:        AppName,
		Description: "HTTP servers: agent intake and internal API",
		Mode:        gen.ApplicationModePermanent,
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

type Supervisor struct {
	act.Supervisor
	cfg Config
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
				Name:    GatewayName,
				Factory: func() gen.ProcessBehavior { return &Gateway{} },
				Args:    []any{sup.cfg},
			},
		},
	}, nil
}

func (sup *Supervisor) HandleMessage(from gen.PID, message any) error { return nil }
func (sup *Supervisor) Terminate(reason error)                        {}

// Gateway is the actor that parents both meta-processes.
//
// It runs no loop of its own — that is what the meta-processes are for. Its
// job is to spawn them and to be the thing the supervisor restarts if one of
// them dies. Restarting the gateway restarts both servers.
type Gateway struct {
	act.Actor
	cfg Config
}

func (b *Gateway) Init(args ...any) error {
	if len(args) == 0 {
		return fmt.Errorf("gateway: missing Config in args")
	}
	cfg, ok := args[0].(Config)
	if !ok {
		return fmt.Errorf("gateway: first arg must be Config, got %T", args[0])
	}
	b.cfg = cfg

	if cfg.IntakeHandler != nil {
		if _, err := b.SpawnMeta(
			&httpServer{name: "agent intake", addr: cfg.IntakeAddr, handler: cfg.IntakeHandler},
			gen.MetaOptions{},
		); err != nil {
			return fmt.Errorf("gateway: intake server: %w", err)
		}
	}

	if cfg.InternalHandler != nil {
		if _, err := b.SpawnMeta(
			&httpServer{name: "internal API", addr: cfg.InternalAddr, handler: cfg.InternalHandler},
			gen.MetaOptions{},
		); err != nil {
			return fmt.Errorf("gateway: panel API server: %w", err)
		}
	}

	return nil
}

func (b *Gateway) HandleMessage(from gen.PID, message any) error { return nil }
func (b *Gateway) Terminate(reason error)                        {}
