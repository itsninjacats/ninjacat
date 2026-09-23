package httpapi

import (
	"crypto/tls"
	"errors"
	"fmt"
	"net"
	"time"

	"ergo.services/ergo/gen"
)

// TCPServer is anything that can accept connections on a listener until
// Close stops it — the non-HTTP analogue of http.Handler's role in Config
// above. intake.TCPLogsServer implements it; this package knows nothing
// else about what happens to a connection, the same way it knows nothing
// about Gin for the HTTP side.
type TCPServer interface {
	Serve(ln net.Listener) error
	Close() error
}

// tcpMeta runs a TCPServer as an Ergo META-PROCESS, for the same reason
// httpServer does (see meta.go's doc comment): Serve blocks by design, and
// an actor's loop must never block — a meta-process is the escape hatch,
// supervised like any other child but free to run its own goroutine.
//
// TLS is opt-in: NINJACAT_TLS_CERT and NINJACAT_TLS_KEY, read once in
// cmd/ninjacat/main.go and handed down as file paths through Config. Neither
// set means a plain listener, which is what the agent needs
// logs_config.logs_no_ssl: true for — its TCP client defaults to TLS on
// port 10516 otherwise (docs/tables/logs.md "TCP transport").
type tcpMeta struct {
	name     string
	addr     string
	certFile string
	keyFile  string
	// server is wired in at construction, unlike httpServer's handler
	// (wired in Init) — a TCPServer already IS the fully-built listener
	// target (it owns the intake.Server it feeds), so there is nothing
	// left to configure freshly on a restart.
	server TCPServer

	ln        net.Listener
	proc      gen.MetaProcess
	startedAt time.Time
}

func (m *tcpMeta) Init(procs gen.MetaProcess) error {
	m.proc = procs
	return nil
}

// Start blocks — that is the whole point of a meta-process, see meta.go.
func (m *tcpMeta) Start() error {
	ln, err := m.listen()
	if err != nil {
		return fmt.Errorf("%s: listen %s: %w", m.name, m.addr, err)
	}
	m.ln = ln
	m.startedAt = time.Now()

	mode := "plain"
	if m.certFile != "" {
		mode = "TLS"
	}
	m.proc.Log().Info("%s listening on %s (%s)", m.name, m.addr, mode)

	err = m.server.Serve(ln)
	if err != nil && !errors.Is(err, net.ErrClosed) {
		return err
	}
	// A requested shutdown is not a failure — mirrors httpServer.Start's
	// handling of http.ErrServerClosed.
	return nil
}

// listen builds a plain or TLS listener depending on whether certFile/keyFile
// were set. Kept separate from Start so the two failure modes (bad address,
// bad certificate) surface as distinctly wrapped errors.
func (m *tcpMeta) listen() (net.Listener, error) {
	if m.certFile == "" && m.keyFile == "" {
		return net.Listen("tcp", m.addr)
	}
	cert, err := tls.LoadX509KeyPair(m.certFile, m.keyFile)
	if err != nil {
		return nil, fmt.Errorf("load TLS keypair (%s, %s): %w", m.certFile, m.keyFile, err)
	}
	return tls.Listen("tcp", m.addr, &tls.Config{Certificates: []tls.Certificate{cert}})
}

func (m *tcpMeta) HandleMessage(from gen.PID, message any) error { return nil }

func (m *tcpMeta) HandleCall(from gen.PID, ref gen.Ref, request any) (any, error) {
	return map[string]any{"name": m.name, "addr": m.addr, "since": m.startedAt}, nil
}

// Terminate stops the TCPServer, which makes Serve return and unblocks
// Start — mirrors httpServer.Terminate's use of http.Server.Shutdown.
func (m *tcpMeta) Terminate(reason error) {
	if m.server == nil {
		return
	}
	_ = m.server.Close()
}

func (m *tcpMeta) HandleInspect(from gen.PID, item ...string) map[string]string {
	return map[string]string{
		"name":   m.name,
		"addr":   m.addr,
		"uptime": time.Since(m.startedAt).Round(time.Second).String(),
	}
}
