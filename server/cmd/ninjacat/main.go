package main

import (
	"log"
	"os"

	"ergo.services/application/observer"
	"ergo.services/ergo"
	"ergo.services/ergo/gen"
	"github.com/itsninjacats/server/apps/apikeys"
	"github.com/itsninjacats/server/apps/httpapi"
	"github.com/itsninjacats/server/apps/query"
	"github.com/itsninjacats/server/apps/selfmon"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/intake"
	"github.com/itsninjacats/server/panelapi"
)

func envOr(name, fallback string) string {
	if v := os.Getenv(name); v != "" {
		return v
	}
	return fallback
}

func main() {
	// The key store is built BEFORE the node, so the HTTP middleware and the
	// actor hold the same pointer. The actor writes to it, the middleware
	// reads from it, without locking — see apps/apikeys/store.go.
	apiKeysApp, keyStore := apikeys.CreateApp()

	intakeSrv := &intake.Server{Store: keyStore}
	panelSrv := &panelapi.Server{Store: keyStore}

	// Everything enters the node as an APPLICATION, which puts every piece
	// under supervision and makes it visible in the observer. Nothing lives
	// beside the tree.
	node, err := ergo.StartNode("ninjacat@localhost", gen.NodeOptions{
		Applications: []gen.ApplicationBehavior{
			apiKeysApp,
			storage.CreateApp(),
			query.CreateApp(),
			httpapi.CreateApp(httpapi.Config{
				IntakeAddr:      envOr("NINJACAT_ADDR", ":8080"),
				IntakeHandler:   intakeSrv.Handler(),
				InternalAddr:    envOr("NINJACAT_INTERNAL_ADDR", ":8081"),
				InternalHandler: panelSrv.Handler(),
			}),
			// selfmon comes after storage: its only dependency is the
			// metrics writer, which it reaches by message.
			selfmon.CreateApp(selfmon.Config{}),
			observer.CreateApp(observer.Options{
				Host: envOr("NINJACAT_OBSERVER_HOST", "localhost"),
				Port: 9911,
			}),
		},
	})
	if err != nil {
		log.Fatalf("cannot start node: %s", err)
	}

	// Message types must be registered for REMOTE delivery. Locally they
	// travel as `any` with no encoding, so this changes nothing today — but
	// the moment roles are split across nodes, an unregistered type makes
	// Send fail. Registering now costs nothing and avoids a confusing
	// failure later.
	for _, register := range []func(gen.Node) error{
		apikeys.RegisterTypes,
		storage.RegisterTypes,
		query.RegisterTypes,
		selfmon.RegisterTypes,
	} {
		if err := register(node); err != nil {
			log.Fatalf("%s", err)
		}
	}

	// The servers need the node in order to message the keeper (for example
	// when the panel asks for a key refresh). We set it after startup,
	// because the node does not exist before that.
	intakeSrv.Node = node
	panelSrv.Node = node

	log.Printf("ninjacat is up")
	log.Printf("  agent intake : %s", envOr("NINJACAT_ADDR", ":8080"))
	log.Printf("  panel API    : %s", envOr("NINJACAT_INTERNAL_ADDR", ":8081"))
	log.Printf("  observer     : http://localhost:9911")

	// We block on the node, not on an HTTP server. The servers live as
	// supervised meta-processes, and they are the ones allowed to die and
	// be restarted.
	node.Wait()
}
