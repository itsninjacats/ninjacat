//! api.<site> and <version>-flare.agent.<site>: the core agent's metrics, checks, events, host
//! metadata and Private Action Runner traffic, and CI Visibility's settings, git metadata and
//! datadog-ci calls.

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{api, ci_visibility, flare};
use ninjapaws_intake::server::{self, Body, Ctx};
use tako::extractors::state::State;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    server::init_tracing();
    let clickhouse = ninjapaws_storage::client_from_env();
    let keys = keys::start(&server::var_or("DATABASE_URL", "")).await?;
    let settings = server::Settings::from_env();

    let mut router = Router::new();
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
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::answer_config(&clickhouse, "settings", ci_visibility::settings_answer, &ctx, &body).await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/ci/tests/skippable",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::answer_config(&clickhouse, "skippable", ci_visibility::skippable_answer, &ctx, &body)
                    .await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/ci/libraries/tests",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::answer_config(&clickhouse, "known_tests", ci_visibility::test_list_answer, &ctx, &body)
                    .await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/ci/libraries/tests/flaky",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::answer_config(&clickhouse, "flaky_tests", ci_visibility::test_list_answer, &ctx, &body)
                    .await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/test/libraries/test-management/tests",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::answer_config(
                    &clickhouse,
                    "test_management",
                    ci_visibility::test_management_answer,
                    &ctx,
                    &body,
                )
                .await
            },
        )
        .middleware(auth::agent);
    router.post("/api/v2/git/repository/search_commits", ci_visibility::handle_search_commits).middleware(auth::agent);
    router.post("/api/v2/git/repository/packfile", ci_visibility::handle_packfile).middleware(auth::agent);
    router
        .post(
            "/api/v2/ci/pipeline/tags",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::handle_pipeline_event(&clickhouse, "tag", &ctx, &body).await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/v2/ci/pipeline/metrics",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::handle_pipeline_event(&clickhouse, "measure", &ctx, &body).await
            },
        )
        .middleware(auth::agent);
    router
        .post(
            "/api/intake/ci/custom_spans",
            |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
                ci_visibility::handle_pipeline_event(&clickhouse, "custom_span", &ctx, &body).await
            },
        )
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);

    // Without a key, on every host.
    router.get("/ping", server::ping);
    router.get("/_health", server::health);
    router.head("/support/flare", server::flare_probe);
    router.head("/support/flare/{case}", server::flare_probe);
    // Anything else: logged, and 404.
    router.middleware(server::log_405);
    router.fallback(server::not_found);

    // What the handlers and the middleware read.
    router.with_state(clickhouse);
    router.with_state(keys);
    router.with_state(settings);
    server::serve("ninjapaws_api", router).await
}
