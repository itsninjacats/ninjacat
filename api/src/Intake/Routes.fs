/// The whole intake in one place: every host Datadog puts a product on, and
/// every path served on it.
///
/// Paths are matched by ASP.NET's router, through Oxpecker. Hosts are not:
/// the router can match a whole host name, but an agent composes
/// `<product>.<site>` for whatever site it was given, so the product is the
/// START of the name. `configure`, at the bottom, picks the host and gives
/// each one a router of its own.
module NinjaCat.Api.Intake.Routes

open System.Threading.Tasks
open Microsoft.AspNetCore.Authorization
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Logging
open Oxpecker
open NinjaCat.Api.Intake.Routers

// ---- every host ----

/// /ping and /_health, which ask for no key on any host.
let probes: Endpoint list =
    [ GET
          [ route "/ping" (json {| message = "pong" |})
            route "/_health" (json {| status = "ok" |}) ] ]

/// What the agent sweeps at startup on whatever host it talks to, before it
/// knows which intake that is; and a flare, which goes to whichever host
/// dd_url resolves to. They ask for the key the host asks for.
let onEveryHost: Endpoint list =
    let flare =
        [ route "/support/flare" (bindBody (Flare.handle ""))
          routef "/support/flare/{%s}" (fun (caseId: string) -> bindBody (Flare.handle caseId)) ]

    [ GET [ route "/api/v1/validate" (json {| valid = true |}) ]
      HEAD flare
      POST flare ]

// ---- the core agent: dd_url ----

/// Served on app.<site> and on api.<site>.
let metricsAndChecks: Endpoint list =
    [ POST
          [ route "/intake/" (bindBody Api.handleIntake)
            route "/api/v1/series" (bindBody Api.handleSeriesV1)
            route "/api/v2/series" (bindBody Api.handleSeriesV2)
            route "/api/v1/check_run" (bindBody Api.handleCheckRun)
            route "/api/v1/metadata" (bindBody Api.handleMetadata)
            route "/api/beta/sketches" (bindBody Api.handleSketches)

            // Not sent by any agent: API clients only.
            route "/api/v1/events" (bindBody Api.handleEvents)
            route "/api/v1/distribution_points" (bindBody Api.handleDistributionPoints)

            route "/api/v2/intake-key" (bindBody Api.handleIntakeKey)
            route "/api/v2/profiles/symbols/query" (bindBody Api.handleSymbolsQuery)
            // The agent sends GET; POST gets the same answer.
            route "/api/v2/validate" (bindBody Api.handleOpmValidate)

            route "/api/unstable/on_prem_runners" (bindBody Api.handleRunnerEnroll)
            route "/api/unstable/on_prem_runners/api_key_only" (bindBody Api.handleRunnerEnroll)
            route "/api/v2/on-prem-management-service/workflow-tasks/dequeue" (bindBody Api.handleRunnerDequeue)
            route "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update" (bindBody Api.handleRunnerTaskUpdate)
            route "/api/v2/on-prem-management-service/workflow-tasks/heartbeat" (bindBody Api.handleRunnerHeartbeat)
            route "/api/v2/actions/connections" (bindBody Api.handleActionConnections) ]
      GET
          [ route "/api/v1/query" (bindBody Api.handleQuery)
            route "/api/v2/validate" (bindBody Api.handleOpmValidate)
            route "/api/v2/on-prem-management-service/runner/health-check" (bindBody Api.handleRunnerHealthCheck) ] ]

/// Metrics v3, on app.<site> only.
let metricsV3: Endpoint list =
    [ POST
          [ route "/api/intake/metrics/v3/series" (bindBody (App.handle "v3series"))
            route "/api/intake/metrics/v3/sketches" (bindBody (App.handle "v3sketches"))
            // A shadow copy of the v2 traffic on /api/beta/sketches.
            route "/api/intake/metrics/v3beta/sketches" (bindBody (App.handle "v3beta")) ] ]

/// app.<site>, <version>-app.agent.<site>
let app: Endpoint list = metricsAndChecks @ metricsV3

/// The CI Visibility endpoints on api.<site>, which app.<site> deliberately
/// does not carry. A tracer never calls the four after `setting` while every
/// flag in the settings answer is false; they exist because a tracer
/// configured by hand must not get a 404 in the middle of a test run.
let ciVisibilityApi: Endpoint list =
    [ POST
          [ route "/api/v2/libraries/tests/services/setting" (bindBody (CiVisibility.answerConfig "settings" CiVisibility.settingsAnswer))
            route "/api/v2/ci/tests/skippable" (bindBody (CiVisibility.answerConfig "skippable" CiVisibility.skippableAnswer))
            route "/api/v2/ci/libraries/tests" (bindBody (CiVisibility.answerConfig "known_tests" CiVisibility.testListAnswer))
            route "/api/v2/ci/libraries/tests/flaky" (bindBody (CiVisibility.answerConfig "flaky_tests" CiVisibility.testListAnswer))
            route
                "/api/v2/test/libraries/test-management/tests"
                (bindBody (CiVisibility.answerConfig "test_management" CiVisibility.testManagementAnswer))
            route "/api/v2/git/repository/search_commits" (bindBody CiVisibility.handleSearchCommits)
            route "/api/v2/git/repository/packfile" (bindBody CiVisibility.handlePackfile)
            route "/api/v2/ci/pipeline/tags" (bindBody (CiVisibility.handlePipelineEvent "tag"))
            route "/api/v2/ci/pipeline/metrics" (bindBody (CiVisibility.handlePipelineEvent "measure"))
            route "/api/intake/ci/custom_spans" (bindBody (CiVisibility.handlePipelineEvent "custom_span")) ] ]

/// api.<site>, <version>-flare.agent.<site>
let api: Endpoint list = metricsAndChecks @ ciVisibilityApi

// ---- tracing ----

/// trace.agent.<site>    apm_config.apm_dd_url
let trace: Endpoint list =
    [ POST
          [ route "/api/v0.2/traces" (bindBody Trace.handleTraces)
            route "/api/v0.2/stats" (bindBody Trace.handleStats)
            route "/api/v0.1/pipeline_stats" (bindBody Trace.handlePipelineStats)
            route "/api/v2/data_streams_messages" (bindBody Trace.handleDataStreamsMessages) ] ]

/// instrumentation-telemetry-intake.<site>    apm_config.telemetry.dd_url
let telemetry: Endpoint list =
    [ POST [ route "/api/v2/apmtelemetry" (bindBody Misc.handleTelemetry) ] ]

/// intake.profile.<site>    apm_config.profiling_dd_url
let profile: Endpoint list =
    [ POST
          [ // What the trace-agent forwards from /profiling/v1/input: the
            // tracer's own multipart, byte for byte.
            route "/api/v2/profile" (bindBody (Profiling.handleProfile "profile"))
            // The old path, still used by the agent's internal profiling.
            route "/v1/input" (bindBody (Profiling.handleProfile "profile-v1")) ] ]

/// debugger-intake.<site>    debugger_diagnostics_dd_url
let debugger: Endpoint list =
    [ POST [ route "/api/v2/debugger" (bindBody Profiling.handleDebugger) ] ]

/// sourcemap-intake.<site>    symbol_endpoints[].site
let sourcemap: Endpoint list =
    [ POST [ route "/api/v2/srcmap" (bindBody Profiling.handleSourcemap) ] ]

// ---- logs, processes, containers ----

/// http-intake.logs.<site>, agent-http-intake.logs.<site>    logs_config.logs_dd_url
let logs: Endpoint list =
    [ POST
          [ route "/api/v2/logs" (bindBody Logs.handle)
            route "/v1/input" (bindBody Logs.handle)
            // Older clients put the key in the path.
            routef "/v1/input/{%s}" (fun (_: string) -> bindBody Logs.handle) ] ]

/// process.<site>    process_config.process_dd_url
let processes: Endpoint list =
    [ POST
          [ route "/api/v1/collector" (bindBody Process.handleCollector)
            route "/api/v1/container" (bindBody Process.handleContainer)
            route "/api/v1/connections" (bindBody Process.handleConnections)
            route "/api/v1/discovery" (bindBody Process.handleDiscovery) ] ]

/// orchestrator.<site>, kubeops-intake.<site>    orchestrator_dd_url
let kubeops: Endpoint list =
    [ POST
          [ route "/api/v2/orch" (bindBody Kubeops.handleOrchestrator)
            route "/api/v2/orchmanif" (bindBody Kubeops.handleManifests)
            route "/api/v1/orchestrator" (bindBody Kubeops.handleOrchestrator)
            route "/api/v2/kubeactions" (bindBody Kubeops.handleActions) ] ]

/// contlcycle-intake.<site>, contimage-intake.<site>    event platform
let containers: Endpoint list =
    [ POST
          [ route "/api/v2/contlcycle" (bindBody Containers.handleLifecycle)
            route "/api/v2/contimage" (bindBody Containers.handleImages) ] ]

/// config.<site>    remote configuration: on or off, no URL
let remoteConfig: Endpoint list =
    [ POST
          [ route "/api/v0.1/configurations" (bindBody Config.handleConfigurations)
            // The agent sends GET; POST is registered so anything else that
            // tries gets the same explicit answer.
            route "/api/v0.1/org" (bindBody Config.notServed)
            route "/api/v0.1/status" (bindBody Config.notServed) ]
      GET
          [ route "/api/v0.1/org" (bindBody Config.notServed)
            route "/api/v0.1/status" (bindBody Config.notServed) ] ]

// ---- databases and network devices: event platform ----

/// dbm-metrics-intake.<site>
let dbm: Endpoint list =
    [ POST
          [ route "/api/v2/dbmmetrics" (bindBody (Dbm.handle "dbmmetrics")) // query and lock metrics
            route "/api/v2/dbmactivity" (bindBody (Dbm.handle "dbmactivity")) // active sessions
            route "/api/v2/databasequery" (bindBody (Dbm.handle "databasequery")) // query samples and plans
            route "/api/v2/dbmmetadata" (bindBody (Dbm.handle "dbmmetadata")) // instances and schemas
            // No producer of these two is in the agent's source: only the
            // envelope can be claimed about them.
            route "/api/v2/dbmhealth" (bindBody (Dbm.handle "dbmhealth"))
            route "/api/v2/dbmcolumnstatistics" (bindBody (Dbm.handle "dbmcolumnstatistics")) ] ]

/// ndm-intake.<site>, snmp-traps-intake.<site>, ndmflow-intake.<site>, netpath-intake.<site>
let ndm: Endpoint list =
    [ POST
          [ route "/api/v2/ndm" (bindBody Ndm.handleMetadata)
            route "/api/v2/ndmconfig" (bindBody Ndm.handleConfig)
            route "/api/v2/ndmtraps" (bindBody Ndm.handleTraps)
            route "/api/v2/ndmflow" (bindBody Ndm.handleFlow)
            route "/api/v2/netpath" (bindBody Ndm.handleNetpath) ] ]

/// resources-intake.<site>
let resources: Endpoint list =
    [ POST [ route "/api/v2/genresources" (bindBody Misc.handleGenResources) ] ]

// ---- security ----

/// cws-intake.<site>    activity_dump remote_storage
let cws: Endpoint list =
    [ POST [ route "/api/v2/secdump" (bindBody Security.handleSecDump) ] ]

/// runtime-security-http-intake.logs.<site>
let runtimeSecurity: Endpoint list =
    [ POST
          [ route "/api/v2/secruntime" (bindBody (Security.handleTrack "secruntime"))
            route "/api/v2/secinfo" (bindBody (Security.handleTrack "secinfo")) ] ]

/// cspm-intake.<site>    compliance_config
let cspm: Endpoint list =
    [ POST [ route "/api/v2/compliance" (bindBody (Security.handleTrack "compliance")) ] ]

/// sbom-intake.<site>    event platform
let sbom: Endpoint list =
    [ POST [ route "/api/v2/sbom" (bindBody Security.handleSbom) ] ]

/// sds-intake.<site>    event platform
let sds: Endpoint list =
    [ POST [ route "/api/v2/sdsresult" (bindBody Security.handleSdsResult) ] ]

// ---- what else the agent reports about itself and its host ----

/// agentdiscovery-intake.<site>    event platform
let agentDiscovery: Endpoint list =
    [ POST [ route "/api/v2/agentdiscovery" (bindBody Evp.handleAgentDiscovery) ] ]

/// agenthealth-intake.<site>    dd_url (a forwarder of its own)
let agentHealth: Endpoint list =
    [ POST [ route "/api/v2/agenthealth" (bindBody Evp.handleAgentHealth) ] ]

/// event-management-intake.<site>    event platform
let eventManagement: Endpoint list =
    [ POST [ route "/api/v2/events" (bindBody Evp.handleEventManagement) ] ]

/// softinv-intake.<site>    event platform
let softwareInventory: Endpoint list =
    [ POST [ route "/api/v2/softinv" (bindBody Evp.handleSoftwareInventory) ] ]

// ---- synthetics: synthetics.collector.enabled ----

/// intake.synthetics.<site>: the poller the agent READS its test list from.
let syntheticsPoller: Endpoint list =
    [ GET [ route "/api/unstable/synthetics/agents/tests" (bindBody CiWebhook.handleSyntheticsAgentTests) ] ]

/// http-synthetics.<site>: where the results come back.
let syntheticsResults: Endpoint list =
    [ POST [ route "/api/v2/synthetics" (bindBody Evp.handleSynthetics) ] ]

// ---- senders that are not the agent ----

/// citestcycle-intake.<site>: tracers in CI.
let ciTestCycle: Endpoint list =
    [ POST [ route "/api/v2/citestcycle" (bindBody CiVisibility.handleTestCycle) ] ]

/// citestcov-intake.<site>: tracers in CI.
let ciTestCoverage: Endpoint list =
    [ POST [ route "/api/v2/citestcov" (bindBody CiVisibility.handleTestCov) ] ]

/// webhook-intake.<site>: CI providers' webhooks. The router takes the path
/// with or without the slash the Jenkins plugin's URL ends in.
let ciWebhook: Endpoint list =
    [ POST [ route "/api/v2/webhook" (bindBody CiWebhook.handle) ] ]

/// data-obs-intake.<site>    ol_proxy_config.dd_url
let dataObs: Endpoint list =
    [ POST
          [ route "/api/v1/lineage" (bindBody Evp.handleOpenLineage)
            route "/api/v2/query-actions" (bindBody Evp.handleQueryActions) ] ]

/// eudm-intake.<site>
let aiUsage: Endpoint list =
    [ POST [ route "/api/v2/aiusage" (bindBody Install.handleAIUsage) ] ]

/// llmobs-intake.<site>
let llmObs: Endpoint list =
    [ POST [ route "/api/v2/llmobs" (bindBody Install.handleLLMObs) ] ]

/// install.datadoghq.com, install.datad0g.com    installer.registry.url (OCI);
/// none for /btfs, whose host is hardcoded in system-probe.
let install: Endpoint list =
    [ HEAD [ route "/" (bindBody Install.handleProbe) ]
      GET
          [ route "/v2/" (bindBody Install.handleVersionCheck)
            routef "/v2/{%s}/manifests/{%s}" (fun (repo: string) (tag: string) -> bindBody (Install.handleManifest repo tag))
            routef "/v2/{%s}/blobs/{%s}" (fun (repo: string) (digest: string) -> bindBody (Install.handleBlob repo digest))
            // The last segment is <kernel>.btf.tar.xz.
            routef "/btfs/{%s}/{%s}/{%s}/{%s}" (fun (_: string) (_: string) (_: string) (_: string) ->
                bindBody Install.handleBtf) ] ]

/// browser-intake.<site>: the RUM and Logs SDKs, browser and mobile.
let rum: Endpoint list =
    [ POST
          [ route "/api/v2/rum" (bindBody Rum.handleRum)
            // Logs from an SDK are the wire format the agent sends; only the
            // framing differs, and the logs handler sniffs it.
            route "/api/v2/logs" (bindBody Logs.handle)
            route "/api/v2/replay" (bindBody Rum.handleReplay)
            route "/api/v2/spans" (bindBody Rum.handleSpans)
            // The browser profiler's multipart is the envelope the tracers
            // send, so it reuses that handler under its own label.
            route "/api/v2/profile" (bindBody (Profiling.handleProfile "profile-browser"))
            route "/api/v2/debugger" (bindBody Profiling.handleDebugger) ]
      GET [ route "/api/v2/profiling/quota" (bindBody Rum.handleProfilingQuota) ] ]

// ---- hosts ----

/// The host of a request without its port, and without the trailing dot that
/// `convert_dd_site_fqdn` adds.
let hostName (ctx: HttpContext) : string = ctx.Request.Host.Host.TrimEnd '.'

let private logUnhandled (ctx: HttpContext) : unit =
    // Logged loudly on purpose: this is how unhandled endpoints are found.
    (Ctx.log ctx).LogWarning(
        "UNHANDLED ENDPOINT: {Method} {Path} (Content-Type: {ContentType}, User-Agent: {UserAgent})",
        ctx.Request.Method,
        ctx.Request.Path.Value,
        ctx.Request.ContentType,
        ctx.Request.Headers.UserAgent.ToString()
    )

/// The answer to a path the host does not serve.
let private unknownEndpoint: EndpointHandler =
    fun ctx ->
        logUnhandled ctx

        if ctx.GetService<IntakeSettings>().AckUnknown then
            (setStatusCode 202 >=> json {||}) ctx
        else
            (setStatusCode 404 >=> json {| errors = [ "unknown endpoint" ] |}) ctx

let private unknownHost: EndpointHandler =
    fun ctx ->
        let host = hostName ctx
        (Ctx.log ctx).LogWarning("UNKNOWN HOST: \"{Host}\" — no intake serves this name", host)
        // The body names the host we saw: the one thing the operator cannot
        // read off their own config, where DD_SITE and each override build it
        // differently.
        (setStatusCode 404 >=> json {| errors = [ $"no intake for host \"{host}\"" ] |}) ctx

/// One host's router: its endpoints, the ones every host has, and the answer
/// for anything else. `scheme` is the authentication scheme its routes ask
/// for (Auth.fs); None for a host whose clients are anonymous.
let mount (app: IApplicationBuilder) (scheme: string option) (endpoints: Endpoint list) : unit =
    let served = endpoints @ onEveryHost

    let guarded =
        match scheme with
        | None -> served
        | Some scheme ->
            served
            |> List.map (configureEndpoint (fun endpoint -> endpoint.RequireAuthorization(AuthorizeAttribute(AuthenticationSchemes = scheme))))

    // A handler that throws is a 500 and a line in the log, not a crash.
    app.UseExceptionHandler(fun (failed: IApplicationBuilder) -> failed.Run(setStatusCode 500))
    |> ignore

    // A known path with the wrong method is answered by the router itself,
    // with 405. It is as unhandled as an unknown path, so it is logged too.
    app.Use(fun (ctx: HttpContext) (next: RequestDelegate) ->
        task {
            do! next.Invoke ctx

            if ctx.Response.StatusCode = 405 then
                logUnhandled ctx
        }
        :> Task)
    |> ignore

    app.UseRouting().UseAuthentication().UseAuthorization().UseOxpecker(guarded @ probes)
    |> ignore

    app.Run unknownEndpoint

/// browser-intake's router, with what sits above it.
let mountBrowser (app: IApplicationBuilder) (allowedOrigins: string list) : unit =
    app.UseCors(fun policy -> Rum.corsPolicy allowedOrigins policy) |> ignore

    app.Use(fun (ctx: HttpContext) (next: RequestDelegate) ->
        Rum.unwrapForward (Ctx.log ctx) ctx
        next.Invoke ctx)
    |> ignore

    mount app (Some ApiKeyAuth.browser) rum

/// The whole intake: the request's host picks the product, the router the
/// handler.
let configure (pipeline: IApplicationBuilder) : unit =
    // Does nothing until the host names the proxies it trusts
    // (ForwardedHeadersOptions); then a request's address is the one such a
    // proxy forwarded, not the proxy's own.
    pipeline.UseForwardedHeaders() |> ignore

    let rumOrigins =
        Rum.parseAllowedOrigins (System.Environment.GetEnvironmentVariable "NINJACAT_RUM_ALLOWED_ORIGINS")

    let onHost (isHost: string -> bool) (scheme: string option) (endpoints: Endpoint list) : unit =
        pipeline.MapWhen((fun ctx -> isHost (hostName ctx)), (fun branch -> mount branch scheme endpoints))
        |> ignore

    // Hosts match by PREFIX, so the Multi-Region Failover copy of a host
    // (`app.mrf.<site>`, `trace.agent.mrf.<site>`) lands on the same intake
    // as the primary.
    let on (prefix: string) (scheme: string) (endpoints: Endpoint list) : unit =
        onHost (fun host -> host.StartsWith prefix) (Some scheme) endpoints

    let agent = ApiKeyAuth.agent

    // The OCI registry, the BTF download and the HEAD probe are anonymous.
    onHost (fun host -> host = "install.datadoghq.com" || host = "install.datad0g.com") None install

    // The agent's forwarder rewrites app.<site> to <version>-app.agent.<site>
    // when the host looks like Datadog's own, and a flare upload to
    // <version>-flare.agent.<site>. A custom dd_url is left alone, so both
    // spellings have to work.
    onHost (fun host -> host.Contains "-app.agent." || host.StartsWith "app.") (Some agent) app
    onHost (fun host -> host.Contains "-flare.agent." || host.StartsWith "api.") (Some agent) api

    on "trace.agent." agent trace
    on "instrumentation-telemetry-intake." agent telemetry
    on "intake.profile." agent profile
    on "debugger-intake." agent debugger
    on "sourcemap-intake." agent sourcemap

    on "http-intake.logs." agent logs
    on "agent-http-intake.logs." agent logs
    on "process." agent processes
    on "orchestrator." agent kubeops
    on "kubeops-intake." agent kubeops
    on "contlcycle-intake." agent containers
    on "contimage-intake." agent containers
    on "config." agent remoteConfig

    on "dbm-metrics-intake." agent dbm
    on "ndm-intake." agent ndm
    on "snmp-traps-intake." agent ndm
    on "ndmflow-intake." agent ndm
    on "netpath-intake." agent ndm
    on "resources-intake." agent resources

    on "cws-intake." agent cws
    on "runtime-security-http-intake.logs." agent runtimeSecurity
    on "cspm-intake." agent cspm
    on "sbom-intake." agent sbom
    on "sds-intake." agent sds

    on "agentdiscovery-intake." agent agentDiscovery
    on "agenthealth-intake." agent agentHealth
    on "event-management-intake." agent eventManagement
    on "softinv-intake." agent softwareInventory

    on "intake.synthetics." agent syntheticsPoller
    on "http-synthetics." agent syntheticsResults

    on "citestcycle-intake." agent ciTestCycle
    on "citestcov-intake." agent ciTestCoverage
    on "webhook-intake." ApiKeyAuth.ciWebhook ciWebhook
    on "data-obs-intake." ApiKeyAuth.lineage dataObs
    on "eudm-intake." agent aiUsage
    on "llmobs-intake." agent llmObs

    // browser-intake is the one host whose clients are not agents: it needs
    // CORS, and a URL rewrite for the SDK's `proxy` form, above its router.
    pipeline.MapWhen((fun ctx -> (hostName ctx).StartsWith "browser-intake."), (fun branch -> mountBrowser branch rumOrigins))
    |> ignore

    pipeline.Run unknownHost
