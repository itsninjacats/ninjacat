package intake

import (
	"net"
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
)

// runConn feeds raw bytes to tl.handleConn through a net.Pipe — a real
// net.Conn, so handleConn reads it exactly as it would read a socket
// (through bufio, subject to SetReadDeadline, etc.) rather than through a
// shortcut. It writes the whole payload, closes the client side, and waits
// for handleConn to return.
func runConn(t *testing.T, tl *TCPLogsServer, payload []byte) {
	t.Helper()

	clientConn, serverConn := net.Pipe()
	done := make(chan struct{})
	go func() {
		tl.handleConn(serverConn)
		close(done)
	}()

	if _, err := clientConn.Write(payload); err != nil {
		t.Fatalf("write: %v", err)
	}
	_ = clientConn.Close()

	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("handleConn did not return in time")
	}
}

// TestTCPLogsServerHandleConnDecodesFramedLines feeds three lines framed the
// way docs/spis-endpointow-datadoga.md §4.8 documents agent-intake.logs.
// <site>: "<api_key> <json>\n" per message. One line is well-formed, one
// carries JSON that does not decode, and one carries a key nobody issued.
//
// The bad-key line comes LAST on purpose: an unknown key closes the WHOLE
// connection right after logging it (tcplogs.go's UNKNOWN KEY note), so
// anything after it would never be read. Putting it last lets one test prove
// all three paths — good line, undecodable line, connection-closing line —
// run over the same connection, in the order a real stream would deliver
// them, rather than three isolated calls.
func TestTCPLogsServerHandleConnDecodesFramedLines(t *testing.T) {
	a, node := newTestServer(t)
	tl := a.TCPLogsServer()

	const unknownKey = "deadbeefdeadbeefdeadbeefdeadbee1"
	payload := []byte(
		testAPIKey + " {\"message\":\"good\",\"hostname\":\"h1\",\"service\":\"svc\"}\n" +
			testAPIKey + " not json at all\n" +
			unknownKey + " {\"message\":\"never stored\"}\n",
	)

	runConn(t, tl, payload)

	logs := Rows[storage.LogRow](node)
	if len(logs) != 1 {
		t.Fatalf("log rows: got %d, want 1 (only the well-formed line)", len(logs))
	}
	if logs[0].Message != "good" || logs[0].Host != "h1" || logs[0].Service != "svc" {
		t.Errorf("log row: got %+v, want message=good host=h1 service=svc", logs[0])
	}
	if logs[0].TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", logs[0].TenantID, testTenant)
	}

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1 (the undecodable line only — "+
			"the bad-key line is never stored)", len(raws))
	}
	r := raws[0]
	if r.Intake != "logs-tcp" || r.Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want logs-tcp/decode_error", r.Intake, r.Reason)
	}
	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", r.TenantID, testTenant)
	}
	if r.Method != "TCP" {
		t.Errorf("method: got %q, want TCP", r.Method)
	}
	if r.Body != "not json at all" {
		t.Errorf("body: got %q, want the undecodable line's payload verbatim", r.Body)
	}
	if r.Headers["Remote-Addr"] == "" {
		t.Error("remote address missing from headers — the task requires it there since there is no Host header on a raw TCP connection")
	}

	// The bad-key line must never have reached storage as anything at all,
	// under either key.
	for _, row := range logs {
		if row.Message == "never stored" {
			t.Error("a line with an unknown key was stored as a LogRow")
		}
	}
}

// TestTCPLogsServerClosesConnectionOnUnknownKey isolates the UNKNOWN KEY
// behaviour: a single line with a key the store does not recognise must
// close the connection (handleConn returns) without storing anything, even
// though the JSON itself is perfectly well-formed — the key, not the
// payload, is what is wrong here.
func TestTCPLogsServerClosesConnectionOnUnknownKey(t *testing.T) {
	a, node := newTestServer(t)
	tl := a.TCPLogsServer()

	payload := []byte("00000000000000000000000000000099 {\"message\":\"m\"}\n")
	runConn(t, tl, payload)

	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a connection whose only line had an unknown key, want 0", n)
	}
}

// TestTCPLogsFramingMatchesDocumentedBytes pins the exact separator bytes
// documented for this transport (server/docs/spis-endpointow-datadoga.md
// §4.8): the prefixer's single 0x20 between the key and the payload, and the
// lineBreakDelimiter's single 0x0A ending the message. Built from literal
// bytes rather than string concatenation, because no agent source for this
// framing is importable from this project's module cache to build the test
// from instead — see the GAP note atop tcplogs.go for what was checked
// before reaching that conclusion.
func TestTCPLogsFramingMatchesDocumentedBytes(t *testing.T) {
	a, node := newTestServer(t)
	tl := a.TCPLogsServer()

	var frame []byte
	frame = append(frame, []byte(testAPIKey)...)
	frame = append(frame, 0x20) // prefixer: exactly one space between key and payload
	frame = append(frame, []byte(`{"message":"framed"}`)...)
	frame = append(frame, 0x0A) // lineBreakDelimiter: exactly one \n per message

	runConn(t, tl, frame)

	logs := Rows[storage.LogRow](node)
	if len(logs) != 1 || logs[0].Message != "framed" {
		t.Fatalf("log rows: got %+v, want exactly one row with message \"framed\"", logs)
	}
}

// TestTCPLogsHandleLineDropsAFrameWithNoKeySeparator covers the shape
// neither the "good line" nor the "unknown key" test exercises: a line with
// no space at all, so there is no key to look up and therefore no tenant to
// file a raw_payloads row under. raw.go's own rule is that a payload with no
// resolvable tenant is dropped, not stored under a guess — this is the TCP
// path hitting that same rule before a Store lookup is even possible.
func TestTCPLogsHandleLineDropsAFrameWithNoKeySeparator(t *testing.T) {
	a, _ := newTestServer(t)
	tl := a.TCPLogsServer()

	res := tl.handleLine([]byte("nospacehere"), "127.0.0.1:1234")
	if res.hasRow || res.hasRaw || res.closeConn {
		t.Errorf("handleLine on a keyless frame: got %+v, want an empty, non-closing result", res)
	}
}
