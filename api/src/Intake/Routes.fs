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
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Logging
open Oxpecker
open NinjaCat.Api.Intake.Routers

/// Runs an intake handler as an Oxpecker one: checks the key, reads and
/// decompresses the body, calls the handler and writes its answer.
let keyed (auth: Auth) (handler: Handler) : EndpointHandler =
    fun http ->
        task {
            let deps = http.RequestServices.GetRequiredService<Deps>()

            let! response =
                task {
                    try
                        match auth http deps.Store with
                        | Error refusal -> return refusal
                        | Ok key ->
                            let! rawBody = Body.read http
                            let encoding = http.Request.Headers.ContentEncoding.ToString()

                            return
                                handler
                                    { Http = http
                                      Body = (if encoding = "" then rawBody else Body.decompress deps.Log encoding rawBody)
                                      Key = key
                                      Sink = deps.Sink
                                      Log = deps.Log }
                    with e ->
                        deps.Log.LogError(e, "handler failed: {Method} {Path}", http.Request.Method, http.Request.Path.Value)
                        return Response.status 500
                }

            do! Response.write http response
        }

/// The agent's guard: the Dd-Api-Key header, then ?api_key=.
let agent: Handler -> EndpointHandler = keyed Auth.standard

/// install.<site>: its clients are anonymous.
let anonymous: Handler -> EndpointHandler = keyed Auth.none

/// webhook-intake: Jenkins sends the header, but a GitLab project webhook has
/// no field for one, so the key travels as `?dd-api-key=`.
let ciWebhookKey: Handler -> EndpointHandler =
    keyed (Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key"; KeyFromQuery "api_key" ])

/// data-obs-intake: OpenLineage's transport sends `Authorization: Bearer`,
/// and the trace-agent's lineage proxy forwards it unchanged.
let lineageKey: Handler -> EndpointHandler =
    keyed (Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ])

/// browser-intake: the browser has no choice but the query string.
let browserKey: Handler -> EndpointHandler = keyed Rum.gate

// ---- every host ----

/// /ping and /_health need no key. The rest the agent sweeps at startup on
/// whatever host it talks to, before it knows which intake that is; a flare
/// goes to whichever host dd_url resolves to.
let onEveryHost (guard: Handler -> EndpointHandler) : Endpoint list =
    let flare =
        [ route "/support/flare" (guard (Flare.handle ""))
          routef "/support/flare/{%s}" (fun (caseId: string) -> guard (Flare.handle caseId)) ]

    [ GET
          [ route "/ping" (fun http -> Response.write http (Response.json 200 """{"message":"pong"}"""))
            route "/_health" (fun http -> Response.write http (Response.json 200 """{"status":"ok"}"""))
            route "/api/v1/validate" (guard (fun _ -> Response.json 200 """{"valid":true}""")) ]
      HEAD flare
      POST flare ]

// ---- the core agent: dd_url ----

/// Served on app.<site> and on api.<site>.
let metricsAndChecks: Endpoint list =
    [ POST
          [ route "/intake/" (agent Api.handleIntake)
            route "/api/v1/series" (agent Api.handleSeriesV1)
            route "/api/v2/series" (agent Api.handleSeriesV2)
            route "/api/v1/check_run" (agent Api.handleCheckRun)
            route "/api/v1/metadata" (agent Api.handleMetadata)
            route "/api/beta/sketches" (agent Api.handleSketches)

            // Not sent by any agent: API clients only.
            route "/api/v1/events" (agent Api.handleEvents)
            route "/api/v1/distribution_points" (agent Api.handleDistributionPoints)

            route "/api/v2/intake-key" (agent Api.handleIntakeKey)
            route "/api/v2/profiles/symbols/query" (agent Api.handleSymbolsQuery)
            // The agent sends GET; POST gets the same answer.
            route "/api/v2/validate" (agent Api.handleOpmValidate)

            route "/api/unstable/on_prem_runners" (agent Api.handleRunnerEnroll)
            route "/api/unstable/on_prem_runners/api_key_only" (agent Api.handleRunnerEnroll)
            route "/api/v2/on-prem-management-service/workflow-tasks/dequeue" (agent Api.handleRunnerDequeue)
            route "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update" (agent Api.handleRunnerTaskUpdate)
            route "/api/v2/on-prem-management-service/workflow-tasks/heartbeat" (agent Api.handleRunnerHeartbeat)
            route "/api/v2/actions/connections" (agent Api.handleActionConnections) ]
      GET
          [ route "/api/v1/query" (agent Api.handleQuery)
            route "/api/v2/validate" (agent Api.handleOpmValidate)
            route "/api/v2/on-prem-management-service/runner/health-check" (agent Api.handleRunnerHealthCheck) ] ]

/// Metrics v3, on app.<site> only.
let metricsV3: Endpoint list =
    [ POST
          [ route "/api/intake/metrics/v3/series" (agent (App.handle "v3series"))
            route "/api/intake/metrics/v3/sketches" (agent (App.handle "v3sketches"))
            // A shadow copy of the v2 traffic on /api/beta/sketches.
            route "/api/intake/metrics/v3beta/sketches" (agent (App.handle "v3beta")) ] ]

/// app.<site>, <version>-app.agent.<site>
let app: Endpoint list = metricsAndChecks @ metricsV3

/// The CI Visibility endpoints on api.<site>, which app.<site> deliberately
/// does not carry. A tracer never calls the four after `setting` while every
/// flag in the settings answer is false; they exist because a tracer
/// configured by hand must not get a 404 in the middle of a test run.
let ciVisibilityApi: Endpoint list =
    [ POST
          [ route "/api/v2/libraries/tests/services/setting" (agent (CiVisibility.answerConfig "settings" CiVisibility.settingsAnswer))
            route "/api/v2/ci/tests/skippable" (agent (CiVisibility.answerConfig "skippable" CiVisibility.skippableAnswer))
            route "/api/v2/ci/libraries/tests" (agent (CiVisibility.answerConfig "known_tests" CiVisibility.testListAnswer))
            route "/api/v2/ci/libraries/tests/flaky" (agent (CiVisibility.answerConfig "flaky_tests" CiVisibility.testListAnswer))
            route
                "/api/v2/test/libraries/test-management/tests"
                (agent (CiVisibility.answerConfig "test_management" CiVisibility.testManagementAnswer))
            route "/api/v2/git/repository/search_commits" (agent CiVisibility.handleSearchCommits)
            route "/api/v2/git/repository/packfile" (agent CiVisibility.handlePackfile)
            route "/api/v2/ci/pipeline/tags" (agent (CiVisibility.handlePipelineEvent "tag"))
            route "/api/v2/ci/pipeline/metrics" (agent (CiVisibility.handlePipelineEvent "measure"))
            route "/api/intake/ci/custom_spans" (agent (CiVisibility.handlePipelineEvent "custom_span")) ] ]

/// api.<site>, <version>-flare.agent.<site>
let api: Endpoint list = metricsAndChecks @ ciVisibilityApi

// ---- tracing ----

/// trace.agent.<site>    apm_config.apm_dd_url
let trace: Endpoint list =
    [ POST
          [ route "/api/v0.2/traces" (agent Trace.handleTraces)
            route "/api/v0.2/stats" (agent Trace.handleStats)
            route "/api/v0.1/pipeline_stats" (agent Trace.handlePipelineStats)
            route "/api/v2/data_streams_messages" (agent Trace.handleDataStreamsMessages) ] ]

/// instrumentation-telemetry-intake.<site>    apm_config.telemetry.dd_url
let telemetry: Endpoint list =
    [ POST [ route "/api/v2/apmtelemetry" (agent Misc.handleTelemetry) ] ]

/// intake.profile.<site>    apm_config.profiling_dd_url
let profile: Endpoint list =
    [ POST
          [ // What the trace-agent forwards from /profiling/v1/input: the
            // tracer's own multipart, byte for byte.
            route "/api/v2/profile" (agent (Profiling.handleProfile "profile"))
            // The old path, still used by the agent's internal profiling.
            route "/v1/input" (agent (Profiling.handleProfile "profile-v1")) ] ]

/// debugger-intake.<site>    debugger_diagnostics_dd_url
let debugger: Endpoint list =
    [ POST [ route "/api/v2/debugger" (agent Profiling.handleDebugger) ] ]

/// sourcemap-intake.<site>    symbol_endpoints[].site
let sourcemap: Endpoint list =
    [ POST [ route "/api/v2/srcmap" (agent Profiling.handleSourcemap) ] ]

// ---- logs, processes, containers ----

/// http-intake.logs.<site>, agent-http-intake.logs.<site>    logs_config.logs_dd_url
let logs: Endpoint list =
    [ POST
          [ route "/api/v2/logs" (agent Logs.handle)
            route "/v1/input" (agent Logs.handle)
            // Older clients put the key in the path.
            routef "/v1/input/{%s}" (fun (_: string) -> agent Logs.handle) ] ]

/// process.<site>    process_config.process_dd_url
let processes: Endpoint list =
    [ POST
          [ route "/api/v1/collector" (agent Process.handleCollector)
            route "/api/v1/container" (agent Process.handleContainer)
            route "/api/v1/connections" (agent Process.handleConnections)
            route "/api/v1/discovery" (agent Process.handleDiscovery) ] ]

/// orchestrator.<site>, kubeops-intake.<site>    orchestrator_dd_url
let kubeops: Endpoint list =
    [ POST
          [ route "/api/v2/orch" (agent Kubeops.handleOrchestrator)
            route "/api/v2/orchmanif" (agent Kubeops.handleManifests)
            route "/api/v1/orchestrator" (agent Kubeops.handleOrchestrator)
            route "/api/v2/kubeactions" (agent Kubeops.handleActions) ] ]

/// contlcycle-intake.<site>, contimage-intake.<site>    event platform
let containers: Endpoint list =
    [ POST
          [ route "/api/v2/contlcycle" (agent Containers.handleLifecycle)
            route "/api/v2/contimage" (agent Containers.handleImages) ] ]

/// config.<site>    remote configuration: on or off, no URL
let remoteConfig: Endpoint list =
    [ POST
          [ route "/api/v0.1/configurations" (agent Config.handleConfigurations)
            // The agent sends GET; POST is registered so anything else that
            // tries gets the same explicit answer.
            route "/api/v0.1/org" (agent Config.notServed)
            route "/api/v0.1/status" (agent Config.notServed) ]
      GET
          [ route "/api/v0.1/org" (agent Config.notServed)
            route "/api/v0.1/status" (agent Config.notServed) ] ]

// ---- databases and network devices: event platform ----

/// dbm-metrics-intake.<site>
let dbm: Endpoint list =
    [ POST
          [ route "/api/v2/dbmmetrics" (agent (Dbm.handle "dbmmetrics")) // query and lock metrics
            route "/api/v2/dbmactivity" (agent (Dbm.handle "dbmactivity")) // active sessions
            route "/api/v2/databasequery" (agent (Dbm.handle "databasequery")) // query samples and plans
            route "/api/v2/dbmmetadata" (agent (Dbm.handle "dbmmetadata")) // instances and schemas
            // No producer of these two is in the agent's source: only the
            // envelope can be claimed about them.
            route "/api/v2/dbmhealth" (agent (Dbm.handle "dbmhealth"))
            route "/api/v2/dbmcolumnstatistics" (agent (Dbm.handle "dbmcolumnstatistics")) ] ]

/// ndm-intake.<site>, snmp-traps-intake.<site>, ndmflow-intake.<site>, netpath-intake.<site>
let ndm: Endpoint list =
    [ POST
          [ route "/api/v2/ndm" (agent Ndm.handleMetadata)
            route "/api/v2/ndmconfig" (agent Ndm.handleConfig)
            route "/api/v2/ndmtraps" (agent Ndm.handleTraps)
            route "/api/v2/ndmflow" (agent Ndm.handleFlow)
            route "/api/v2/netpath" (agent Ndm.handleNetpath) ] ]

/// resources-intake.<site>
let resources: Endpoint list =
    [ POST [ route "/api/v2/genresources" (agent Misc.handleGenResources) ] ]

// ---- security ----

/// cws-intake.<site>    activity_dump remote_storage
let cws: Endpoint list =
    [ POST [ route "/api/v2/secdump" (agent Security.handleSecDump) ] ]

/// runtime-security-http-intake.logs.<site>
let runtimeSecurity: Endpoint list =
    [ POST
          [ route "/api/v2/secruntime" (agent (Security.handleTrack "secruntime"))
            route "/api/v2/secinfo" (agent (Security.handleTrack "secinfo")) ] ]

/// cspm-intake.<site>    compliance_config
let cspm: Endpoint list =
    [ POST [ route "/api/v2/compliance" (agent (Security.handleTrack "compliance")) ] ]

/// sbom-intake.<site>    event platform
let sbom: Endpoint list =
    [ POST [ route "/api/v2/sbom" (agent Security.handleSbom) ] ]

/// sds-intake.<site>    event platform
let sds: Endpoint list =
    [ POST [ route "/api/v2/sdsresult" (agent Security.handleSdsResult) ] ]

// ---- what else the agent reports about itself and its host ----

/// agentdiscovery-intake.<site>    event platform
let agentDiscovery: Endpoint list =
    [ POST [ route "/api/v2/agentdiscovery" (agent Evp.handleAgentDiscovery) ] ]

/// agenthealth-intake.<site>    dd_url (a forwarder of its own)
let agentHealth: Endpoint list =
    [ POST [ route "/api/v2/agenthealth" (agent Evp.handleAgentHealth) ] ]

/// event-management-intake.<site>    event platform
let eventManagement: Endpoint list =
    [ POST [ route "/api/v2/events" (agent Evp.handleEventManagement) ] ]

/// softinv-intake.<site>    event platform
let softwareInventory: Endpoint list =
    [ POST [ route "/api/v2/softinv" (agent Evp.handleSoftwareInventory) ] ]

// ---- synthetics: synthetics.collector.enabled ----

/// intake.synthetics.<site>: the poller the agent READS its test list from.
let syntheticsPoller: Endpoint list =
    [ GET [ route "/api/unstable/synthetics/agents/tests" (agent CiWebhook.handleSyntheticsAgentTests) ] ]

/// http-synthetics.<site>: where the results come back.
let syntheticsResults: Endpoint list =
    [ POST [ route "/api/v2/synthetics" (agent Evp.handleSynthetics) ] ]

// ---- senders that are not the agent ----

/// citestcycle-intake.<site>: tracers in CI.
let ciTestCycle: Endpoint list =
    [ POST [ route "/api/v2/citestcycle" (agent CiVisibility.handleTestCycle) ] ]

/// citestcov-intake.<site>: tracers in CI.
let ciTestCoverage: Endpoint list =
    [ POST [ route "/api/v2/citestcov" (agent CiVisibility.handleTestCov) ] ]

/// webhook-intake.<site>: CI providers' webhooks. The router takes the path
/// with or without the slash the Jenkins plugin's URL ends in.
let ciWebhook: Endpoint list =
    [ POST [ route "/api/v2/webhook" (ciWebhookKey CiWebhook.handle) ] ]

/// data-obs-intake.<site>    ol_proxy_config.dd_url
let dataObs: Endpoint list =
    [ POST
          [ route "/api/v1/lineage" (lineageKey Evp.handleOpenLineage)
            route "/api/v2/query-actions" (lineageKey Evp.handleQueryActions) ] ]

/// eudm-intake.<site>
let aiUsage: Endpoint list =
    [ POST [ route "/api/v2/aiusage" (agent Install.handleAIUsage) ] ]

/// llmobs-intake.<site>
let llmObs: Endpoint list =
    [ POST [ route "/api/v2/llmobs" (agent Install.handleLLMObs) ] ]

/// install.datadoghq.com, install.datad0g.com    installer.registry.url (OCI);
/// none for /btfs, whose host is hardcoded in system-probe.
let install: Endpoint list =
    [ HEAD [ route "/" (anonymous Install.handleProbe) ]
      GET
          [ route "/v2/" (anonymous Install.handleVersionCheck)
            routef "/v2/{%s}/manifests/{%s}" (fun (repo: string) (tag: string) -> anonymous (Install.handleManifest repo tag))
            routef "/v2/{%s}/blobs/{%s}" (fun (repo: string) (digest: string) -> anonymous (Install.handleBlob repo digest))
            // The last segment is <kernel>.btf.tar.xz.
            routef "/btfs/{%s}/{%s}/{%s}/{%s}" (fun (_: string) (_: string) (_: string) (_: string) ->
                anonymous Install.handleBtf) ] ]

/// browser-intake.<site>: the RUM and Logs SDKs, browser and mobile.
let rum: Endpoint list =
    [ POST
          [ route "/api/v2/rum" (browserKey Rum.handleRum)
            // Logs from an SDK are the wire format the agent sends; only the
            // framing differs, and the logs handler sniffs it.
            route "/api/v2/logs" (browserKey Logs.handle)
            route "/api/v2/replay" (browserKey Rum.handleReplay)
            route "/api/v2/spans" (browserKey Rum.handleSpans)
            // The browser profiler's multipart is the envelope the tracers
            // send, so it reuses that handler under its own label.
            route "/api/v2/profile" (browserKey (Profiling.handleProfile "profile-browser"))
            route "/api/v2/debugger" (browserKey Profiling.handleDebugger) ]
      GET [ route "/api/v2/profiling/quota" (browserKey Rum.handleProfilingQuota) ] ]

// ---- hosts ----

/// The host of a request without its port, and without the trailing dot that
/// `convert_dd_site_fqdn` adds.
let hostName (http: HttpContext) : string = http.Request.Host.Host.TrimEnd '.'

let private unhandled (http: HttpContext) : unit =
    let deps = http.RequestServices.GetRequiredService<Deps>()

    // Logged loudly on purpose: this is how unhandled endpoints are found.
    deps.Log.LogWarning(
        "UNHANDLED ENDPOINT: {Method} {Path} (Content-Type: {ContentType}, User-Agent: {UserAgent})",
        http.Request.Method,
        http.Request.Path.Value,
        http.Request.ContentType,
        http.Request.Headers.UserAgent.ToString()
    )

/// The answer to a path the host does not serve.
let private unknownEndpoint (http: HttpContext) : Task =
    unhandled http
    let deps = http.RequestServices.GetRequiredService<Deps>()
    Response.write http (if deps.AckUnknown then Response.json 202 "{}" else Response.errors 404 [ "unknown endpoint" ])

let private unknownHost (http: HttpContext) : Task =
    let deps = http.RequestServices.GetRequiredService<Deps>()
    let host = hostName http
    deps.Log.LogWarning("UNKNOWN HOST: \"{Host}\" — no intake serves this name", host)
    // The body names the host we saw: the one thing the operator cannot read
    // off their own config, where DD_SITE and each override build it differently.
    let body = System.Text.Encoding.UTF8.GetBytes($"{{\"errors\":[\"no intake for host \\\"{host}\\\"\"]}}\n")
    Response.write http (Response.bytes 404 "application/json" body)

/// One host's router: its endpoints, the ones every host has, and the answer
/// for anything else. `guard` is the host's own, for the shared endpoints.
let mount (app: IApplicationBuilder) (guard: Handler -> EndpointHandler) (endpoints: Endpoint list) : unit =
    // A known path with the wrong method is answered by the router itself,
    // with 405. It is as unhandled as an unknown path, so it is logged too.
    app.Use(fun (http: HttpContext) (next: RequestDelegate) ->
        task {
            do! next.Invoke http

            if http.Response.StatusCode = 405 then
                unhandled http
        }
        :> Task)
    |> ignore

    app.UseRouting().UseOxpecker(endpoints @ onEveryHost guard) |> ignore
    app.Run unknownEndpoint

/// browser-intake's router, with what sits above it.
let mountBrowser (app: IApplicationBuilder) (log: ILogger) (allowedOrigins: string list) : unit =
    app.UseCors(fun policy -> Rum.corsPolicy allowedOrigins policy) |> ignore

    app.Use(fun (http: HttpContext) (next: RequestDelegate) ->
        Rum.unwrapForward log http
        next.Invoke http)
    |> ignore

    mount app browserKey rum

/// The whole intake: the request's host picks the product, the router the
/// handler.
let configure (pipeline: IApplicationBuilder) : unit =
    let deps = pipeline.ApplicationServices.GetRequiredService<Deps>()

    // Does nothing until the host names the proxies it trusts
    // (ForwardedHeadersOptions); then a request's address is the one such a
    // proxy forwarded, not the proxy's own.
    pipeline.UseForwardedHeaders() |> ignore

    let rumOrigins =
        Rum.parseAllowedOrigins (System.Environment.GetEnvironmentVariable "NINJACAT_RUM_ALLOWED_ORIGINS")

    let onHost (isHost: string -> bool) (guard: Handler -> EndpointHandler) (endpoints: Endpoint list) : unit =
        pipeline.MapWhen((fun http -> isHost (hostName http)), (fun branch -> mount branch guard endpoints))
        |> ignore

    // Hosts match by PREFIX, so the Multi-Region Failover copy of a host
    // (`app.mrf.<site>`, `trace.agent.mrf.<site>`) lands on the same intake
    // as the primary.
    let on (prefix: string) (guard: Handler -> EndpointHandler) (endpoints: Endpoint list) : unit =
        onHost (fun host -> host.StartsWith prefix) guard endpoints

    onHost (fun host -> host = "install.datadoghq.com" || host = "install.datad0g.com") anonymous install

    // The agent's forwarder rewrites app.<site> to <version>-app.agent.<site>
    // when the host looks like Datadog's own, and a flare upload to
    // <version>-flare.agent.<site>. A custom dd_url is left alone, so both
    // spellings have to work.
    onHost (fun host -> host.Contains "-app.agent." || host.StartsWith "app.") agent app
    onHost (fun host -> host.Contains "-flare.agent." || host.StartsWith "api.") agent api

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
    on "webhook-intake." ciWebhookKey ciWebhook
    on "data-obs-intake." lineageKey dataObs
    on "eudm-intake." agent aiUsage
    on "llmobs-intake." agent llmObs

    // browser-intake is the one host whose clients are not agents: it needs
    // CORS, and a URL rewrite for the SDK's `proxy` form, above its router.
    pipeline.MapWhen((fun http -> (hostName http).StartsWith "browser-intake."), (fun branch -> mountBrowser branch deps.Log rumOrigins))
    |> ignore

    pipeline.Run unknownHost
