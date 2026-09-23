package intake

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"sync"
	"time"

	"github.com/DataDog/agent-payload/v5/pb"
	"github.com/itsninjacats/server/apps/storage"
)

// agent-intake.logs.<site> — the Datadog Agent's TCP transport for logs.
//
//	config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL, with
//	        logs_config.use_http NOT forced true and HTTPS unreachable at
//	        agent startup (see router_logs.go's header comment).
//
// This is a raw TCP(+TLS) byte stream, not HTTP: it never reaches routes.go
// or a gin.Engine, which is why it gets its OWN listener in apps/httpapi
// (see TCPServer/tcpMeta there) instead of a route in Server.Handler().
//
// TWO FRAMINGS, DETECTED PER CONNECTION — server/docs/spis-endpointow-datadoga.md
// §4.8, reverse-engineering the agent's own comp/logs-library/client/tcp/:
//
//	dev_mode_use_proto: true  (§4.8: "DOMYŚLNE" — the agent's DEFAULT)
//	    → proto encoder + lengthPrefixDelimiter:
//	      [4B big-endian uint32 len][<api_key> ][proto.Marshal(pb.Log)]
//	      "4-bajtowy big-endian uint32 liczący apikey + spacja + protobuf" —
//	      the length counts everything AFTER itself, key+space+payload.
//	dev_mode_use_proto: false (§4.8: "nietypowe", unusual)
//	    → raw encoder + lineBreakDelimiter:
//	      <api_key> <payload>\n — one 0x0A per message, nothing else.
//
// Both share the same prefixer regardless of which delimiter follows:
// exactly one space (0x20) glued between the api key and the payload — see
// §4.8: "`prefixer` dokleja `<api_key>` + spację (0x20)". The agent never
// reads back from this socket once connected (no ack, no handshake) — it
// only notices the connection dying, which shapes the UNKNOWN KEY behaviour
// below.
//
// dev_mode_use_proto is a per-agent-process setting, not a per-message one,
// so this listener decides the framing ONCE PER CONNECTION rather than per
// line: detectTCPFraming peeks the first 33 bytes without consuming them.
// 32 hex characters followed by a space is the raw encoder's shape — an API
// key (Datadog-shaped, 32 hex chars) is never anything else at that
// position, and a valid 4-byte length whose bytes also happen to satisfy
// that exact pattern is vanishingly unlikely, and would in any case desync
// at the very next read the same way any other malformed frame does here.
// Anything that does not match is read as the length-prefixed default.
//
// pb.Log (github.com/DataDog/agent-payload/v5/pb, generated from
// proto/logs/agent_logs_payload.proto — agent-payload is already a direct
// dependency of this module, see go.mod) declares exactly seven fields:
// message, status, timestamp, hostname, service, source, tags. Every one of
// them already has a column of its own on LogRow (see logRowFromProto), so
// nothing is left to put in `attributes` — it stays "{}" for a proto-sourced
// row, honestly, rather than being padded with something invented.
// Timestamp has no unit comment on the .proto field itself; read as
// milliseconds since epoch, matching every other numeric Datadog log
// timestamp on this path (the HTTP body's own `timestamp` is documented as
// milliseconds, §4.8: "timestamp (ms)", and logTimestamp in router_logs.go
// already treats a bare numeric timestamp/date the same way).
//
// THE RAW ENCODER'S OUTPUT IS NOT DOCUMENTED AS JSON. §4.8 describes the
// HTTP body's JSON shape SEPARATELY from this transport ("Ciało HTTP:
// tablica JSON obiektów..."), and never says the TCP raw encoder produces
// JSON at all — "raw", as opposed to the proto encoder, reads as "the log
// line's message text, unwrapped". decodeLogItemJSON (router_logs.go) is
// tried first regardless, so a sender that DOES ship JSON on this framing
// keeps every structured field HTTP would give it; anything that fails to
// parse, or parses but does not fit datadogV2.HTTPLogItem, becomes a LogRow
// whose Message is the WHOLE line rather than an error — see rawLineLogRow
// and docs/tables/logs.md's "TCP transport" section for why this is a
// deliberate fallback, not a decode failure.
//
// UNKNOWN KEY: closes the WHOLE connection right after logging it, for
// either framing — there is no per-message channel to answer a 403 on, and
// continuing to accept bytes under a key nobody issued is not a decision
// this server gets to make quietly.
//
// UNDECODABLE PROTO: a length-prefixed frame whose payload fails
// proto.Unmarshal (bad bytes, not a bad connection) goes to raw_payloads as
// ("logs-tcp", "decode_error") with the remote address in headers, since
// there is no gin.Context or Host header to build the usual note from. The
// raw encoder has no equivalent path — see the note above.
//
// FRAME SAFETY: a length-prefixed frame declaring more than
// tcpLogsMaxFrameLen is rejected WITHOUT reading its body (no allocation for
// a length nobody should ever send), and any I/O error that lands mid-frame
// — the 4-byte length only partly read, or the frame body cut short by a
// timeout or the connection closing — is treated as unrecoverable: once
// bytes belonging to an in-progress frame are abandoned, the stream's byte
// alignment can no longer be trusted, so the connection is closed rather
// than resumed (see readTCPFrame). A read timeout with NOTHING yet consumed
// toward a new frame is the ordinary idle case and just triggers a flush.
//
// BATCHING: one storage.Send per message would turn a busy connection into
// a storm of single-row messages. Buffered per connection and flushed at
// 500 lines or 1s, whichever comes first — see tcpLogsMaxBatch /
// tcpLogsFlushInterval below.

const (
	// tcpLogsMaxBatch and tcpLogsFlushInterval bound how long a decoded row
	// waits before it reaches storage — see the BATCHING note above.
	tcpLogsMaxBatch      = 500
	tcpLogsFlushInterval = time.Second

	// tcpLogsReadBuffer is headroom for one raw-encoder line: a log message
	// plus its attributes JSON comfortably fits, and bufio.Reader grows past
	// this on its own if one line genuinely needs more. It must also be at
	// least tcpLogsAPIKeyLen+1 for detectTCPFraming's Peek.
	tcpLogsReadBuffer = 64 * 1024

	// tcpLogsAPIKeyLen is the length of a Datadog-shaped API key (hex — see
	// CLAUDE.md: "API keys are Datadog-shaped (32 hex chars)"). Both
	// detectTCPFraming and the frame/line key-payload split rely on the
	// prefixer always gluing on exactly this many hex characters.
	tcpLogsAPIKeyLen = 32

	// tcpLogsMaxFrameLen bounds a length-prefixed frame's DECLARED size,
	// checked before any allocation for it — see the FRAME SAFETY note.
	tcpLogsMaxFrameLen = 1 << 20 // 1 MiB
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

// errTCPLogsIdleTimeout signals that the read deadline for a WHOLE NEW
// frame fired before any byte of it arrived — nothing was lost, so the
// caller is free to flush its batch and wait again. readTCPFrame returns
// any OTHER error, timeout included, when bytes belonging to an
// already-started frame were consumed and then abandoned: the stream's byte
// alignment can no longer be trusted after that (see the FRAME SAFETY note
// atop this file), so the caller must close the connection rather than
// resume it.
var errTCPLogsIdleTimeout = errors.New("tcplogs: idle timeout, no frame in progress")

// handleConn owns one connection end to end: detect its framing once, then
// read a frame, resolve its key, decode it, buffer the row, and flush on
// the batch/interval limits above. It never returns an error — a broken
// connection is simply a closed connection, logged where the reason
// matters, because there is nobody upstream to propagate an error to:
// Serve has already moved on to the next Accept.
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

	// The framing decision reads ahead without consuming anything (Peek), so
	// it needs its own deadline before the main loop's begins — an agent
	// that connects and then sends nothing must not hang this goroutine
	// forever.
	_ = conn.SetReadDeadline(time.Now().Add(tcpLogsFlushInterval))
	useProto := detectTCPFraming(r)

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
		// The read deadline doubles as the flush timer: an idle timeout
		// (nothing consumed toward a new frame yet) is exactly when a
		// partial batch should stop waiting for company. A busy connection
		// flushes at tcpLogsMaxBatch instead, well before the deadline ever
		// matters.
		_ = conn.SetReadDeadline(time.Now().Add(tcpLogsFlushInterval))

		frame, err := readTCPFrame(r, useProto)
		if err == nil {
			res := t.handleFrame(frame, remote, useProto)
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
				// flush() above still runs, so frames already buffered
				// from earlier in this connection are not lost.
				return
			}
			continue
		}

		if errors.Is(err, errTCPLogsIdleTimeout) {
			flush()
			continue
		}
		// EOF (the agent closed the connection cleanly), a hard read error,
		// an absurd declared length, or a frame abandoned mid-read: nothing
		// more can be safely read from this connection either way.
		if !errors.Is(err, io.EOF) {
			log.Printf("[logs-tcp] %s: closing connection: %v", remote, err)
		}
		return
	}
}

// detectTCPFraming peeks the first tcpLogsAPIKeyLen+1 bytes of a connection
// to tell which of §4.8's two encodings is in use, WITHOUT consuming them —
// bufio.Reader.Peek leaves the bytes in the buffer for the real read that
// follows. See the file-level "TWO FRAMINGS" note for the reasoning.
//
// A short or failed Peek (fewer than tcpLogsAPIKeyLen+1 bytes ever arrive)
// cannot confirm the raw encoder's shape, so it falls through to the
// length-prefixed default — the same "otherwise" the file-level note
// describes — and readTCPFrame will fail fast and close the connection if
// that guess was wrong for a connection that never sends anything usable.
func detectTCPFraming(r *bufio.Reader) (useProto bool) {
	peek, _ := r.Peek(tcpLogsAPIKeyLen + 1)
	if len(peek) == tcpLogsAPIKeyLen+1 &&
		isHexKeyPrefix(peek[:tcpLogsAPIKeyLen]) &&
		peek[tcpLogsAPIKeyLen] == ' ' {
		return false
	}
	return true
}

func isHexKeyPrefix(b []byte) bool {
	for _, c := range b {
		switch {
		case c >= '0' && c <= '9':
		case c >= 'a' && c <= 'f':
		case c >= 'A' && c <= 'F':
		default:
			return false
		}
	}
	return true
}

// isTimeout reports whether err is a net.Error that timed out — the only
// distinction readTCPFrame's callers need between "nothing arrived in time"
// and every other kind of failure.
func isTimeout(err error) bool {
	var ne net.Error
	return errors.As(err, &ne) && ne.Timeout()
}

// readTCPFrame reads one message off the wire according to useProto (the
// framing detected once for this connection, see detectTCPFraming): a
// \n-terminated line for the raw encoder, or a 4-byte big-endian length
// followed by that many bytes for the proto encoder's lengthPrefixDelimiter
// (§4.8). The returned bytes exclude the delimiter/length prefix itself in
// both cases — what is left is always "<api_key><space><payload>", ready
// for handleFrame's bytes.Cut.
//
// Every error path distinguishes "nothing consumed yet" (safe to retry —
// returns errTCPLogsIdleTimeout) from "some bytes of this frame were read
// and then the rest could not be" (unsafe to retry — returns a plain error
// instead, which handleConn treats as fatal). See the file-level FRAME
// SAFETY note for why a partially-read frame can never be resumed.
func readTCPFrame(r *bufio.Reader, useProto bool) ([]byte, error) {
	if !useProto {
		line, err := r.ReadBytes('\n')
		if err != nil {
			if len(line) == 0 && isTimeout(err) {
				return nil, errTCPLogsIdleTimeout
			}
			if len(line) == 0 {
				return nil, err
			}
			return nil, fmt.Errorf("line abandoned after %d bytes: %w", len(line), err)
		}
		return bytes.TrimSuffix(line, []byte("\n")), nil
	}

	var lenBuf [4]byte
	n, err := io.ReadFull(r, lenBuf[:])
	if err != nil {
		if n == 0 && isTimeout(err) {
			return nil, errTCPLogsIdleTimeout
		}
		if n == 0 {
			return nil, err
		}
		return nil, fmt.Errorf("length prefix abandoned after %d of 4 bytes: %w", n, err)
	}

	frameLen := binary.BigEndian.Uint32(lenBuf[:])
	if frameLen > tcpLogsMaxFrameLen {
		return nil, fmt.Errorf("frame declares %d bytes, over the %d byte limit", frameLen, tcpLogsMaxFrameLen)
	}

	frame := make([]byte, frameLen)
	if _, err := io.ReadFull(r, frame); err != nil {
		return nil, fmt.Errorf("frame body abandoned after the length prefix: %w", err)
	}
	return frame, nil
}

// lineResult is what one frame produced: at most one row (a LogRow for
// storage.LogsWriter or a RawPayloadRow for storage.RawPayloadsWriter,
// never both), plus whether the caller must close the connection.
type lineResult struct {
	row       storage.LogRow
	hasRow    bool
	raw       storage.RawPayloadRow
	hasRaw    bool
	closeConn bool
}

// handleFrame turns one message — already framed and stripped of its
// delimiter/length prefix by readTCPFrame — into a lineResult. See the
// file-level notes for the rules implemented here.
func (t *TCPLogsServer) handleFrame(frame []byte, remote string, useProto bool) lineResult {
	if len(frame) == 0 {
		return lineResult{}
	}

	key, payload, ok := bytes.Cut(frame, []byte(" "))
	if !ok {
		// Not the "<api_key> <payload>" shape at all. With no key there is
		// no tenant to file a raw_payloads row under — raw.go's own rule: a
		// made-up tenant would put the row where no query ever looks — so
		// this is logged and dropped, not stored.
		log.Printf("[logs-tcp] %s: frame has no api key prefix, dropped (%d B)", remote, len(frame))
		return lineResult{}
	}

	info, found := t.Store.Lookup(string(key))
	if !found {
		log.Printf("[logs-tcp] %s: unknown API key, closing connection", remote)
		return lineResult{closeConn: true}
	}
	tenant := info.TenantID

	if useProto {
		var item pb.Log
		if err := item.Unmarshal(payload); err != nil {
			return lineResult{hasRaw: true, raw: storage.RawPayloadRow{
				TenantID:    tenant,
				ReceivedAt:  time.Now().UTC(),
				Intake:      "logs-tcp",
				Reason:      "decode_error",
				Method:      "TCP",
				ContentType: "application/x-protobuf",
				Headers:     map[string]string{"Remote-Addr": remote},
				Body:        string(payload),
				BodyBytes:   uint64(len(payload)),
				Note:        "length-prefixed proto: " + err.Error(),
			}}
		}
		return lineResult{hasRow: true, row: logRowFromProto(tenant, &item, time.Now().UTC())}
	}

	// The raw encoder (§4.8) is not documented as JSON the way the HTTP body
	// is — decodeLogItemJSON is tried first so a sender that DOES ship JSON
	// keeps every structured field, but anything that does not decode, or
	// does not fit datadogV2.HTTPLogItem, falls back to a LogRow carrying
	// the WHOLE line as its message rather than being treated as an error.
	// See the file-level note and docs/tables/logs.md "TCP transport".
	item, _, err := decodeLogItemJSON(payload)
	if err != nil {
		return lineResult{hasRow: true, row: rawLineLogRow(tenant, payload)}
	}
	row, err := logRow(tenant, item, time.Now().UTC(), "", "", "", nil)
	if err != nil {
		return lineResult{hasRow: true, row: rawLineLogRow(tenant, payload)}
	}
	return lineResult{hasRow: true, row: row}
}

// rawLineLogRow builds the fallback row for the raw encoder's non-JSON (or
// non-log-shaped) output: the whole payload becomes Message verbatim,
// arrival time stands in for a timestamp the raw encoder does not carry,
// and nothing else about it is guessed at.
func rawLineLogRow(tenant string, payload []byte) storage.LogRow {
	return storage.LogRow{
		TenantID:        tenant,
		Timestamp:       time.Now().UTC(),
		Message:         string(payload),
		TimestampSource: "arrival",
	}
}

// logRowFromProto turns one decoded pb.Log into the row the length-prefixed
// framing carries. pb.Log declares exactly message, status, timestamp,
// hostname, service, source, tags — see the file-level note for where each
// one lands and why nothing is left for `attributes`.
func logRowFromProto(tenant string, item *pb.Log, arrival time.Time) storage.LogRow {
	ts := arrival
	source := "arrival"
	if item.Timestamp > 0 {
		// Milliseconds since epoch — see the file-level Timestamp note.
		ts = time.UnixMilli(item.Timestamp).UTC()
		source = "tcp_proto_ms"
	}

	return storage.LogRow{
		TenantID:        tenant,
		Timestamp:       ts,
		Host:            item.Hostname,
		Service:         item.Service,
		Source:          item.Source,
		Status:          item.Status,
		Message:         item.Message,
		Tags:            tagsToMultiMap(item.Tags),
		TimestampSource: source,
	}
}
