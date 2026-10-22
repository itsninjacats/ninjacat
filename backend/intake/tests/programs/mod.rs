//! Every intake program's routes, as its `main` in `api/src/bin` registers them: a binary cannot
//! be imported, so the tests keep this copy. A route added to a program is added here too.

#![allow(dead_code)]

use std::sync::Arc;

use ninjapaws_intake::auth;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::{
    api, app, ci_visibility, ci_webhook, config, containers, dbm, evp, flare, install, kubeops, logs, misc, ndm,
    process, profiling, rum, security, trace,
};
use ninjapaws_intake::server::{self, Body, Ctx};
use tako::extractors::state::State;
use tako::router::Router;

/// ninjapaws_agent_discovery
pub fn agent_discovery(router: &mut Router) {
    router.post("/api/v2/agentdiscovery", evp::handle_agent_discovery).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_agent_health
pub fn agent_health(router: &mut Router) {
    router.post("/api/v2/agenthealth", evp::handle_agent_health).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_ai_usage
pub fn ai_usage(router: &mut Router) {
    router.post("/api/v2/aiusage", install::handle_ai_usage).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_api
pub fn api(router: &mut Router) {
    router.post("/intake/", api::handle_intake).middleware(auth::agent);
    router.post("/api/v1/series", api::handle_series_v1).middleware(auth::agent);
    router.post("/api/v2/series", api::handle_series_v2).middleware(auth::agent);
    router.post("/api/v1/check_run", api::handle_check_run).middleware(auth::agent);
    router.post("/api/v1/metadata", api::handle_metadata).middleware(auth::agent);
    router.post("/api/beta/sketches", api::handle_sketches).middleware(auth::agent);

    // Not sent by any agent: API clients only.
    router.post("/api/v1/events", api::handle_events).middleware(auth::agent);
    router.post("/api/v1/distribution_points", api::handle_distribution_points).middleware(auth::agent);

    router.post("/api/v2/intake-key", api::handle_intake_key).middleware(auth::agent);
    router.post("/api/v2/profiles/symbols/query", api::handle_symbols_query).middleware(auth::agent);
    // The agent sends GET; POST gets the same answer.
    router.post("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);
    router.get("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);

    router.post("/api/unstable/on_prem_runners", api::handle_runner_enroll).middleware(auth::agent);
    router.post("/api/unstable/on_prem_runners/api_key_only", api::handle_runner_enroll).middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/dequeue", api::handle_runner_dequeue)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", api::handle_runner_task_update)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/heartbeat", api::handle_runner_heartbeat)
        .middleware(auth::agent);
    router
        .get("/api/v2/on-prem-management-service/runner/health-check", api::handle_runner_health_check)
        .middleware(auth::agent);
    router.post("/api/v2/actions/connections", api::handle_action_connections).middleware(auth::agent);

    router.get("/api/v1/query", api::handle_query).middleware(auth::agent);

    // CI Visibility, which app.<site> deliberately does not carry. A tracer never calls the four
    // after `setting` while every flag in the settings answer is false; they exist because a
    // tracer configured by hand must not get a 404 in the middle of a test run.
    router
        .post(
            "/api/v2/libraries/tests/services/setting",
            |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
                ci_visibility::answer_config("settings", ci_visibility::settings_answer, ctx, clickhouse, body)
            },
        )
        .middleware(auth::agent);
    router
        .post("/api/v2/ci/tests/skippable", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::answer_config("skippable", ci_visibility::skippable_answer, ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/ci/libraries/tests", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::answer_config("known_tests", ci_visibility::test_list_answer, ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/ci/libraries/tests/flaky", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::answer_config("flaky_tests", ci_visibility::test_list_answer, ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/test/libraries/test-management/tests",
            |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
                ci_visibility::answer_config(
                    "test_management",
                    ci_visibility::test_management_answer,
                    ctx,
                    clickhouse,
                    body,
                )
            },
        )
        .middleware(auth::agent);
    router.post("/api/v2/git/repository/search_commits", ci_visibility::handle_search_commits).middleware(auth::agent);
    router.post("/api/v2/git/repository/packfile", ci_visibility::handle_packfile).middleware(auth::agent);
    router
        .post("/api/v2/ci/pipeline/tags", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::handle_pipeline_event("tag", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/ci/pipeline/metrics", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::handle_pipeline_event("measure", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/intake/ci/custom_spans", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            ci_visibility::handle_pipeline_event("custom_span", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_app
pub fn app(router: &mut Router) {
    router.post("/intake/", api::handle_intake).middleware(auth::agent);
    router.post("/api/v1/series", api::handle_series_v1).middleware(auth::agent);
    router.post("/api/v2/series", api::handle_series_v2).middleware(auth::agent);
    router.post("/api/v1/check_run", api::handle_check_run).middleware(auth::agent);
    router.post("/api/v1/metadata", api::handle_metadata).middleware(auth::agent);
    router.post("/api/beta/sketches", api::handle_sketches).middleware(auth::agent);

    // Not sent by any agent: API clients only.
    router.post("/api/v1/events", api::handle_events).middleware(auth::agent);
    router.post("/api/v1/distribution_points", api::handle_distribution_points).middleware(auth::agent);

    router.post("/api/v2/intake-key", api::handle_intake_key).middleware(auth::agent);
    router.post("/api/v2/profiles/symbols/query", api::handle_symbols_query).middleware(auth::agent);
    // The agent sends GET; POST gets the same answer.
    router.post("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);
    router.get("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);

    router.post("/api/unstable/on_prem_runners", api::handle_runner_enroll).middleware(auth::agent);
    router.post("/api/unstable/on_prem_runners/api_key_only", api::handle_runner_enroll).middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/dequeue", api::handle_runner_dequeue)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", api::handle_runner_task_update)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/heartbeat", api::handle_runner_heartbeat)
        .middleware(auth::agent);
    router
        .get("/api/v2/on-prem-management-service/runner/health-check", api::handle_runner_health_check)
        .middleware(auth::agent);
    router.post("/api/v2/actions/connections", api::handle_action_connections).middleware(auth::agent);

    router.get("/api/v1/query", api::handle_query).middleware(auth::agent);

    // Columnar metrics, intake v3.
    router
        .post("/api/intake/metrics/v3/series", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3series", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/intake/metrics/v3/sketches", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3sketches", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    // A shadow copy of the v2 traffic on /api/beta/sketches.
    router
        .post("/api/intake/metrics/v3beta/sketches", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3beta", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_browser
pub fn browser(router: &mut Router) {
    router.post("/api/v2/rum", rum::handle_rum).middleware(auth::browser);
    // Logs from an SDK are the wire format the agent sends; only the framing differs, and the logs
    // handler sniffs it.
    router.post("/api/v2/logs", logs::handle).middleware(auth::browser);
    router.post("/api/v2/replay", rum::handle_replay).middleware(auth::browser);
    router.post("/api/v2/spans", rum::handle_spans).middleware(auth::browser);
    // The browser profiler's multipart is the envelope the tracers send, so it reuses that handler
    // under its own label.
    router
        .post("/api/v2/profile", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            profiling::handle_profile("profile-browser", ctx, clickhouse, body)
        })
        .middleware(auth::browser);
    router.post("/api/v2/debugger", profiling::handle_debugger).middleware(auth::browser);
    router.get("/api/v2/profiling/quota", rum::handle_profiling_quota).middleware(auth::browser);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::browser);
    router.post("/support/flare", flare::handle).middleware(auth::browser);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::browser);
}

/// ninjapaws_ci_test_coverage
pub fn ci_test_coverage(router: &mut Router) {
    router.post("/api/v2/citestcov", ci_visibility::handle_test_cov).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_ci_test_cycle
pub fn ci_test_cycle(router: &mut Router) {
    router.post("/api/v2/citestcycle", ci_visibility::handle_test_cycle).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_ci_webhook
pub fn ci_webhook(router: &mut Router) {
    // With or without the slash the Jenkins plugin's URL ends in.
    router.post("/api/v2/webhook", ci_webhook::handle).middleware(auth::ci_webhook);
    router.post("/api/v2/webhook/", ci_webhook::handle).middleware(auth::ci_webhook);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::ci_webhook);
    router.post("/support/flare", flare::handle).middleware(auth::ci_webhook);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::ci_webhook);
}

/// ninjapaws_containers
pub fn containers(router: &mut Router) {
    router.post("/api/v2/contlcycle", containers::handle_lifecycle).middleware(auth::agent);
    router.post("/api/v2/contimage", containers::handle_images).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_cspm
pub fn cspm(router: &mut Router) {
    router
        .post("/api/v2/compliance", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            security::handle_track("compliance", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_cws
pub fn cws(router: &mut Router) {
    router.post("/api/v2/secdump", security::handle_sec_dump).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_data_obs
pub fn data_obs(router: &mut Router) {
    router.post("/api/v1/lineage", evp::handle_open_lineage).middleware(auth::lineage);
    router.post("/api/v2/query-actions", evp::handle_query_actions).middleware(auth::lineage);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::lineage);
    router.post("/support/flare", flare::handle).middleware(auth::lineage);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::lineage);
}

/// ninjapaws_dbm
pub fn dbm(router: &mut Router) {
    router
        .post("/api/v2/dbmmetrics", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmmetrics", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // query and lock metrics
    router
        .post("/api/v2/dbmactivity", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmactivity", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // active sessions
    router
        .post("/api/v2/databasequery", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("databasequery", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // query samples and plans
    router
        .post("/api/v2/dbmmetadata", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmmetadata", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // instances and schemas
    // No producer of these two is in the agent's source: only the envelope can be claimed about
    // them.
    router
        .post("/api/v2/dbmhealth", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmhealth", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/dbmcolumnstatistics", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmcolumnstatistics", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_debugger
pub fn debugger(router: &mut Router) {
    router.post("/api/v2/debugger", profiling::handle_debugger).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_event_management
pub fn event_management(router: &mut Router) {
    router.post("/api/v2/events", evp::handle_event_management).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_install
pub fn install(router: &mut Router) {
    router.head("/", install::handle_probe);
    router.get("/v2/", install::handle_version_check);
    router.get("/v2/{repo}/manifests/{tag}", install::handle_manifest);
    router.get("/v2/{repo}/blobs/{digest}", install::handle_blob);
    // The last segment is <kernel>.btf.tar.xz.
    router.get("/btfs/{distribution}/{version}/{architecture}/{kernel}", install::handle_btf);
}

/// ninjapaws_kubeops
pub fn kubeops(router: &mut Router) {
    // Kubernetes resource collections; /api/v1/orchestrator is the same under
    // orchestrator_explorer.use_legacy_endpoint.
    router
        .post("/api/v2/orch", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orch", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v1/orchestrator", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orch", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    // Raw manifests (types 80-82): the object's own YAML or JSON, "exactly what kubectl would
    // show". Same frame, same decoder.
    router
        .post("/api/v2/orchmanif", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orchmanif", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router.post("/api/v2/kubeactions", kubeops::handle_actions).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_llm_obs
pub fn llm_obs(router: &mut Router) {
    router.post("/api/v2/llmobs", install::handle_llm_obs).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_logs
pub fn logs(router: &mut Router) {
    router.post("/api/v2/logs", logs::handle).middleware(auth::agent);
    router.post("/v1/input", logs::handle).middleware(auth::agent);
    // Older clients put the key in the path.
    router.post("/v1/input/{key}", logs::handle).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_ndm
pub fn ndm(router: &mut Router) {
    router.post("/api/v2/ndm", ndm::handle_metadata).middleware(auth::agent);
    router.post("/api/v2/ndmconfig", ndm::handle_config).middleware(auth::agent);
    router.post("/api/v2/ndmtraps", ndm::handle_traps).middleware(auth::agent);
    router.post("/api/v2/ndmflow", ndm::handle_flow).middleware(auth::agent);
    router.post("/api/v2/netpath", ndm::handle_netpath).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_processes
pub fn processes(router: &mut Router) {
    router.post("/api/v1/collector", process::handle_collector).middleware(auth::agent);
    router.post("/api/v1/container", process::handle_container).middleware(auth::agent);
    router.post("/api/v1/connections", process::handle_connections).middleware(auth::agent);
    router.post("/api/v1/discovery", process::handle_discovery).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_profile
pub fn profile(router: &mut Router) {
    // What the trace-agent forwards from /profiling/v1/input: the tracer's own multipart, byte for
    // byte.
    router
        .post("/api/v2/profile", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            profiling::handle_profile("profile", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    // The old path, still used by the agent's internal profiling.
    router
        .post("/v1/input", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            profiling::handle_profile("profile-v1", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_remote_config
pub fn remote_config(router: &mut Router) {
    router.post("/api/v0.1/configurations", config::handle_configurations).middleware(auth::agent);
    // The agent sends GET; POST is registered so anything else that tries gets the same explicit
    // answer.
    router.get("/api/v0.1/org", config::not_served).middleware(auth::agent);
    router.post("/api/v0.1/org", config::not_served).middleware(auth::agent);
    router.get("/api/v0.1/status", config::not_served).middleware(auth::agent);
    router.post("/api/v0.1/status", config::not_served).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_resources
pub fn resources(router: &mut Router) {
    router.post("/api/v2/genresources", misc::handle_gen_resources).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_runtime_security
pub fn runtime_security(router: &mut Router) {
    router
        .post("/api/v2/secruntime", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            security::handle_track("secruntime", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/secinfo", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            security::handle_track("secinfo", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_sbom
pub fn sbom(router: &mut Router) {
    router.post("/api/v2/sbom", security::handle_sbom).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_sds
pub fn sds(router: &mut Router) {
    router.post("/api/v2/sdsresult", security::handle_sds_result).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_software_inventory
pub fn software_inventory(router: &mut Router) {
    router.post("/api/v2/softinv", evp::handle_software_inventory).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_sourcemap
pub fn sourcemap(router: &mut Router) {
    router.post("/api/v2/srcmap", profiling::handle_sourcemap).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_synthetics_poller
pub fn synthetics_poller(router: &mut Router) {
    router
        .get("/api/unstable/synthetics/agents/tests", ci_webhook::handle_synthetics_agent_tests)
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_synthetics_results
pub fn synthetics_results(router: &mut Router) {
    router.post("/api/v2/synthetics", evp::handle_synthetics).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_telemetry
pub fn telemetry(router: &mut Router) {
    router.post("/api/v2/apmtelemetry", misc::handle_telemetry).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// ninjapaws_trace
pub fn trace(router: &mut Router) {
    router.post("/api/v0.2/traces", trace::handle_traces).middleware(auth::agent);
    router.post("/api/v0.2/stats", trace::handle_stats).middleware(auth::agent);
    router.post("/api/v0.1/pipeline_stats", trace::handle_pipeline_stats).middleware(auth::agent);
    router.post("/api/v2/data_streams_messages", trace::handle_data_streams_messages).middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// A program's router as its `main` hands it to `serve`, with the state its `main` gives it.
pub fn router_of(
    program: fn(&mut Router),
    clickhouse: clickhouse::Client,
    keys: Arc<Store>,
    settings: server::Settings,
) -> Router {
    let mut router = Router::new();
    program(&mut router);
    router.get("/ping", server::ping);
    router.get("/_health", server::health);
    router.head("/support/flare", server::flare_probe);
    router.head("/support/flare/{case}", server::flare_probe);
    router.middleware(server::log_405);
    router.fallback(server::not_found);
    router.with_state(clickhouse);
    router.with_state(keys);
    router.with_state(settings);
    router
}

/// `router_of` with one key, `key`, which belongs to `tenant`, and bodies up to 64 MiB.
pub fn router_for(program: fn(&mut Router), clickhouse: &clickhouse::Client, key: &str, tenant: &str) -> Router {
    let store = Store::default();
    store.publish([(keys::hash(key), Key { id: "test".into(), name: "test".into(), tenant_id: tenant.into() })]);
    let settings = server::Settings { ack_unknown: false, max_body_bytes: 64 << 20 };
    router_of(program, clickhouse.clone(), Arc::new(store), settings)
}
