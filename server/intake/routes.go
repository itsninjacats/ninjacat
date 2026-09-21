package intake

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/capture"
)

// Handler builds the router.
//
// Datadog puts every product on its own hostname. We mirror that, so setting
// DD_SITE to a ninjacat domain makes the agent compose the rest by itself,
// exactly as it does against datadoghq.com.
//
//	app.<site>                  dd_url                        router_app.go
//	api.<site>                  dd_url                        router_api.go
//	trace.agent.<site>          apm_config.apm_dd_url         router_trace.go
//	http-intake.logs.<site>     logs_config.logs_dd_url       router_logs.go
//	agent-http-intake.logs.<site>                             router_logs.go
//	process.<site>              process_config.process_dd_url router_process.go
//	orchestrator.<site>         orchestrator_dd_url           router_kubeops.go
//	kubeops-intake.<site>       kubeactions.forwarder         router_kubeops.go
//	config.<site>               (on/off only)                 router_config.go
//	contlcycle-intake.<site>    (event platform, no dd_url)   router_containers.go
//	contimage-intake.<site>     (event platform)              router_containers.go
//	dbm-metrics-intake.<site>   (event platform)              router_dbm.go
//	ndm-intake.<site>           (event platform)              router_ndm.go
//	snmp-traps-intake.<site>    (event platform)              router_ndm.go
//	ndmflow-intake.<site>       (event platform)              router_ndm.go
//	netpath-intake.<site>       (event platform)              router_ndm.go
//	resources-intake.<site>     (event platform)              router_misc.go
//	instrumentation-telemetry-intake.<site>                   router_misc.go
//	intake.profile.<site>       profiling_dd_url              router_profiling.go
//	debugger-intake.<site>      debugger_diagnostics_dd_url   router_profiling.go
//	sourcemap-intake.<site>     symbol_endpoints[].site       router_profiling.go
//	cws-intake.<site>           activity_dump remote_storage  router_security.go
//	runtime-security-http-intake.logs.<site>                  router_security.go
//	cspm-intake.<site>          compliance_config             router_security.go
//	sbom-intake.<site>          (event platform)              router_security.go
//	sds-intake.<site>           (event platform)              router_security.go
//	agentdiscovery-intake.<site>                              router_evp.go
//	agenthealth-intake.<site>   dd_url (own forwarder)        router_evp.go
//	event-management-intake.<site>                            router_evp.go
//	softinv-intake.<site>       (event platform)              router_evp.go
//	http-synthetics.<site>      (event platform)              router_evp.go
//	data-obs-intake.<site>      ol_proxy_config.dd_url        router_evp.go
//	eudm-intake.<site>          (via local evp_proxy)         router_install.go
//	llmobs-intake.<site>        (diagnose probe only)         router_install.go
//	install.datadoghq.com       installer.registry.url        router_install.go
//
// Multi-Region Failover doubles most of these: the agent dual-ships to a host
// with "mrf." spliced in after the product prefix (app.mrf.<site>,
// sbom-intake.logs.mrf.<site>, trace.agent.mrf.<site>). The prefix match
// below stops at that prefix, so the failover copy lands on the same intake
// as the primary. See docs/spis-endpointow-datadoga.md §0.6.
//
// Anything else is refused by name, so a misconfigured DD_SITE fails loudly
// instead of landing on the wrong decoder.
func (a *Server) Handler() http.Handler {
	api := a.engine(a.routeAPI)
	app := a.engine(a.routeAPI, a.routeApp)
	trace := a.engine(a.routeTrace)
	logs := a.engine(a.routeLogs)
	process := a.engine(a.routeProcess)
	kubeops := a.engine(a.routeKubeops)
	config := a.engine(a.routeConfig)
	containers := a.engine(a.routeContainers)
	dbm := a.engine(a.routeDBM)
	ndm := a.engine(a.routeNDM)
	resources := a.engine(a.routeResources)
	telemetry := a.engine(a.routeTelemetry)
	profile := a.engine(a.routeProfile)
	debugger := a.engine(a.routeDebugger)
	sourcemap := a.engine(a.routeSourcemap)
	cws := a.engine(a.routeCWS)
	secLogs := a.engine(a.routeRuntimeSecurity)
	cspm := a.engine(a.routeCSPM)
	sbom := a.engine(a.routeSBOM)
	sds := a.engine(a.routeSDS)
	discovery := a.engine(a.routeAgentDiscovery)
	health := a.engine(a.routeAgentHealth)
	events := a.engine(a.routeEventManagement)
	softinv := a.engine(a.routeSoftwareInventory)
	synthetics := a.engine(a.routeSynthetics)
	dataobs := a.engine(a.routeDataObs)
	aiusage := a.engine(a.routeAIUsage)
	llmobs := a.engine(a.routeLLMObs)

	// install.datadoghq.com is the one host that never sends a key: the OCI
	// registry, the BTF download and the HEAD probe are all anonymous.
	install := a.publicEngine(a.routeInstall)

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		host, _, _ := strings.Cut(r.Host, ":") // drop the port
		host = strings.TrimSuffix(host, ".")   // convert_dd_site_fqdn adds a trailing dot

		switch {
		case host == "install.datadoghq.com", host == "install.datad0g.com":
			install.ServeHTTP(w, r)

		// The core forwarder rewrites app.<site> to <maj>-<min>-<patch>-app.agent.<site>
		// when the host matches Datadog's own pattern. A custom dd_url is left alone,
		// so both spellings have to work.
		case strings.HasPrefix(host, "app."), strings.Contains(host, "-app.agent."):
			app.ServeHTTP(w, r)

		case strings.HasPrefix(host, "api."):
			api.ServeHTTP(w, r)
		case strings.HasPrefix(host, "trace.agent."):
			trace.ServeHTTP(w, r)
		case strings.HasPrefix(host, "http-intake.logs."),
			strings.HasPrefix(host, "agent-http-intake.logs."):
			logs.ServeHTTP(w, r)
		case strings.HasPrefix(host, "process."):
			process.ServeHTTP(w, r)
		case strings.HasPrefix(host, "kubeops-intake."),
			strings.HasPrefix(host, "orchestrator."):
			kubeops.ServeHTTP(w, r)
		case strings.HasPrefix(host, "config."):
			config.ServeHTTP(w, r)
		case strings.HasPrefix(host, "contlcycle-intake."),
			strings.HasPrefix(host, "contimage-intake."):
			containers.ServeHTTP(w, r)
		case strings.HasPrefix(host, "dbm-metrics-intake."):
			dbm.ServeHTTP(w, r)
		case strings.HasPrefix(host, "ndm-intake."),
			strings.HasPrefix(host, "snmp-traps-intake."),
			strings.HasPrefix(host, "ndmflow-intake."),
			strings.HasPrefix(host, "netpath-intake."):
			ndm.ServeHTTP(w, r)
		case strings.HasPrefix(host, "resources-intake."):
			resources.ServeHTTP(w, r)
		case strings.HasPrefix(host, "instrumentation-telemetry-intake."):
			telemetry.ServeHTTP(w, r)
		case strings.HasPrefix(host, "intake.profile."):
			profile.ServeHTTP(w, r)
		case strings.HasPrefix(host, "debugger-intake."):
			debugger.ServeHTTP(w, r)
		case strings.HasPrefix(host, "sourcemap-intake."):
			sourcemap.ServeHTTP(w, r)
		case strings.HasPrefix(host, "cws-intake."):
			cws.ServeHTTP(w, r)
		case strings.HasPrefix(host, "runtime-security-http-intake.logs."):
			secLogs.ServeHTTP(w, r)
		case strings.HasPrefix(host, "cspm-intake."):
			cspm.ServeHTTP(w, r)
		case strings.HasPrefix(host, "sbom-intake."):
			sbom.ServeHTTP(w, r)
		case strings.HasPrefix(host, "sds-intake."):
			sds.ServeHTTP(w, r)
		case strings.HasPrefix(host, "agentdiscovery-intake."):
			discovery.ServeHTTP(w, r)
		case strings.HasPrefix(host, "agenthealth-intake."):
			health.ServeHTTP(w, r)
		case strings.HasPrefix(host, "event-management-intake."):
			events.ServeHTTP(w, r)
		case strings.HasPrefix(host, "softinv-intake."):
			softinv.ServeHTTP(w, r)
		case strings.HasPrefix(host, "http-synthetics."):
			synthetics.ServeHTTP(w, r)
		case strings.HasPrefix(host, "data-obs-intake."):
			dataobs.ServeHTTP(w, r)
		case strings.HasPrefix(host, "eudm-intake."):
			aiusage.ServeHTTP(w, r)
		case strings.HasPrefix(host, "llmobs-intake."):
			llmobs.ServeHTTP(w, r)
		default:
			// Every intake has its own hostname. Serving them all on one
			// name needs a route that is claimed twice (POST /v1/input, by
			// the logs intake and by the profiler), and telling those apart
			// by Content-Type hid more than it helped.
			unknownHost(w, host)
		}
	})
}

// publicEngine is engine without the API-key group.
//
// Only install.<site> uses it. Everything else must present a key.
func (a *Server) publicEngine(routes ...func(*gin.RouterGroup)) *gin.Engine {
	r := a.baseEngine()
	g := r.Group("")
	for _, add := range routes {
		add(g)
	}
	r.NoRoute(a.HandleUnknown)
	return r
}

// baseEngine is the part every engine shares: middleware and the two probes
// that never need a key.
func (a *Server) baseEngine() *gin.Engine {
	r := gin.New()
	r.Use(gin.Recovery())
	r.Use(Decompress())
	if os.Getenv("DEBUG") == "true" {
		r.Use(gin.Logger())
		r.Use(capture.RequestLogger())
	}

	// No API key: for checking the receiver is alive.
	r.GET("/ping", a.HandlePing)
	r.GET("/_health", a.HandleHealth)
	return r
}

// engine builds one Gin router from the given route sets.
func (a *Server) engine(routes ...func(*gin.RouterGroup)) *gin.Engine {
	r := a.baseEngine()

	// Everything else needs a valid Dd-Api-Key — see apikey_mw.go.
	g := r.Group("", RequireAPIKey(a.Store))
	for _, add := range routes {
		add(g)
	}

	// The agent sweeps these at startup, on whatever host it is talking to,
	// before it knows which intake that is.
	g.GET("/api/v1/validate", a.HandleValidate)
	g.HEAD("/support/flare", a.HandleFlare)
	g.POST("/support/flare", a.HandleFlare)

	r.NoRoute(a.HandleUnknown)
	return r
}

// unknownHost answers a request whose Host matches no intake.
//
// The body names the host we saw, because that is the one thing the operator
// cannot read off their own config: DD_SITE, dd_url and the per-product
// overrides each build it differently.
func unknownHost(w http.ResponseWriter, host string) {
	log.Printf("UNKNOWN HOST: %q — no intake serves this name", host)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusNotFound)
	fmt.Fprintf(w, `{"errors":["no intake for host %q"]}`+"\n", host)
}
