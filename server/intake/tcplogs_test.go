package intake

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"io"
	"net"
	"strings"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/pb"
	"github.com/itsninjacats/server/apps/storage"
)

// runConn feeds raw bytes to tl.handleConn through a net.Pipe — a real
// net.Conn, so handleConn reads it exactly as it would read a socket
// (through bufio, subject to SetReadDeadline, framing detection via Peek,
// etc.) rather than through a shortcut. It writes the whole payload, closes
// the client side, and waits for handleConn to return.
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

// frameProtoPayload builds one length-prefixed frame exactly as §4.8
// documents it: "4-bajtowy big-endian uint32 liczący apikey + spacja +
// protobuf" — the 4-byte length counts the api key, the space, and the
// payload together, not the payload alone.
func frameProtoPayload(key string, payload []byte) []byte {
	content := append([]byte(key+" "), payload...)
	var lenBuf [4]byte
	binary.BigEndian.PutUint32(lenBuf[:], uint32(len(content)))
	return append(lenBuf[:], content...)
}

// TestTCPLogsServerHandleConnDecodesFramedLines feeds three raw-encoder
// (newline-delimited) lines through one connection: a well-formed JSON log,
// a plain-text line that is not JSON at all, and a line carrying a key
// nobody issued.
//
// The bad-key line comes LAST on purpose: an unknown key closes the WHOLE
// connection right after logging it (tcplogs.go's UNKNOWN KEY note), so
// anything after it would never be read. Putting it last lets one test prove
// all three paths — good line, raw-fallback line, connection-closing line —
// run over the same connection, in the order a real stream would deliver
// them.
//
// The non-JSON line is NOT expected to become a raw_payloads row: §4.8 does
// not document the raw encoder's output as JSON, so tcplogs.go stores it as
// a LogRow with the whole line as Message instead (see rawLineLogRow and
// docs/tables/logs.md "TCP transport").
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
	if len(logs) != 2 {
		t.Fatalf("log rows: got %d, want 2 (the structured line and the raw-fallback line)", len(logs))
	}
	if logs[0].Message != "good" || logs[0].Host != "h1" || logs[0].Service != "svc" {
		t.Errorf("log row 0: got %+v, want message=good host=h1 service=svc", logs[0])
	}
	if logs[1].Message != "not json at all" {
		t.Errorf("log row 1 message: got %q, want the whole non-JSON line verbatim", logs[1].Message)
	}
	if logs[1].TimestampSource != "arrival" {
		t.Errorf("log row 1 timestamp_source: got %q, want arrival — the raw encoder carries no timestamp", logs[1].TimestampSource)
	}
	for i, row := range logs {
		if row.TenantID != testTenant {
			t.Errorf("row %d tenant: got %q, want %q", i, row.TenantID, testTenant)
		}
	}

	if raws := Rows[storage.RawPayloadRow](node); len(raws) != 0 {
		t.Errorf("raw payload rows: got %d, want 0 — the raw encoder never produces raw_payloads, only the proto framing does", len(raws))
	}

	// The bad-key line must never have reached storage as anything at all.
	for _, row := range logs {
		if row.Message == "never stored" {
			t.Error("a line with an unknown key was stored as a LogRow")
		}
	}
}

// TestTCPLogsRawEncoderNonJSONLineBecomesLogRow isolates exactly the rule
// TestTCPLogsServerHandleConnDecodesFramedLines exercises in passing: a
// raw-encoder line that is not JSON-shaped is not an error, it is a LogRow
// whose Message is the line verbatim.
func TestTCPLogsRawEncoderNonJSONLineBecomesLogRow(t *testing.T) {
	a, _ := newTestServer(t)
	tl := a.TCPLogsServer()

	res := tl.handleFrame([]byte(testAPIKey+" plain text, not JSON, exactly what a raw encoder line looks like"), "127.0.0.1:1234", false)
	if !res.hasRow || res.hasRaw || res.closeConn {
		t.Fatalf("handleFrame result: got %+v, want a LogRow and nothing else", res)
	}
	if res.row.Message != "plain text, not JSON, exactly what a raw encoder line looks like" {
		t.Errorf("message: got %q, want the payload verbatim", res.row.Message)
	}
	if res.row.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", res.row.TenantID, testTenant)
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

// TestTCPLogsRawFramingMatchesDocumentedBytes pins the exact separator
// bytes documented for the raw encoder (server/docs/spis-endpointow-datadoga.md
// §4.8): the prefixer's single 0x20 between the key and the payload, and
// the lineBreakDelimiter's single 0x0A ending the message. Built from
// literal bytes rather than string concatenation, because no agent source
// for this framing is importable from this project's module cache to build
// the test from instead — see tcplogs.go's header comment for what was
// checked before reaching that conclusion.
func TestTCPLogsRawFramingMatchesDocumentedBytes(t *testing.T) {
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

// TestTCPLogsHandleFrameDropsAFrameWithNoKeySeparator covers the shape
// neither the "good line" nor the "unknown key" test exercises: a line with
// no space at all, so there is no key to look up and therefore no tenant to
// file a raw_payloads row under. raw.go's own rule is that a payload with no
// resolvable tenant is dropped, not stored under a guess — this is the TCP
// path hitting that same rule before a Store lookup is even possible.
func TestTCPLogsHandleFrameDropsAFrameWithNoKeySeparator(t *testing.T) {
	a, _ := newTestServer(t)
	tl := a.TCPLogsServer()

	res := tl.handleFrame([]byte("nospacehere"), "127.0.0.1:1234", false)
	if res.hasRow || res.hasRaw || res.closeConn {
		t.Errorf("handleFrame on a keyless frame: got %+v, want an empty, non-closing result", res)
	}
}

// TestTCPLogsFramingDetection is the detection test for both encodings §4.8
// documents: the raw encoder's shape (32 hex characters, one space) picks
// the newline path, and anything else — including bytes that only coincide
// with hex digits for part of the key length — falls back to the
// length-prefixed default. It also pins that Peek does not consume: the
// full original bytes must still be readable afterwards.
func TestTCPLogsFramingDetection(t *testing.T) {
	cases := []struct {
		name string
		buf  []byte
		want bool // useProto
	}{
		{
			name: "raw encoder: 32 hex characters then a space",
			buf:  []byte(testAPIKey + " {\"message\":\"m\"}\n"),
			want: false,
		},
		{
			name: "proto encoder: a length prefix, not hex+space",
			buf:  frameProtoPayload(testAPIKey, []byte{0x0a, 0x01, 0x6d}),
			want: true,
		},
		{
			name: "too short to confirm the raw shape falls back to proto",
			buf:  []byte(testAPIKey[:10]),
			want: true,
		},
		{
			name: "32 hex characters but the 33rd byte is not a space",
			buf:  []byte(testAPIKey + "X{\"message\":\"m\"}\n"),
			want: true,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			r := bufio.NewReader(bytes.NewReader(tc.buf))
			if got := detectTCPFraming(r); got != tc.want {
				t.Errorf("detectTCPFraming: got useProto=%v, want %v", got, tc.want)
			}
			all, _ := io.ReadAll(r)
			if !bytes.Equal(all, tc.buf) {
				t.Errorf("Peek consumed bytes: reader afterwards has %q, want the original %q", all, tc.buf)
			}
		})
	}
}

// TestTCPLogsServerHandleConnDecodesLengthPrefixedProtoFrames is the proto
// counterpart of TestTCPLogsServerHandleConnDecodesFramedLines: two valid
// pb.Log messages and one bad frame, all length-prefixed, in one
// connection. The bad frame sits BETWEEN the two good ones — unlike an
// unknown key, an undecodable proto payload does not close the connection,
// so this also proves the connection keeps going afterwards.
//
// The bad frame is built by marshalling a real pb.Log and then truncating
// the result: valid protobuf bytes with the tail cut off reliably fail
// Unmarshal (a length-delimited field's declared length runs past what
// remains), without hand-guessing a byte layout for the wire format.
func TestTCPLogsServerHandleConnDecodesLengthPrefixedProtoFrames(t *testing.T) {
	a, node := newTestServer(t)
	tl := a.TCPLogsServer()

	first := &pb.Log{
		Message: "first", Status: "info", Timestamp: 1790151330000, // 2026-09-23T08:15:30Z, ms
		Hostname: "h1", Service: "svc", Source: "go",
		Tags: []string{"env:prod", "env:staging"},
	}
	second := &pb.Log{Message: "second", Hostname: "h2"}

	firstData, err := first.Marshal()
	if err != nil {
		t.Fatalf("marshal first: %v", err)
	}
	secondData, err := second.Marshal()
	if err != nil {
		t.Fatalf("marshal second: %v", err)
	}
	badData := append([]byte{}, firstData...)
	badData = badData[:len(badData)-5] // truncated mid-field: reliably breaks Unmarshal

	var conn []byte
	conn = append(conn, frameProtoPayload(testAPIKey, firstData)...)
	conn = append(conn, frameProtoPayload(testAPIKey, badData)...)
	conn = append(conn, frameProtoPayload(testAPIKey, secondData)...)

	runConn(t, tl, conn)

	logs := Rows[storage.LogRow](node)
	if len(logs) != 2 {
		t.Fatalf("log rows: got %d, want 2 (the bad frame in between must not stop the connection)", len(logs))
	}

	wantTime := time.Date(2026, 9, 23, 8, 15, 30, 0, time.UTC)
	if !logs[0].Timestamp.Equal(wantTime) {
		t.Errorf("row 0 timestamp: got %s, want %s", logs[0].Timestamp, wantTime)
	}
	if logs[0].TimestampSource != "tcp_proto_ms" {
		t.Errorf("row 0 timestamp_source: got %q, want tcp_proto_ms", logs[0].TimestampSource)
	}
	if logs[0].Message != "first" || logs[0].Host != "h1" || logs[0].Service != "svc" || logs[0].Source != "go" || logs[0].Status != "info" {
		t.Errorf("row 0: got %+v, want the marshalled fields back unchanged", logs[0])
	}
	if got := logs[0].Tags["env"]; len(got) != 2 || got[0] != "prod" || got[1] != "staging" {
		t.Errorf("row 0 tags: got %v, want env: [prod staging] — pb.Log.Tags is the same multiset shape as HTTP ddtags", logs[0].Tags)
	}
	if logs[0].Attributes != "" {
		t.Errorf("row 0 attributes: got %q, want empty — pb.Log has no field left over to put there", logs[0].Attributes)
	}

	if logs[1].Message != "second" || logs[1].Host != "h2" {
		t.Errorf("row 1: got %+v, want message=second host=h2", logs[1])
	}
	if logs[1].TimestampSource != "arrival" {
		t.Errorf("row 1 timestamp_source: got %q, want arrival — Timestamp was never set", logs[1].TimestampSource)
	}

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1 (the bad frame)", len(raws))
	}
	r := raws[0]
	if r.Intake != "logs-tcp" || r.Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want logs-tcp/decode_error", r.Intake, r.Reason)
	}
	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", r.TenantID, testTenant)
	}
	if r.ContentType != "application/x-protobuf" {
		t.Errorf("content_type: got %q, want application/x-protobuf", r.ContentType)
	}
	if !strings.Contains(r.Note, "proto") {
		t.Errorf("note: got %q, want it to name the detected framing (proto)", r.Note)
	}
	if r.Headers["Remote-Addr"] == "" {
		t.Error("remote address missing from headers")
	}
}

// TestTCPLogsServerRejectsAnAbsurdLengthPrefix covers the FRAME SAFETY rule:
// a declared length over tcpLogsMaxFrameLen must never be read into a
// buffer — the connection is closed instead, without attempting the
// allocation.
func TestTCPLogsServerRejectsAnAbsurdLengthPrefix(t *testing.T) {
	a, node := newTestServer(t)
	tl := a.TCPLogsServer()

	// A length far larger than any real frame, sent as a bare 4-byte
	// big-endian prefix with no body following (there is nothing this
	// server should ever try to read for it).
	var lenBuf [4]byte
	binary.BigEndian.PutUint32(lenBuf[:], 0xFFFFFFFF)

	runConn(t, tl, lenBuf[:])

	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a connection whose only frame declared an absurd length, want 0", n)
	}
}
