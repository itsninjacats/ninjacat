package intake

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"reflect"
	"sync"
	"testing"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
	"github.com/itsninjacats/server/apps/storage"
)

// Test support for handler-level tests: a fake node that records what a
// handler sent to storage, a Server wired to it, and the same engine
// production builds.
//
// Why this is worth having rather than testing converters alone: the
// converter tests in this package call the pure functions directly and prove
// the ROW is right. They cannot prove the handler ran at all, that the tenant
// reached the row, that the right writer was addressed, or that the middleware
// let the request through. Those are the failures that ship — a handler that
// returns 202 and sends nothing looks perfectly healthy from outside.
//
// Server.store() already short-circuits when Node is nil, which is what lets
// converter tests use a bare &Server{}. Here we want the opposite: a Node that
// accepts everything and remembers it.

// The key every test request carries, and the tenant it maps to. The key is
// Datadog-shaped (32 hex characters) because that is what the real store
// holds; nothing here depends on the value.
const (
	testAPIKey = "00000000000000000000000000000001"
	testTenant = "test"
)

// captureNode is a gen.Node that only implements Send.
//
// The embedded interface is nil, so any OTHER method panics if a handler ever
// calls one — deliberately: this fake claims to support exactly one operation,
// and a silent zero value from a method nobody thought about would be worse
// than the panic that names it.
type captureNode struct {
	gen.Node

	mu   sync.Mutex
	sent []capturedSend
}

// capturedSend is one Send: the writer it was addressed to, and the message.
type capturedSend struct {
	To  any
	Msg any
}

func (n *captureNode) Send(to any, message any) error {
	n.mu.Lock()
	defer n.mu.Unlock()
	n.sent = append(n.sent, capturedSend{To: to, Msg: message})
	return nil
}

// Sends returns every Send in order. Copied under the lock: the writers'
// background flushes do not exist here, but a handler that spawns a goroutine
// would otherwise race the assertions.
func (n *captureNode) Sends() []capturedSend {
	n.mu.Lock()
	defer n.mu.Unlock()
	out := make([]capturedSend, len(n.sent))
	copy(out, n.sent)
	return out
}

// SentTo returns the messages addressed to one writer.
func (n *captureNode) SentTo(writer gen.Atom) []any {
	var out []any
	for _, s := range n.Sends() {
		if s.To == any(writer) {
			out = append(out, s.Msg)
		}
	}
	return out
}

// Rows flattens every captured Write* message into the rows it carried, for
// the row type asked for:
//
//	rows := Rows[storage.LogRow](node)
//
// It works by reflection over the message's first exported slice field, which
// is what every Write* message is by construction (messages.go: one concrete
// slice, never []Row). Reflection rather than a type switch on purpose — a
// type switch would need a new case for every table, in this shared file, and
// this file must not be something thirteen parallel branches all edit.
func Rows[T any](n *captureNode) []T {
	want := reflect.TypeFor[T]()
	var out []T

	for _, s := range n.Sends() {
		v := reflect.ValueOf(s.Msg)
		if v.Kind() != reflect.Struct {
			continue
		}
		t := v.Type()
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() || f.Type.Kind() != reflect.Slice {
				continue
			}
			// First exported slice field only: if it is not the type asked
			// for, this message is about another table.
			if f.Type.Elem() == want {
				slice := v.Field(i)
				for j := 0; j < slice.Len(); j++ {
					out = append(out, slice.Index(j).Interface().(T))
				}
			}
			break
		}
	}
	return out
}

// newTestServer returns a Server whose sends are captured and whose store
// holds exactly one key: testAPIKey, owned by testTenant.
//
// The store is built the way the keeper builds it — NewStore plus Publish —
// so tests exercise the real snapshot path rather than a test-only backdoor.
func newTestServer(t *testing.T) (*Server, *captureNode) {
	t.Helper()
	gin.SetMode(gin.TestMode)

	store := apikeys.NewStore()
	store.Publish(
		[]apikeys.Key{{ID: "test", Name: "test", TenantID: testTenant}},
		[]string{apikeys.Hash(testAPIKey)},
	)

	node := &captureNode{}
	return &Server{Node: node, Store: store}, node
}

// newTestEngine builds the engine production builds, for one route set: the
// same middleware, the same API-key guard, the same NoRoute. A test that
// registered routes on a bare gin.New() would pass with the middleware broken.
func newTestEngine(t *testing.T, a *Server, route func(*gin.RouterGroup)) *gin.Engine {
	t.Helper()
	return a.engine(route)
}

// post sends a body to the engine with the test key in the header.
func post(t *testing.T, e *gin.Engine, path string, body []byte) *httptest.ResponseRecorder {
	t.Helper()

	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// The worked example downstream tables should copy: a real request, through
// the real engine, asserting on the rows that reached storage.
//
// It pins three things no converter test can: that the key guard admits the
// request, that the tenant from the key lands on every row (tenant_id is the
// first ORDER BY column of every table — a row with the wrong one is invisible
// to every query), and that the batch arrives at the writer that owns the
// table rather than at whichever writer was nearest.
func TestHandleLogsStoresEveryItem(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	body := []byte(`[
		{"message":"first",  "hostname":"h1", "service":"svc", "ddsource":"go", "ddtags":"env:prod"},
		{"message":"second", "hostname":"h2", "service":"svc", "ddsource":"go", "ddtags":"env:prod"}
	]`)

	if w := post(t, e, "/api/v2/logs", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/logs: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	if msgs := node.SentTo(storage.LogsWriter); len(msgs) != 1 {
		t.Fatalf("messages to %s: got %d, want 1 — one batch per request",
			storage.LogsWriter, len(msgs))
	}

	rows := Rows[storage.LogRow](node)
	if len(rows) != 2 {
		t.Fatalf("stored rows: got %d, want 2 — every item in a batch becomes a row", len(rows))
	}
	for i, r := range rows {
		if r.TenantID != testTenant {
			t.Errorf("row %d tenant: got %q, want %q", i, r.TenantID, testTenant)
		}
	}
	if rows[0].Message != "first" || rows[1].Message != "second" {
		t.Errorf("messages: got %q and %q, want first and second — order must survive",
			rows[0].Message, rows[1].Message)
	}
	if rows[0].Host != "h1" {
		t.Errorf("row 0 host: got %q, want h1", rows[0].Host)
	}
}

// A request without the key must never reach a handler, and nothing must be
// stored. The guard is applied by engine(), so this also pins that
// newTestEngine really builds the production engine and not a bare router.
func TestEngineRejectsRequestWithoutKey(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLogs)

	req := httptest.NewRequest(http.MethodPost, "/api/v2/logs", bytes.NewReader([]byte(`[{"message":"x"}]`)))
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusForbidden {
		t.Errorf("no key: got %d, want 403", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a rejected request, want 0", n)
	}
}

// storeRaw is the fallback every undecodable payload goes through, so its own
// wiring gets a test: the tenant, the router label and reason, the request
// metadata, and the header allowlist that must never carry a credential.
func TestStoreRawBuildsRowFromRequest(t *testing.T) {
	a, node := newTestServer(t)

	e := a.engine(func(g *gin.RouterGroup) {
		g.POST("/api/v2/databasequery", func(c *gin.Context) {
			a.storeRaw(c, "dbm", "no_schema", "no decoder for this track yet", []byte(`{"x":1}`))
			c.Status(http.StatusAccepted)
		})
	})

	req := httptest.NewRequest(http.MethodPost, "/api/v2/databasequery?api-version=2", bytes.NewReader(nil))
	req.Host = "dbm-metrics-intake.ninjacat.local"
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")
	// The body is already decompressed by the time a handler sees it, but the
	// header still says what the sender used — and that is what we store.
	req.Header.Set("Content-Encoding", "gzip")
	req.Header.Set("User-Agent", "datadog-agent/7.58.2")
	req.Header.Set("Authorization", "Bearer secret-value")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(rows))
	}
	r := rows[0]

	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", r.TenantID, testTenant)
	}
	if r.Intake != "dbm" || r.Reason != "no_schema" {
		t.Errorf("intake/reason: got %q/%q, want dbm/no_schema", r.Intake, r.Reason)
	}
	if r.Host != "dbm-metrics-intake.ninjacat.local" {
		t.Errorf("host: got %q — the Host header is the dispatch key and must be kept", r.Host)
	}
	if r.Path != "/api/v2/databasequery" {
		t.Errorf("path: got %q", r.Path)
	}
	if got := r.Query["api-version"]; len(got) != 1 || got[0] != "2" {
		t.Errorf("query: got %v, want api-version=[2]", r.Query)
	}
	if r.ContentEncoding != "gzip" {
		t.Errorf("content_encoding: got %q, want gzip (the sender's codec, not the stored body's)", r.ContentEncoding)
	}
	if r.BodyBytes != uint64(len(r.Body)) || r.Body != `{"x":1}` {
		t.Errorf("body: got %q (%d bytes)", r.Body, r.BodyBytes)
	}
	if r.Headers["User-Agent"] != "datadog-agent/7.58.2" {
		t.Errorf("User-Agent: got %q", r.Headers["User-Agent"])
	}
	// The allowlist exists for this line: a debugging table with a 30-day TTL
	// must not become a place credentials are durable.
	for _, forbidden := range []string{"Authorization", "Dd-Api-Key"} {
		if v, ok := r.Headers[forbidden]; ok {
			t.Errorf("%s was stored (%q) — the header allowlist leaked a credential", forbidden, v)
		}
	}
}

// A payload with no tenant is dropped rather than stored under a guess:
// tenant_id is the first ORDER BY column everywhere, so a made-up value puts
// rows where no query looks.
func TestStoreRawWithoutTenantStoresNothing(t *testing.T) {
	a, node := newTestServer(t)

	// No key guard here, so the handler runs with no key in the context.
	r := gin.New()
	r.POST("/x", func(c *gin.Context) {
		a.storeRaw(c, "dbm", "decode_error", "", []byte("body"))
		c.Status(http.StatusAccepted)
	})

	req := httptest.NewRequest(http.MethodPost, "/x", nil)
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)

	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored without a tenant, want 0", n)
	}
}

// The agent's startup sweep posts "{}" (and sometimes nothing) to every
// intake without the diagnose header, so storeRaw must recognise the probe
// by its body alone — a real 7.83 agent left fourteen such rows per boot in
// the e2e lab before this rule.
func TestStoreRawSkipsEmptyProbeBodies(t *testing.T) {
	for _, body := range []string{"", "{}", "[]", " {} \n"} {
		a, node := newTestServer(t)
		e := newTestEngine(t, a, func(g *gin.RouterGroup) {
			g.POST("/probe", func(c *gin.Context) {
				a.storeRaw(c, "probe", "decode_error", "test", []byte(body))
				c.Status(http.StatusAccepted)
			})
		})
		post(t, e, "/probe", []byte(body))
		if got := len(Rows[storage.RawPayloadRow](node)); got != 0 {
			t.Errorf("body %q: %d raw rows stored, want 0", body, got)
		}
	}
	if rawIsProbe([]byte("{\"a\":1}")) || rawIsProbe([]byte("x")) {
		t.Errorf("non-empty bodies must not be treated as probes")
	}
}
