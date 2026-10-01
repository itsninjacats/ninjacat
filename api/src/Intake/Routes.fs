/// The intake as a whole: every Datadog product on its own hostname.
///
/// Datadog puts each product on its own host, so pointing DD_SITE at a
/// ninjacat domain makes the agent compose the rest by itself, exactly as it
/// does against datadoghq.com. A host that matches no intake is refused by
/// name, so a wrong DD_SITE fails loudly instead of landing on the wrong
/// decoder.
module NinjaCat.Api.Intake.Routes

open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake.Routers

/// One set of routes, under the name the Go server gave it. The golden
/// fixtures recorded those names; nothing else uses them.
type RouteSet =
    { GoName: string
      Auth: Auth
      Routes: Route list }

/// webhook-intake: Jenkins sends the header, but a GitLab project webhook has
/// no field for one, so the key travels as `?dd-api-key=`.
let private ciWebhookAuth: Auth =
    Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key"; KeyFromQuery "api_key" ]

/// data-obs-intake: OpenLineage's transport sends `Authorization: Bearer`,
/// and the trace-agent's lineage proxy forwards it unchanged.
let private dataObsAuth: Auth =
    Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ]

let routeSets: RouteSet list =
    let standard (name: string) (routes: Route list) =
        { GoName = name; Auth = Auth.standard; Routes = routes }

    [ standard "routeAPI" Api.routes
      standard "routeCIVisibilityAPI" CiVisibility.apiRoutes
      standard "routeApp" App.routes
      standard "routeTrace" Trace.routes
      standard "routeLogs" Logs.routes
      standard "routeProcess" Process.routes
      standard "routeKubeops" Kubeops.routes
      standard "routeConfig" Config.routes
      standard "routeContainers" Containers.routes
      standard "routeDBM" Dbm.routes
      standard "routeNDM" Ndm.routes
      standard "routeResources" Misc.resourcesRoutes
      standard "routeTelemetry" Misc.telemetryRoutes
      standard "routeProfile" Profiling.profileRoutes
      standard "routeDebugger" Profiling.debuggerRoutes
      standard "routeSourcemap" Profiling.sourcemapRoutes
      standard "routeCWS" Security.cwsRoutes
      standard "routeRuntimeSecurity" Security.runtimeSecurityRoutes
      standard "routeCSPM" Security.cspmRoutes
      standard "routeSBOM" Security.sbomRoutes
      standard "routeSDS" Security.sdsRoutes
      standard "routeAgentDiscovery" Evp.agentDiscoveryRoutes
      standard "routeAgentHealth" Evp.agentHealthRoutes
      standard "routeEventManagement" Evp.eventManagementRoutes
      standard "routeSoftwareInventory" Evp.softwareInventoryRoutes
      standard "routeSynthetics" Evp.syntheticsRoutes
      standard "routeCITestCycle" CiVisibility.testCycleRoutes
      standard "routeCITestCov" CiVisibility.testCovRoutes
      { GoName = "routeCIWebhook"; Auth = ciWebhookAuth; Routes = CiWebhook.routes }
      standard "routeSyntheticsAgent" CiWebhook.syntheticsAgentRoutes
      { GoName = "routeDataObs"; Auth = dataObsAuth; Routes = Evp.dataObsRoutes }
      { GoName = "routeRUM"; Auth = Rum.gate; Routes = Rum.routes }
      standard "routeAIUsage" Install.aiUsageRoutes
      standard "routeLLMObs" Install.llmObsRoutes
      // The OCI registry, the BTF download and the HEAD probe are anonymous.
      { GoName = "routeInstall"; Auth = Auth.none; Routes = Install.routes } ]

/// A whole intake host: takes the request and its raw body.
type Intake = HttpContext -> byte[] -> Response

/// The intake made of the named route sets, guarded as the first one is.
let byGoNames (deps: Deps) (names: string list) : Intake =
    let sets = names |> List.map (fun name -> routeSets |> List.find (fun s -> s.GoName = name))
    let engine = Engine.create sets.Head.Auth (sets |> List.map _.Routes)
    Engine.respond deps engine

/// The host of a request without its port, and without the trailing dot that
/// `convert_dd_site_fqdn` adds.
let hostName (http: HttpContext) : string = http.Request.Host.Host.TrimEnd '.'

let private unknownHost (deps: Deps) (host: string) : Response =
    deps.Log.LogWarning("UNKNOWN HOST: \"{Host}\" — no intake serves this name", host)
    // The body names the host we saw: the one thing the operator cannot read
    // off their own config, where DD_SITE and each override build it differently.
    Response.bytes 404 "application/json" (System.Text.Encoding.UTF8.GetBytes($"{{\"errors\":[\"no intake for host \\\"{host}\\\"\"]}}\n"))

/// The whole intake: the request's host picks the product.
///
/// Hosts match by PREFIX, so the Multi-Region Failover copy of a host
/// (`app.mrf.<site>`, `trace.agent.mrf.<site>`) lands on the same intake as
/// the primary.
///
///   host prefix                          agent config key              router
///   app.  /  *-app.agent.                dd_url                        App.fs, Api.fs
///   api.  /  *-flare.agent.              dd_url                        Api.fs, CiVisibility.fs
///   trace.agent.                         apm_config.apm_dd_url         Trace.fs
///   http-intake.logs.                    logs_config.logs_dd_url       Logs.fs
///   agent-http-intake.logs.                                            Logs.fs
///   process.                             process_config.process_dd_url Process.fs
///   orchestrator. / kubeops-intake.      orchestrator_dd_url           Kubeops.fs
///   config.                              (remote config, on/off only)  Config.fs
///   contlcycle-intake. contimage-intake. (event platform)              Containers.fs
///   dbm-metrics-intake.                  (event platform)              Dbm.fs
///   ndm-intake. snmp-traps-intake.       (event platform)              Ndm.fs
///   ndmflow-intake. netpath-intake.      (event platform)              Ndm.fs
///   resources-intake.                    (event platform)              Misc.fs
///   instrumentation-telemetry-intake.                                  Misc.fs
///   intake.profile.                      profiling_dd_url              Profiling.fs
///   debugger-intake.                     debugger_diagnostics_dd_url   Profiling.fs
///   sourcemap-intake.                    symbol_endpoints[].site       Profiling.fs
///   cws-intake.                          activity_dump remote_storage  Security.fs
///   runtime-security-http-intake.logs.                                 Security.fs
///   cspm-intake.                         compliance_config             Security.fs
///   sbom-intake.  sds-intake.            (event platform)              Security.fs
///   agentdiscovery-intake.                                             Evp.fs
///   agenthealth-intake.                  dd_url (own forwarder)        Evp.fs
///   event-management-intake.                                           Evp.fs
///   softinv-intake.  http-synthetics.    (event platform)              Evp.fs
///   data-obs-intake.                     ol_proxy_config.dd_url        Evp.fs
///   citestcycle-intake. citestcov-intake. CI Visibility (tracers)      CiVisibility.fs
///   webhook-intake.                      CI provider webhooks          CiWebhook.fs
///   intake.synthetics.                   synthetics.collector.enabled  CiWebhook.fs
///   eudm-intake.  llmobs-intake.                                       Install.fs
///   install.datadoghq.com                installer.registry.url        Install.fs
///   browser-intake.                      RUM and Logs SDKs             Rum.fs
///
/// Keep this table in step with the list below when adding a router.
let create (deps: Deps) : Intake =
    let engine (names: string list) = byGoNames deps names

    // api.<site> also carries the CI Visibility endpoints tracers consult;
    // app.<site> deliberately does not.
    let api = engine [ "routeAPI"; "routeCIVisibilityAPI" ]
    let app = engine [ "routeAPI"; "routeApp" ]
    let logs = engine [ "routeLogs" ]
    let kubeops = engine [ "routeKubeops" ]
    let containers = engine [ "routeContainers" ]
    let ndm = engine [ "routeNDM" ]
    let install = engine [ "routeInstall" ]
    // browser-intake is the one host whose clients are not agents: it needs
    // CORS and a URL rewrite above its router.
    let rum =
        Rum.wrapWith
            deps.Log
            (Rum.parseAllowedOrigins (System.Environment.GetEnvironmentVariable "NINJACAT_RUM_ALLOWED_ORIGINS"))
            (engine [ "routeRUM" ])

    let byPrefix: (string * Intake) list =
        [ "app.", app
          "api.", api
          "trace.agent.", engine [ "routeTrace" ]
          "http-intake.logs.", logs
          "agent-http-intake.logs.", logs
          "process.", engine [ "routeProcess" ]
          "kubeops-intake.", kubeops
          "orchestrator.", kubeops
          "config.", engine [ "routeConfig" ]
          "contlcycle-intake.", containers
          "contimage-intake.", containers
          "dbm-metrics-intake.", engine [ "routeDBM" ]
          "ndm-intake.", ndm
          "snmp-traps-intake.", ndm
          "ndmflow-intake.", ndm
          "netpath-intake.", ndm
          "resources-intake.", engine [ "routeResources" ]
          "instrumentation-telemetry-intake.", engine [ "routeTelemetry" ]
          "intake.profile.", engine [ "routeProfile" ]
          "debugger-intake.", engine [ "routeDebugger" ]
          "sourcemap-intake.", engine [ "routeSourcemap" ]
          "cws-intake.", engine [ "routeCWS" ]
          "runtime-security-http-intake.logs.", engine [ "routeRuntimeSecurity" ]
          "cspm-intake.", engine [ "routeCSPM" ]
          "sbom-intake.", engine [ "routeSBOM" ]
          "sds-intake.", engine [ "routeSDS" ]
          "agentdiscovery-intake.", engine [ "routeAgentDiscovery" ]
          "agenthealth-intake.", engine [ "routeAgentHealth" ]
          "event-management-intake.", engine [ "routeEventManagement" ]
          "softinv-intake.", engine [ "routeSoftwareInventory" ]
          "http-synthetics.", engine [ "routeSynthetics" ]
          // The poller the agent READS its test list from; results come back
          // on http-synthetics.
          "intake.synthetics.", engine [ "routeSyntheticsAgent" ]
          "citestcycle-intake.", engine [ "routeCITestCycle" ]
          "citestcov-intake.", engine [ "routeCITestCov" ]
          "webhook-intake.", engine [ "routeCIWebhook" ]
          "data-obs-intake.", engine [ "routeDataObs" ]
          "eudm-intake.", engine [ "routeAIUsage" ]
          "llmobs-intake.", engine [ "routeLLMObs" ]
          "browser-intake.", rum ]

    fun http rawBody ->
        let host = hostName http

        if host = "install.datadoghq.com" || host = "install.datad0g.com" then
            install http rawBody
        // The agent's forwarder rewrites app.<site> to <version>-app.agent.<site>
        // when the host looks like Datadog's own, and a flare upload to
        // <version>-flare.agent.<site>. A custom dd_url is left alone, so both
        // spellings have to work.
        elif host.Contains "-app.agent." then
            app http rawBody
        elif host.Contains "-flare.agent." then
            api http rawBody
        else
            match byPrefix |> List.tryFind (fun (prefix, _) -> host.StartsWith prefix) with
            | Some(_, intake) -> intake http rawBody
            | None -> unknownHost deps host
