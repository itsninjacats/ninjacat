package httpapi

import (
	"context"
	"errors"
	"net/http"
	"time"

	"ergo.services/ergo/gen"
)

// httpServer is an HTTP server running as an Ergo META-PROCESS.
//
// Why a meta-process and not a plain actor:
//
// An actor handles one message at a time and its loop must never block. An
// HTTP server blocks by nature — ListenAndServe sits there until something
// shuts it down. Putting that inside an actor's Start() would freeze its
// mailbox for as long as the server runs.
//
// A meta-process exists for exactly this: its Start() is ALLOWED to block.
// It belongs to a parent process and is supervised by it, but it runs on its
// own goroutine.
//
// The contract (gen.MetaBehavior):
//
//	Init          hands you a handle to the process (Send, SendAfter, Log...)
//	Start         the blocking loop — the server lives HERE
//	HandleMessage messages from the parent
//	HandleCall    synchronous questions
//	Terminate     cleanup; this is what unblocks Start
//	HandleInspect state for tooling such as the observer
type httpServer struct {
	name string
	addr string
	// The handler is wired in Init rather than at construction, so a
	// restarted server comes back with clean configuration.
	handler http.Handler

	srv       *http.Server
	proc      gen.MetaProcess
	startedAt time.Time
}

func (m *httpServer) Init(procs gen.MetaProcess) error {
	m.proc = procs
	m.srv = &http.Server{
		Addr:              m.addr,
		Handler:           m.handler,
		ReadHeaderTimeout: 10 * time.Second,
	}
	return nil
}

// Start blocks. That is the whole point of a meta-process — here it is allowed.
func (m *httpServer) Start() error {
	m.startedAt = time.Now()
	m.proc.Log().Info("%s listening on %s", m.name, m.addr)

	err := m.srv.ListenAndServe()
	if errors.Is(err, http.ErrServerClosed) {
		// A requested shutdown is not a failure.
		return nil
	}
	return err
}

func (m *httpServer) HandleMessage(from gen.PID, message any) error { return nil }

func (m *httpServer) HandleCall(from gen.PID, ref gen.Ref, request any) (any, error) {
	return map[string]any{"name": m.name, "addr": m.addr, "since": m.startedAt}, nil
}

// Terminate shuts the server down, which makes ListenAndServe return and
// unblocks Start.
func (m *httpServer) Terminate(reason error) {
	if m.srv == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = m.srv.Shutdown(ctx)
}

func (m *httpServer) HandleInspect(from gen.PID, item ...string) map[string]string {
	return map[string]string{
		"name":   m.name,
		"addr":   m.addr,
		"uptime": time.Since(m.startedAt).Round(time.Second).String(),
	}
}
