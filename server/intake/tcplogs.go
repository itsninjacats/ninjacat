package intake

import (
	"bufio"
	"bytes"
	"errors"
	"log"
	"net"
	"sync"
	"time"

	"github.com/itsninjacats/server/apps/storage"
)

// agent-intake.logs.<site> — the Datadog Agent's legacy TCP transport for
// logs.
//
//	config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL, with
//	        logs_config.use_http NOT forced true and HTTPS unreachable at
//	        agent startup (see the force_use_http doc in the vendored
//	        pkg/config/config_template.yaml, quoted below).
//
// This is a raw TCP(+TLS) byte stream, not HTTP: it never reaches routes.go
// or a gin.Engine, which is why it gets its OWN listener in apps/httpapi
// (see TCPServer/tcpMeta there) instead of a route in Server.Handler(). What
// it stores is identical to http-intake.logs.<site> (router_logs.go) — same
// LogRow fields, same storage.LogsWriter, same tenant-from-key model —
// because every line goes through the exact same decodeLogItemJSON + logRow
// conversion that HTTP batch items do.
//
// FRAMING — one log per line: "<api_key> <json>\n". The api key (Datadog-
// shaped, 32 hex chars) and the JSON-encoded log are separated by exactly one
// space (0x20), and the message ends with exactly one \n (0x0A). This is the
// agent's own "prefixer" (glues the key on) followed by its
// "lineBreakDelimiter" (marks message boundaries with \n), per
// server/docs/spis-endpointow-datadoga.md §4.8 — the project's own
// reverse-engineering notes on comp/logs-library/client/tcp/, and the ONLY
// description of this transport available anywhere in this codebase or its
// module cache (see the gap below). The agent never reads back from this
// socket once connected — no ack, no handshake — it only notices the
// connection dying, which shapes the UNKNOWN KEY behaviour below.
//
// GAP — NOT VERIFIED, NOT IMPLEMENTED: that same doc says the agent's real
// default (dev_mode_use_proto: true) is a DIFFERENT framing — 4-byte
// big-endian length prefix counting "<api_key> " + a protobuf-encoded
// message, not a \n-delimited JSON line. This file does not implement it.
// Checked before deciding that: this module's go.mod depends on exactly
// three narrow datadog-agent payload packages (pkg/proto,
// comp/netflow/payload, pkg/networkpath/payload) plus pkg/network/payload
// indirectly — none of them carry a logs message type. A second, full
// pseudo-version checkout of github.com/DataDog/datadog-agent found
// separately in GOMODCACHE (not a dependency of this module) was grepped
// for confirmation and has neither pkg/logs/client/tcp nor
// comp/logs-library nor pkg/config/setup at all — zero hits across its
// ~17900 files for "lengthPrefixDelimiter", "dev_mode_use_proto" or
// "prefixer". That flag is also absent from that checkout's
// pkg/config/config_template.yaml, the file documenting every user-facing
// logs_config.* default — consistent with it being an internal/dev-only
// switch, but not a way to confirm which framing a production agent
// actually opens with. Implementing a binary framing with no schema to
// check field numbers against would be guessing, which CLAUDE.md and this
// task both rule out; the byte-level facts used above (the single 0x20, the
// single 0x0A) are not guesses — they come straight from the doc's own
// research. An operator who needs this listener to work should set
// logs_config.dev_mode_use_proto: false explicitly (undocumented, but
// readable by any agent build) until the protobuf path is verified against
// real agent source and added here.
//
// UNKNOWN KEY: the agent never reads back from this socket, so there is no
// per-message channel to answer a 403 on the way RequireAPIKey does for
// HTTP. A key that does not resolve in apikeys.Store closes the WHOLE
// connection right after logging it — continuing to accept bytes under a
// key nobody issued is not a decision this server gets to make quietly.
//
// UNDECODABLE JSON: kept, never dropped, mirroring storeRaw's rule in
// raw.go — but with a single reason, "decode_error", rather than HTTP's
// "decode_error" vs. "unexpected_shape" split: a bare TCP line has no
// gin.Context to build the richer note from, and the extra distinction is
// not needed to satisfy the "never dropped" contract raw_payloads exists
// for.
//
// BATCHING: one storage.Send per line would turn a busy connection into a
// storm of single-row messages. Buffered per connection and flushed at 500
// lines or 1s, whichever comes first — see tcpLogsMaxBatch /
// tcpLogsFlushInterval below.

const (
	// tcpLogsMaxBatch and tcpLogsFlushInterval bound how long a decoded row
	// waits before it reaches storage — see the BATCHING note above.
	tcpLogsMaxBatch      = 500
	tcpLogsFlushInterval = time.Second

	// tcpLogsReadBuffer is headroom for one line: a log message plus its
	// attributes JSON comfortably fits, and bufio.Reader grows past this on
	// its own if one line genuinely needs more.
	tcpLogsReadBuffer = 64 * 1024
)

// TCPLogsServer accepts agent-intake.logs.<site> connections.
//
// It is deliberately shaped like net/http.Server: Serve(ln) blocks until the
// listener (or the server itself) closes, and Close stops it — so
// apps/httpapi can drive it through a meta-process exactly the way it drives
// the two HTTP servers (see apps/httpapi/meta.go's doc comment for why a
// meta-process exists at all: an actor's loop must never block, and Serve
// blocks by design).
//
// It embeds *Server rather than holding a Store field of its own: the
// lookup and the store() writer/message-batch plumbing are exactly
// RequireAPIKey's and HandleLogs' machinery, and this type exists to feed
// the same tenant model through a different wire, not a parallel one.
type TCPLogsServer struct {
	*Server

	mu       sync.Mutex
	listener net.Listener
	conns    map[net.Conn]struct{}
	closed   bool
}

// TCPLogsServer builds the listener for this Server's key store and writers.
// Built once, in cmd/ninjacat/main.go, the same way Handler() is — before
// the node exists, which is safe because Server.store() no-ops on a nil Node
// (server.go) and both this and Handler() are wired to the same *Server the
// node gets attached to right after startup.
func (a *Server) TCPLogsServer() *TCPLogsServer {
	return &TCPLogsServer{Server: a, conns: make(map[net.Conn]struct{})}
}

// Serve accepts connections until the listener errors or Close is called.
// Every connection gets its own goroutine and its own decode/batch state —
// there is no mutable state shared between connections beyond the Store
// lookup and the storage writers, both already safe for concurrent use
// (apps/apikeys/store.go's atomic.Pointer snapshot; apps/storage's actors).
func (t *TCPLogsServer) Serve(ln net.Listener) error {
	t.mu.Lock()
	t.listener = ln
	t.mu.Unlock()

	for {
		conn, err := ln.Accept()
		if err != nil {
			if t.isClosed() {
				// A requested shutdown is not a failure — mirrors
				// httpServer.Start's handling of http.ErrServerClosed in
				// apps/httpapi/meta.go.
				return nil
			}
			return err
		}
		t.trackConn(conn, true)
		go t.handleConn(conn)
	}
}

// Close stops Serve and every open connection. Safe to call more than once.
func (t *TCPLogsServer) Close() error {
	t.mu.Lock()
	if t.closed {
		t.mu.Unlock()
		return nil
	}
	t.closed = true
	ln := t.listener
	conns := make([]net.Conn, 0, len(t.conns))
	for c := range t.conns {
		conns = append(conns, c)
	}
	t.mu.Unlock()

	if ln != nil {
		_ = ln.Close()
	}
	for _, c := range conns {
		_ = c.Close()
	}
	return nil
}

func (t *TCPLogsServer) isClosed() bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.closed
}

func (t *TCPLogsServer) trackConn(c net.Conn, add bool) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if add {
		t.conns[c] = struct{}{}
	} else {
		delete(t.conns, c)
	}
}

// handleConn owns one connection end to end: read a line, resolve its key,
// decode it, buffer the row, and flush on the batch/interval limits above.
// It never returns an error — a broken connection is simply a closed
// connection, logged where the reason matters, because there is nobody
// upstream to propagate an error to: Serve has already moved on to the next
// Accept.
func (t *TCPLogsServer) handleConn(conn net.Conn) {
	remote := conn.RemoteAddr().String()
	defer func() {
		t.trackConn(conn, false)
		_ = conn.Close()
	}()

	if t.Store == nil {
		// Mirrors RequireAPIKeyFrom's "no store is a configuration fault,
		// not the caller's problem" (apikey_mw.go) — there is no key this
		// connection could send that would validate.
		log.Printf("[logs-tcp] %s: key store unavailable, closing connection", remote)
		return
	}

	r := bufio.NewReaderSize(conn, tcpLogsReadBuffer)

	var logBuf []storage.LogRow
	var rawBuf []storage.RawPayloadRow
	flush := func() {
		if len(logBuf) > 0 {
			t.store(storage.LogsWriter, storage.WriteLogs{Entries: logBuf}, len(logBuf))
			logBuf = nil
		}
		if len(rawBuf) > 0 {
			t.store(storage.RawPayloadsWriter, storage.WriteRawPayloads{Payloads: rawBuf}, len(rawBuf))
			rawBuf = nil
		}
	}
	defer flush()

	for {
		// The read deadline doubles as the flush timer: ReadBytes returns a
		// timeout error once nothing has arrived for tcpLogsFlushInterval,
		// which is exactly when a partial batch should stop waiting for
		// company. An idle connection still flushes on time; a busy one
		// flushes at tcpLogsMaxBatch instead, well before the deadline ever
		// matters.
		_ = conn.SetReadDeadline(time.Now().Add(tcpLogsFlushInterval))

		line, readErr := r.ReadBytes('\n')
		if len(line) > 0 {
			res := t.handleLine(bytes.TrimSuffix(line, []byte("\n")), remote)
			if res.hasRow {
				logBuf = append(logBuf, res.row)
			}
			if res.hasRaw {
				rawBuf = append(rawBuf, res.raw)
			}
			if len(logBuf) >= tcpLogsMaxBatch || len(rawBuf) >= tcpLogsMaxBatch {
				flush()
			}
			if res.closeConn {
				// UNKNOWN KEY — see the file-level note. The deferred
				// flush() above still runs, so lines already buffered from
				// earlier in this connection are not lost.
				return
			}
		}

		if readErr != nil {
			var ne net.Error
			if errors.As(readErr, &ne) && ne.Timeout() {
				flush()
				continue
			}
			// EOF (the agent closed the connection cleanly) or any other
			// read error: nothing more is coming either way.
			return
		}
	}
}

// lineResult is what one framed line produced: at most one row (a LogRow
// for storage.LogsWriter or a RawPayloadRow for storage.RawPayloadsWriter,
// never both), plus whether the caller must close the connection.
type lineResult struct {
	row       storage.LogRow
	hasRow    bool
	raw       storage.RawPayloadRow
	hasRaw    bool
	closeConn bool
}

// handleLine turns one line — already stripped of its trailing \n — into a
// lineResult. See the file-level FRAMING, UNKNOWN KEY and UNDECODABLE JSON
// notes for the rules implemented here.
func (t *TCPLogsServer) handleLine(line []byte, remote string) lineResult {
	if len(line) == 0 {
		return lineResult{}
	}

	key, payload, ok := bytes.Cut(line, []byte(" "))
	if !ok {
		// Not the "<api_key> <json>" shape at all. With no key there is no
		// tenant to file a raw_payloads row under — raw.go's own rule: a
		// made-up tenant would put the row where no query ever looks — so
		// this is logged and dropped, not stored.
		log.Printf("[logs-tcp] %s: line has no api key prefix, dropped (%d B)", remote, len(line))
		return lineResult{}
	}

	info, found := t.Store.Lookup(string(key))
	if !found {
		log.Printf("[logs-tcp] %s: unknown API key, closing connection", remote)
		return lineResult{closeConn: true}
	}
	tenant := info.TenantID

	rawResult := func(note string) lineResult {
		return lineResult{hasRaw: true, raw: storage.RawPayloadRow{
			TenantID:    tenant,
			ReceivedAt:  time.Now().UTC(),
			Intake:      "logs-tcp",
			Reason:      "decode_error",
			Method:      "TCP",
			ContentType: "application/json",
			Headers:     map[string]string{"Remote-Addr": remote},
			Body:        string(payload),
			BodyBytes:   uint64(len(payload)),
			Note:        note,
		}}
	}

	item, _, err := decodeLogItemJSON(payload)
	if err != nil {
		// TCP collapses HTTP's decode_error/unexpected_shape split into one
		// reason — see the file-level UNDECODABLE JSON note.
		return rawResult(err.Error())
	}

	row, err := logRow(tenant, item, time.Now().UTC(), "", "", "", nil)
	if err != nil {
		return rawResult("attributes: " + err.Error())
	}

	return lineResult{hasRow: true, row: row}
}
