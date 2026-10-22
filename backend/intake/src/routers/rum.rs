//! browser-intake.<site> — Real User Monitoring, from the browser, iOS and Android SDKs.
//!
//!   config: browser SDK `proxy`, dd-sdk-ios `customEndpoint`, dd-sdk-android `useCustomEndpoint`
//!
//! One host for all three, because Datadog has no mobile-intake either: an SDK is redirected by
//! changing a host and nothing else. The platforms differ in how they authenticate and how they
//! signal compression, not in where they send.
//!
//!   POST /api/v2/rum       NDJSON of RUM events  → rum_views, rum_events, rum_telemetry,
//!                                                  rum_timeseries
//!   POST /api/v2/logs      NDJSON or JSON array  → logs (logs.rs)
//!   POST /api/v2/replay    multipart             → rum_replay_segments
//!   POST /api/v2/spans     NDJSON of envelopes   → rum_spans
//!   POST /api/v2/profile   multipart             → (profiling.rs)
//!   GET  /api/v2/profiling/quota                 → nothing; every session is admitted
//!   POST /api/v2/debugger  NDJSON or multipart   → (profiling.rs)
//!
//! 202 always. Android drops a batch on 200, iOS on anything but 202, and the browser retries only
//! 408, 429 and 5xx: 202 is the one answer all three read as "kept". Whatever does not decode goes
//! to raw_payloads as "rum".

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

use bytes::Bytes;
use chrono::Utc;
use http::uri::PathAndQuery;
use http::{HeaderName, Method, StatusCode, Uri};
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::rum::{
    RumEvent, RumReplaySegment, RumRequest, RumSpan, RumTelemetry, RumTimeseries, RumView,
};
use serde_json::value::RawValue;
use serde_json::{Map, Value};
use tako::extractors::ipaddr::IpAddrConfig;
use tako::plugins::cors::{CorsBuilder, CorsPlugin};
use tako::router::Router;
use tako::types::{Request, Response};

use crate::body::{self, Part};
use crate::json;
use crate::server::{Body, Ctx, accepted, json as json_response, query_pairs};
use crate::wire::{tags, text, time};

/// The label of this intake in raw_payloads.
const INTAKE: &str = "rum";

/// What a preflight is told it may send. DD-API-KEY and DD-EVP-* are for the mobile SDKs and for a
/// browser deployment that chose headers over the query string; Content-Encoding because a proxy
/// in front of the page may add it.
const CORS_ALLOWED_HEADERS: [&str; 8] = [
    "content-type",
    "content-encoding",
    "dd-api-key",
    "dd-client-token",
    "dd-evp-origin",
    "dd-evp-origin-version",
    "dd-request-id",
    "dd-idempotency-key",
];

/// The value of NINJACAT_RUM_ALLOWED_ORIGINS as a list: comma-separated origins allowed to post RUM
/// data. Unset, empty or "*" gives no list, which means "any origin".
///
/// Any origin is the default on purpose. The key is compiled into a web page, so an allowlist
/// keeps nobody out, and a mandatory one would turn every new subdomain into silent data loss.
pub fn parse_allowed_origins(raw: &str) -> Vec<String> {
    if raw.trim() == "*" {
        return Vec::new();
    }
    raw.split(',').map(str::trim).filter(|origin| !origin.is_empty()).map(String::from).collect()
}

/// The CORS policy of browser-intake.
///
/// The middleware puts the headers on EVERY answer to an allowed origin, the 403 and the 404
/// included, and that matters: to the browser SDK a cross-origin response it may not read is
/// status 0 while online, which it counts as success. It drops the batch and tells nobody, so a
/// bad key or an unserved path would be invisible on both sides. A preflight carries no credential
/// and is answered by the middleware itself, whatever the path.
pub fn cors(allowed_origins: &[String]) -> CorsPlugin {
    let mut policy = CorsBuilder::new();
    if allowed_origins.is_empty() {
        // The origin is echoed back rather than answered with `*`.
        policy = policy.allow_origin_predicate(|_| true);
    }
    for origin in allowed_origins {
        policy = policy.allow_origin(origin.clone());
    }
    let headers: Vec<HeaderName> = CORS_ALLOWED_HEADERS.iter().map(|name| HeaderName::from_static(name)).collect();
    policy.allow_methods(&[Method::POST, Method::GET]).allow_headers(&headers).max_age_secs(86_400).build()
}

/// The reverse proxies whose X-Forwarded-For is believed (NINJACAT_TRUSTED_PROXIES): addresses or
/// networks (`10.0.0.0/8`), comma-separated. The loopback is trusted as well.
pub fn trusted_proxies(raw: &str) -> Result<IpAddrConfig, String> {
    let mut trusted =
        IpAddrConfig::new().trust_network("127.0.0.0/8".parse().unwrap()).trust("::1".parse::<IpAddr>().unwrap());
    for proxy in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        trusted = if proxy.contains('/') {
            trusted.trust_network(proxy.parse().map_err(|e| format!("NINJACAT_TRUSTED_PROXIES: {proxy}: {e}"))?)
        } else {
            trusted.trust(proxy.parse().map_err(|e| format!("NINJACAT_TRUSTED_PROXIES: {proxy}: {e}"))?)
        };
    }
    Ok(trusted)
}

/// browser-intake's router with what sits above it: its CORS policy, the proxies it trusts, and
/// the SDK's `proxy` form turned back into the request it stands for, before any route is picked.
pub fn above(mut router: Router, allowed_origins: &[String], trusted: IpAddrConfig) -> Router {
    router.plugin(cors(allowed_origins));
    let router = Arc::new(router);
    let mut gate = Router::new();
    gate.fallback(move |mut req: Request| {
        let (router, trusted) = (router.clone(), trusted.clone());
        async move {
            req.extensions_mut().insert(trusted);
            unwrap_forward(&mut req);
            router.dispatch(req).await
        }
    });
    gate
}

/// Turns the browser SDK's `proxy: '<url>'` form back into the request it stands for. That form
/// sends POST <url>?ddforward=<encoded path and query>, with the track, ddsource and the key all
/// inside the one parameter.
///
/// The forwarded query wins on conflict; outer parameters that do not collide are kept, so nothing
/// an intermediary added is thrown away.
pub fn unwrap_forward(req: &mut Request) {
    let outer = query_pairs(req.uri());
    let is_forward = |name: &str| name.eq_ignore_ascii_case("ddforward");
    let Some(forward) = outer.iter().find(|(name, _)| is_forward(name)).map(|(_, value)| value.as_str()) else {
        return;
    };
    let without_fragment = forward.split('#').next().unwrap_or_default();
    let (path, query) = without_fragment.split_once('?').unwrap_or((without_fragment, ""));
    if !path.starts_with('/') {
        // Left alone: a 404 naming the path that was actually sent is more use to whoever
        // misconfigured this than a guess would be. Not the value: it may carry the key in its
        // query.
        tracing::warn!(
            "[rum] ddforward ({} characters) is not a path+query, routing the request as it came",
            forward.chars().count()
        );
        return;
    }
    let mut merged: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let forwarded: HashSet<String> = merged.iter().map(|(name, _)| name.to_lowercase()).collect();
    merged.extend(
        outer.iter().filter(|(name, _)| !is_forward(name) && !forwarded.contains(&name.to_lowercase())).cloned(),
    );
    let query = form_urlencoded::Serializer::new(String::new()).extend_pairs(&merged).finish();
    let target = if query.is_empty() { path.to_string() } else { format!("{path}?{query}") };
    let mut uri = req.uri().clone().into_parts();
    match target.parse::<PathAndQuery>() {
        Ok(path_and_query) => {
            uri.path_and_query = Some(path_and_query);
            if let Ok(uri) = Uri::from_parts(uri) {
                *req.uri_mut() = uri;
            }
        }
        Err(_) => {
            tracing::warn!("[rum] ddforward is not a path+query, routing the request as it came")
        }
    }
}

// Every event variant is read with `json::at`, a path through nested objects: their `view` and
// `session` objects are the same JSON whatever the variant, and a field no schema declares yet is
// read like any other. No number passes through a float unless it was written as one.

static EMPTY_OBJECT: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));

/// The whole numbers of a list; anything else in it is passed over.
fn int64s(value: Option<&Value>) -> Vec<i64> {
    json::lenient_items(value).iter().filter_map(json::int64_of).collect()
}

/// Timeseries and span starts are NANOSECONDS, unlike `date`.
fn wire_nanos(nanos: i64) -> i64 {
    if nanos <= 0 { Utc::now().timestamp_nanos_opt().unwrap_or_default() } else { nanos }
}

/// Query parameters that have a column, and both spellings of the credential. Everything else goes
/// to query_extra; these do not, so a key never becomes durable in a table.
const REQUEST_COLUMNS: [&str; 11] = [
    "ddsource",
    "dd-evp-origin",
    "dd-evp-origin-version",
    "dd-request-id",
    "dd-evp-encoding",
    "batch_time",
    "_dd.api",
    "_dd.retry_count",
    "_dd.retry_after",
    "dd-api-key",
    "api_key",
];

/// The query parameters without a column as JSON, or "". Values stay arrays: a parameter may
/// legally repeat.
fn query_extra(ctx: &Ctx) -> String {
    let mut extra: Map<String, Value> = Map::new();
    for (name, value) in query_pairs(&ctx.parts.uri) {
        if !REQUEST_COLUMNS.contains(&name.to_lowercase().as_str()) {
            let values = extra.entry(name).or_insert_with(|| Value::Array(Vec::new()));
            if let Value::Array(values) = values {
                values.push(Value::String(value));
            }
        }
    }
    if extra.is_empty() { String::new() } else { json::compact(&Value::Object(extra)) }
}

/// The sender columns shared by every RUM table. The browser puts its identity in the query string
/// (it sets no headers at all), iOS and Android put theirs in headers, so each field is looked for
/// in both places.
pub fn request_info(ctx: &Ctx) -> RumRequest {
    // The browser spells a retry as _dd.retry_count / _dd.retry_after, iOS and Android as
    // ddtags=retry_count:N,retry_after:CODE. retry_after is stored as sent: a delay in
    // milliseconds from the browser, the HTTP status that caused the retry from the mobile SDKs.
    let retry_tags = tags::to_map(tags::split_ddtags(&ctx.query("ddtags")));
    let retry = |name: &str| {
        let tagged = retry_tags.get(name).map(String::as_str).unwrap_or_default();
        text::first_non_empty(&[&ctx.query(&format!("_dd.{name}")), tagged]).parse::<i64>().ok()
    };
    let either =
        |query: &str, header: &str| text::first_non_empty(&[&ctx.query(query), ctx.header(header)]).to_string();

    RumRequest {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        ddsource: ctx.query("ddsource"),
        evp_origin: either("dd-evp-origin", "dd-evp-origin"),
        evp_origin_version: either("dd-evp-origin-version", "dd-evp-origin-version"),
        // The only record that the batch was compressed: an SDK release that silently stops
        // compressing would otherwise be visible nowhere.
        evp_encoding: either("dd-evp-encoding", "content-encoding"),
        request_id: either("dd-request-id", "dd-request-id"),
        idempotency_key: ctx.header("dd-idempotency-key").into(),
        dd_api: ctx.query("_dd.api"),
        batch_time: ctx.query("batch_time").parse::<i64>().ok().map(time::from_unix_millis),
        retry_count: retry("retry_count").filter(|n| *n >= 0).map(|n| n as u32),
        retry_after: retry("retry_after"),
        remote_addr: ctx.client_ip(),
        user_agent: ctx.header("user-agent").into(),
        query_extra: query_extra(ctx),
    }
}

/// The body, inflated when the browser said so in the query string. The browser sends no
/// Content-Encoding header — ?dd-evp-encoding=deflate is its only signal — so the body has come
/// through untouched. A body that does not inflate comes back as it was.
fn inflated(ctx: &Ctx, body: Bytes) -> Bytes {
    if !body.is_empty() && ctx.query("dd-evp-encoding").eq_ignore_ascii_case("deflate") {
        body::decompress("deflate", body)
    } else {
        body
    }
}

/// A batch as its non-empty lines. The SDKs write no trailing newline and no \r, but a proxy that
/// rewrote the body might, and a blank line is not an event.
fn ndjson_lines(body: &[u8]) -> Vec<&[u8]> {
    let is_space = |b: &u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 11 | 12);
    body.split(|b| *b == b'\n')
        .map(|line| {
            let first = line.iter().position(|b| !is_space(b)).unwrap_or(line.len());
            let last = line.iter().rposition(|b| !is_space(b)).map_or(first, |at| at + 1);
            &line[first..last.max(first)]
        })
        .filter(|line| !line.is_empty())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// "view" or "view_update" → rum_views.
    View(String),
    /// action, error, resource, long_task, vital, transition → rum_events.
    Plain(String),
    Telemetry,
    Timeseries,
}

#[derive(PartialEq)]
enum Discriminator<'a> {
    Absent,
    Text(&'a str),
    /// Present, but not a string: it can match nothing.
    NotText,
}

fn discriminator<'a>(path: &[&str], event: &'a Value) -> Discriminator<'a> {
    let mut current = event;
    for key in path {
        let Some(object) = current.as_object() else {
            return Discriminator::NotText;
        };
        match object.get(*key) {
            Some(next) => current = next,
            None => return Discriminator::Absent,
        }
    }
    current.as_str().map_or(Discriminator::NotText, Discriminator::Text)
}

/// Which table an event belongs to, decided by the discriminator fields Datadog's schemas declare
/// (`type`, then `vital.type`, `telemetry.type`, `telemetry.status`, `timeseries.name`) and never
/// by shape. The error is the `type` that matched nothing: an SDK newer than this build.
pub fn classify(event: &Value) -> Result<EventKind, String> {
    let event_type = match discriminator(&["type"], event) {
        Discriminator::Text(name) => name,
        _ => "",
    };
    let unknown = Err(event_type.to_string());
    match event_type {
        "view" | "view_update" => Ok(EventKind::View(event_type.into())),
        "action" | "transition" | "error" | "long_task" | "resource" => Ok(EventKind::Plain(event_type.into())),
        "vital" => match discriminator(&["vital", "type"], event) {
            Discriminator::Text("duration" | "operation_step" | "app_launch") => {
                Ok(EventKind::Plain(event_type.into()))
            }
            _ => unknown,
        },
        "telemetry" => {
            let status = discriminator(&["telemetry", "status"], event);
            let logged = status == Discriminator::Text("error") || status == Discriminator::Text("debug");
            match discriminator(&["telemetry", "type"], event) {
                Discriminator::Text("configuration" | "usage") => Ok(EventKind::Telemetry),
                // A log event may leave telemetry.type out; its status is what tells an error from
                // a debug message.
                Discriminator::Absent | Discriminator::Text("log") if logged => Ok(EventKind::Telemetry),
                _ => unknown,
            }
        }
        "timeseries" => match discriminator(&["timeseries", "name"], event) {
            Discriminator::Text("memory" | "cpu") => Ok(EventKind::Timeseries),
            _ => unknown,
        },
        _ => unknown,
    }
}

fn lenient_at(path: &[&str], root: &Value) -> String {
    json::lenient_string(json::at(path, root))
}

fn int64_at(path: &[&str], root: &Value) -> Option<i64> {
    json::at(path, root).and_then(json::truncated_int64_of)
}

fn bool_at(path: &[&str], root: &Value) -> Option<bool> {
    json::at(path, root).and_then(json::bool_of)
}

/// An object member, or an empty object.
fn object_at<'a>(path: &[&str], root: &'a Value) -> &'a Value {
    json::at(path, root).filter(|v| v.is_object()).unwrap_or(&EMPTY_OBJECT)
}

fn date_of(o: &Value) -> chrono::DateTime<Utc> {
    time::wire_millis(int64_at(&["date"], o).unwrap_or(0))
}

fn ddtags_of(o: &Value) -> ninjapaws_storage::rows::Tags {
    tags::to_multi_map(tags::split_ddtags(&lenient_at(&["ddtags"], o)))
}

/// A view or view_update as the columns rum_views filters by. Both land in the same row: they carry
/// the same identity and _dd.document_version, and view_update is the newer wire spelling of an
/// update this table already models as a replacement.
pub fn view_row(request: &RumRequest, event_type: &str, o: &Value, raw: &str) -> RumView {
    let view = object_at(&["view"], o);
    let performance = object_at(&["performance"], view);

    // Web Vitals moved from flat view.* fields into view.performance.* and both spellings are
    // still on the wire: the modern one first, the deprecated one as the fallback.
    let vital = |modern: &[&str], deprecated: &str| {
        json::at(modern, performance).or_else(|| json::at(&[deprecated], view)).and_then(json::truncated_int64_of)
    };
    let r = request.clone();

    RumView {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        date: date_of(o),
        application_id: lenient_at(&["application", "id"], o),
        session_id: lenient_at(&["session", "id"], o),
        view_id: lenient_at(&["view", "id"], o),
        document_version: int64_at(&["_dd", "document_version"], o).unwrap_or(0).max(0) as u64,
        event_type: event_type.into(),
        service: lenient_at(&["service"], o),
        version: lenient_at(&["version"], o),
        build_version: lenient_at(&["build_version"], o),
        build_id: lenient_at(&["build_id"], o),
        source: lenient_at(&["source"], o),
        session_type: lenient_at(&["session", "type"], o),
        session_has_replay: bool_at(&["session", "has_replay"], o),
        session_is_active: bool_at(&["session", "is_active"], o),
        session_sampled_for_replay: bool_at(&["session", "sampled_for_replay"], o),
        usr_id: lenient_at(&["usr", "id"], o),
        usr_name: lenient_at(&["usr", "name"], o),
        usr_email: lenient_at(&["usr", "email"], o),
        usr_anonymous_id: lenient_at(&["usr", "anonymous_id"], o),
        account_id: lenient_at(&["account", "id"], o),
        account_name: lenient_at(&["account", "name"], o),
        view_url: lenient_at(&["url"], view),
        view_name: lenient_at(&["name"], view),
        view_referrer: lenient_at(&["referrer"], view),
        view_loading_type: lenient_at(&["loading_type"], view),
        view_loading_time: int64_at(&["loading_time"], view),
        view_time_spent: int64_at(&["time_spent"], view).unwrap_or(0),
        view_is_active: bool_at(&["is_active"], view),
        view_is_slow_rendered: bool_at(&["is_slow_rendered"], view),
        action_count: int64_at(&["action", "count"], view),
        error_count: int64_at(&["error", "count"], view),
        crash_count: int64_at(&["crash", "count"], view),
        long_task_count: int64_at(&["long_task", "count"], view),
        frozen_frame_count: int64_at(&["frozen_frame", "count"], view),
        resource_count: int64_at(&["resource", "count"], view),
        frustration_count: int64_at(&["frustration", "count"], view),
        lcp: vital(&["lcp", "timestamp"], "largest_contentful_paint"),
        cls: json::at(&["cls", "score"], performance)
            .and_then(json::float_of)
            .or_else(|| json::at(&["cumulative_layout_shift"], view).and_then(json::float_of)),
        inp: vital(&["inp", "duration"], "interaction_to_next_paint"),
        fcp: vital(&["fcp", "timestamp"], "first_contentful_paint"),
        fid: vital(&["fid", "duration"], "first_input_delay"),
        fbc: int64_at(&["fbc", "timestamp"], performance),
        ttfb: int64_at(&["first_byte"], view),
        device_type: lenient_at(&["device", "type"], o),
        device_brand: lenient_at(&["device", "brand"], o),
        device_model: lenient_at(&["device", "model"], o),
        device_name: lenient_at(&["device", "name"], o),
        os_name: lenient_at(&["os", "name"], o),
        os_version: lenient_at(&["os", "version"], o),
        connectivity_status: lenient_at(&["connectivity", "status"], o),
        context: json::compact_or_empty(json::at(&["context"], o)),
        feature_flags: json::compact_or_empty(json::at(&["feature_flags"], o)),
        ddtags: ddtags_of(o),
        event: raw.into(),
    }
}

/// An action, error, resource, long_task, vital or transition. Each kind fills its own block of
/// columns and leaves the others empty.
pub fn event_row(request: &RumRequest, event_type: &str, o: &Value, raw: &str) -> RumEvent {
    let r = request.clone();
    RumEvent {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        date: date_of(o),
        application_id: lenient_at(&["application", "id"], o),
        session_id: lenient_at(&["session", "id"], o),
        view_id: lenient_at(&["view", "id"], o),
        event_type: event_type.into(),
        service: lenient_at(&["service"], o),
        version: lenient_at(&["version"], o),
        build_version: lenient_at(&["build_version"], o),
        build_id: lenient_at(&["build_id"], o),
        source: lenient_at(&["source"], o),
        session_type: lenient_at(&["session", "type"], o),
        session_has_replay: bool_at(&["session", "has_replay"], o),
        usr_id: lenient_at(&["usr", "id"], o),
        usr_name: lenient_at(&["usr", "name"], o),
        usr_email: lenient_at(&["usr", "email"], o),
        usr_anonymous_id: lenient_at(&["usr", "anonymous_id"], o),
        account_id: lenient_at(&["account", "id"], o),
        account_name: lenient_at(&["account", "name"], o),
        view_url: lenient_at(&["view", "url"], o),
        view_name: lenient_at(&["view", "name"], o),
        view_referrer: lenient_at(&["view", "referrer"], o),
        device_type: lenient_at(&["device", "type"], o),
        device_brand: lenient_at(&["device", "brand"], o),
        device_model: lenient_at(&["device", "model"], o),
        device_name: lenient_at(&["device", "name"], o),
        os_name: lenient_at(&["os", "name"], o),
        os_version: lenient_at(&["os", "version"], o),
        connectivity_status: lenient_at(&["connectivity", "status"], o),
        action_type: lenient_at(&["action", "type"], o),
        // The label a user sees is action.target.name, not a field on action.
        action_name: lenient_at(&["action", "target", "name"], o),
        action_id: lenient_at(&["action", "id"], o),
        action_frustration_types: json::text_list(json::at(&["action", "frustration", "type"], o)),
        error_id: lenient_at(&["error", "id"], o),
        error_message: lenient_at(&["error", "message"], o),
        error_type: lenient_at(&["error", "type"], o),
        error_source: lenient_at(&["error", "source"], o),
        error_stack: lenient_at(&["error", "stack"], o),
        // The crash and ANR reports the mobile SDKs write at the NEXT launch are ordinary errors
        // with this flag set, which is why their date can predate the session they belong to.
        error_is_crash: bool_at(&["error", "is_crash"], o),
        error_handling: lenient_at(&["error", "handling"], o),
        error_fingerprint: lenient_at(&["error", "fingerprint"], o),
        resource_id: lenient_at(&["resource", "id"], o),
        resource_type: lenient_at(&["resource", "type"], o),
        resource_url: lenient_at(&["resource", "url"], o),
        resource_method: lenient_at(&["resource", "method"], o),
        resource_status_code: int64_at(&["resource", "status_code"], o),
        resource_duration: int64_at(&["resource", "duration"], o),
        resource_size: int64_at(&["resource", "size"], o),
        long_task_id: lenient_at(&["long_task", "id"], o),
        long_task_duration: int64_at(&["long_task", "duration"], o),
        vital_id: lenient_at(&["vital", "id"], o),
        vital_type: lenient_at(&["vital", "type"], o),
        vital_name: lenient_at(&["vital", "name"], o),
        vital_duration: int64_at(&["vital", "duration"], o),
        context: json::compact_or_empty(json::at(&["context"], o)),
        ddtags: ddtags_of(o),
        event: raw.into(),
    }
}

/// A type:"telemetry" event: the SDK reporting on itself. Every SDK sends these on the RUM track,
/// including one configured for Logs only.
pub fn telemetry_row(request: &RumRequest, o: &Value, raw: &str) -> RumTelemetry {
    let telemetry = object_at(&["telemetry"], o);
    let r = request.clone();
    RumTelemetry {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        date: date_of(o),
        application_id: lenient_at(&["application", "id"], o),
        session_id: lenient_at(&["session", "id"], o),
        view_id: lenient_at(&["view", "id"], o),
        action_id: lenient_at(&["action", "id"], o),
        r#type: lenient_at(&["type"], o),
        status: lenient_at(&["status"], telemetry),
        telemetry_type: lenient_at(&["type"], telemetry),
        message: lenient_at(&["message"], telemetry),
        error_stack: lenient_at(&["error", "stack"], telemetry),
        error_kind: lenient_at(&["error", "kind"], telemetry),
        // ~150 optional keys that change with every SDK release: worth keeping whole, not worth a
        // column each.
        configuration: json::compact_or_empty(json::at(&["configuration"], telemetry)),
        usage_feature: lenient_at(&["usage", "feature"], telemetry),
        service: lenient_at(&["service"], o),
        version: lenient_at(&["version"], o),
        source: lenient_at(&["source"], o),
        effective_sample_rate: json::at(&["effective_sample_rate"], o).and_then(json::float_of),
        experimental_features: json::text_list(json::at(&["experimental_features"], o)),
        device_brand: lenient_at(&["device", "brand"], telemetry),
        device_model: lenient_at(&["device", "model"], telemetry),
        device_architecture: lenient_at(&["device", "architecture"], telemetry),
        os_name: lenient_at(&["os", "name"], telemetry),
        os_version: lenient_at(&["os", "version"], telemetry),
        os_build: lenient_at(&["os", "build"], telemetry),
        event: raw.into(),
    }
}

/// A type:"timeseries" event: a mobile-only run of cpu or memory samples, timestamps in one array
/// and the measured quantities in a parallel one.
pub fn timeseries_row(request: &RumRequest, o: &Value, raw: &str) -> RumTimeseries {
    let series = object_at(&["timeseries"], o);
    let r = request.clone();
    RumTimeseries {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        date: date_of(o),
        application_id: lenient_at(&["application", "id"], o),
        session_id: lenient_at(&["session", "id"], o),
        view_id: lenient_at(&["view", "id"], o),
        name: lenient_at(&["name"], series),
        timeseries_id: lenient_at(&["id"], series),
        timeseries_schema: lenient_at(&["schema"], series),
        start: wire_nanos(int64_at(&["start"], series).unwrap_or(0)),
        end: wire_nanos(int64_at(&["end"], series).unwrap_or(0)),
        timestamps: int64s(json::at(&["data", "timestamps"], series)),
        values: json::compact_or_empty(json::at(&["data", "values"], series)),
        service: lenient_at(&["service"], o),
        version: lenient_at(&["version"], o),
        source: lenient_at(&["source"], o),
        context: json::compact_or_empty(json::at(&["context"], o)),
        ddtags: ddtags_of(o),
        event: raw.into(),
    }
}

/// POST /api/v2/rum: NDJSON, one event per line, from every SDK.
pub async fn handle_rum(ctx: Ctx, body: Body) -> Response {
    if ctx.is_sweep() {
        return accepted();
    }
    let body = inflated(&ctx, body.0);
    if body.is_empty() {
        return accepted();
    }
    let request = request_info(&ctx);
    let (mut views, mut events, mut telemetry, mut series) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for line in ndjson_lines(&body) {
        match json::try_parse(line) {
            Err(problem) => ctx.raw(INTAKE, "decode_error", &format!("not a JSON object: {problem}"), line),
            Ok(event) if !event.is_object() => ctx.raw(INTAKE, "decode_error", "not a JSON object", line),
            Ok(event) => {
                let raw = text::utf8(line);
                match classify(&event) {
                    // SDKs ship on schema snapshots months apart, so a newer one sends event types
                    // this build has never seen. That line is kept raw and the rest of the batch is
                    // stored.
                    Err(unknown_type) => ctx.raw(INTAKE, "unknown_event", &unknown_type, line),
                    Ok(EventKind::View(event_type)) => views.push(view_row(&request, &event_type, &event, &raw)),
                    Ok(EventKind::Plain(event_type)) => events.push(event_row(&request, &event_type, &event, &raw)),
                    Ok(EventKind::Telemetry) => telemetry.push(telemetry_row(&request, &event, &raw)),
                    Ok(EventKind::Timeseries) => series.push(timeseries_row(&request, &event, &raw)),
                }
            }
        }
    }
    ctx.write(views);
    ctx.write(events);
    ctx.write(telemetry);
    ctx.write(series);
    accepted()
}

/// The elements of a JSON list, each decoded and as the JSON text it had in `list`.
fn elements_with_text(list: &[u8]) -> Vec<(Value, String)> {
    let elements: Vec<&RawValue> = serde_json::from_slice(list).unwrap_or_default();
    elements
        .into_iter()
        .map(|element| (serde_json::from_str(element.get()).unwrap_or(Value::Null), element.get().to_string()))
        .collect()
}

/// The `event` part: one object from the browser, an array from iOS and Android, because a mobile
/// batch spans several views and ships a segment for each. Every entry comes back decoded and as
/// the JSON text stored with its row; None when the part is neither shape.
pub fn replay_metadata(data: &[u8]) -> Option<Vec<(Value, String)>> {
    match json::try_parse(data).ok()? {
        Value::Array(_) => Some(
            elements_with_text(&json::repair(data))
                .into_iter()
                .map(|(entry, raw)| (if entry.is_object() { entry } else { EMPTY_OBJECT.clone() }, raw))
                .collect(),
        ),
        root @ Value::Object(_) => Some(vec![(root, text::utf8(data))]),
        // `null` stands for an object with nothing in it.
        Value::Null => Some(vec![(EMPTY_OBJECT.clone(), text::utf8(data))]),
        _ => None,
    }
}

/// A recording segment or a canvas resource. The part name is the primary signal ("image" for
/// resources, "segment" or "file<i>" for recordings); the metadata's own "type" is for a producer
/// that names its parts differently.
pub fn replay_variant(part_name: &str, meta: &Value) -> &'static str {
    if part_name.starts_with("image") {
        "resource"
    } else if part_name == "segment" || part_name.starts_with("file") {
        "segment"
    } else if lenient_at(&["type"], meta) == "resource" {
        "resource"
    } else {
        "segment"
    }
}

pub fn replay_row(request: &RumRequest, part: &Part, meta: &Value, meta_raw: &str) -> RumReplaySegment {
    let variant = replay_variant(&part.name, meta);
    let millis_at = |name: &str| int64_at(&[name], meta).map(time::from_unix_millis);
    let r = request.clone();
    RumReplaySegment {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        application_id: lenient_at(&["application", "id"], meta),
        session_id: lenient_at(&["session", "id"], meta),
        view_id: lenient_at(&["view", "id"], meta),
        source: lenient_at(&["source"], meta),
        variant: variant.into(),
        part_name: part.name.clone(),
        part_filename: part.file_name.clone(),
        // None, never the epoch: a resource has no window at all, and 1970 would put it on a
        // timeline.
        start: millis_at("start"),
        end: millis_at("end"),
        records_count: int64_at(&["records_count"], meta),
        has_full_snapshot: bool_at(&["has_full_snapshot"], meta),
        // The browser sends an index, Android an explicit null, iOS omits the key: three states,
        // so an option.
        index_in_view: int64_at(&["index_in_view"], meta),
        creation_reason: lenient_at(&["creation_reason"], meta),
        raw_segment_size: int64_at(&["raw_segment_size"], meta),
        compressed_segment_size: int64_at(&["compressed_segment_size"], meta),
        // The bytes go in the column that says what they are, untouched.
        segment: Blob(if variant == "resource" { Vec::new() } else { part.data.to_vec() }),
        image: Blob(if variant == "resource" { part.data.to_vec() } else { Vec::new() }),
        event: meta_raw.into(),
    }
}

/// POST /api/v2/replay: Session Replay uploads, binary parts plus one `event` part of metadata.
///
///   browser segment    part "segment" (zlib) + "event" (one object)
///   mobile segment     parts "file0".."fileN" (zlib) + "event" (an array)
///   resource (canvas)  parts "image" (raw, repeated on mobile) + "event"
///
/// The blobs are not decoded, by design: Datadog's own intake stores segments unprocessed, and the
/// SDK emits zlib streams meant to be concatenated later, which inflating and re-deflating would
/// throw away.
pub async fn handle_replay(ctx: Ctx, body: Body) -> Response {
    if ctx.is_sweep() {
        return accepted();
    }
    let body = inflated(&ctx, body.0);
    if body.is_empty() {
        return accepted();
    }
    let parts = match body::try_parts(ctx.header("content-type"), body.clone()).await {
        Ok(parts) => parts,
        Err(problem) => {
            ctx.raw(INTAKE, "unexpected_shape", &format!("replay body is not multipart: {problem}"), &body);
            return accepted();
        }
    };
    let request = request_info(&ctx);
    let mut metadata = Vec::new();
    for part in parts.iter().filter(|part| part.name == "event") {
        metadata = replay_metadata(&part.data).unwrap_or_else(|| {
            ctx.raw(INTAKE, "decode_error", "replay event part is neither an object nor an array", &part.data);
            Vec::new()
        });
    }
    // Entry i describes blob i, in wire order.
    let rows: Vec<RumReplaySegment> = parts
        .iter()
        .filter(|part| part.name != "event")
        .enumerate()
        .map(|(i, part)| match metadata.get(i) {
            Some((meta, meta_raw)) => replay_row(&request, part, meta, meta_raw),
            None => replay_row(&request, part, &EMPTY_OBJECT, ""),
        })
        .collect();
    if !metadata.is_empty() && rows.len() != metadata.len() {
        // Every blob is still stored, but some got the wrong metadata, and that is worth seeing.
        tracing::warn!(
            "[rum-replay] {} blob parts against {} metadata entries: the pairing is by position and they disagree",
            rows.len(),
            metadata.len()
        );
    }
    ctx.write(rows);
    accepted()
}

/// The span's meta as a flat map with dotted keys. meta.device and meta.os arrive as nested objects
/// among flat keys, and flattening is what keeps meta['device.brand'] reachable. A plain map, not
/// the tag multiset: a JSON object cannot repeat a key.
pub fn flatten_meta(meta: &Value) -> BTreeMap<String, String> {
    fn flatten(prefix: &str, object: &Value, found: &mut BTreeMap<String, String>) {
        for (name, value) in json::members(object) {
            let key = if prefix.is_empty() { name } else { format!("{prefix}.{name}") };
            if value.is_object() {
                flatten(&key, &value, found);
            } else {
                // A null keeps its key with no value: "sent as null" stays distinct from "not
                // sent".
                found.insert(key, json::text(Some(&value)));
            }
        }
    }
    let mut found = BTreeMap::new();
    flatten("", meta, &mut found);
    found
}

fn span_metrics(metrics: &Value) -> BTreeMap<String, f64> {
    json::members(metrics).into_iter().filter_map(|(name, value)| json::float_of(&value).map(|f| (name, f))).collect()
}

pub fn span_row(request: &RumRequest, env: &str, envelope_extra: &str, o: &Value, raw: &str) -> RumSpan {
    let r = request.clone();
    RumSpan {
        tenant_id: r.tenant_id,
        received_at: r.received_at,
        ddsource: r.ddsource,
        evp_origin: r.evp_origin,
        evp_origin_version: r.evp_origin_version,
        evp_encoding: r.evp_encoding,
        request_id: r.request_id,
        idempotency_key: r.idempotency_key,
        dd_api: r.dd_api,
        batch_time: r.batch_time,
        retry_count: r.retry_count,
        retry_after: r.retry_after,
        remote_addr: r.remote_addr,
        user_agent: r.user_agent,
        query_extra: r.query_extra,
        // Text, never a number: iOS writes the low 64 bits as hex, Android types all three as
        // strings.
        trace_id: json::text(json::at(&["trace_id"], o)),
        span_id: json::text(json::at(&["span_id"], o)),
        parent_id: json::text(json::at(&["parent_id"], o)),
        name: lenient_at(&["name"], o),
        service: lenient_at(&["service"], o),
        resource: lenient_at(&["resource"], o),
        r#type: lenient_at(&["type"], o),
        env: env.into(),
        start: wire_nanos(int64_at(&["start"], o).unwrap_or(0)),
        duration_ns: int64_at(&["duration"], o).unwrap_or(0),
        // As Go's int8 conversion did: it wraps.
        error: int64_at(&["error"], o).unwrap_or(0) as i8,
        meta: flatten_meta(object_at(&["meta"], o)),
        metrics: span_metrics(object_at(&["metrics"], o)),
        span: raw.into(),
        envelope_extra: envelope_extra.into(),
    }
}

/// Whatever the envelope carried besides spans and env, as JSON; usually "". A key a future SDK
/// release adds is stored rather than lost.
fn envelope_extra(envelope: &Value) -> String {
    let extra: Map<String, Value> = envelope
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(name, _)| *name != "spans" && *name != "env")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    if extra.is_empty() { String::new() } else { json::compact(&Value::Object(extra)) }
}

/// POST /api/v2/spans: the mobile SDKs' own trace format.
///
/// Not the agent's /v0.4/traces: NDJSON of {"spans":[...],"env":"..."} envelopes with a flat,
/// hand-rolled span shape that only dd-sdk-ios and dd-sdk-android produce, with no published
/// schema.
pub async fn handle_spans(ctx: Ctx, body: Body) -> Response {
    if ctx.is_sweep() {
        return accepted();
    }
    let body = inflated(&ctx, body.0);
    if body.is_empty() {
        return accepted();
    }
    let request = request_info(&ctx);
    let mut rows = Vec::new();
    for line in ndjson_lines(&body) {
        let envelope = match json::try_parse(line) {
            Err(problem) => {
                ctx.raw(INTAKE, "decode_error", &problem, line);
                continue;
            }
            Ok(envelope) if !envelope.is_object() && !envelope.is_null() => {
                ctx.raw(INTAKE, "decode_error", "span envelope is not an object", line);
                continue;
            }
            // A `null` envelope, a `null` spans array and a `null` span are each read as empty.
            Ok(Value::Null) => EMPTY_OBJECT.clone(),
            Ok(envelope) => envelope,
        };
        match envelope.get("spans") {
            None => ctx.raw(INTAKE, "unexpected_shape", "span envelope without a spans array", line),
            Some(Value::Null) => {}
            Some(spans) if !spans.is_array() => ctx.raw(INTAKE, "decode_error", "spans is not an array", line),
            Some(_) => {
                let env = json::lenient_string(json::field("env", &envelope));
                let extra = envelope_extra(&envelope);
                // Each span's own text, so it is stored as sent.
                let repaired = json::repair(line);
                let members: HashMap<String, &RawValue> = serde_json::from_slice(&repaired).unwrap_or_default();
                let spans = members.get("spans").map(|spans| elements_with_text(spans.get().as_bytes()));
                for (span, raw) in spans.unwrap_or_default() {
                    match span {
                        Value::Object(_) => rows.push(span_row(&request, &env, &extra, &span, &raw)),
                        Value::Null => rows.push(span_row(&request, &env, &extra, &EMPTY_OBJECT, &raw)),
                        _ => ctx.raw(INTAKE, "decode_error", "span is not an object", raw.as_bytes()),
                    }
                }
            }
        }
    }
    ctx.write(rows);
    accepted()
}

/// GET /api/v2/profiling/quota?session_id=… — the browser profiler asks before it starts whether
/// this session may be profiled. There is no quota here, so every session is admitted. Without an
/// answer the SDK profiles anyway, but marks the session's events with the reason "api-error".
pub async fn handle_profiling_quota(_: Ctx, _: Body) -> Response {
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "data": { "attributes": { "admitted": true, "reason": "quota_ok" } } }),
    )
}
