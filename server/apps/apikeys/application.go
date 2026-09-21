package apikeys

import (
	"ergo.services/ergo/act"
	"ergo.services/ergo/app"
	"ergo.services/ergo/gen"
)

// Names under which these processes register in the node.
const (
	AppName        = gen.Atom("apikeys")
	SupervisorName = gen.Atom("apikeys_sup")
	KeeperName     = gen.Atom("apikeys_keeper")
)

// App holds the API keys in memory.
//
// Tree:
//
//	apikeys (application)
//	  └── apikeys_sup (supervisor, one_for_one)
//	        └── apikeys_keeper (actor — Postgres pool + periodic refresh)
//
// Store is shared: the application creates it, the keeper writes to it and
// the HTTP layer reads from it. It is the one place where memory is shared,
// which is exactly why what it holds is immutable — see store.go.
type App struct {
	app.Application
	store *Store
}

// CreateApp builds the application and hands back the Store alongside it, so
// the HTTP middleware can be wired to it before the node even starts.
func CreateApp() (gen.ApplicationBehavior, *Store) {
	store := NewStore()
	return &App{store: store}, store
}

func (a *App) Load(args ...any) (gen.ApplicationSpec, error) {
	spec := gen.ApplicationSpec{
		Name:        AppName,
		Description: "Agent API keys held in memory",
		Mode:        gen.ApplicationModePermanent,
		Group: []gen.ApplicationMemberSpec{
			{
				Name:    SupervisorName,
				Factory: func() gen.ProcessBehavior { return &Supervisor{store: a.store} },
			},
		},
	}
	return a.Tune(spec, args...)
}

func (a *App) Tune(spec gen.ApplicationSpec, args ...any) (gen.ApplicationSpec, error) {
	return spec, nil
}

// Supervisor watches the keeper. one_for_one, because there is one child.
//
// Permanent strategy: the keeper must always be alive. If it dies — say the
// database disappears during the first fetch — the supervisor restarts it.
// After three attempts within 10 seconds it gives up and escalates, because
// by then the problem is not a blip.
type Supervisor struct {
	act.Supervisor
	store *Store
}

func (sup *Supervisor) Init(args ...any) (act.SupervisorSpec, error) {
	spec := act.SupervisorSpec{
		Type: act.SupervisorTypeOneForOne,
		Restart: act.SupervisorRestart{
			Strategy:  act.SupervisorStrategyPermanent,
			Intensity: 3,
			Period:    10,
		},
		Children: []act.SupervisorChildSpec{
			{
				Name:    KeeperName,
				Factory: func() gen.ProcessBehavior { return &Keeper{} },
				Args:    []any{sup.store},
			},
		},
	}
	return spec, nil
}

func (sup *Supervisor) HandleMessage(from gen.PID, message any) error { return nil }
func (sup *Supervisor) Terminate(reason error)                        {}
