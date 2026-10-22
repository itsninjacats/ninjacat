//! Two hosts with nothing in common beyond arriving together.
//!
//!   webhook-intake.<site>     POST /api/v2/webhook, /api/v2/webhook/
//!   intake.synthetics.<site>  GET  /api/unstable/synthetics/agents/tests
//!
//! webhook-intake is Datadog's one generic "CI provider webhook" intake. The Jenkins plugin posts
//! here with ?service=<ci instance name> and the headers DD-API-KEY and DD-CI-PROVIDER-NAME:
//! jenkins; Datadog's GitLab documentation gives the same URL for GitLab's webhook integration,
//! whose key arrives as ?dd-api-key= because GitLab's webhook form sets no headers.
//!
//! The body is a JSON array of elements batched to 5 MB. One batch MIXES levels: each element
//! says "level": "pipeline" | "stage" | "job", and that field — not the URL, not a header — tells
//! them apart.
//!
//! Unverified — GitLab: whether it sends Datadog's level-shaped schema or its own native webhook
//! JSON (object_kind, project, builds…) was not confirmed, so the decoder reads both and keeps
//! every element verbatim.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use http::StatusCode;
use ninjapaws_storage as storage;
use ninjapaws_storage::rows::ci::CiWebhookEvent;
use serde_json::value::RawValue;
use tako::extractors::state::State;
use tako::types::Response;

use super::ci_visibility::{Value, raw_root};
use crate::json;
use crate::raw;
use crate::server::{Body, Ctx, accepted, json, storage_failed};
use crate::wire::{tags, text, time};

/// Request headers kept on a webhook row. An allowlist, never a blocklist, so a table kept for 90
/// days cannot become a place a credential lasts: DD-API-KEY and Authorization are absent on
/// purpose.
const KEPT_HEADERS: [&str; 12] = [
    "User-Agent",
    "Dd-Ci-Provider-Name",
    "Dd-Evp-Origin",
    "Dd-Evp-Origin-Version",
    "X-Gitlab-Event",
    "X-Gitlab-Event-Uuid",
    "X-Gitlab-Instance",
    "X-Github-Event",
    "X-Github-Delivery",
    "X-Github-Hook-Id",
    "X-Jenkins",
    "Via",
];

/// The top-level keys of an element with a column of their own; the rest go to `extra`.
const KNOWN_KEYS: [&str; 28] = [
    "payload_version",
    "level",
    "url",
    "start",
    "end",
    "partial_retry",
    "queue_time",
    "status",
    "is_manual",
    "trace_id",
    "span_id",
    "parent_span_id",
    "pipeline_id",
    "unique_id",
    "name",
    "id",
    "user",
    "parameters",
    "tags",
    "node",
    "git",
    "parent_pipeline",
    "pipeline_unique_id",
    "pipeline_name",
    "stage_id",
    "stage_name",
    "parent_stage_id",
    "error",
];

/// Which CI system sent an element: what the element says, then the DD-CI-PROVIDER-NAME header,
/// then GitLab's native shape, then "unknown". Never a guess that yields a real provider name: a
/// row tagged "jenkins" that came from elsewhere is worse than one tagged "unknown".
fn provider(element: &Value, header_provider: &str) -> String {
    let named = element.field_text("provider");
    if !named.is_empty() {
        named.to_lowercase()
    } else if !header_provider.is_empty() {
        header_provider.into()
    // GitLab's own webhooks carry object_kind; Datadog's schema does not.
    } else if element.has("object_kind") {
        "gitlab".into()
    } else {
        "unknown".into()
    }
}

/// A JSON bool for a Nullable(UInt8) column: None when the key is absent or not a bool, so "the
/// provider never said" stays apart from false.
fn flag(value: &Value) -> Option<u8> {
    match value {
        Value::Bool(b) => Some(text::flag(*b)),
        _ => None,
    }
}

/// What the request says about every element of its batch.
#[derive(Clone, Debug, Default)]
pub struct Delivery {
    /// DD-CI-PROVIDER-NAME, lower-cased: only a default, the element's own `provider` wins.
    pub provider: String,
    /// The ?service= the Jenkins plugin appends: its CI instance name.
    pub service: String,
    pub id: String,
    pub headers: BTreeMap<String, String>,
}

/// One element of a batch as a row.
///
/// Every id is read as text even where Jenkins sends a number: the same fields carry UUIDs from
/// other providers. A JSON number keeps its literal, so a numeric id keeps every digit.
pub fn webhook_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    delivery: &Delivery,
    body: &str,
    element: &Value,
) -> CiWebhookEvent {
    let node = element.field("node");
    let git = element.field("git");
    let user = element.field("user");
    let error = element.field("error");
    let parent_pipeline = element.field("parent_pipeline");
    let level = element.field_text("level");
    let start_raw = element.field_text("start");
    let end_raw = element.field_text("end");
    CiWebhookEvent {
        tenant_id: tenant.into(),
        received_at,
        provider: provider(element, &delivery.provider),
        // Datadog's "level" first, then GitLab's native "object_kind".
        level: if level.is_empty() { element.field_text("object_kind") } else { level },
        service: delivery.service.clone(),
        payload_version: element.field("payload_version").try_i64(),
        partial_retry: flag(element.field("partial_retry")),
        is_manual: flag(element.field("is_manual")),
        trace_id: element.field_text("trace_id"),
        span_id: element.field_text("span_id"),
        parent_span_id: element.field_text("parent_span_id"),
        id: element.field_text("id"),
        unique_id: element.field_text("unique_id"),
        pipeline_id: element.field_text("pipeline_id"),
        pipeline_unique_id: element.field_text("pipeline_unique_id"),
        pipeline_name: element.field_text("pipeline_name"),
        stage_id: element.field_text("stage_id"),
        stage_name: element.field_text("stage_name"),
        parent_stage_id: element.field_text("parent_stage_id"),
        name: element.field_text("name"),
        url: element.field_text("url"),
        status: element.field_text("status"),
        // The text is kept next to the parse: a format we do not recognise loses the index, never
        // the value, and an absent time stays NULL.
        start_parsed: time::try_rfc3339(&start_raw),
        start_raw,
        end_parsed: time::try_rfc3339(&end_raw),
        end_raw,
        queue_time_ms: element.field("queue_time").try_i64(),
        node_name: node.field_text("name"),
        node_hostname: node.field_text("hostname"),
        node_workspace: node.field_text("workspace"),
        node_labels: node.field("labels").strings(),
        git_repository_url: git.field_text("repository_url"),
        git_default_branch: git.field_text("default_branch"),
        git_branch: git.field_text("branch"),
        git_sha: git.field_text("sha"),
        git_tag: git.field_text("tag"),
        git_message: git.field_text("message"),
        git_author_name: git.field_text("author_name"),
        git_author_email: git.field_text("author_email"),
        git_author_time: git.field_text("author_time"),
        git_committer_name: git.field_text("committer_name"),
        git_committer_email: git.field_text("committer_email"),
        git_commit_time: git.field_text("commit_time"),
        user_name: user.field_text("name"),
        user_email: user.field_text("email"),
        error_message: error.field_text("message"),
        error_type: error.field_text("type"),
        error_domain: error.field_text("domain"),
        error_stack: error.field_text("stack"),
        parent_pipeline_trace_id: parent_pipeline.field_text("trace_id"),
        parent_pipeline_url: parent_pipeline.field_text("url"),
        parameters: element.field("parameters").string_map(),
        // "key:value" strings, and a real multiset: the Jenkins plugin repeats keys for
        // per-configuration axes.
        tags: tags::to_multi_map(element.field("tags").strings()),
        delivery_id: delivery.id.clone(),
        headers: delivery.headers.clone(),
        extra: element.unknown(&KNOWN_KEYS),
        body: body.into(),
    }
}

/// A batch as its elements, each with the text it was written in. A batch is an array; a provider
/// that skipped the batcher sends one object. A bare null is an empty batch.
fn elements<'a>(root: &'a serde_json::Value, written: &'a RawValue) -> Vec<(&'a serde_json::Value, &'a RawValue)> {
    match root {
        serde_json::Value::Array(items) => {
            let texts: Vec<&RawValue> = serde_json::from_str(written.get()).unwrap_or_default();
            items.iter().zip(texts).collect()
        }
        serde_json::Value::Null => Vec::new(),
        _ => vec![(root, written)],
    }
}

async fn store(clickhouse: &clickhouse::Client, ctx: &Ctx, body: &[u8]) -> Result<(), String> {
    let root = match json::try_parse(body) {
        Ok(root) => root,
        Err(e) => {
            raw::store(clickhouse, ctx, "webhook", "decode_error", &format!("batch: {e}"), body).await;
            return Ok(());
        }
    };
    // The text of what parsed, for each element's `body`.
    let repaired = json::repair(body);
    let Ok(written) = raw_root(&repaired) else {
        return Ok(());
    };
    let delivery = Delivery {
        provider: ctx.header("dd-ci-provider-name").trim().to_lowercase(),
        service: ctx.query("service"),
        id: text::first_non_empty(&[
            ctx.header("x-gitlab-event-uuid"),
            ctx.header("x-github-delivery"),
            ctx.header("dd-request-id"),
        ])
        .into(),
        headers: KEPT_HEADERS
            .iter()
            .filter_map(|name| match ctx.header(name) {
                "" => None,
                value => Some((name.to_string(), value.to_string())),
            })
            .collect(),
    };
    let now = Utc::now();
    let mut rows = Vec::new();
    for (element, written) in elements(&root, written) {
        let body = written.get().trim();
        match Value::of_json(element) {
            element @ Value::Object(_) => rows.push(webhook_row(ctx.tenant(), now, &delivery, body, &element)),
            _ => {
                raw::store(
                    clickhouse,
                    ctx,
                    "webhook",
                    "unexpected_shape",
                    "element is not a JSON object",
                    body.as_bytes(),
                )
                .await
            }
        }
    }
    storage::insert(clickhouse, &rows).await
}

/// A batch of CI pipeline, stage and job events.
///
/// The Jenkins plugin only tells 2xx from everything else, so 202 with an empty object is the
/// whole contract.
pub async fn handle(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if !body.is_empty()
        && let Err(e) = store(&clickhouse, &ctx, &body).await
    {
        return storage_failed(e);
    }
    accepted()
}

/// intake.synthetics.<site> — the agent's synthetics test poller (synthetics.collector.enabled).
///
/// The one GET whose answer DRIVES an agent loop: the agent polls it every two seconds with
/// ?agent_hostname=&agent_version=, anything but 200 counts as a failure, and five failures in a
/// row flip the poller unhealthy. So the answer to "what should I run" while nothing exists is an
/// empty list, not a 404.
///
/// The shape is unforgiving: the whole body is decoded in one go, and one test with a subtype
/// other than "UDP", "TCP" or "ICMP" discards EVERY test in the answer.
///
/// Serving real tests from synthetics_test_configs needs a read path the intake does not have;
/// until then the empty list is the honest answer. Nothing is stored: the poll carries no
/// telemetry.
pub async fn handle_synthetics_agent_tests(_: Ctx, _: Body) -> Response {
    json(StatusCode::OK, &serde_json::json!({ "tests": [] }))
}
