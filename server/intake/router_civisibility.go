package intake

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math"
	"mime"
	"mime/multipart"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/tinylib/msgp/msgp"

	"github.com/itsninjacats/server/apps/storage"
)

// CI Visibility / Test Optimization — three hosts and ten routes on api.<site>.
//
//	citestcycle-intake.<site>  POST /api/v2/citestcycle   msgpack (or JSON)
//	citestcov-intake.<site>    POST /api/v2/citestcov     multipart + msgpack
//	api.<site>                 POST /api/v2/libraries/tests/services/setting
//	                           POST /api/v2/ci/tests/skippable
//	                           POST /api/v2/ci/libraries/tests
//	                           POST /api/v2/ci/libraries/tests/flaky
//	                           POST /api/v2/test/libraries/test-management/tests
//	                           POST /api/v2/git/repository/search_commits
//	                           POST /api/v2/git/repository/packfile
//	                           POST /api/v2/ci/pipeline/tags
//	                           POST /api/v2/ci/pipeline/metrics
//	                           POST /api/intake/ci/custom_spans
//
// WHAT MAKES THIS DIFFERENT FROM EVERY OTHER INTAKE IN THIS PACKAGE, and it
// is worth stating before the code: most of these endpoints are READS. The
// tracer asks us what to do — is coverage on, which tests may be skipped,
// which commits do you already have — and then BEHAVES according to the
// answer. Everywhere else in this server a wrong answer costs a stored row;
// here a wrong answer changes what the customer's test suite does. That is
// why every response struct below mirrors the field names of the client
// struct that parses it, with the file it came from named above it, and why
// the safe answer is uniformly "feature off, list empty" rather than
// anything clever.
//
// TWO TRANSPORTS, ONE SET OF PATHS. Each endpoint reaches a real Datadog
// either agentless (the tracer talks to <subdomain>.<site> directly, header
// dd-api-key) or tunnelled through the local agent's EVP proxy (the tracer
// talks to <agent>/evp_proxy/v2/<path> with X-Datadog-EVP-Subdomain naming
// the subdomain, and the agent rewrites the host). We serve the agentless
// spelling, which is the one that reaches us by hostname. The agent's proxy
// strips nearly every header and adds its own, so the four columns we keep
// (evp_subdomain, container_id, agent_hostname, agent_version) exist to say
// which of the two paths a payload took — see civAgentHeaders below for the
// real header names, which are not the obvious ones.
//
// SOURCES, all read directly rather than from documentation:
//
//	dd-trace-go@main ddtrace/tracer/civisibility_tslv.go          §citestcycle
//	                 internal/civisibility/utils/net/coverage.go   §citestcov
//	                 .../settings_api.go, skippable.go,
//	                 known_tests_api.go, test_management_tests_api.go,
//	                 searchcommits_api.go, sendpackfiles_api.go, client.go
//	datadog-agent    pkg/trace/api/evp_proxy.go,
//	                 pkg/trace/api/internal/header/headers.go
//
// The authoritative backend-side field catalogue (DataDog/datadog-ci-spec)
// is not public, so "we handle every field" means every field a shipped
// client writes. That is also why each table keeps a raw column.
//
// WHERE THE ROWS GO:
//
//	/api/v2/citestcycle                     ci_test_events
//	/api/v2/citestcov                       ci_coverage
//	/api/v2/git/repository/search_commits   git_commits
//	/api/v2/git/repository/packfile         git_packfiles + git_commits
//	the four tracer-configuration endpoints ci_settings_requests
//	/api/v2/ci/pipeline/{tags,metrics},
//	    /api/intake/ci/custom_spans         ci_pipeline_events
//
// Anything that fails to decode goes to raw_payloads through storeRaw with
// its own intake label (citestcycle, citestcov, gitmeta, cisettings,
// cipipeline), per the rule in intake/raw.go.

func (a *Server) routeCITestCycle(g *gin.RouterGroup) {
	g.POST("/api/v2/citestcycle", a.HandleCITestCycle)
}

func (a *Server) routeCITestCov(g *gin.RouterGroup) {
	g.POST("/api/v2/citestcov", a.HandleCITestCov)
}

// routeCIVisibilityAPI adds the CI Visibility endpoints that live on
// api.<site> alongside the forwarder's own (router_api.go). They are a
// separate route function rather than lines in routeAPI because they are a
// separate product with a separate contract: routeAPI's handlers all answer
// 202 and store, these mostly answer a document the caller obeys.
func (a *Server) routeCIVisibilityAPI(g *gin.RouterGroup) {
	// Tracer configuration. Each is individually gated by a flag in the
	// settings response, so keeping every flag false means a tracer never
	// calls the other three — they are implemented anyway, because a tracer
	// configured by hand, or a future version that stops asking first, must
	// not get a 404 in the middle of a test run.
	g.POST("/api/v2/libraries/tests/services/setting", a.HandleCISettings)
	g.POST("/api/v2/ci/tests/skippable", a.HandleCISkippableTests)
	g.POST("/api/v2/ci/libraries/tests", a.HandleCIKnownTests)
	g.POST("/api/v2/ci/libraries/tests/flaky", a.HandleCIFlakyTests)
	g.POST("/api/v2/test/libraries/test-management/tests", a.HandleCITestManagement)

	// Git metadata, shared by the tracers and by `datadog-ci git-metadata
	// upload`.
	g.POST("/api/v2/git/repository/search_commits", a.HandleGitSearchCommits)
	g.POST("/api/v2/git/repository/packfile", a.HandleGitPackfile)

	// The datadog-ci CLI's own write endpoints.
	g.POST("/api/v2/ci/pipeline/tags", a.HandleCIPipelineTags)
	g.POST("/api/v2/ci/pipeline/metrics", a.HandleCIPipelineMetrics)
	g.POST("/api/intake/ci/custom_spans", a.HandleCICustomSpans)
}

// ---------------------------------------------------------------------------
// Headers the agent's EVP proxy adds
// ---------------------------------------------------------------------------

// civAgentHeaders is what a payload carries about the hop it took.
//
// The header names are the REAL ones, from datadog-agent's
// pkg/trace/api/evp_proxy.go and pkg/trace/api/internal/header/headers.go,
// and three of them are not what one would guess:
//
//   - the container id header is "Datadog-Container-ID", with NO X- prefix.
//     "X-Datadog-Container-Id" does not exist anywhere in the agent.
//   - there is no "X-Datadog-AgentVersion". The proxy announces itself in
//     "Via: trace-agent <version>", so that is where the version comes from;
//     the header is still read first in case a future agent adds it.
//   - "X-Datadog-EVP-Subdomain" is CONSUMED by the proxy (it is what selects
//     the destination host) and not forwarded, so a non-empty value here
//     means something other than a datadog-agent sent the request.
type civAgentHeaders struct {
	Subdomain    string
	ContainerID  string
	Hostname     string
	AgentVersion string
}

func civHeaders(c *gin.Context) civAgentHeaders {
	h := civAgentHeaders{
		Subdomain:    c.GetHeader("X-Datadog-EVP-Subdomain"),
		ContainerID:  c.GetHeader("Datadog-Container-Id"),
		Hostname:     c.GetHeader("X-Datadog-Hostname"),
		AgentVersion: c.GetHeader("X-Datadog-Agentversion"),
	}
	if h.ContainerID == "" {
		// Not a real agent header; read anyway so a proxy that invented it
		// does not leave the column empty.
		h.ContainerID = c.GetHeader("X-Datadog-Container-Id")
	}
	if h.AgentVersion == "" {
		// "Via: trace-agent 7.83.0" — take the token after the product name.
		if via := c.GetHeader("Via"); strings.HasPrefix(via, "trace-agent ") {
			h.AgentVersion = strings.TrimSpace(strings.TrimPrefix(via, "trace-agent "))
		}
	}
	return h
}

// ---------------------------------------------------------------------------
// Generic value helpers
//
// One citestcycle payload can arrive as msgpack or as JSON, and both decode
// into ordinary Go values through different paths: msgp gives int64/uint64/
// float64/string/[]byte/map[string]interface{}, encoding/json with UseNumber
// gives json.Number/string/bool/map[string]any. Every reader below accepts
// both, so the row builders are written once.
//
// The rule that shapes them: a uint64 id NEVER passes through float64. A
// test_session_id is a full 64-bit value and 2^53 is not a theoretical
// ceiling for one — a float round-trip turns a join into a miss, silently.
// ---------------------------------------------------------------------------

// civMap reads a value as an object, nil-safe.
func civMap(v any) map[string]any {
	m, _ := v.(map[string]any)
	return m
}

// civField reads one key of an object, nil-safe when the object is nil.
func civField(m map[string]any, key string) any {
	if m == nil {
		return nil
	}
	return m[key]
}

// civString renders a decoded value as the string a column holds. Exact, not
// truncated: this is the storage path, not a log line.
func civString(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case []byte:
		return string(t)
	case json.Number:
		return t.String()
	case bool:
		if t {
			return "true"
		}
		return "false"
	case int64:
		return strconv.FormatInt(t, 10)
	case uint64:
		return strconv.FormatUint(t, 10)
	case float64:
		return strconv.FormatFloat(t, 'g', -1, 64)
	case float32:
		return strconv.FormatFloat(float64(t), 'g', -1, 32)
	default:
		b, err := json.Marshal(t)
		if err != nil {
			return ""
		}
		return string(b)
	}
}

// civFieldString is civString for one key of an object.
func civFieldString(m map[string]any, key string) string {
	return civString(civField(m, key))
}

// civUint64 reads a value as a uint64 id.
//
// A float64 is accepted only when it is exactly integral and below 2^53 —
// past that the value in the float is already not the value that was sent,
// and returning 0 (which reads as "absent") beats returning a number that is
// almost right. No shipped client sends an id as a float; this branch is for
// the one that will.
func civUint64(v any) uint64 {
	switch t := v.(type) {
	case uint64:
		return t
	case int64:
		if t < 0 {
			return 0
		}
		return uint64(t)
	case int32:
		if t < 0 {
			return 0
		}
		return uint64(t)
	case json.Number:
		if n, err := strconv.ParseUint(t.String(), 10, 64); err == nil {
			return n
		}
		return 0
	case string:
		if n, err := strconv.ParseUint(t, 10, 64); err == nil {
			return n
		}
		return 0
	case float64:
		if t < 0 || t != float64(int64(t)) || t >= 1<<53 {
			return 0
		}
		return uint64(t)
	default:
		return 0
	}
}

// civInt64 reads a value as a signed integer, with the same no-float rule.
func civInt64(v any) int64 {
	switch t := v.(type) {
	case int64:
		return t
	case int32:
		return int64(t)
	case uint64:
		if t > 1<<63-1 {
			return 0
		}
		return int64(t)
	case json.Number:
		if n, err := t.Int64(); err == nil {
			return n
		}
		return 0
	case string:
		if n, err := strconv.ParseInt(t, 10, 64); err == nil {
			return n
		}
		return 0
	case float64:
		if t != float64(int64(t)) || t >= 1<<53 || t <= -(1<<53) {
			return 0
		}
		return int64(t)
	default:
		return 0
	}
}

// civInt64Ptr is civInt64 for a Nullable column: nil when the value is
// absent, so "the producer never said" stays distinct from "zero".
func civInt64Ptr(v any) *int64 {
	if v == nil {
		return nil
	}
	n := civInt64(v)
	return &n
}

// civFloat64 reads a value as a float, for a metrics map.
func civFloat64(v any) float64 {
	switch t := v.(type) {
	case float64:
		return t
	case float32:
		return float64(t)
	case int64:
		return float64(t)
	case uint64:
		return float64(t)
	case json.Number:
		f, _ := t.Float64()
		return f
	case string:
		f, _ := strconv.ParseFloat(t, 64)
		return f
	default:
		return 0
	}
}

// civList reads a value as an array, nil-safe.
func civList(v any) []any {
	l, _ := v.([]any)
	return l
}

// civStrings reads a value as a string list. A bare string counts as a
// one-element list: several producers send a single label unwrapped.
func civStrings(v any) []string {
	switch t := v.(type) {
	case []any:
		if len(t) == 0 {
			return nil
		}
		out := make([]string, 0, len(t))
		for _, it := range t {
			out = append(out, civString(it))
		}
		return out
	case []string:
		return t
	case string:
		if t == "" {
			return nil
		}
		return []string{t}
	default:
		return nil
	}
}

// civStringMap reads a value as a string map, rendering each value exactly.
func civStringMap(v any) map[string]string {
	m := civMap(v)
	if len(m) == 0 {
		return nil
	}
	out := make(map[string]string, len(m))
	for k, val := range m {
		out[k] = civString(val)
	}
	return out
}

// civFloatMap reads a value as a numeric map, for a metrics column.
func civFloatMap(v any) map[string]float64 {
	m := civMap(v)
	if len(m) == 0 {
		return nil
	}
	out := make(map[string]float64, len(m))
	for k, val := range m {
		out[k] = civFloat64(val)
	}
	return out
}

// civJSON re-marshals a decoded value for a JSON-text column. nil renders as
// "" (no value at all), which is distinct from an explicit null.
func civJSON(v any) string {
	if v == nil {
		return ""
	}
	b, err := json.Marshal(v)
	if err != nil {
		return ""
	}
	return string(b)
}

// civUnknown builds a map of the keys of m that known does not name, each
// value re-marshaled to JSON so a nested object survives. nil when there is
// nothing undeclared, matching orEmpty's contract at the storage layer.
func civUnknown(m map[string]any, known map[string]bool) map[string]string {
	if len(m) == 0 {
		return nil
	}
	var out map[string]string
	for k, v := range m {
		if known[k] {
			continue
		}
		if out == nil {
			out = make(map[string]string, len(m))
		}
		out[k] = civJSON(v)
	}
	return out
}

// civBoolTag reads a boolean the tracer wrote into a STRING meta map.
//
// Absent means nil, not false: a tracer with early-flake-detection off sends
// no test.is_new tag at all, and a report counting new tests must be able to
// tell "this tracer was not looking" from "this test is not new".
func civBoolTag(meta map[string]string, key string) *uint8 {
	raw, ok := meta[key]
	if !ok {
		return nil
	}
	var u uint8
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "true", "1", "yes":
		u = 1
	}
	return &u
}

// civNumTag reads a number the tracer may have written into either map:
// dd-trace-go's SetTag routes a numeric value into metrics and a string into
// meta, and test.source.start has been seen in both.
func civNumTag(meta map[string]string, metrics map[string]float64, key string) *int64 {
	if f, ok := metrics[key]; ok {
		n := int64(f)
		return &n
	}
	if s, ok := meta[key]; ok {
		if n, err := strconv.ParseInt(strings.TrimSpace(s), 10, 64); err == nil {
			return &n
		}
	}
	return nil
}

// civDecodeAny decodes a body that may be msgpack or JSON into ordinary Go
// values, returning which it was.
//
// Content-Type decides when it says something usable — the tracer sends
// application/msgpack for citestcycle — and otherwise the first byte does:
// JSON payloads start with '{' or '[' after whitespace, and no msgpack map
// or array header shares those bytes (0x7b is fixint 123, which cannot start
// a payload whose top level is a map). Sniffing rather than refusing matters
// because the coverage part's content type differs between the two shipped
// clients.
func civDecodeAny(contentType string, body []byte) (value any, format string, err error) {
	ct := strings.ToLower(contentType)
	switch {
	case strings.Contains(ct, "msgpack"):
		v, err := civDecodeMsgpack(body)
		return v, "msgpack", err
	case strings.Contains(ct, "json"):
		v, err := civDecodeJSON(body)
		return v, "json", err
	}
	if civLooksJSON(body) {
		v, err := civDecodeJSON(body)
		return v, "json", err
	}
	v, err := civDecodeMsgpack(body)
	return v, "msgpack", err
}

func civLooksJSON(body []byte) bool {
	t := bytes.TrimLeft(body, " \t\r\n")
	return len(t) > 0 && (t[0] == '{' || t[0] == '[')
}

// civDecodeJSON decodes with UseNumber so a 64-bit id keeps every digit.
func civDecodeJSON(body []byte) (any, error) {
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	return v, nil
}

// civDecodeMsgpack decodes a whole msgpack document generically.
//
// ReadIntf rather than a generated struct: the tracer's own type
// (ciTestCyclePayload) is in dd-trace-go, which is not a dependency of this
// module and would be an absurd thing to vendor to read one payload. msgp's
// generic reader maps an integer to int64/uint64 rather than float64, which
// is precisely the property this payload needs.
func civDecodeMsgpack(body []byte) (any, error) {
	if len(body) == 0 {
		return nil, fmt.Errorf("empty body")
	}
	r := msgp.NewReader(bytes.NewReader(body))
	// ReadIntf allocates an array or map from the length prefix before it
	// has read a single element, and its only guard is GetMaxElements, which
	// defaults to MaxUint32 — i.e. never fires, because the prefix is itself
	// a uint32. Every element costs at least one byte on the wire, so the
	// body length is a sound and generous ceiling: a real payload is never
	// near it, and a forged header now fails to decode (and falls back to
	// storeRaw) instead of asking for a hundred gigabytes and killing the
	// process with an unrecoverable out-of-memory throw.
	maxElems := uint32(math.MaxUint32)
	if len(body) < math.MaxUint32 {
		maxElems = uint32(len(body))
	}
	r.SetMaxElements(maxElems)
	return r.ReadIntf()
}

// ---------------------------------------------------------------------------
// citestcycle-intake.<site> — the test results themselves
// ---------------------------------------------------------------------------

// The meta/metrics tag names lifted into their own columns. Every one is a
// constant from dd-trace-go's internal/civisibility/constants — ci.go,
// git.go, os.go, runtime.go, test_tags.go and tags.go. They stay in the meta
// map as well; these columns are copies for query speed, so a tracer that
// renames one loses a column and keeps the data.
const (
	civTagEnv                 = "env"
	civTagTestName            = "test.name"
	civTagTestSuite           = "test.suite"
	civTagTestModule          = "test.module"
	civTagTestFramework       = "test.framework"
	civTagTestFrameworkVer    = "test.framework_version"
	civTagTestStatus          = "test.status"
	civTagTestType            = "test.type"
	civTagTestSourceFile      = "test.source.file"
	civTagTestSourceStart     = "test.source.start"
	civTagTestSourceEnd       = "test.source.end"
	civTagTestParameters      = "test.parameters"
	civTagTestCodeowners      = "test.codeowners"
	civTagTestCommand         = "test.command"
	civTagTestSessionName     = "test_session.name"
	civTagTestIsNew           = "test.is_new"
	civTagTestIsRetry         = "test.is_retry"
	civTagTestIsModified      = "test.is_modified"
	civTagTestSkippedByITR    = "test.skipped_by_itr"
	civTagTestRetryReason     = "test.retry_reason"
	civTagEFDAbortReason      = "test.early_flake.abort_reason"
	civTagITRUnskippable      = "test.itr.unskippable"
	civTagITRForcedRun        = "test.itr.forced_run"
	civTagCodeCoverageEnabled = "test.code_coverage.enabled"

	civTagGitRepositoryURL  = "git.repository_url"
	civTagGitBranch         = "git.branch"
	civTagGitTag            = "git.tag"
	civTagGitCommitSHA      = "git.commit.sha"
	civTagGitCommitMessage  = "git.commit.message"
	civTagGitAuthorName     = "git.commit.author.name"
	civTagGitAuthorEmail    = "git.commit.author.email"
	civTagGitAuthorDate     = "git.commit.author.date"
	civTagGitCommitterName  = "git.commit.committer.name"
	civTagGitCommitterEmail = "git.commit.committer.email"
	civTagGitCommitterDate  = "git.commit.committer.date"

	civTagCIProviderName   = "ci.provider.name"
	civTagCIPipelineID     = "ci.pipeline.id"
	civTagCIPipelineName   = "ci.pipeline.name"
	civTagCIPipelineNumber = "ci.pipeline.number"
	civTagCIPipelineURL    = "ci.pipeline.url"
	civTagCIJobID          = "ci.job.id"
	civTagCIJobName        = "ci.job.name"
	civTagCIJobURL         = "ci.job.url"
	civTagCIStageName      = "ci.stage.name"
	civTagCIWorkspacePath  = "ci.workspace_path"
	civTagCINodeName       = "ci.node.name"
	civTagCINodeLabels     = "ci.node.labels"

	civTagOSPlatform     = "os.platform"
	civTagOSVersion      = "os.version"
	civTagOSArchitecture = "os.architecture"
	civTagRuntimeName    = "runtime.name"
	civTagRuntimeVersion = "runtime.version"
	civTagLanguage       = "language"
	civTagRuntimeID      = "runtime-id"
	civTagLibraryVersion = "library_version"
)

// civCycleLogLimit caps per-event log lines. A log cap only: decoding and
// storage never stop at it.
const civCycleLogLimit = 20

// HandleCITestCycle accepts a test-cycle payload: the envelope
// {version, metadata, events[]} whose events are the tests, suites, modules
// and sessions a CI run produced.
//
// The transport on the other end (dd-trace-go civisibility_transport.go:188)
// treats any status below 400 as success and has NO retry at all — unlike
// every other CI Visibility endpoint. A payload we reject is a payload that
// is simply gone, which is why a decode failure here answers 202 and stores
// the bytes rather than returning an error the tracer would drop on the floor.
func (a *Server) HandleCITestCycle(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[citestcycle] cannot read body: %v", err)
		return
	}
	if len(body) == 0 {
		log.Printf("[citestcycle] empty body — probe?")
		return
	}

	ct := c.GetHeader("Content-Type")
	decoded, format, err := civDecodeAny(ct, body)
	if err != nil {
		log.Printf("[citestcycle] %s decode: %v (%d bytes)", format, err, len(body))
		describe("citestcycle", ct, body)
		a.storeRaw(c, "citestcycle", "decode_error",
			format+" envelope, content-type "+orUnknown(ct)+": "+err.Error(), body)
		return
	}

	envelope := civMap(decoded)
	if envelope == nil {
		a.storeRaw(c, "citestcycle", "unexpected_shape",
			"payload is not a "+format+" map", body)
		return
	}

	events := civList(envelope["events"])
	payloadVersion := int32(civInt64(envelope["version"]))
	metadata := civCycleMetadata(envelope["metadata"])

	log.Printf("[citestcycle] %s v%d, %d events (%d bytes) metadata for: %s",
		format, payloadVersion, len(events), len(body), strings.Join(sortedKeys(metadata), ", "))

	if len(events) == 0 {
		// An envelope with no events is not an error — the encoder emits one
		// when a flush finds nothing — but it is also not a shape with a
		// published meaning, so keep it rather than guess.
		a.storeRaw(c, "citestcycle", "unexpected_shape",
			"envelope carries no events", body)
		return
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()
	headers := civHeaders(c)

	rows := make([]storage.CITestEventRow, 0, len(events))
	counts := map[string]int{}
	for i, raw := range events {
		event := civMap(raw)
		if event == nil {
			a.storeRaw(c, "citestcycle", "decode_error",
				fmt.Sprintf("event %d is not a map", i), body)
			continue
		}
		row := civTestEventRow(event, payloadVersion, metadata)
		counts[row.EventType]++

		if i < civCycleLogLimit {
			log.Printf("   %s v%d session=%d module=%d suite=%d span=%d %s/%s %s status=%s %dns err=%d",
				row.EventType, row.EventVersion, row.SessionID, row.ModuleID, row.SuiteID,
				row.SpanID, orDash(row.Service), orDash(row.Name), orDash(row.TestName),
				orDash(row.TestStatus), row.DurationNs, row.Error)
		} else if i == civCycleLogLimit {
			log.Printf("   ... %d more events", len(events)-i)
		}

		if tenant == "" {
			continue
		}
		row.TenantID = tenant
		row.ReceivedAt = now
		row.EVPSubdomain = headers.Subdomain
		row.ContainerID = headers.ContainerID
		row.AgentHostname = headers.Hostname
		row.AgentVersion = headers.AgentVersion
		rows = append(rows, row)
	}
	log.Printf("[citestcycle] by type: %s", civCounts(counts))

	if tenant == "" {
		return
	}
	a.store(storage.CITestEventsWriter, storage.WriteCITestEvents{Events: rows}, len(rows))
}

// civCycleMetadata flattens the envelope's metadata map.
//
// On the wire it is map[eventType]map[string]string, where the key "*"
// carries defaults for every type (dd-trace-py writer.py puts language,
// runtime-id, library_version and env under the wildcard). Flattening it
// here with the per-type entry winning means each row can be read without a
// join back to a payload it no longer belongs to — the reason the column is
// on the event row at all.
func civCycleMetadata(v any) map[string]map[string]string {
	raw := civMap(v)
	if len(raw) == 0 {
		return nil
	}
	out := make(map[string]map[string]string, len(raw))
	for eventType, entry := range raw {
		if m := civStringMap(entry); m != nil {
			out[eventType] = m
		}
	}
	return out
}

// civMetadataFor merges the wildcard entry with the one for this event type.
// The per-type entry wins: it is the more specific statement.
func civMetadataFor(metadata map[string]map[string]string, eventType string) map[string]string {
	star, typed := metadata["*"], metadata[eventType]
	if len(star) == 0 && len(typed) == 0 {
		return nil
	}
	out := make(map[string]string, len(star)+len(typed))
	for k, v := range star {
		out[k] = v
	}
	for k, v := range typed {
		out[k] = v
	}
	return out
}

// civTestEventRow converts one event. TenantID/ReceivedAt and the agent
// headers are left zero-valued and filled by the caller, which is where the
// request context lives.
func civTestEventRow(event map[string]any, payloadVersion int32,
	metadata map[string]map[string]string) storage.CITestEventRow {

	eventType := civFieldString(event, "type")
	content := civMap(event["content"])

	meta := civStringMap(civField(content, "meta"))
	metrics := civFloatMap(civField(content, "metrics"))
	merged := civMetadataFor(metadata, eventType)

	// env comes from the span's meta when the tracer set it there and from
	// the envelope metadata otherwise — dd-trace-py puts it under "*".
	env := meta[civTagEnv]
	if env == "" {
		env = merged[civTagEnv]
	}

	row := storage.CITestEventRow{
		EventType:      eventType,
		EventVersion:   int32(civInt64(civField(event, "version"))),
		PayloadVersion: payloadVersion,

		SessionID: civUint64(civField(content, "test_session_id")),
		ModuleID:  civUint64(civField(content, "test_module_id")),
		SuiteID:   civUint64(civField(content, "test_suite_id")),

		TraceID:  civUint64(civField(content, "trace_id")),
		SpanID:   civUint64(civField(content, "span_id")),
		ParentID: civUint64(civField(content, "parent_id")),

		ITRCorrelationID: civFieldString(content, "itr_correlation_id"),

		Service:  civFieldString(content, "service"),
		Env:      env,
		Name:     civFieldString(content, "name"),
		Resource: civFieldString(content, "resource"),
		SpanType: civFieldString(content, "type"),

		// start is ns since epoch. Not defaulted to now() when absent: a row
		// dated 1970 says "the producer sent no start", and inventing a
		// timestamp would make that indistinguishable from a real one.
		Start:      time.Unix(0, civInt64(civField(content, "start"))).UTC(),
		DurationNs: civInt64(civField(content, "duration")),
		Error:      int32(civInt64(civField(content, "error"))),

		TestName:             meta[civTagTestName],
		TestSuite:            meta[civTagTestSuite],
		TestModule:           meta[civTagTestModule],
		TestFramework:        meta[civTagTestFramework],
		TestFrameworkVersion: meta[civTagTestFrameworkVer],
		TestStatus:           meta[civTagTestStatus],
		TestType:             meta[civTagTestType],
		TestSourceFile:       meta[civTagTestSourceFile],
		TestSourceStart:      civNumTag(meta, metrics, civTagTestSourceStart),
		TestSourceEnd:        civNumTag(meta, metrics, civTagTestSourceEnd),
		TestParameters:       meta[civTagTestParameters],
		TestCodeowners:       meta[civTagTestCodeowners],
		TestCommand:          meta[civTagTestCommand],
		TestSessionName:      meta[civTagTestSessionName],

		GitRepositoryURL:        meta[civTagGitRepositoryURL],
		GitBranch:               meta[civTagGitBranch],
		GitTag:                  meta[civTagGitTag],
		GitCommitSHA:            meta[civTagGitCommitSHA],
		GitCommitMessage:        meta[civTagGitCommitMessage],
		GitCommitAuthorName:     meta[civTagGitAuthorName],
		GitCommitAuthorEmail:    meta[civTagGitAuthorEmail],
		GitCommitAuthorDate:     meta[civTagGitAuthorDate],
		GitCommitCommitterName:  meta[civTagGitCommitterName],
		GitCommitCommitterEmail: meta[civTagGitCommitterEmail],
		GitCommitCommitterDate:  meta[civTagGitCommitterDate],

		CIProviderName:   meta[civTagCIProviderName],
		CIPipelineID:     meta[civTagCIPipelineID],
		CIPipelineName:   meta[civTagCIPipelineName],
		CIPipelineNumber: meta[civTagCIPipelineNumber],
		CIPipelineURL:    meta[civTagCIPipelineURL],
		CIJobID:          meta[civTagCIJobID],
		CIJobName:        meta[civTagCIJobName],
		CIJobURL:         meta[civTagCIJobURL],
		CIStageName:      meta[civTagCIStageName],
		CIWorkspacePath:  meta[civTagCIWorkspacePath],
		CINodeName:       meta[civTagCINodeName],
		CINodeLabels:     meta[civTagCINodeLabels],

		OSPlatform:     meta[civTagOSPlatform],
		OSVersion:      meta[civTagOSVersion],
		OSArchitecture: meta[civTagOSArchitecture],
		RuntimeName:    meta[civTagRuntimeName],
		RuntimeVersion: meta[civTagRuntimeVersion],

		TestIsNew:             civBoolTag(meta, civTagTestIsNew),
		TestIsRetry:           civBoolTag(meta, civTagTestIsRetry),
		TestIsModified:        civBoolTag(meta, civTagTestIsModified),
		TestSkippedByITR:      civBoolTag(meta, civTagTestSkippedByITR),
		ITRUnskippable:        civBoolTag(meta, civTagITRUnskippable),
		ITRForcedRun:          civBoolTag(meta, civTagITRForcedRun),
		CodeCoverageEnabled:   civBoolTag(meta, civTagCodeCoverageEnabled),
		TestRetryReason:       meta[civTagTestRetryReason],
		EarlyFlakeAbortReason: meta[civTagEFDAbortReason],

		Meta:     meta,
		Metrics:  metrics,
		Metadata: merged,

		// JSON, whatever the request's format was: the tracer's own doc
		// comment documents the JSON shape of an event, so that is the
		// spelling somebody debugging a payload will recognise.
		Content: civJSON(event["content"]),
	}

	// The three that are the ENVELOPE's to give, with the span's own tag
	// preferred when it set one.
	row.Language = civFirstNonEmpty(meta[civTagLanguage], merged[civTagLanguage])
	row.RuntimeID = civFirstNonEmpty(meta[civTagRuntimeID], merged[civTagRuntimeID])
	row.LibraryVersion = civFirstNonEmpty(meta[civTagLibraryVersion], merged[civTagLibraryVersion])
	return row
}

func civFirstNonEmpty(values ...string) string {
	for _, v := range values {
		if v != "" {
			return v
		}
	}
	return ""
}

// civCounts renders "key=count" pairs, sorted, for a log line.
func civCounts(counts map[string]int) string {
	keys := make([]string, 0, len(counts))
	for k := range counts {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, k+"="+strconv.Itoa(counts[k]))
	}
	return strings.Join(parts, " ")
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

// ---------------------------------------------------------------------------
// citestcov-intake.<site> — per-test line coverage
// ---------------------------------------------------------------------------

// civPart is one multipart part with the metadata profParts drops.
//
// router_profiling.go's profParts returns name -> bytes, which is enough for
// every producer that names its parts consistently. This one does not: the
// coverage part is called "coveragex" by dd-trace-go (coverage.go:80) and
// "coverage1" by dd-trace-py (encoder.py:312), and both are shipped clients.
// So the part has to be recognised by its CONTENT TYPE, and that means
// keeping it.
type civPart struct {
	Name        string
	FileName    string
	ContentType string
	Data        []byte
}

// civParts reads a multipart body, keeping wire order and per-part metadata.
func civParts(label, contentType string, body []byte) ([]civPart, error) {
	mediaType, params, err := mime.ParseMediaType(contentType)
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(mediaType, "multipart/") {
		return nil, fmt.Errorf("content-type %s is not multipart", mediaType)
	}
	boundary := params["boundary"]
	if boundary == "" {
		return nil, fmt.Errorf("multipart without boundary")
	}

	log.Printf("[%s] %s %d B", label, mediaType, len(body))
	var out []civPart
	mr := multipart.NewReader(bytes.NewReader(body), boundary)
	for {
		p, err := mr.NextPart()
		if err == io.EOF {
			break
		}
		if err != nil {
			return out, err
		}
		data, err := io.ReadAll(p)
		if err != nil {
			return out, err
		}
		part := civPart{
			Name:        p.FormName(),
			FileName:    p.FileName(),
			ContentType: p.Header.Get("Content-Type"),
			Data:        data,
		}
		log.Printf("   part name=%q filename=%q type=%s %d B",
			part.Name, part.FileName, orUnknown(part.ContentType), len(data))
		// Appended, not keyed: a repeated form name is data, not a conflict,
		// and the coverage part is identified by content type anyway.
		out = append(out, part)
	}
	return out, nil
}

// civCoverageKnownKeys are the keys of one coverages[] entry this decoder
// names. Anything else lands in the row's extra map.
var civCoverageKnownKeys = map[string]bool{
	"test_session_id": true, "test_suite_id": true, "span_id": true, "files": true,
}

// HandleCITestCov accepts a code-coverage upload.
//
// multipart with two parts: a dummy JSON "event" ({"dummy": true} in both
// shipped clients) and the coverage payload, msgpack or JSON, whose shape is
//
//	{"version": 2, "coverages": [{"test_session_id": u64, "test_suite_id": u64,
//	  "span_id": u64, "files": [{"filename": "...", "bitmap": <bytes>}]}]}
//
// span_id is OMITTED when the tracer is in suite-skipping mode
// (dd-trace-py encoder.py:375) — hence Nullable in the table, not 0.
//
// The bitmap is never decoded. dd-trace-go states the contract outright
// (skippable.go: "Go coverage metadata is stored and returned by the backend
// as FileBitmap bytes; the backend does not translate bitmap encodings"), so
// our job is to keep bytes we can hand back verbatim on a skippable-tests
// response, not to understand them.
func (a *Server) HandleCITestCov(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[citestcov] cannot read body: %v", err)
		return
	}
	if len(body) == 0 {
		log.Printf("[citestcov] empty body — probe?")
		return
	}

	ct := c.GetHeader("Content-Type")
	parts, err := civParts("citestcov", ct, body)
	if err != nil {
		log.Printf("[citestcov] multipart: %v", err)
		describe("citestcov", ct, body)
		a.storeRaw(c, "citestcov", "decode_error",
			"multipart, content-type "+orUnknown(ct)+": "+err.Error(), body)
		return
	}

	var eventPart, coveragePart *civPart
	for i := range parts {
		p := &parts[i]
		if p.Name == "event" {
			eventPart = p
			continue
		}
		// The first non-event part is the coverage payload whatever it is
		// called — see civPart's comment on coveragex vs coverage1.
		if coveragePart == nil {
			coveragePart = p
		}
	}
	if coveragePart == nil {
		a.storeRaw(c, "citestcov", "unexpected_shape",
			"multipart carries no coverage part (only "+civPartNames(parts)+")", body)
		return
	}

	entries, payloadVersion, format, err := civCoverageEntries(coveragePart.ContentType, coveragePart.Data)
	if err != nil {
		log.Printf("[citestcov] %s coverage payload: %v (%d bytes)", format, err, len(coveragePart.Data))
		a.storeRaw(c, "citestcov", "decode_error",
			"coverage part ("+orUnknown(coveragePart.ContentType)+"): "+err.Error(), body)
		return
	}

	var event string
	if eventPart != nil {
		event = string(eventPart.Data)
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()
	headers := civHeaders(c)

	log.Printf("[citestcov] %s v%d, %d coverage entries", format, payloadVersion, len(entries))
	rows := make([]storage.CICoverageRow, 0, len(entries))
	for i, entry := range entries {
		row := civCoverageRow(entry, payloadVersion, format)
		row.Event = event
		if i < civCycleLogLimit {
			span := "-"
			if row.SpanID != nil {
				span = strconv.FormatUint(*row.SpanID, 10)
			}
			log.Printf("   session=%d suite=%d span=%s files=%d",
				row.SessionID, row.SuiteID, span, len(row.FilesFilename))
		}
		if tenant == "" {
			continue
		}
		row.TenantID = tenant
		row.ReceivedAt = now
		row.EVPSubdomain = headers.Subdomain
		row.ContainerID = headers.ContainerID
		row.AgentHostname = headers.Hostname
		row.AgentVersion = headers.AgentVersion
		rows = append(rows, row)
	}

	if tenant == "" {
		return
	}
	a.store(storage.CICoverageWriter, storage.WriteCICoverage{Coverage: rows}, len(rows))
}

func civPartNames(parts []civPart) string {
	names := make([]string, 0, len(parts))
	for _, p := range parts {
		names = append(names, p.Name)
	}
	if len(names) == 0 {
		return "none"
	}
	return strings.Join(names, ", ")
}

// civCoverageEntry is one coverages[] element: its decoded value and the
// bytes it occupied, undecoded.
//
// Keeping the raw bytes PER ENTRY rather than per request is the whole
// reason this is not one civDecodeAny call: a payload holds one entry per
// test, and storing the whole part on every row would cost the square of the
// payload size.
type civCoverageEntry struct {
	Value map[string]any
	Raw   []byte
}

// civCoverageEntries splits the coverage payload into per-entry values and
// the exact bytes each occupied.
func civCoverageEntries(contentType string, data []byte) ([]civCoverageEntry, int32, string, error) {
	ct := strings.ToLower(contentType)
	useJSON := strings.Contains(ct, "json") || (!strings.Contains(ct, "msgpack") && civLooksJSON(data))
	if useJSON {
		entries, version, err := civCoverageEntriesJSON(data)
		return entries, version, "json", err
	}
	entries, version, err := civCoverageEntriesMsgpack(data)
	return entries, version, "msgpack", err
}

func civCoverageEntriesJSON(data []byte) ([]civCoverageEntry, int32, error) {
	var envelope struct {
		Version   json.Number       `json:"version"`
		Coverages []json.RawMessage `json:"coverages"`
	}
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	if err := dec.Decode(&envelope); err != nil {
		return nil, 0, err
	}
	version, _ := envelope.Version.Int64()

	out := make([]civCoverageEntry, 0, len(envelope.Coverages))
	for i, raw := range envelope.Coverages {
		v, err := civDecodeJSON(raw)
		if err != nil {
			return nil, int32(version), fmt.Errorf("coverages[%d]: %w", i, err)
		}
		out = append(out, civCoverageEntry{Value: civMap(v), Raw: raw})
	}
	return out, int32(version), nil
}

// civCoverageEntriesMsgpack walks the msgpack document with the *Bytes API so
// each entry's exact bytes can be delimited: msgp.Skip returns the remainder
// after one value, and the difference is that value's encoding.
func civCoverageEntriesMsgpack(data []byte) ([]civCoverageEntry, int32, error) {
	n, rest, err := msgp.ReadMapHeaderBytes(data)
	if err != nil {
		return nil, 0, fmt.Errorf("envelope map header: %w", err)
	}

	var version int32
	var out []civCoverageEntry
	for i := uint32(0); i < n; i++ {
		var key string
		key, rest, err = msgp.ReadStringBytes(rest)
		if err != nil {
			return nil, version, fmt.Errorf("envelope key %d: %w", i, err)
		}
		switch key {
		case "version":
			// Read generically rather than with ReadInt64Bytes: an encoder
			// that switched this field to another numeric width, or to a
			// string, would otherwise make a payload full of real coverage
			// undecodable over a field nothing depends on.
			var v any
			v, rest, err = msgp.ReadIntfBytes(rest)
			if err != nil {
				return nil, version, fmt.Errorf("version: %w", err)
			}
			version = int32(civInt64(v))
		case "coverages":
			var count uint32
			count, rest, err = msgp.ReadArrayHeaderBytes(rest)
			if err != nil {
				return nil, version, fmt.Errorf("coverages array header: %w", err)
			}
			// The count is the wire's claim, not a fact: msgp's *Bytes
			// API decodes the length prefix without checking it against
			// what is left to read, so a 16-byte request can announce
			// 2^32-2 entries. Preallocating from it would ask the
			// allocator for tens of gigabytes, and Go answers an
			// impossible allocation with a fatal throw, which gin's
			// Recovery cannot catch — one malformed coverage upload would
			// take the node down for every tenant. Grow with append
			// instead and let ReadIntfBytes below end the loop with
			// ErrShortBytes on the first entry that is not really there,
			// the same shape dsmReadArray uses in router_trace.go.
			for j := uint32(0); j < count; j++ {
				before := rest
				var value any
				value, rest, err = msgp.ReadIntfBytes(before)
				if err != nil {
					return nil, version, fmt.Errorf("coverages[%d]: %w", j, err)
				}
				raw := before[:len(before)-len(rest)]
				out = append(out, civCoverageEntry{Value: civMap(value), Raw: raw})
			}
		default:
			// An envelope key this decoder does not know is skipped rather
			// than refused: a newer encoder adding a field must not make a
			// payload full of real coverage undecodable.
			rest, err = msgp.Skip(rest)
			if err != nil {
				return nil, version, fmt.Errorf("skipping %q: %w", key, err)
			}
		}
	}
	return out, version, nil
}

// civCoverageRow converts one coverages[] entry. TenantID/ReceivedAt and the
// agent headers are filled by the caller.
func civCoverageRow(entry civCoverageEntry, payloadVersion int32, format string) storage.CICoverageRow {
	m := entry.Value

	files := civList(civField(m, "files"))
	// Parallel arrays that stay the same length: a file reported without
	// line-level data keeps an empty bitmap at its index rather than shifting
	// the two out of step.
	names := make([]string, 0, len(files))
	bitmaps := make([]string, 0, len(files))
	for _, raw := range files {
		f := civMap(raw)
		names = append(names, civFieldString(f, "filename"))
		bitmaps = append(bitmaps, civString(civField(f, "bitmap")))
	}

	var spanID *uint64
	if v := civField(m, "span_id"); v != nil {
		id := civUint64(v)
		spanID = &id
	}

	return storage.CICoverageRow{
		PayloadVersion: payloadVersion,
		SessionID:      civUint64(civField(m, "test_session_id")),
		SuiteID:        civUint64(civField(m, "test_suite_id")),
		SpanID:         spanID,
		FilesFilename:  names,
		FilesBitmap:    bitmaps,
		Raw:            string(entry.Raw),
		RawFormat:      format,
		Extra:          civUnknown(m, civCoverageKnownKeys),
	}
}

// ---------------------------------------------------------------------------
// api.<site> — the endpoints a tracer CONSULTS
//
// Every response type below mirrors, field name for field name, the struct in
// the client that parses it. Do not "tidy" a key: the client unmarshals by
// tag, and a renamed one reads as the zero value, which for a feature flag
// means the feature silently stays off (harmless) and for a list means the
// tracer decides nothing was returned (also harmless) — but for page_info it
// means an infinite pagination loop (not harmless).
// ---------------------------------------------------------------------------

// ciSettingsResponse mirrors dd-trace-go's settingsResponse
// (internal/civisibility/utils/net/settings_api.go:44-77), cross-checked
// against dd-trace-py's _api_client.py.
type ciSettingsResponse struct {
	Data ciSettingsResponseData `json:"data"`
}

type ciSettingsResponseData struct {
	ID         string               `json:"id"`
	Type       string               `json:"type"`
	Attributes ciSettingsAttributes `json:"attributes"`
}

type ciSettingsAttributes struct {
	CodeCoverage                bool                    `json:"code_coverage"`
	CoverageReportUploadEnabled bool                    `json:"coverage_report_upload_enabled"`
	EarlyFlakeDetection         ciEarlyFlakeDetection   `json:"early_flake_detection"`
	FlakyTestRetriesEnabled     bool                    `json:"flaky_test_retries_enabled"`
	ItrEnabled                  bool                    `json:"itr_enabled"`
	RequireGit                  bool                    `json:"require_git"`
	TestsSkipping               bool                    `json:"tests_skipping"`
	KnownTestsEnabled           bool                    `json:"known_tests_enabled"`
	ImpactedTestsEnabled        bool                    `json:"impacted_tests_enabled"`
	TestManagement              ciTestManagementSetting `json:"test_management"`
}

type ciEarlyFlakeDetection struct {
	Enabled         bool              `json:"enabled"`
	SlowTestRetries ciSlowTestRetries `json:"slow_test_retries"`
	// Pointer because the client's field is *int: null is a legal value and
	// says "no threshold", which is what we mean while the feature is off.
	FaultySessionThreshold *int `json:"faulty_session_threshold"`
}

type ciSlowTestRetries struct {
	FiveS   int `json:"5s"`
	TenS    int `json:"10s"`
	ThirtyS int `json:"30s"`
	FiveM   int `json:"5m"`
}

type ciTestManagementSetting struct {
	Enabled             bool `json:"enabled"`
	AttemptToFixRetries int  `json:"attempt_to_fix_retries"`
}

// ciJSONAPIRequest is the envelope every one of these endpoints wraps its
// question in: {"data": {"id", "type", "attributes": {...}}}.
type ciJSONAPIRequest struct {
	Data struct {
		ID         string          `json:"id"`
		Type       string          `json:"type"`
		Attributes json.RawMessage `json:"attributes"`
	} `json:"data"`
}

// ciConfigRequest is the union of the attributes the five configuration
// endpoints send. They overlap heavily and no endpoint sends all of it, so
// one struct with everything optional beats five nearly-identical ones —
// the raw body is stored next to it regardless, so nothing rests on this
// being exhaustive.
type ciConfigRequest struct {
	Service        string          `json:"service"`
	Env            string          `json:"env"`
	RepositoryURL  string          `json:"repository_url"`
	Branch         string          `json:"branch"`
	Sha            string          `json:"sha"`
	TestLevel      string          `json:"test_level"`
	Module         string          `json:"module"`
	CommitMessage  string          `json:"commit_message"`
	Configurations json.RawMessage `json:"configurations"`
	PageInfo       *struct {
		PageState string `json:"page_state"`
	} `json:"page_info"`
}

// civConfigRequest decodes the envelope and its attributes. A body that does
// not parse still produces a row — the endpoint label, the raw body and the
// answer we gave — because "a tracer is sending something we cannot read" is
// exactly what this table is for.
func (a *Server) civConfigRequest(c *gin.Context, endpoint string, body []byte) (ciJSONAPIRequest, ciConfigRequest, bool) {
	var envelope ciJSONAPIRequest
	var attrs ciConfigRequest

	if err := json.Unmarshal(body, &envelope); err != nil {
		log.Printf("[cisettings] %s: envelope: %v (%d bytes)", endpoint, err, len(body))
		a.storeRaw(c, "cisettings", "decode_error", endpoint+" envelope: "+err.Error(), body)
		return envelope, attrs, false
	}
	if len(envelope.Data.Attributes) > 0 {
		if err := json.Unmarshal(envelope.Data.Attributes, &attrs); err != nil {
			log.Printf("[cisettings] %s: attributes: %v", endpoint, err)
			a.storeRaw(c, "cisettings", "decode_error", endpoint+" attributes: "+err.Error(), body)
			return envelope, attrs, false
		}
	}
	return envelope, attrs, true
}

// civStoreConfigRequest keeps the question and the answer.
func (a *Server) civStoreConfigRequest(c *gin.Context, endpoint string,
	envelope ciJSONAPIRequest, attrs ciConfigRequest, body []byte, response any) {

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	pageState := ""
	if attrs.PageInfo != nil {
		pageState = attrs.PageInfo.PageState
	}
	row := storage.CISettingsRequestRow{
		TenantID:       tenant,
		ReceivedAt:     time.Now().UTC(),
		Endpoint:       endpoint,
		RequestID:      envelope.Data.ID,
		RequestType:    envelope.Data.Type,
		Service:        attrs.Service,
		Env:            attrs.Env,
		RepositoryURL:  attrs.RepositoryURL,
		Branch:         attrs.Branch,
		SHA:            attrs.Sha,
		TestLevel:      attrs.TestLevel,
		Module:         attrs.Module,
		CommitMessage:  attrs.CommitMessage,
		PageState:      pageState,
		Configurations: string(attrs.Configurations),
		RequestBody:    string(body),
	}
	if response != nil {
		if b, err := json.Marshal(response); err == nil {
			row.ResponseBody = string(b)
		}
	}
	a.store(storage.CISettingsRequestsWriter,
		storage.WriteCISettingsRequests{Requests: []storage.CISettingsRequestRow{row}}, 1)
}

// HandleCISettings answers the question every tracer asks first: which Test
// Optimization features are on.
//
// EVERY FLAG IS FALSE, and that is a feature, not a stub. Each of the other
// four configuration endpoints is individually gated by one of these flags in
// the tracer, so an all-false answer makes a real tracer behave exactly as it
// does against a backend with no Test Optimization: it still posts its test
// results to /api/v2/citestcycle (unconditional — that is the data, not a
// feature) and still uploads git metadata when configured to, but never asks
// for skippable tests, known tests or test-management state. Turning one of
// these true is what a future ninjacat release does when the feature behind
// it actually exists; doing it earlier makes a customer's test suite skip
// tests on the strength of data we do not have.
//
// The safe shape is confirmed by the client itself: dd-trace-go returns a
// zero-value SettingsResponseData when its cache is unavailable
// (settings_api.go:88), which is field for field what we send.
func (a *Server) HandleCISettings(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[cisettings] cannot read body: %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"errors": []string{"cannot read body"}})
		return
	}

	envelope, attrs, ok := a.civConfigRequest(c, "settings", body)
	log.Printf("[cisettings] settings service=%s env=%s repo=%s branch=%s sha=%s ok=%t",
		orDash(attrs.Service), orDash(attrs.Env), orDash(attrs.RepositoryURL),
		orDash(attrs.Branch), orDash(attrs.Sha), ok)

	response := ciSettingsResponse{Data: ciSettingsResponseData{
		ID: envelope.Data.ID,
		// Echoed rather than invented. Neither client reads the response's
		// type, so this is documentation for whoever reads a capture.
		Type: "ci_app_test_service_libraries_settings",
		// The zero value of every flag. Written out rather than left implicit
		// so that turning one on is a visible edit.
		Attributes: ciSettingsAttributes{
			CodeCoverage:                false,
			CoverageReportUploadEnabled: false,
			EarlyFlakeDetection: ciEarlyFlakeDetection{
				Enabled:                false,
				SlowTestRetries:        ciSlowTestRetries{},
				FaultySessionThreshold: nil,
			},
			FlakyTestRetriesEnabled: false,
			ItrEnabled:              false,
			// require_git false is what stops a tracer unshallowing and
			// uploading packfiles it was not already going to upload.
			RequireGit:           false,
			TestsSkipping:        false,
			KnownTestsEnabled:    false,
			ImpactedTestsEnabled: false,
			TestManagement: ciTestManagementSetting{
				Enabled:             false,
				AttemptToFixRetries: 0,
			},
		},
	}}

	a.civStoreConfigRequest(c, "settings", envelope, attrs, body, response)
	c.JSON(http.StatusOK, response)
}

// ciSkippableResponse mirrors dd-trace-go's skippableResponse
// (skippable.go:46-68).
type ciSkippableResponse struct {
	Meta ciSkippableMeta `json:"meta"`
	Data []any           `json:"data"`
}

type ciSkippableMeta struct {
	CorrelationID string            `json:"correlation_id"`
	Coverage      map[string]string `json:"coverage"`
}

// HandleCISkippableTests answers "which tests may be skipped": none.
//
// An empty data list means nothing is skipped, which is the only answer we
// can give honestly — deciding a test is safe to skip needs coverage data
// joined against a diff, and neither half is implemented. meta is sent
// (rather than omitted) because dd-trace-py treats a missing meta as a
// malformed response and logs it; an empty correlation_id and an empty
// coverage map are well-formed and mean what they say.
//
// Note on meta.coverage for whoever implements this: the values are base64
// of raw FileBitmap bytes and the backend is explicitly NOT expected to
// understand them (skippable.go:235) — they come from a /api/v2/citestcov
// upload and go back out unchanged. ci_coverage.files_bitmap is where they
// already are.
func (a *Server) HandleCISkippableTests(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[cisettings] skippable: cannot read body: %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"errors": []string{"cannot read body"}})
		return
	}

	envelope, attrs, ok := a.civConfigRequest(c, "skippable", body)
	log.Printf("[cisettings] skippable service=%s env=%s repo=%s sha=%s level=%s ok=%t",
		orDash(attrs.Service), orDash(attrs.Env), orDash(attrs.RepositoryURL),
		orDash(attrs.Sha), orDash(attrs.TestLevel), ok)

	response := ciSkippableResponse{
		Meta: ciSkippableMeta{CorrelationID: "", Coverage: map[string]string{}},
		Data: []any{},
	}
	a.civStoreConfigRequest(c, "skippable", envelope, attrs, body, response)
	c.JSON(http.StatusOK, response)
}

// ciKnownTestsResponse mirrors dd-trace-go's knownTestsResponse
// (known_tests_api.go:54-68).
type ciKnownTestsResponse struct {
	Data ciKnownTestsData `json:"data"`
}

type ciKnownTestsData struct {
	ID         string                 `json:"id"`
	Type       string                 `json:"type"`
	Attributes ciKnownTestsAttributes `json:"attributes"`
}

type ciKnownTestsAttributes struct {
	// module -> suite -> test names.
	Tests    map[string]map[string][]string `json:"tests"`
	PageInfo ciKnownTestsPageInfo           `json:"page_info"`
}

type ciKnownTestsPageInfo struct {
	Cursor  string `json:"cursor"`
	Size    int    `json:"size"`
	HasNext bool   `json:"has_next"`
}

// HandleCIKnownTests answers "which tests has this service run before": none.
//
// HAS_NEXT MUST BE FALSE. The client loop (known_tests_api.go:114-165) keeps
// POSTing with the previous cursor for as long as has_next is true, and
// dd-trace-go has no iteration cap — a response that forgot this field, or
// spelled it differently so it unmarshalled as its zero value, would be
// fine, but one that set it true without ever changing the cursor would spin
// a customer's test process forever. It is the one field here where the
// wrong answer is not merely useless.
func (a *Server) HandleCIKnownTests(c *gin.Context) {
	a.handleCITestList(c, "known_tests", "ci_app_libraries_tests")
}

// HandleCIFlakyTests answers the flaky-test list endpoint with the same empty
// envelope.
//
// UNVERIFIED: unlike every other route in this file, this path was not read
// from a shipped client — it is listed in our own task spec and in older
// dd-trace-py releases, and no current source we could reach names it. It is
// implemented because answering an empty list is harmless whatever the exact
// shape turns out to be, and a 404 in the middle of a test run is not; the
// request body reaches ci_settings_requests regardless, which is how we will
// learn the real shape the first time a tracer calls it.
func (a *Server) HandleCIFlakyTests(c *gin.Context) {
	a.handleCITestList(c, "flaky_tests", "ci_app_libraries_tests")
}

func (a *Server) handleCITestList(c *gin.Context, endpoint, responseType string) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[cisettings] %s: cannot read body: %v", endpoint, err)
		c.JSON(http.StatusBadRequest, gin.H{"errors": []string{"cannot read body"}})
		return
	}

	envelope, attrs, ok := a.civConfigRequest(c, endpoint, body)
	pageState := ""
	if attrs.PageInfo != nil {
		pageState = attrs.PageInfo.PageState
	}
	log.Printf("[cisettings] %s service=%s env=%s repo=%s page_state=%s ok=%t",
		endpoint, orDash(attrs.Service), orDash(attrs.Env),
		orDash(attrs.RepositoryURL), orDash(pageState), ok)

	response := ciKnownTestsResponse{Data: ciKnownTestsData{
		ID:   envelope.Data.ID,
		Type: responseType,
		Attributes: ciKnownTestsAttributes{
			Tests: map[string]map[string][]string{},
			// has_next false ends the pagination loop after this one page.
			PageInfo: ciKnownTestsPageInfo{Cursor: "", Size: 0, HasNext: false},
		},
	}}
	a.civStoreConfigRequest(c, endpoint, envelope, attrs, body, response)
	c.JSON(http.StatusOK, response)
}

// ciTestManagementResponse mirrors dd-trace-go's
// testManagementTestsResponse (test_management_tests_api.go:44-72). The
// nesting is real: modules -> suites -> tests -> properties.
type ciTestManagementResponse struct {
	Data ciTestManagementData `json:"data"`
}

type ciTestManagementData struct {
	ID         string                     `json:"id"`
	Type       string                     `json:"type"`
	Attributes ciTestManagementAttributes `json:"attributes"`
}

type ciTestManagementAttributes struct {
	Modules map[string]any `json:"modules"`
}

// HandleCITestManagement answers "which tests are quarantined, disabled or
// being attempted-to-fix": none, an empty modules map.
func (a *Server) HandleCITestManagement(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[cisettings] test_management: cannot read body: %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"errors": []string{"cannot read body"}})
		return
	}

	envelope, attrs, ok := a.civConfigRequest(c, "test_management", body)
	log.Printf("[cisettings] test_management repo=%s sha=%s module=%s branch=%s ok=%t",
		orDash(attrs.RepositoryURL), orDash(attrs.Sha),
		orDash(attrs.Module), orDash(attrs.Branch), ok)

	response := ciTestManagementResponse{Data: ciTestManagementData{
		ID:         envelope.Data.ID,
		Type:       "ci_app_libraries_tests",
		Attributes: ciTestManagementAttributes{Modules: map[string]any{}},
	}}
	a.civStoreConfigRequest(c, "test_management", envelope, attrs, body, response)
	c.JSON(http.StatusOK, response)
}

// ---------------------------------------------------------------------------
// api.<site> — git metadata
// ---------------------------------------------------------------------------

// ciSearchCommits is both the request and the response shape: the client
// sends its local commit list and expects the subset we already hold back in
// the identical envelope (searchcommits_api.go:22-33 and :84-93).
type ciSearchCommits struct {
	Data []ciCommitRef      `json:"data"`
	Meta ciSearchCommitMeta `json:"meta"`
}

type ciCommitRef struct {
	ID   string `json:"id"`
	Type string `json:"type"`
}

type ciSearchCommitMeta struct {
	RepositoryURL string `json:"repository_url"`
}

// HandleGitSearchCommits records the shas a client asked about and answers
// with an empty list.
//
// WHY EMPTY, given the table is called git_commits. The honest answer to
// "which of these do you already have" is "which packfiles have you
// uploaded", and that is a READ against ClickHouse. The intake has no read
// path by design — reads live in the query app so a slow query can never
// stall ingest (see CLAUDE.md) — so this handler cannot know. Answering
// empty makes the client unshallow its clone and upload the packs, which is
// the behaviour we want while git_packfiles is the only place those objects
// can come from. When the query app grows a lookup, this is the one line to
// change, and the rows this handler already writes are the index it will
// read.
//
// The client sets ExpectJSONResponse on this request, so the response MUST
// carry a JSON content type: a 200 with anything else is treated as a
// transient failure and retried (http.go:375-385).
func (a *Server) HandleGitSearchCommits(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[gitmeta] search_commits: cannot read body: %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"errors": []string{"cannot read body"}})
		return
	}

	var request ciSearchCommits
	if err := json.Unmarshal(body, &request); err != nil {
		log.Printf("[gitmeta] search_commits: %v (%d bytes)", err, len(body))
		a.storeRaw(c, "gitmeta", "decode_error", "search_commits: "+err.Error(), body)
		// Still a well-formed answer: an empty list is valid whatever the
		// request was, and a 4xx here is terminal for the client.
		c.JSON(http.StatusOK, ciSearchCommits{Data: []ciCommitRef{}})
		return
	}

	log.Printf("[gitmeta] search_commits repo=%s, %d candidate commits",
		orDash(request.Meta.RepositoryURL), len(request.Data))

	tenant := TenantFromContext(c)
	if tenant != "" && len(request.Data) > 0 {
		now := time.Now().UTC()
		rows := make([]storage.GitCommitRow, 0, len(request.Data))
		for _, ref := range request.Data {
			if ref.ID == "" {
				continue
			}
			rows = append(rows, storage.GitCommitRow{
				TenantID:      tenant,
				RepositoryURL: request.Meta.RepositoryURL,
				SHA:           ref.ID,
				SeenAt:        now,
				// NULL: the client is ASKING about this sha, which says
				// nothing about whether we hold its objects.
				PackfileID: nil,
				Source:     "search_commits",
			})
		}
		a.store(storage.GitCommitsWriter, storage.WriteGitCommits{Commits: rows}, len(rows))
	}

	c.JSON(http.StatusOK, ciSearchCommits{
		Data: []ciCommitRef{},
		Meta: ciSearchCommitMeta{RepositoryURL: request.Meta.RepositoryURL},
	})
}

// ciPushedSha is the JSON part of a packfile upload
// (sendpackfiles_api.go:54-89, identical in dd-trace-py's git_client.py).
type ciPushedSha struct {
	Data ciCommitRef        `json:"data"`
	Meta ciSearchCommitMeta `json:"meta"`
}

// HandleGitPackfile accepts one git packfile.
//
// multipart with a "pushedSha" JSON part and a "packfile" octet-stream part,
// one request per .pack file. The answer is 204 No Content with an empty
// body: dd-trace-go accepts any 2xx (sendpackfiles_api.go:107) but
// dd-trace-py checks for exactly 204 (git_client.py:430), so 204 is the only
// status both shipped clients call success.
func (a *Server) HandleGitPackfile(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[gitmeta] packfile: cannot read body: %v", err)
		c.Status(http.StatusBadRequest)
		return
	}

	ct := c.GetHeader("Content-Type")
	parts, err := civParts("gitpackfile", ct, body)
	if err != nil {
		log.Printf("[gitmeta] packfile multipart: %v", err)
		describe("gitpackfile", ct, body)
		a.storeRaw(c, "gitmeta", "decode_error",
			"packfile multipart, content-type "+orUnknown(ct)+": "+err.Error(), body)
		c.Status(http.StatusNoContent)
		return
	}

	var pushed ciPushedSha
	var pack []byte
	filename := ""
	other := map[string]string{}
	for _, p := range parts {
		switch p.Name {
		case "pushedSha":
			if err := json.Unmarshal(p.Data, &pushed); err != nil {
				log.Printf("[gitmeta] packfile pushedSha: %v", err)
				other["pushedSha.undecodable"] = string(p.Data)
			}
		case "packfile":
			pack = p.Data
			filename = p.FileName
		default:
			other[p.Name] = string(p.Data)
		}
	}

	sum := sha256.Sum256(pack)
	packfileID := hex.EncodeToString(sum[:])
	log.Printf("[gitmeta] packfile repo=%s pushed_sha=%s file=%s %d B id=%s",
		orDash(pushed.Meta.RepositoryURL), orDash(pushed.Data.ID),
		orDash(filename), len(pack), packfileID[:12])

	tenant := TenantFromContext(c)
	if tenant == "" {
		c.Status(http.StatusNoContent)
		return
	}
	now := time.Now().UTC()

	a.store(storage.GitPackfilesWriter, storage.WriteGitPackfiles{
		Packfiles: []storage.GitPackfileRow{{
			TenantID:      tenant,
			ReceivedAt:    now,
			RepositoryURL: pushed.Meta.RepositoryURL,
			PushedSHA:     pushed.Data.ID,
			PackfileID:    packfileID,
			Filename:      filename,
			Packfile:      string(pack),
			SizeBytes:     uint64(len(pack)),
			OtherParts:    other,
		}},
	}, 1)

	// The pushed sha also becomes a git_commits row, this time WITH a
	// packfile id: this is the one path where "we have the objects" is true,
	// and ReplacingMergeTree lets it overwrite the NULL a search_commits
	// sighting left behind.
	if pushed.Data.ID != "" {
		a.store(storage.GitCommitsWriter, storage.WriteGitCommits{
			Commits: []storage.GitCommitRow{{
				TenantID:      tenant,
				RepositoryURL: pushed.Meta.RepositoryURL,
				SHA:           pushed.Data.ID,
				SeenAt:        now,
				PackfileID:    &packfileID,
				Source:        "packfile",
			}},
		}, 1)
	}

	c.Status(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// api.<site> — the datadog-ci CLI's write endpoints
// ---------------------------------------------------------------------------

// ciPipelinePayload is the JSON:API envelope all three CLI endpoints use:
// {"data": {"type": "<kind>", "attributes": {...}}}.
type ciPipelinePayload struct {
	Data struct {
		Type       string          `json:"type"`
		Attributes json.RawMessage `json:"attributes"`
	} `json:"data"`
}

// civPipelineKnownKeys are the attribute keys these three endpoints name.
var civPipelineKnownKeys = map[string]bool{
	"ci_env": true, "ci_level": true, "provider": true,
	"tags": true, "metrics": true, "measures": true,
	"name": true, "start_time": true, "end_time": true,
}

// HandleCIPipelineTags accepts `datadog-ci tag`.
func (a *Server) HandleCIPipelineTags(c *gin.Context) {
	a.handleCIPipelineEvent(c, "tag")
}

// HandleCIPipelineMetrics accepts `datadog-ci measure`.
func (a *Server) HandleCIPipelineMetrics(c *gin.Context) {
	a.handleCIPipelineEvent(c, "measure")
}

// HandleCICustomSpans accepts `datadog-ci trace`.
//
// UNVERIFIED, and called out because it is the one response contract in this
// file with no primary source: the CLI's custom-span Payload type was not
// readable, so the hot columns (span_name, span_start_raw, span_end_raw) are
// filled from the obvious key names and left empty when they are not there.
// attributes holds the truth either way.
func (a *Server) HandleCICustomSpans(c *gin.Context) {
	a.handleCIPipelineEvent(c, "custom_span")
}

// handleCIPipelineEvent decodes one CLI call. All three share an envelope and
// differ only in which attributes they populate, so they share a handler —
// the kind column says which route it arrived on.
//
// 202 with an empty JSON object: the CLI treats any 2xx as success and reads
// no body (it retries five times on a thrown error, so a 5xx here would stall
// somebody's CI job for half a minute).
func (a *Server) handleCIPipelineEvent(c *gin.Context, kind string) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[cipipeline] %s: cannot read body: %v", kind, err)
		return
	}
	if len(body) == 0 {
		log.Printf("[cipipeline] %s: empty body", kind)
		return
	}

	var envelope ciPipelinePayload
	if err := json.Unmarshal(body, &envelope); err != nil {
		log.Printf("[cipipeline] %s: %v (%d bytes)", kind, err, len(body))
		a.storeRaw(c, "cipipeline", "decode_error", kind+" envelope: "+err.Error(), body)
		return
	}

	attrs := map[string]any{}
	if len(envelope.Data.Attributes) > 0 {
		v, err := civDecodeJSON(envelope.Data.Attributes)
		if err != nil {
			log.Printf("[cipipeline] %s attributes: %v", kind, err)
			a.storeRaw(c, "cipipeline", "decode_error", kind+" attributes: "+err.Error(), body)
			return
		}
		attrs = civMap(v)
	}

	// measure calls the map "metrics"; a custom span may call the same idea
	// "measures". Both land in the metrics column.
	metrics := civFloatMap(attrs["metrics"])
	if metrics == nil {
		metrics = civFloatMap(attrs["measures"])
	}

	row := storage.CIPipelineEventRow{
		Kind:         kind,
		DataType:     envelope.Data.Type,
		Provider:     civFieldString(attrs, "provider"),
		CILevel:      civInt64Ptr(civField(attrs, "ci_level")),
		CIEnv:        civStringMap(attrs["ci_env"]),
		Tags:         civStringMap(attrs["tags"]),
		Metrics:      metrics,
		SpanName:     civFieldString(attrs, "name"),
		SpanStartRaw: civFieldString(attrs, "start_time"),
		SpanEndRaw:   civFieldString(attrs, "end_time"),
		Attributes:   string(envelope.Data.Attributes),
		Body:         string(body),
		Extra:        civUnknown(attrs, civPipelineKnownKeys),
	}

	log.Printf("[cipipeline] %s type=%s provider=%s level=%s tags=%d metrics=%d ci_env=%d",
		kind, orDash(row.DataType), orDash(row.Provider),
		civLevelString(row.CILevel), len(row.Tags), len(row.Metrics), len(row.CIEnv))

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	row.TenantID = tenant
	row.ReceivedAt = time.Now().UTC()
	a.store(storage.CIPipelineEventsWriter,
		storage.WriteCIPipelineEvents{Events: []storage.CIPipelineEventRow{row}}, 1)
}

func civLevelString(level *int64) string {
	if level == nil {
		return "-"
	}
	// datadog-ci's LEVEL_TO_NUMBER.
	switch *level {
	case 1:
		return "pipeline"
	case 2:
		return "stage"
	case 3:
		return "job"
	default:
		return strconv.FormatInt(*level, 10)
	}
}
