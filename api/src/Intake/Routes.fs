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

/// A whole intake host: takes the request and its raw body.
type Intake = HttpContext -> byte[] -> Response

/// The intake that serves `routes`, with `auth` deciding where the key is read.
let serve (deps: Deps) (auth: Auth) (routes: Route list) : Intake =
    Engine.respond deps (Engine.create auth routes)

/// webhook-intake: Jenkins sends the header, but a GitLab project webhook has
/// no field for one, so the key travels as `?dd-api-key=`.
let private ciWebhookAuth: Auth =
    Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key"; KeyFromQuery "api_key" ]

/// data-obs-intake: OpenLineage's transport sends `Authorization: Bearer`,
/// and the trace-agent's lineage proxy forwards it unchanged.
let private dataObsAuth: Auth =
    Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ]

/// The host of a request without its port, and without the trailing dot that
/// `convert_dd_site_fqdn` adds.
let hostName (http: HttpContext) : string = http.Request.Host.Host.TrimEnd '.'

let private unknownHost (deps: Deps) (host: string) : Response =
    deps.Log.LogWarning("UNKNOWN HOST: \"{Host}\" — no intake serves this name", host)
    // The body names the host we saw: the one thing the operator cannot read
    // off their own config, where DD_SITE and each override build it differently.
    Response.bytes 404 "application/json" (System.Text.Encoding.UTF8.GetBytes($"{{\"errors\":[\"no intake for host \\\"{host}\\\"\"]}}\n"))

/// The whole intake: the request's host picks the product.
let create (deps: Deps) : Intake =
    let agent (routes: Route list) : Intake = serve deps Auth.standard routes

    // Each intake is built once, here. The function at the bottom only picks.

    // api.<site> also carries the CI Visibility endpoints tracers consult;
    // app.<site> deliberately does not.
    let app = agent (Api.routes @ App.routes)
    let api = agent (Api.routes @ CiVisibility.apiRoutes)

    let trace = agent Trace.routes
    let telemetry = agent Misc.telemetryRoutes
    let profile = agent Profiling.profileRoutes
    let debugger = agent Profiling.debuggerRoutes
    let sourcemap = agent Profiling.sourcemapRoutes

    let logs = agent Logs.routes
    let processes = agent Process.routes
    let kubeops = agent Kubeops.routes
    let containers = agent Containers.routes
    let remoteConfig = agent Config.routes

    let dbm = agent Dbm.routes
    let ndm = agent Ndm.routes
    let resources = agent Misc.resourcesRoutes

    let cws = agent Security.cwsRoutes
    let runtimeSecurity = agent Security.runtimeSecurityRoutes
    let cspm = agent Security.cspmRoutes
    let sbom = agent Security.sbomRoutes
    let sds = agent Security.sdsRoutes

    let agentDiscovery = agent Evp.agentDiscoveryRoutes
    let agentHealth = agent Evp.agentHealthRoutes
    let eventManagement = agent Evp.eventManagementRoutes
    let softwareInventory = agent Evp.softwareInventoryRoutes
    let syntheticsResults = agent Evp.syntheticsRoutes
    let syntheticsPoller = agent CiWebhook.syntheticsAgentRoutes

    let ciTestCycle = agent CiVisibility.testCycleRoutes
    let ciTestCoverage = agent CiVisibility.testCovRoutes
    let ciWebhook = serve deps ciWebhookAuth CiWebhook.routes
    let dataObs = serve deps dataObsAuth Evp.dataObsRoutes
    let aiUsage = agent Install.aiUsageRoutes
    let llmObs = agent Install.llmObsRoutes

    // The OCI registry, the BTF download and the HEAD probe are anonymous.
    let install = serve deps Auth.none Install.routes

    // browser-intake is the one host whose clients are not agents: it needs
    // CORS and a URL rewrite above its router.
    let rum =
        Rum.wrapWith
            deps.Log
            (Rum.parseAllowedOrigins (System.Environment.GetEnvironmentVariable "NINJACAT_RUM_ALLOWED_ORIGINS"))
            (serve deps Rum.gate Rum.routes)

    fun http rawBody ->
        let host = hostName http

        // Hosts match by PREFIX, so the Multi-Region Failover copy of a host
        // (`app.mrf.<site>`, `trace.agent.mrf.<site>`) lands on the same
        // intake as the primary.
        let on (prefix: string) : bool = host.StartsWith prefix

        // Beside each host: the agent setting that points at it. "event
        // platform" hosts are set per product, under <product>.logs_dd_url.

        // installer.registry.url
        if host = "install.datadoghq.com" || host = "install.datad0g.com" then
            install http rawBody

        // The core agent: dd_url. Its forwarder rewrites app.<site> to
        // <version>-app.agent.<site> when the host looks like Datadog's own,
        // and a flare upload to <version>-flare.agent.<site>. A custom dd_url
        // is left alone, so both spellings have to work.
        elif host.Contains "-app.agent." || on "app." then
            app http rawBody
        elif host.Contains "-flare.agent." || on "api." then
            api http rawBody

        // Tracing.
        elif on "trace.agent." then // apm_config.apm_dd_url
            trace http rawBody
        elif on "instrumentation-telemetry-intake." then // apm_config.telemetry.dd_url
            telemetry http rawBody
        elif on "intake.profile." then // apm_config.profiling_dd_url
            profile http rawBody
        elif on "debugger-intake." then // debugger_diagnostics_dd_url
            debugger http rawBody
        elif on "sourcemap-intake." then // symbol_endpoints[].site
            sourcemap http rawBody

        // Logs, processes, containers.
        elif on "http-intake.logs." || on "agent-http-intake.logs." then // logs_config.logs_dd_url
            logs http rawBody
        elif on "process." then // process_config.process_dd_url
            processes http rawBody
        elif on "orchestrator." || on "kubeops-intake." then // orchestrator_dd_url
            kubeops http rawBody
        elif on "contlcycle-intake." || on "contimage-intake." then // event platform
            containers http rawBody
        elif on "config." then // remote configuration: on or off, no URL
            remoteConfig http rawBody

        // Databases and network devices: event platform.
        elif on "dbm-metrics-intake." then
            dbm http rawBody
        elif on "ndm-intake." || on "snmp-traps-intake." || on "ndmflow-intake." || on "netpath-intake." then
            ndm http rawBody
        elif on "resources-intake." then
            resources http rawBody

        // Security.
        elif on "cws-intake." then // activity_dump remote_storage
            cws http rawBody
        elif on "runtime-security-http-intake.logs." then
            runtimeSecurity http rawBody
        elif on "cspm-intake." then // compliance_config
            cspm http rawBody
        elif on "sbom-intake." then // event platform
            sbom http rawBody
        elif on "sds-intake." then // event platform
            sds http rawBody

        // What else the agent reports about itself and its host.
        elif on "agentdiscovery-intake." then // event platform
            agentDiscovery http rawBody
        elif on "agenthealth-intake." then // dd_url (a forwarder of its own)
            agentHealth http rawBody
        elif on "event-management-intake." then // event platform
            eventManagement http rawBody
        elif on "softinv-intake." then // event platform
            softwareInventory http rawBody

        // Synthetics: the agent READS its test list from the poller and
        // sends results to http-synthetics. synthetics.collector.enabled
        elif on "intake.synthetics." then
            syntheticsPoller http rawBody
        elif on "http-synthetics." then
            syntheticsResults http rawBody

        // Senders that are not the agent: tracers in CI, CI providers'
        // webhooks, OpenLineage (ol_proxy_config.dd_url), AI usage, LLM
        // Observability, and the browser and mobile SDKs.
        elif on "citestcycle-intake." then
            ciTestCycle http rawBody
        elif on "citestcov-intake." then
            ciTestCoverage http rawBody
        elif on "webhook-intake." then
            ciWebhook http rawBody
        elif on "data-obs-intake." then
            dataObs http rawBody
        elif on "eudm-intake." then
            aiUsage http rawBody
        elif on "llmobs-intake." then
            llmObs http rawBody
        elif on "browser-intake." then
            rum http rawBody

        else
            unknownHost deps host
