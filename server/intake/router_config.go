package intake

import (
	"log"
	"net/http"
	"strings"

	pbgo "github.com/DataDog/datadog-agent/pkg/proto/pbgo/core"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/proto"
)

// config.<site> — remote configuration.
//
//	config: DD_REMOTE_CONFIGURATION_ENABLED (on/off only, no dd_url)
//
// THE ONLY INTAKE WHERE DATA FLOWS DOWNWARDS. Everything else in this package
// receives; this one is asked, and answers. The agent polls it and applies
// whatever comes back — integration configs, sampling rates, and in Kubernetes
// the actions the cluster agent then performs.
//
// Which is why it is also the one with a trust problem: the agent verifies
// what it receives against a TUF root, and a wrong answer here is not a
// missing metric but a config change nobody asked for. Whether that root can
// be replaced is measured in docs/zadania/remote-config-tuf.md.
//
// Until we serve real configuration, answering 404 is the honest thing: the
// agent then keeps whatever it already has rather than acting on an empty
// response.
//
// Wire format is protobuf from pkg/proto/pbgo/core, the same module the trace
// intake already uses (pkg/config/remote/api/http.go in datadog-agent):
//
//	POST /api/v0.1/configurations  LatestConfigsRequest  -> LatestConfigsResponse
//	GET  /api/v0.1/org             (no body)             -> OrgDataResponse
//	GET  /api/v0.1/status          (no body)             -> OrgStatusResponse
func (a *Server) routeConfig(g *gin.RouterGroup) {
	g.POST("/api/v0.1/configurations", a.HandleRemoteConfig)
	// The agent issues GET here; POST stays registered so the explicit 404
	// message reaches anything else that tries.
	g.GET("/api/v0.1/org", a.HandleRemoteConfigOrg)
	g.POST("/api/v0.1/org", a.HandleRemoteConfigOrg)
	g.GET("/api/v0.1/status", a.HandleRemoteConfigStatus)
	g.POST("/api/v0.1/status", a.HandleRemoteConfigStatus)
}

// HandleRemoteConfig is the poll the agent makes for new configuration.
//
// The request is the agent's full picture of itself: identity, the TUF
// versions it currently holds, the products it wants, and every client
// (tracers, the agent itself, the updater) that registered with it. Decoded
// here so we can see what the agent would ask a real backend for.
func (a *Server) HandleRemoteConfig(c *gin.Context) {
	// TODO(ninjacat): serving configuration needs TUF signing first. Until
	// then a 404 leaves the agent on its existing config.
	defer c.JSON(http.StatusNotFound, gin.H{"error": "remote configuration not served"})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[remote-config] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	// Decoded straight into the Datadog type and kept whole: every field of
	// LatestConfigsRequest and its nested Client / ClientState / ClientAgent /
	// ClientTracer / ClientUpdater / PackageState / ConfigState survives
	// unmarshal untouched, including the opaque BackendClientState bytes and
	// the uint64 versions and timestamps (never routed through float64).
	req := &pbgo.LatestConfigsRequest{}
	if err := proto.Unmarshal(body, req); err != nil {
		log.Printf("[remote-config] protobuf: %v (%d bytes)", err, len(body))
		describe("remote-config", c.GetHeader("Content-Type"), body)
		return
	}

	if req.GetHostname() == "" && len(req.GetProducts()) == 0 && len(req.GetActiveClients()) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a
		// payload meant for another endpoint decodes "successfully" into an
		// empty struct. An empty decode is the one signal we get.
		log.Printf("[remote-config] decoded to an empty request (%d bytes) — wrong payload type?", len(body))
		describe("remote-config", c.GetHeader("Content-Type"), body)
		return
	}

	// The log below is a partial view for the operator; it does not consume
	// or reshape req.
	rcLogRequest(req)

	_ = req // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// rcLogRequest reports one configuration poll. Read-only: it prints a
// selection of fields and leaves the request untouched. Nested messages are
// read through getters, which are nil-safe.
func rcLogRequest(req *pbgo.LatestConfigsRequest) {
	log.Printf("[remote-config] host=%s agent=%s agent_uuid=%s org=%s trace_env=%s",
		req.GetHostname(), req.GetAgentVersion(), orUnknown(req.GetAgentUuid()),
		orUnknown(req.GetOrgUuid()), orUnknown(req.GetTraceAgentEnv()))
	log.Printf("   products=%s new=%s",
		rcJoin(req.GetProducts()), rcJoin(req.GetNewProducts()))
	log.Printf("   versions snapshot=%d config_root=%d director_root=%d backend_state=%d B",
		req.GetCurrentConfigSnapshotVersion(), req.GetCurrentConfigRootVersion(),
		req.GetCurrentDirectorRootVersion(), len(req.GetBackendClientState()))
	if tags := req.GetTags(); len(tags) > 0 {
		log.Printf("   tags %s", strings.Join(tags, ","))
	}
	if req.GetHasError() {
		// The agent failed to apply what it got last time and is telling the
		// backend so. Worth hearing even while we serve nothing.
		log.Printf("[remote-config] agent reports error: %s", req.GetError())
	}

	clients := req.GetActiveClients()
	log.Printf("   %d active clients", len(clients))
	for _, cl := range clients {
		rcLogClient(cl)
	}
}

// rcLogClient reports one registered client. The kind is a set of booleans
// with a matching detail message; a client is normally exactly one of them.
func rcLogClient(cl *pbgo.Client) {
	var (
		kind   = "unknown"
		detail string
	)
	switch {
	case cl.GetIsTracer() && cl.GetClientTracer() != nil:
		t := cl.GetClientTracer()
		kind = "tracer"
		detail = "lang=" + t.GetLanguage() + " version=" + t.GetTracerVersion() +
			" service=" + t.GetService() + " env=" + t.GetEnv() + " app=" + t.GetAppVersion()
	case cl.GetIsAgent() && cl.GetClientAgent() != nil:
		ag := cl.GetClientAgent()
		kind = "agent"
		detail = "name=" + ag.GetName() + " version=" + ag.GetVersion()
		if ag.GetClusterName() != "" {
			detail += " cluster=" + ag.GetClusterName()
		}
	case cl.GetIsUpdater() && cl.GetClientUpdater() != nil:
		up := cl.GetClientUpdater()
		kind = "updater"
		detail = "packages=" + rcJoin(rcPackageNames(up.GetPackages()))
	}

	log.Printf("   %-8s id=%s products=%s %s", kind, cl.GetId(), rcJoin(cl.GetProducts()), detail)
	if st := cl.GetState(); st != nil {
		log.Printf("            root=%d targets=%d configs=%d",
			st.GetRootVersion(), st.GetTargetsVersion(), len(st.GetConfigStates()))
		if st.GetHasError() {
			log.Printf("            client error: %s", st.GetError())
		}
	}
}

func rcPackageNames(pkgs []*pbgo.PackageState) []string {
	names := make([]string, 0, len(pkgs))
	for _, p := range pkgs {
		names = append(names, p.GetPackage()+"@"+p.GetStableVersion())
	}
	return names
}

func rcJoin(s []string) string {
	if len(s) == 0 {
		return "-"
	}
	return strings.Join(s, ",")
}

// HandleRemoteConfigOrg answers the org identity lookup.
//
// The agent sends a GET with an empty body (FetchOrgData in datadog-agent),
// so there is nothing to decode; the whole exchange is the OrgDataResponse we
// do not serve yet.
func (a *Server) HandleRemoteConfigOrg(c *gin.Context) {
	log.Printf("[remote-config] org poll (%s, no request body)", c.Request.Method)
	c.JSON(http.StatusNotFound, gin.H{"error": "not served"})
}

// HandleRemoteConfigStatus answers the org/key status check.
//
// Same shape as org: GET, empty body (FetchOrgStatus), OrgStatusResponse
// expected back.
func (a *Server) HandleRemoteConfigStatus(c *gin.Context) {
	log.Printf("[remote-config] status poll (%s, no request body)", c.Request.Method)
	c.JSON(http.StatusNotFound, gin.H{"error": "not served"})
}
