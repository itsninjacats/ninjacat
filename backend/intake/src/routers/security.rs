//! The security hosts:
//!
//!   cws-intake.<site>                         /api/v2/secdump     → cws_activity_dumps, cws_dump_nodes
//!   runtime-security-http-intake.logs.<site>  /api/v2/secruntime,
//!                                             /api/v2/secinfo     → security_events
//!   cspm-intake.<site>                        /api/v2/compliance  → security_events
//!   sbom-intake.<site>                        /api/v2/sbom        → sbom_entities, sbom_components, sbom_vulnerabilities
//!   sds-intake.<site>                         /api/v2/sdsresult   → sds_scans, sds_results, sds_matches
//!
//!   agent config: runtime_security_config.activity_dump.remote_storage.endpoints.logs_dd_url (secdump),
//!                 runtime_security_config.endpoints.logs_dd_url (secruntime, secinfo),
//!                 compliance_config.endpoints.logs_dd_url (compliance);
//!                 sbom and sdsresult are event platform tracks, which follow DD_SITE.

// A node's syscalls and image_tags are deprecated in the .proto, but agents still send them and
// the node row counts them.
#![allow(deprecated)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use ninjapaws_proto::cyclonedx::v1_4::{
    Classification, Component, HashAlg, ImpactAnalysisJustification, ImpactAnalysisState, LicenseChoice, Property,
    Scope, Vulnerability, VulnerabilityResponse, license, license_choice,
};
use ninjapaws_proto::datadog::cws::dumpsv1::{GenerationType, ProcessActivityNode, SecDump};
use ninjapaws_proto::datadog::sbom::{SbomPayload, SbomSourceType, SbomStatus, sbom_entity};
use ninjapaws_proto::datadog::sds::sds_result_payload::scan_location::{ScanLocation, ScanLocationType};
use ninjapaws_proto::datadog::sds::sds_result_payload::scan_metadata::scan_task_metadata::Status;
use ninjapaws_proto::datadog::sds::sds_result_payload::{self, ScanLocation as Location};
use ninjapaws_proto::datadog::sds::{ScanStats, ScanningSource, SdsResultPayload, scanning_source};
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::Extra;
use ninjapaws_storage::rows::sds::{SdsLocation, SdsMatch, SdsResult, SdsScan};
use ninjapaws_storage::rows::security::{
    CwsActivityDump, CwsDumpNode, SbomComponent, SbomEntity, SbomVulnerability, SecurityEvent,
};
use prost::Message;
use prost::encoding::{DecodeContext, WireType};
use serde_json::Value;
use tako::types::Response;
use uuid::Uuid;

use crate::body;
use crate::json;
use crate::server::{Ctx, Routes, accepted};
use crate::wire::{enum_name, tags, text, time};

/// cws-intake.<site>
pub fn mount_cws(routes: &mut Routes) {
    routes.post("/api/v2/secdump", handle_sec_dump);
}

/// runtime-security-http-intake.logs.<site>
pub fn mount_runtime_security(routes: &mut Routes) {
    routes.post("/api/v2/secruntime", |ctx, body| handle_track("secruntime", ctx, body));
    routes.post("/api/v2/secinfo", |ctx, body| handle_track("secinfo", ctx, body));
}

/// cspm-intake.<site>
pub fn mount_cspm(routes: &mut Routes) {
    routes.post("/api/v2/compliance", |ctx, body| handle_track("compliance", ctx, body));
}

/// sbom-intake.<site>
pub fn mount_sbom(routes: &mut Routes) {
    routes.post("/api/v2/sbom", handle_sbom);
}

/// sds-intake.<site>
pub fn mount_sds(routes: &mut Routes) {
    routes.post("/api/v2/sdsresult", handle_sds_result);
}

/// Header keys with a column of their own; the rest go to header_extra.
static KNOWN_HEADER_KEYS: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| BTreeSet::from(["host", "service", "ddsource", "ddtags", "dns_names"]));

/// The "dump" part as a SecDump. None when the bytes do not decode, or decode to nothing: protobuf
/// reads almost any bytes as some message.
pub fn decode_dump(data: &[u8]) -> Option<SecDump> {
    let dump = SecDump::decode(data).ok()?;
    (dump.metadata.is_some() || !dump.tree.is_empty()).then_some(dump)
}

fn count_tree(nodes: &[ProcessActivityNode]) -> u32 {
    nodes.iter().map(|node| 1 + count_tree(&node.children)).sum()
}

/// The cws_activity_dumps row, from whichever of the two parts decoded. `header` is the "event"
/// part (the agent's ActivityDumpHeader) as a JSON object. Both parts are kept as they arrived,
/// decoded or not.
pub fn dump_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    dump_id: Uuid,
    header: Option<&Value>,
    header_bytes: &[u8],
    dump: Option<&SecDump>,
    dump_bytes: &[u8],
) -> CwsActivityDump {
    let mut row = CwsActivityDump {
        tenant_id: tenant.into(),
        received_at,
        dump_id,
        header_host: String::new(),
        header_service: String::new(),
        header_source: String::new(),
        header_tags: Default::default(),
        dns_names: String::new(),
        header_extra: Extra::new(),
        header_raw: Blob(header_bytes.to_vec()),
        dump_host: String::new(),
        dump_service: String::new(),
        dump_source: String::new(),
        dump_tags: Default::default(),
        agent_version: String::new(),
        agent_commit: String::new(),
        kernel_version: String::new(),
        linux_distribution: String::new(),
        arch: String::new(),
        metadata_name: String::new(),
        protobuf_version: String::new(),
        differentiate_args: 0,
        comm: String::new(),
        container_id: String::new(),
        start_raw: None,
        end_raw: None,
        size_raw: None,
        serialization: String::new(),
        cgroup_id: String::new(),
        cgroup_manager: String::new(),
        tree_node_count: 0,
        dump: Blob(dump_bytes.to_vec()),
    };

    if let Some(h) = header {
        row.header_host = json::lenient_string(json::field("host", h));
        row.header_service = json::lenient_string(json::field("service", h));
        row.header_source = json::lenient_string(json::field("ddsource", h));
        row.header_tags = tags::to_multi_map(json::tag_list(json::field("ddtags", h)));
        row.dns_names = json::compact_or_empty(json::field("dns_names", h));
        row.header_extra = json::other_members(&KNOWN_HEADER_KEYS, json::compact, Some(h));
    }

    if let Some(d) = dump {
        row.dump_host = d.host.clone();
        row.dump_service = d.service.clone();
        row.dump_source = d.source.clone();
        row.dump_tags = tags::to_multi_map(&d.tags);
        row.tree_node_count = count_tree(&d.tree);
        if let Some(metadata) = &d.metadata {
            row.agent_version = metadata.agent_version.clone();
            row.agent_commit = metadata.agent_commit.clone();
            row.kernel_version = metadata.kernel_version.clone();
            row.linux_distribution = metadata.linux_distribution.clone();
            row.arch = metadata.arch.clone();
            row.metadata_name = metadata.name.clone();
            row.protobuf_version = metadata.protobuf_version.clone();
            row.differentiate_args = text::flag(metadata.differentiate_args);
            row.comm = metadata.comm.clone();
            row.container_id = metadata.container_id.clone();
            row.start_raw = Some(metadata.start);
            row.end_raw = Some(metadata.end);
            row.size_raw = Some(metadata.size);
            row.serialization = metadata.serialization.clone();
            row.cgroup_id = metadata.cgroup_id.clone();
            row.cgroup_manager = metadata.cgroup_manager.clone();
        }
    }
    row
}

/// One row per node of the dump's process tree, depth-first. node_path is the index path from the
/// root, parent_path the parent's.
pub fn flatten_tree(
    tenant: &str,
    received_at: DateTime<Utc>,
    dump_id: Uuid,
    tree: Vec<ProcessActivityNode>,
) -> Vec<CwsDumpNode> {
    let mut rows = Vec::new();
    walk(tenant, received_at, dump_id, tree, &[], &mut rows);
    rows
}

fn walk(
    tenant: &str,
    received_at: DateTime<Utc>,
    dump_id: Uuid,
    nodes: Vec<ProcessActivityNode>,
    path: &[u32],
    rows: &mut Vec<CwsDumpNode>,
) {
    for (i, mut node) in nodes.into_iter().enumerate() {
        let mut node_path = path.to_vec();
        node_path.push(i as u32);
        // The node column holds the node without its subtree: the children have rows of their own.
        let children = std::mem::take(&mut node.children);
        let process = node.process.clone().unwrap_or_default();
        rows.push(CwsDumpNode {
            tenant_id: tenant.into(),
            received_at,
            dump_id,
            depth: node_path.len() as u16,
            parent_path: path.to_vec(),
            node: json::proto::message(Some(&node)),
            pid: process.pid,
            ppid: process.ppid,
            comm: process.comm,
            container_id: process.container_id,
            file_path: process.file.map(|file| file.path).unwrap_or_default(),
            args: process.args,
            image_tags: node.image_tags,
            matched_rule_ids: node.matched_rules.into_iter().map(|rule| rule.rule_id).collect(),
            generation_type: enum_name(node.generation_type, GenerationType::as_str_name),
            files_count: node.files.len() as u32,
            dns_count: node.dns_names.len() as u32,
            sockets_count: node.sockets.len() as u32,
            syscalls_count: node.syscalls.len() as u32,
            node_path: node_path.clone(),
        });
        walk(tenant, received_at, dump_id, children, &node_path, rows);
    }
}

/// POST /api/v2/secdump: gzip(multipart/form-data) with two parts, "event" (the JSON header) and
/// "dump" (SecDump protobuf, which the agent labels application/json — its own bug, the part
/// header is ignored).
///
/// The parts are decoded independently: a broken "event" does not lose the "dump", and the other
/// way round.
pub async fn handle_sec_dump(ctx: Ctx, body: Bytes) -> Response {
    let Some(boundary) = body::boundary(ctx.header("content-type")) else {
        ctx.raw("secdump", "decode_error", "content-type is not multipart/form-data", &body);
        return accepted();
    };
    let parts = body::parts(&boundary, body.clone()).await;
    for part in &parts {
        if part.name != "event" && part.name != "dump" {
            tracing::warn!("[secdump] unexpected part {} ({} B)", part.name, part.data.len());
        }
    }
    let part_data = |name: &str| parts.iter().rev().find(|part| part.name == name).map(|part| part.data.clone());
    let header_bytes = part_data("event");
    let dump_bytes = part_data("dump");
    let header = header_bytes.as_deref().and_then(json::try_parse_object).map(Value::Object);
    let dump = dump_bytes.as_deref().and_then(decode_dump);

    if let (Some(bytes), None) = (&header_bytes, &header) {
        tracing::warn!("[secdump] the event part is not a JSON object ({} B)", bytes.len());
    }
    if let (Some(bytes), None) = (&dump_bytes, &dump) {
        tracing::warn!("[secdump] the dump part did not decode ({} B)", bytes.len());
    }

    if header.is_none() && dump.is_none() {
        ctx.raw("secdump", "decode_error", "neither multipart part decoded", &body);
        return accepted();
    }
    let received_at = Utc::now();
    let dump_id = Uuid::new_v4();
    let row = dump_row(
        ctx.tenant(),
        received_at,
        dump_id,
        header.as_ref(),
        header_bytes.as_deref().unwrap_or_default(),
        dump.as_ref(),
        dump_bytes.as_deref().unwrap_or_default(),
    );
    ctx.write(vec![row]);
    if let Some(dump) = dump {
        ctx.write(flatten_tree(ctx.tenant(), received_at, dump_id, dump.tree));
    }
    accepted()
}

/// Envelope keys with a column of their own; the rest go to extra.
static KNOWN_ENVELOPE_KEYS: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| BTreeSet::from(["message", "status", "timestamp", "hostname", "service", "ddsource", "ddtags"]));

/// The envelope's timestamp: a millisecond epoch as a number or as text, or RFC 3339 text,
/// depending on the producer. None when absent or unreadable.
pub fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    match value? {
        Value::String(written) => {
            time::try_rfc3339(written).or_else(|| written.parse::<i64>().ok().map(time::from_unix_millis))
        }
        number @ Value::Number(_) => json::int64_of(number).map(time::from_unix_millis),
        _ => None,
    }
}

/// The event inside an envelope's "message": an object, or a string holding one as escaped JSON.
/// None when it is missing or neither.
pub fn inner_message(message: Option<&Value>) -> Option<Value> {
    match message? {
        object @ Value::Object(_) => Some(object.clone()),
        Value::String(text) => json::try_parse_object(text.as_bytes()).map(Value::Object),
        _ => None,
    }
}

/// A body as a list of envelopes, or why it is not one. A null entry is an empty envelope, a null
/// body an empty batch.
pub fn decode_envelopes(body: &[u8]) -> Result<Vec<Value>, String> {
    let envelopes = json::try_parse_array(body)?;
    match envelopes.iter().position(|e| !e.is_object() && !e.is_null()) {
        Some(index) => Err(format!("entry {index} is not a JSON object")),
        None => Ok(envelopes),
    }
}

/// One row per envelope {message, status, timestamp, hostname, service, ddsource, ddtags}, in
/// arrival order.
///
/// The event sits in "message" as escaped JSON: for secruntime a CWS event, for compliance a
/// check event or resource log. The agent publishes no type for any of them, so the event is kept
/// as JSON. An envelope whose message does not decode still gets its row.
pub fn event_rows(tenant: &str, received_at: DateTime<Utc>, track: &str, envelopes: &[Value]) -> Vec<SecurityEvent> {
    envelopes
        .iter()
        .enumerate()
        .map(|(i, envelope)| {
            let message = json::field("message", envelope);
            let event = inner_message(message);
            let event_text = |path: &[&str]| json::lenient_string(event.as_ref().and_then(|e| json::at(path, e)));
            SecurityEvent {
                tenant_id: tenant.into(),
                received_at,
                track: track.into(),
                seq_in_batch: i as u32,
                timestamp: parse_timestamp(json::field("timestamp", envelope)),
                hostname: json::lenient_string(json::field("hostname", envelope)),
                service: json::lenient_string(json::field("service", envelope)),
                ddsource: json::lenient_string(json::field("ddsource", envelope)),
                status: json::lenient_string(json::field("status", envelope)),
                ddtags: tags::to_multi_map(json::tag_list(json::field("ddtags", envelope))),
                extra: json::other_members(&KNOWN_ENVELOPE_KEYS, json::compact, Some(envelope)),
                message_raw: json::compact_or_empty(message),
                message: json::compact_sorted_or_empty(event.as_ref()),
                message_decoded: text::flag(event.is_some()),
                rule_id: event_text(&["agent", "rule_id"]),
                policy_name: event_text(&["agent", "policy_name"]),
                evt_name: event_text(&["evt", "name"]),
                evt_category: event_text(&["evt", "category"]),
                title: event_text(&["title"]),
                event_kind: event_text(&["kind"]),
            }
        })
        .collect()
}

/// A logs-pipeline batch: a JSON array of envelopes. One handler serves three tracks; the rows
/// share a table and carry the track.
pub async fn handle_track(track: &'static str, ctx: Ctx, body: Bytes) -> Response {
    match decode_envelopes(&body) {
        Err(problem) => {
            ctx.raw(track, "unexpected_shape", &format!("the body is not a list of envelopes: {problem}"), &body)
        }
        Ok(envelopes) => ctx.write(event_rows(ctx.tenant(), Utc::now(), track, &envelopes)),
    }
    accepted()
}

/// A property list as a multiset: CycloneDX allows a name to repeat, and real scanners do repeat
/// them.
fn property_multi_map(properties: &[Property]) -> BTreeMap<String, Vec<String>> {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for property in properties {
        by_name.entry(property.name.clone()).or_default().push(property.value.clone().unwrap_or_default());
    }
    by_name
}

/// A component's licenses, each by the most specific name it has: the SPDX id, else the license's
/// name, else the expression.
fn license_names(licenses: &[LicenseChoice]) -> Vec<String> {
    licenses
        .iter()
        .filter_map(|choice| match &choice.choice {
            Some(license_choice::Choice::License(found)) => Some(match &found.license {
                Some(license::License::Id(id)) if !id.is_empty() => id.clone(),
                Some(license::License::Name(name)) => name.clone(),
                _ => String::new(),
            }),
            Some(license_choice::Choice::Expression(expression)) if !expression.is_empty() => Some(expression.clone()),
            _ => None,
        })
        .collect()
}

/// A component list as rows, each component followed by its sub-components.
fn component_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    entity_id: Uuid,
    components: &[Component],
    parent_bom_ref: &str,
    depth: u16,
    rows: &mut Vec<SbomComponent>,
) {
    for c in components {
        let bom_ref = c.bom_ref.clone().unwrap_or_default();
        rows.push(SbomComponent {
            tenant_id: tenant.into(),
            received_at,
            entity_id,
            bom_ref: bom_ref.clone(),
            parent_bom_ref: parent_bom_ref.into(),
            depth,
            kind: enum_name(c.r#type, Classification::as_str_name),
            name: c.name.clone(),
            version: c.version.clone(),
            // Optional on the wire: "never set" is NULL, not "".
            purl: c.purl.clone(),
            cpe: c.cpe.clone(),
            group: c.group.clone(),
            publisher: c.publisher.clone(),
            author: c.author.clone(),
            description: c.description.clone(),
            scope: enum_name(c.scope.unwrap_or_default(), Scope::as_str_name),
            licenses: license_names(&c.licenses),
            hashes: c.hashes.iter().map(|h| (enum_name(h.alg, HashAlg::as_str_name), h.value.clone())).collect(),
            properties: property_multi_map(&c.properties),
            external_references: json::proto::messages(&c.external_references),
            evidence: json::proto::messages(&c.evidence),
        });
        component_rows(tenant, received_at, entity_id, &c.components, &bom_ref, depth + 1, rows);
    }
}

fn vulnerability_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    entity_id: Uuid,
    vulnerabilities: &[Vulnerability],
) -> Vec<SbomVulnerability> {
    vulnerabilities
        .iter()
        .map(|v| {
            let analysis = v.analysis.as_ref();
            SbomVulnerability {
                tenant_id: tenant.into(),
                received_at,
                entity_id,
                bom_ref: v.bom_ref.clone().unwrap_or_default(),
                id: v.id.clone().unwrap_or_default(),
                source_name: v.source.as_ref().and_then(|s| s.name.clone()),
                source_url: v.source.as_ref().and_then(|s| s.url.clone()),
                ratings: json::proto::messages(&v.ratings),
                cwes: v.cwes.clone(),
                description: v.description.clone(),
                detail: v.detail.clone(),
                recommendation: v.recommendation.clone(),
                advisories: json::proto::messages(&v.advisories),
                created: time::of_timestamp(v.created.as_ref()),
                published: time::of_timestamp(v.published.as_ref()),
                updated: time::of_timestamp(v.updated.as_ref()),
                analysis_state: analysis
                    .map(|a| enum_name(a.state.unwrap_or_default(), ImpactAnalysisState::as_str_name))
                    .unwrap_or_default(),
                analysis_justification: analysis
                    .map(|a| enum_name(a.justification.unwrap_or_default(), ImpactAnalysisJustification::as_str_name))
                    .unwrap_or_default(),
                analysis_response: analysis
                    .map(|a| a.response.iter().map(|r| enum_name(*r, VulnerabilityResponse::as_str_name)).collect())
                    .unwrap_or_default(),
                analysis_detail: analysis.and_then(|a| a.detail.clone()).unwrap_or_default(),
                affects_refs: v.affects.iter().map(|a| a.r#ref.clone()).collect(),
                affects: json::proto::messages(&v.affects),
                properties: property_multi_map(&v.properties),
            }
        })
        .collect()
}

/// A protobuf Duration in whole milliseconds. One that does not fit 64 bits of nanoseconds is kept
/// at that limit rather than wrapped around.
fn duration_millis(duration: &pbjson_types::Duration) -> i64 {
    let nanoseconds = i128::from(duration.seconds) * 1_000_000_000 + i128::from(duration.nanos);
    let nanoseconds = nanoseconds.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
    nanoseconds / 1_000_000
}

/// The rows of an SBOM payload: one per entity, plus every component and vulnerability its
/// CycloneDX BOM unpacks into.
pub fn sbom_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    payload: &SbomPayload,
) -> (Vec<SbomEntity>, Vec<SbomComponent>, Vec<SbomVulnerability>) {
    let mut entities = Vec::new();
    let mut components = Vec::new();
    let mut vulnerabilities = Vec::new();

    for entity in &payload.entities {
        let entity_id = Uuid::new_v4();
        let mut row = SbomEntity {
            tenant_id: tenant.into(),
            received_at,
            entity_id,
            payload_version: payload.version,
            host: payload.host.clone(),
            // Source and dd_env are optional fields of the payload, shared by its entities: unset
            // is NULL, not "".
            source: payload.source.clone(),
            dd_env: payload.dd_env.clone(),
            kind: enum_name(entity.r#type, SbomSourceType::as_str_name),
            id: entity.id.clone(),
            generated_at: time::of_timestamp(entity.generated_at.as_ref()),
            repo_tags: entity.repo_tags.clone(),
            repo_digests: entity.repo_digests.clone(),
            in_use: text::flag(entity.in_use),
            generation_duration_ms: entity.generation_duration.as_ref().map(duration_millis),
            dd_tags: tags::to_multi_map(&entity.dd_tags),
            heartbeat: text::flag(entity.heartbeat),
            hash: entity.hash.clone(),
            status: enum_name(entity.status, SbomStatus::as_str_name),
            kernel_version: entity.kernel_version.clone(),
            cpu_architecture: entity.cpu_architecture.clone(),
            error: String::new(),
            bom: String::new(),
            bom_raw: String::new(),
            component_count: 0,
            vulnerability_count: 0,
        };

        match &entity.sbom {
            Some(sbom_entity::Sbom::Error(error)) => row.error = error.clone(),
            Some(sbom_entity::Sbom::Cyclonedx(bom)) => {
                let mut entity_components = Vec::new();
                component_rows(tenant, received_at, entity_id, &bom.components, "", 0, &mut entity_components);
                let entity_vulnerabilities = vulnerability_rows(tenant, received_at, entity_id, &bom.vulnerabilities);
                row.component_count = entity_components.len() as u32;
                row.vulnerability_count = entity_vulnerabilities.len() as u32;
                row.bom = json::proto::message(Some(bom));
                components.extend(entity_components);
                vulnerabilities.extend(entity_vulnerabilities);
            }
            None => {}
        }
        entities.push(row);
    }
    (entities, components, vulnerabilities)
}

/// POST /api/v2/sbom: one SBOMPayload per request, protobuf.
pub async fn handle_sbom(ctx: Ctx, body: Bytes) -> Response {
    match SbomPayload::decode(&body[..]) {
        Err(problem) => ctx.raw("sbom", "decode_error", &problem.to_string(), &body),
        // Protobuf reads almost any bytes as some message.
        Ok(payload) if payload.entities.is_empty() => {
            ctx.raw("sbom", "unexpected_shape", "decoded to zero entities", &body)
        }
        Ok(payload) => {
            let (entities, components, vulnerabilities) = sbom_rows(ctx.tenant(), Utc::now(), &payload);
            ctx.write(entities);
            ctx.write(components);
            ctx.write(vulnerabilities);
        }
    }
    accepted()
}

fn wire_type_name(wire_type: WireType) -> String {
    match wire_type {
        WireType::Varint => "varint".into(),
        WireType::ThirtyTwoBit => "fixed32".into(),
        WireType::SixtyFourBit => "fixed64".into(),
        WireType::LengthDelimited => "bytes".into(),
        other => format!("type{}", other as u8),
    }
}

/// A protobuf message's top-level fields in wire order, as "<number>:<wire type>[xN]" entries; a
/// run of the same number is one entry. None when the bytes are not a sequence of fields. Nested
/// messages are not entered: without the schema they cannot be told from strings.
pub fn wire_layout(body: &[u8]) -> Option<String> {
    // Consecutive top-level fields of one number: the number, its wire type, how many.
    let mut runs: Vec<(u32, WireType, usize)> = Vec::new();
    let mut input = body;
    while !input.is_empty() {
        let (field, wire_type) = prost::encoding::decode_key(&mut input).ok()?;
        prost::encoding::skip_field(wire_type, field, &mut input, DecodeContext::default()).ok()?;
        match runs.last_mut() {
            Some(last) if last.0 == field => last.2 += 1,
            _ => runs.push((field, wire_type, 1)),
        }
    }
    let entries: Vec<String> = runs
        .into_iter()
        .map(|(field, wire_type, count)| {
            let entry = format!("{field}:{}", wire_type_name(wire_type));
            if count > 1 { format!("{entry}x{count}") } else { entry }
        })
        .collect();
    Some(entries.join(" "))
}

/// The deprecated rules map as one JSON object, rule id to rule; "" when empty.
fn sds_rules(payload: &SdsResultPayload) -> String {
    if payload.rules.is_empty() {
        return String::new();
    }
    let sorted: BTreeMap<&String, &sds_result_payload::RuleInfo> = payload.rules.iter().collect();
    let entries: Vec<String> = sorted
        .into_iter()
        .map(|(id, rule)| format!("{}:{}", json::quoted(id), json::proto::message(Some(rule))))
        .collect();
    format!("{{{}}}", entries.join(","))
}

/// The columns of a result's location. Each kind names its database, schema and table
/// differently; everything else of it stays in the JSON.
fn sds_location(location: Option<&Location>) -> SdsLocation {
    let none = SdsLocation::default();
    let Some(location) = location else { return none };
    match &location.scan_location {
        Some(ScanLocation::PostgresTable(table)) => SdsLocation {
            kind: "postgres_table".into(),
            database_name: table.database_name.clone(),
            schema_name: table.schema_name.clone(),
            table_name: table.table_name.clone(),
            ..none
        },
        Some(ScanLocation::SnowflakeTable(table)) => SdsLocation {
            kind: "snowflake_table".into(),
            database_name: table.database_name.clone(),
            schema_name: table.schema_name.clone(),
            table_name: table.table_name.clone(),
            ..none
        },
        Some(ScanLocation::RdsTable(table)) => SdsLocation {
            kind: "rds_table".into(),
            database_name: table.database_name.clone(),
            table_name: table.table_name.clone(),
            ..none
        },
        Some(ScanLocation::S3File(file)) => SdsLocation { kind: "s3_file".into(), path: file.path.clone(), ..none },
        // The deprecated fields, which older scanners still fill.
        None => match &location.scan_location_type {
            Some(ScanLocationType::Path(path)) => SdsLocation { kind: "path".into(), path: path.clone(), ..none },
            Some(ScanLocationType::Database(database)) => SdsLocation {
                kind: "database".into(),
                database_name: database.clone(),
                table_name: location.table.clone(),
                ..none
            },
            None => none,
        },
    }
}

/// What a table location says about the table: None where its kind has no such field.
#[derive(Default)]
struct SdsTableFacts {
    table_row_count: Option<i64>,
    scanned_row_count: Option<i64>,
    column_names: Vec<String>,
    column_types: Vec<String>,
}

fn sds_table_facts(location: Option<&Location>) -> SdsTableFacts {
    match location.and_then(|l| l.scan_location.as_ref()) {
        Some(ScanLocation::PostgresTable(table)) => SdsTableFacts {
            table_row_count: Some(table.table_row_count),
            scanned_row_count: Some(table.scanned_row_count),
            column_names: table.scanned_columns.iter().map(|c| c.name.clone()).collect(),
            column_types: table.scanned_columns.iter().map(|c| c.data_type.clone()).collect(),
        },
        Some(ScanLocation::SnowflakeTable(table)) => SdsTableFacts {
            table_row_count: Some(table.table_row_count),
            scanned_row_count: Some(table.scanned_row_count),
            column_names: table.scanned_columns.iter().map(|c| c.name.clone()).collect(),
            column_types: table.scanned_columns.iter().map(|c| c.data_type.clone()).collect(),
        },
        Some(ScanLocation::RdsTable(table)) => SdsTableFacts {
            table_row_count: table.table_row_count,
            scanned_row_count: table.scanned_row_count,
            ..SdsTableFacts::default()
        },
        _ => SdsTableFacts::default(),
    }
}

/// Who scanned: the kind, and that kind's own fields.
#[derive(Default)]
struct SdsSource {
    kind: String,
    version: String,
    region: String,
    hostname: String,
    service_name: String,
}

fn sds_source(source: Option<&ScanningSource>) -> SdsSource {
    match source.and_then(|s| s.source.as_ref()) {
        Some(scanning_source::Source::Agent(agent)) => SdsSource {
            kind: "agent".into(),
            version: agent.version.clone(),
            hostname: agent.hostname.clone(),
            ..SdsSource::default()
        },
        Some(scanning_source::Source::Agentless(agentless)) => SdsSource {
            kind: "agentless".into(),
            version: agentless.version.clone(),
            region: agentless.region.clone(),
            ..SdsSource::default()
        },
        Some(scanning_source::Source::DatadogCrawler(crawler)) => SdsSource {
            kind: "datadog_crawler".into(),
            service_name: crawler.service_name.clone(),
            ..SdsSource::default()
        },
        None => SdsSource::default(),
    }
}

/// One payload as the rows of its three tables.
pub struct SdsRows {
    pub scan: SdsScan,
    pub results: Vec<SdsResult>,
    pub matches: Vec<SdsMatch>,
}

pub fn sds_rows(tenant: &str, received_at: DateTime<Utc>, scan_id: Uuid, payload: &SdsResultPayload) -> SdsRows {
    let (resource_type, resource_name) =
        payload.resource.as_ref().map(|r| (r.r#type.clone(), r.name.clone())).unwrap_or_default();
    let source = sds_source(payload.scanning_source.as_ref());
    let stats = payload.scan_stats.as_ref();
    let stat = |read: fn(&ScanStats) -> i64| stats.map(read);
    let scanner = payload.scanner_metadata.as_ref();

    let scan = SdsScan {
        tenant_id: tenant.into(),
        received_at,
        scan_id,
        timestamp: (payload.timestamp != 0).then(|| time::from_unix_millis(payload.timestamp)),
        resource_type: resource_type.clone(),
        resource_name: resource_name.clone(),
        scanning_source: source.kind,
        source_version: source.version,
        source_region: source.region,
        source_hostname: source.hostname,
        source_service_name: source.service_name,
        scanner_version: scanner.and_then(|s| s.version.clone()).unwrap_or_default(),
        scanner_region: scanner.and_then(|s| s.region.clone()).unwrap_or_default(),
        rule_ids: payload.rule_ids.clone(),
        rules: sds_rules(payload),
        scan_duration_ms: stat(|s| s.scan_duration_ms),
        total_files_found: stat(|s| s.total_files_found),
        files_scanned: stat(|s| s.files_scanned),
        files_skipped_unsupported_type: stat(|s| s.files_skipped_unsupported_type),
        files_partially_scanned_size_limit: stat(|s| s.files_partially_scanned_size_limit),
        total_data_scanned_bytes: stat(|s| s.total_data_scanned_bytes),
        skipped_files_by_type: stats
            .map(|s| s.skipped_files_by_type.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default(),
        result_count: payload.scan_results.len() as u32,
    };

    let mut results = Vec::new();
    let mut matches = Vec::new();
    for (i, result) in payload.scan_results.iter().enumerate() {
        let index = i as u32;
        let location = sds_location(result.location.as_ref());
        let table = sds_table_facts(result.location.as_ref());
        let task = result.scan_metadata.as_ref().and_then(|m| m.scan_task_metadata.as_ref());

        results.push(SdsResult {
            tenant_id: tenant.into(),
            received_at,
            scan_id,
            result_index: index,
            resource_type: resource_type.clone(),
            resource_name: resource_name.clone(),
            location_kind: location.kind.clone(),
            database_name: location.database_name.clone(),
            schema_name: location.schema_name.clone(),
            table_name: location.table_name.clone(),
            path: location.path.clone(),
            table_row_count: table.table_row_count,
            scanned_row_count: table.scanned_row_count,
            scanned_column_names: table.column_names,
            scanned_column_types: table.column_types,
            location: json::proto::message(result.location.as_ref()),
            duration: result.duration,
            task_id: task.map(|t| t.task_id.clone()).unwrap_or_default(),
            sub_task_id: task.map(|t| t.sub_task_id.clone()).unwrap_or_default(),
            task_started_at: task.and_then(|t| time::of_timestamp(t.started_at.as_ref())),
            task_ended_at: task.and_then(|t| time::of_timestamp(t.ended_at.as_ref())),
            task_status: task.map(|t| enum_name(t.status, Status::as_str_name)).unwrap_or_default(),
            failure_reason: task.and_then(|t| t.failure_reason.clone()).unwrap_or_default(),
            match_count: result.matches.len() as u32,
            table_match_count: result.table_matches.len() as u32,
        });

        let blank = SdsMatch {
            tenant_id: tenant.into(),
            received_at,
            scan_id,
            result_index: index,
            resource_type: resource_type.clone(),
            resource_name: resource_name.clone(),
            location_kind: location.kind.clone(),
            database_name: location.database_name.clone(),
            schema_name: location.schema_name.clone(),
            table_name: location.table_name.clone(),
            path: location.path.clone(),
            kind: String::new(),
            rule_id: String::new(),
            sample: String::new(),
            start_index: 0,
            end_index: 0,
            match_path: None,
            line: None,
            column: None,
            start_line: None,
            end_line: None,
            row: None,
            start_index_in_line: None,
            end_index_in_line: None,
            match_status: None,
            column_name: String::new(),
            count_matched_rows: 0,
            count_total_rows: 0,
            count_matches: 0,
        };
        for m in &result.matches {
            matches.push(SdsMatch {
                kind: "match".into(),
                rule_id: m.rule_id.clone(),
                sample: m.sample.clone(),
                start_index: m.start_index,
                end_index: m.end_index,
                match_path: m.path.clone(),
                line: m.line,
                column: m.column,
                start_line: m.start_line,
                end_line: m.end_line,
                row: m.row,
                start_index_in_line: m.start_index_in_line,
                end_index_in_line: m.end_index_in_line,
                match_status: m.match_status.clone(),
                ..blank.clone()
            });
        }
        for m in &result.table_matches {
            matches.push(SdsMatch {
                kind: "table_match".into(),
                rule_id: m.rule_id.clone(),
                column_name: m.column_name.clone(),
                count_matched_rows: m.count_matched_rows,
                count_total_rows: m.count_total_rows,
                count_matches: m.count_matches,
                ..blank.clone()
            });
        }
    }
    SdsRows { scan, results, matches }
}

fn sds_is_blank(payload: &SdsResultPayload) -> bool {
    let unnamed = payload.resource.as_ref().is_none_or(|r| r.r#type.is_empty() && r.name.is_empty());
    unnamed && payload.scan_results.is_empty()
}

/// POST /api/v2/sdsresult: Sensitive Data Scanner results, one SdsResultPayload per request
/// (datadog/sds/sds_result.proto).
///
/// A protobuf parser fails open: bytes meant for another endpoint decode into a message with
/// nothing in it. A payload that names no resource and has no result is therefore kept raw, its
/// wire layout in the note, instead of becoming an empty scan.
pub async fn handle_sds_result(ctx: Ctx, body: Bytes) -> Response {
    let layout = || match wire_layout(&body).as_deref() {
        None => "protobuf did not parse as a sequence of top-level fields".to_string(),
        Some("") => "no fields (empty or wrong payload type?)".to_string(),
        Some(layout) => layout.to_string(),
    };
    match SdsResultPayload::decode(&body[..]) {
        Err(problem) => {
            ctx.raw("sds", "decode_error", &format!("protobuf SdsResultPayload: {problem}; {}", layout()), &body)
        }
        Ok(payload) if sds_is_blank(&payload) => ctx.raw(
            "sds",
            "unexpected_shape",
            &format!("not an SdsResultPayload: no resource and no results; {}", layout()),
            &body,
        ),
        Ok(payload) => {
            let rows = sds_rows(ctx.tenant(), Utc::now(), Uuid::new_v4(), &payload);
            ctx.write(vec![rows.scan]);
            ctx.write(rows.results);
            ctx.write(rows.matches);
        }
    }
    accepted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninjapaws_proto::cyclonedx::v1_4::{
        Bom, ExternalReference, ExternalReferenceType, Hash, License, Severity, Source, VulnerabilityAffects,
        VulnerabilityAnalysis, VulnerabilityRating,
    };
    use ninjapaws_proto::datadog::cws::dumpsv1::{FileInfo, MatchedRule, Metadata, ProcessInfo};
    use ninjapaws_proto::datadog::sbom::SbomEntity as Entity;
    use ninjapaws_proto::datadog::sds::sds_result_payload::{
        PostgresTable, Resource, RuleInfo, S3File, ScanMatch, ScanMetadata, ScanResult, TableMatch, postgres_table,
        scan_metadata,
    };

    fn value(text: &str) -> Value {
        json::try_parse(text.as_bytes()).unwrap()
    }

    fn dump_id() -> Uuid {
        Uuid::parse_str("01020304-0506-0708-090a-0b0c0d0e0f10").unwrap()
    }

    fn now() -> DateTime<Utc> {
        "2026-09-23T10:00:00Z".parse().unwrap()
    }

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn node(pid: u32, ppid: u32, comm: &str) -> ProcessActivityNode {
        ProcessActivityNode {
            process: Some(ProcessInfo { pid, ppid, comm: comm.into(), ..Default::default() }),
            ..Default::default()
        }
    }

    /// init → bash (-c "cat file", matched by r1) → cat: one root, one child with a rule match and
    /// args, one grandchild.
    fn three_level_tree() -> ProcessActivityNode {
        let mut grandchild = node(3, 2, "cat");
        grandchild.process.as_mut().unwrap().file = Some(FileInfo { path: "/bin/cat".into(), ..Default::default() });
        let mut child = node(2, 1, "bash");
        child.process.as_mut().unwrap().args = vec!["-c".into(), "cat file > out".into()];
        child.matched_rules.push(MatchedRule {
            rule_id: "r1".into(),
            policy_name: "default".into(),
            ..Default::default()
        });
        child.children.push(grandchild);
        let mut root = node(1, 0, "init");
        root.children.push(child);
        root
    }

    #[test]
    fn a_tree_is_flattened_depth_first_each_node_knowing_its_path() {
        let second = ProcessActivityNode { generation_type: 7, ..Default::default() };
        let rows = flatten_tree("default", now(), dump_id(), vec![three_level_tree(), second]);
        assert_eq!(rows.len(), 4);
        let (root, child, grandchild, other) = (&rows[0], &rows[1], &rows[2], &rows[3]);

        assert_eq!(root.node_path, [0]);
        assert!(root.parent_path.is_empty());
        assert_eq!((root.depth, root.comm.as_str(), root.pid), (1, "init", 1));
        assert_eq!(root.generation_type, "UNKNOWN");

        assert_eq!(child.node_path, [0, 0]);
        assert_eq!(child.parent_path, root.node_path);
        assert_eq!(child.depth, 2);
        assert_eq!(child.args, ["-c", "cat file > out"]);
        assert_eq!(child.matched_rule_ids, ["r1"]);
        assert_eq!(child.ppid, 1);

        assert_eq!(grandchild.node_path, [0, 0, 0]);
        assert_eq!(grandchild.parent_path, child.node_path);
        assert_eq!(grandchild.depth, 3);
        assert_eq!(grandchild.file_path, "/bin/cat");

        // A node without a process, and a generation type this build does not know.
        assert_eq!(other.node_path, [1]);
        assert_eq!(other.comm, "");
        assert_eq!(other.generation_type, "7");

        assert!(rows.iter().all(|row| row.dump_id == dump_id() && row.tenant_id == "default"));
    }

    #[test]
    fn a_nodes_json_leaves_its_children_out_and_stays_readable() {
        let rows = flatten_tree("default", now(), dump_id(), vec![three_level_tree()]);
        let child = value(&rows[1].node);
        assert!(child.get("children").is_none());
        assert_eq!(child["process"]["comm"], "bash");
        assert_eq!(child["matchedRules"][0]["ruleId"], "r1");
        // The > is not escaped: the text is searched as it is in ClickHouse.
        assert!(rows[1].node.contains("cat file > out"));
    }

    #[test]
    fn a_dump_without_metadata_has_no_start_end_or_size() {
        let dump = SecDump { host: "h1".into(), tree: vec![three_level_tree()], ..Default::default() };
        let row = dump_row("default", now(), dump_id(), None, &[], Some(&dump), &[0x08, 0x01]);
        assert_eq!((row.start_raw, row.end_raw, row.size_raw), (None, None, None));
        assert_eq!(row.tree_node_count, 3);
        assert_eq!(row.dump_host, "h1");
        assert_eq!(row.dump, Blob(vec![0x08, 0x01]));
    }

    /// A start of 0 with metadata present is a real reading: the counters are relative to kernel
    /// boot.
    #[test]
    fn metadata_with_a_zero_counter_is_a_present_zero() {
        let dump = SecDump {
            metadata: Some(Metadata {
                agent_version: "7.58.2".into(),
                start: 0,
                end: 500,
                differentiate_args: true,
                ..Default::default()
            }),
            tags: vec!["env:prod".into(), "kube_service:a".into(), "kube_service:b".into()],
            ..Default::default()
        };
        let row = dump_row("default", now(), dump_id(), None, &[], Some(&dump), &[]);
        assert_eq!((row.start_raw, row.end_raw, row.size_raw), (Some(0), Some(500), Some(0)));
        assert_eq!(row.agent_version, "7.58.2");
        assert_eq!(row.differentiate_args, 1);
        assert_eq!(row.dump_tags["kube_service"], ["a", "b"]);
    }

    #[test]
    fn the_headers_tags_are_a_multiset_and_its_unknown_keys_are_kept() {
        let header = value(
            r#"{"host": "h1", "service": "cws-agent", "ddsource": "security-agent",
                "ddtags": ["env:prod","kube_service:a","kube_service:b"],
                "dns_names": ["example.com"], "unexpected": {"nested":true}, "n": 15}"#,
        );
        let row = dump_row("default", now(), dump_id(), Some(&header), &[], None, &[]);
        assert_eq!(
            [row.header_host.as_str(), &row.header_service, &row.header_source],
            ["h1", "cws-agent", "security-agent"]
        );
        assert_eq!(row.header_tags["kube_service"], ["a", "b"]);
        assert_eq!(row.dns_names, r#"["example.com"]"#);
        assert_eq!(row.header_extra["unexpected"], r#"{"nested":true}"#);
        assert_eq!(row.header_extra["n"], "15");
        assert_eq!(row.header_extra.len(), 2);
    }

    #[test]
    fn ddtags_may_be_a_list_or_one_comma_separated_string() {
        for (ddtags, expected) in [
            ("\"env:prod,team:sec\"", "env=prod team=sec"),
            ("[\"a:b\", null, \"a:c\"]", "= a=b,c"),
            ("[\"a:b\", 7]", ""),
            ("\"\"", ""),
            ("null", ""),
            ("5", ""),
        ] {
            let header = value(&format!("{{\"ddtags\": {ddtags}}}"));
            let row = dump_row("default", now(), dump_id(), Some(&header), &[], None, &[]);
            let shown: Vec<String> = row.header_tags.iter().map(|(k, v)| format!("{k}={}", v.join(","))).collect();
            assert_eq!(shown.join(" "), expected, "{ddtags}");
        }
    }

    #[test]
    fn an_event_part_that_is_not_json_keeps_its_bytes_beside_a_good_dump() {
        let dump = SecDump {
            host: "h1".into(),
            metadata: Some(Metadata { agent_version: "7.58.2".into(), ..Default::default() }),
            ..Default::default()
        };
        let row = dump_row("default", now(), dump_id(), None, b"not a json object", Some(&dump), &[0x08, 0x01]);
        assert_eq!(row.header_raw, Blob(b"not a json object".to_vec()));
        assert_eq!(row.header_host, "");
        assert_eq!(row.dump_host, "h1");
        assert_eq!(row.agent_version, "7.58.2");
    }

    #[test]
    fn bytes_that_decode_to_an_empty_dump_are_not_a_dump() {
        assert!(decode_dump(&[]).is_none());
        // A valid message of some other type: field 1 only, no metadata, no tree.
        assert!(decode_dump(&[0x0a, 0x02, b'h', b'1']).is_none());
        assert!(decode_dump(b"not protobuf").is_none());
        assert!(
            decode_dump(&SecDump { metadata: Some(Metadata::default()), ..Default::default() }.encode_to_vec())
                .is_some()
        );
    }

    fn envelopes(body: &str) -> Vec<Value> {
        decode_envelopes(body.as_bytes()).unwrap()
    }

    #[test]
    fn an_envelope_whose_message_does_not_decode_still_gets_its_row() {
        let batch = envelopes(
            r#"[
            {"hostname":"h1","service":"cws-agent","ddsource":"security-agent","status":"info",
             "timestamp":1700000000000,"ddtags":["env:prod","team:sec"],
             "message":"{\"agent\":{\"rule_id\":\"r1\",\"policy_name\":\"default\"},\"evt\":{\"name\":\"exec\",\"category\":\"process\"}}"},
            {"hostname":"h2","service":"cws-agent","ddsource":"security-agent","status":"info","message":"not json"}
        ]"#,
        );
        let rows = event_rows("default", now(), "secruntime", &batch);
        assert_eq!(rows.len(), 2);
        let (ok, bad) = (&rows[0], &rows[1]);
        assert_eq!(ok.message_decoded, 1);
        assert_eq!(
            [ok.rule_id.as_str(), &ok.policy_name, &ok.evt_name, &ok.evt_category],
            ["r1", "default", "exec", "process"]
        );
        assert_eq!(ok.ddtags["team"], ["sec"]);
        assert_eq!(ok.timestamp, Some(at("2023-11-14T22:13:20Z")));
        assert_eq!((ok.seq_in_batch, bad.seq_in_batch), (0, 1));
        assert_eq!(
            [bad.hostname.as_str(), &bad.service, &bad.ddsource, &bad.status, &bad.track],
            ["h2", "cws-agent", "security-agent", "info", "secruntime"]
        );
        assert_eq!(bad.message_decoded, 0);
        assert_eq!(bad.message, "");
        assert_eq!(bad.message_raw, "\"not json\"");
        assert_eq!(bad.timestamp, None);
    }

    #[test]
    fn an_event_is_kept_as_json_with_sorted_keys_a_repeated_key_keeping_its_last_value() {
        let batch = envelopes(
            r#"[{"custom":{"a":[1,2]},"n":15,"hostname":5,
                 "message":"{\"title\":\"T <b>\",\"kind\":\"alert\",\"big\":12345678901234567890,\"z\":1,\"z\":2,\"f\":1.0}"}]"#,
        );
        let rows = event_rows("default", now(), "compliance", &batch);
        let row = &rows[0];
        assert_eq!(row.message, r#"{"big":12345678901234567890,"f":1.0,"kind":"alert","title":"T <b>","z":2}"#);
        assert_eq!((row.title.as_str(), row.event_kind.as_str()), ("T <b>", "alert"));
        // What is not a known key is kept as the JSON it arrived as.
        assert_eq!(row.extra["custom"], r#"{"a":[1,2]}"#);
        assert_eq!(row.extra["n"], "15");
        assert_eq!(row.extra.len(), 2);
        // A hostname that is not a string is no hostname.
        assert_eq!(row.hostname, "");
    }

    /// The inner event: an object, or a string holding one.
    #[test]
    fn a_message_is_decoded_when_it_is_an_object_or_holds_one() {
        for (message, decoded, rule_id) in [
            (r#"{"agent":{"rule_id":"r1"}}"#, 1, "r1"),
            (r#""{\"agent\":{\"rule_id\":\"r1\"}}""#, 1, "r1"),
            (r#"{"agent":{"rule_id":5}}"#, 1, ""),
            (r#"{"agent":"not an object"}"#, 1, ""),
            (r#""{}""#, 1, ""),
            (r#""null""#, 0, ""),
            (r#""[1,2]""#, 0, ""),
            (r#""{} trailing""#, 0, ""),
            (r#""""#, 0, ""),
            ("null", 0, ""),
            ("7", 0, ""),
        ] {
            let rows = event_rows("t", now(), "secinfo", &envelopes(&format!("[{{\"message\": {message}}}]")));
            let row = &rows[0];
            assert_eq!(row.message_decoded, decoded, "{message}");
            assert_eq!(row.rule_id, rule_id, "{message}");
            // A null message is the same as none.
            assert_eq!(row.message_raw, if message == "null" { "" } else { message }, "{message}");
            assert_eq!(decoded == 1, !row.message.is_empty(), "{message}");
        }
    }

    #[test]
    fn an_envelopes_timestamp_is_a_millisecond_epoch_or_rfc_3339_text() {
        for (sent, expected) in [
            ("1700000000123", "2023-11-14T22:13:20.123Z"),
            ("\"1700000000456\"", "2023-11-14T22:13:20.456Z"),
            ("\"2025-09-23T10:30:00.250+02:00\"", "2025-09-23T08:30:00.250Z"),
            ("\"2025-09-23T10:30:00.123456789Z\"", "2025-09-23T10:30:00.1234567Z"),
            // Zero and negative epochs are times here, unlike in the other intakes.
            ("0", "1970-01-01T00:00:00Z"),
            ("-1000", "1969-12-31T23:59:59Z"),
            ("1700000000.5", ""),
            ("\"soon\"", ""),
            ("\" 1700000000456\"", ""),
            ("null", ""),
            ("true", ""),
            // Past year 9999: kept at the limit.
            ("999999999999999999", "9999-12-31T23:59:59.999Z"),
        ] {
            let parsed = parse_timestamp(Some(&value(sent)));
            assert_eq!(parsed, (!expected.is_empty()).then(|| at(expected)), "{sent}");
        }
        assert_eq!(parse_timestamp(None), None);
    }

    #[test]
    fn null_entries_are_empty_envelopes_and_a_null_body_an_empty_batch() {
        let rows = event_rows("t", now(), "secruntime", &envelopes("[null, {}]"));
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].message_raw.as_str(), rows[1].message_raw.as_str()), ("", ""));
        assert!(rows[0].extra.is_empty());
        assert!(envelopes("null").is_empty());
        assert!(envelopes(" [] ").is_empty());
    }

    #[test]
    fn a_batch_that_is_not_a_list_of_envelopes_says_why() {
        assert_eq!(decode_envelopes(br#"{"hostname":"h1"}"#).unwrap_err(), "expected a list, got object");
        assert_eq!(decode_envelopes(b"\"text\"").unwrap_err(), "expected a list, got string");
        assert_eq!(decode_envelopes(br#"[{"hostname":"h1"}, 7]"#).unwrap_err(), "entry 1 is not a JSON object");
    }

    fn payload_of(entity: Entity) -> SbomPayload {
        SbomPayload { version: 1, host: "h1".into(), entities: vec![entity], ..Default::default() }
    }

    fn with_bom(bom: Bom) -> SbomPayload {
        payload_of(Entity { id: "e1".into(), sbom: Some(sbom_entity::Sbom::Cyclonedx(bom)), ..Default::default() })
    }

    fn component(bom_ref: &str, name: &str) -> Component {
        Component { bom_ref: Some(bom_ref.into()), name: name.into(), ..Default::default() }
    }

    /// An entity with two components, one nested under the other, and one vulnerability.
    #[test]
    fn an_sbom_is_unpacked_into_its_entity_components_and_vulnerabilities() {
        let nested = Component {
            version: "2.0".into(),
            r#type: Classification::Library as i32,
            ..component("pkg:c1-sub", "libbar")
        };
        let top = Component {
            version: "1.0".into(),
            r#type: Classification::Library as i32,
            licenses: vec![LicenseChoice {
                choice: Some(license_choice::Choice::License(License {
                    license: Some(license::License::Id("MIT".into())),
                    ..Default::default()
                })),
            }],
            hashes: vec![Hash { alg: HashAlg::Sha256 as i32, value: "abcd".into() }],
            components: vec![nested],
            ..component("pkg:c1", "libfoo")
        };
        let vulnerability = Vulnerability {
            bom_ref: Some("pkg:c1".into()),
            id: Some("CVE-2024-1234".into()),
            source: Some(Source { name: Some("NVD".into()), url: None }),
            cwes: vec![79],
            affects: vec![VulnerabilityAffects { r#ref: "pkg:c1".into(), ..Default::default() }],
            ..Default::default()
        };
        let bom = Bom {
            spec_version: "1.4".into(),
            components: vec![top],
            vulnerabilities: vec![vulnerability],
            ..Default::default()
        };
        let payload = payload_of(Entity {
            r#type: SbomSourceType::ContainerImageLayers as i32,
            id: "e1".into(),
            sbom: Some(sbom_entity::Sbom::Cyclonedx(bom)),
            ..Default::default()
        });
        let (entities, components, vulnerabilities) = sbom_rows("default", now(), &payload);

        let entity = &entities[0];
        assert_eq!((entity.component_count, entity.vulnerability_count), (2, 1));
        assert_eq!(entity.kind, "CONTAINER_IMAGE_LAYERS");
        assert_eq!(entity.status, "SUCCESS");
        assert_eq!(value(&entity.bom)["specVersion"], "1.4");
        assert_eq!((entity.bom_raw.as_str(), entity.error.as_str()), ("", ""));
        // Never set on the payload: absent, not "".
        assert_eq!((entity.source.as_deref(), entity.dd_env.as_deref()), (None, None));
        assert_eq!((entity.generated_at, entity.generation_duration_ms), (None, None));

        assert_eq!(components.len(), 2);
        let (parent, child) = (&components[0], &components[1]);
        assert_eq!(
            [parent.bom_ref.as_str(), &parent.parent_bom_ref, &child.bom_ref, &child.parent_bom_ref],
            ["pkg:c1", "", "pkg:c1-sub", "pkg:c1"]
        );
        assert_eq!((parent.depth, child.depth), (0, 1));
        assert_eq!(parent.kind, "CLASSIFICATION_LIBRARY");
        assert_eq!(parent.scope, "SCOPE_UNSPECIFIED");
        assert_eq!(parent.licenses, ["MIT"]);
        assert_eq!(parent.hashes["HASH_ALG_SHA_256"], "abcd");
        assert_eq!(
            [&parent.purl, &parent.cpe, &parent.group, &parent.publisher, &parent.author, &parent.description],
            [&None, &None, &None, &None, &None, &None]
        );
        assert_eq!((parent.external_references.as_str(), parent.evidence.as_str()), ("", ""));
        // The id minted for the entity is what its rows join on.
        assert_eq!((parent.entity_id, child.entity_id), (entity.entity_id, entity.entity_id));

        let found = &vulnerabilities[0];
        assert_eq!(found.id, "CVE-2024-1234");
        assert_eq!((found.source_name.as_deref(), found.source_url.as_deref()), (Some("NVD"), None));
        assert_eq!(found.cwes, [79]);
        assert_eq!(found.affects_refs, ["pkg:c1"]);
        assert_eq!(found.affects, r#"[{"ref":"pkg:c1"}]"#);
        assert_eq!(found.entity_id, entity.entity_id);
        // No analysis at all: empty, not the zero value's name.
        assert_eq!((found.analysis_state.as_str(), found.analysis_justification.as_str()), ("", ""));
    }

    #[test]
    fn an_entity_that_failed_carries_its_error_and_no_bom() {
        let entity = Entity {
            id: "e1".into(),
            r#type: 42,
            status: SbomStatus::Failed as i32,
            sbom: Some(sbom_entity::Sbom::Error("generation timed out".into())),
            ..Default::default()
        };
        let (entities, components, vulnerabilities) = sbom_rows("default", now(), &payload_of(entity));
        let row = &entities[0];
        assert_eq!(row.error, "generation timed out");
        assert_eq!(row.bom, "");
        assert_eq!(row.status, "FAILED");
        // A source type this build does not know keeps its number.
        assert_eq!(row.kind, "42");
        assert!(components.is_empty() && vulnerabilities.is_empty());
    }

    /// Scanners repeat a property name (trivy's PkgID, for one): every value must survive, on a
    /// component and on a vulnerability.
    #[test]
    fn properties_that_repeat_a_name_keep_every_value() {
        let property = |name: &str, value: Option<&str>| Property { name: name.into(), value: value.map(String::from) };
        let comp = Component {
            properties: vec![
                property("trivy:PkgID", Some("libfoo@1.0")),
                property("other", Some("x")),
                property("trivy:PkgID", Some("libfoo@1.0-alt")),
                property("no value", None),
            ],
            ..component("pkg:c1", "libfoo")
        };
        let vulnerability = Vulnerability {
            id: Some("CVE-2024-1234".into()),
            properties: vec![property("trivy:SrcName", Some("libfoo")), property("trivy:SrcName", Some("libfoo-src"))],
            ..Default::default()
        };
        let bom = Bom { components: vec![comp], vulnerabilities: vec![vulnerability], ..Default::default() };
        let (_, components, vulnerabilities) = sbom_rows("default", now(), &with_bom(bom));
        assert_eq!(components[0].properties["trivy:PkgID"], ["libfoo@1.0", "libfoo@1.0-alt"]);
        assert_eq!(components[0].properties["no value"], [""]);
        assert_eq!(vulnerabilities[0].properties["trivy:SrcName"], ["libfoo", "libfoo-src"]);
    }

    #[test]
    fn what_the_payload_and_the_entity_say_about_themselves_reaches_the_row() {
        let entity = Entity {
            id: "img@sha256:1".into(),
            in_use: true,
            heartbeat: true,
            hash: "h".into(),
            kernel_version: "6.1".into(),
            cpu_architecture: "arm64".into(),
            generated_at: Some(pbjson_types::Timestamp { seconds: 1714979289, nanos: 123456789 }),
            generation_duration: Some(pbjson_types::Duration { seconds: 2, nanos: 345678901 }),
            repo_tags: vec!["nginx:1".into(), "nginx:latest".into()],
            repo_digests: vec!["nginx@sha256:1".into()],
            dd_tags: vec!["team:a".into(), "team:b".into()],
            ..Default::default()
        };
        let mut payload = payload_of(entity);
        payload.source = Some("agent".into());
        payload.dd_env = Some(String::new());
        let (entities, _, _) = sbom_rows("default", now(), &payload);
        let row = &entities[0];
        assert_eq!(row.source.as_deref(), Some("agent"));
        // Set to the empty string is not the same as never set.
        assert_eq!(row.dd_env.as_deref(), Some(""));
        // To 100 ns, as protobuf's Timestamp reads into the F# server's DateTime.
        assert_eq!(row.generated_at, Some(at("2024-05-06T07:08:09.1234567Z")));
        assert_eq!(row.generation_duration_ms, Some(2345));
        assert_eq!(row.repo_tags, ["nginx:1", "nginx:latest"]);
        assert_eq!(row.repo_digests, ["nginx@sha256:1"]);
        assert_eq!(row.dd_tags["team"], ["a", "b"]);
        assert_eq!((row.in_use, row.heartbeat), (1, 1));
        assert_eq!(
            [row.hash.as_str(), &row.kernel_version, &row.cpu_architecture, &row.host],
            ["h", "6.1", "arm64", "h1"]
        );
        assert_eq!(row.payload_version, 1);
        // Neither arm of the oneof.
        assert_eq!((row.error.as_str(), row.bom.as_str()), ("", ""));
    }

    /// Whole milliseconds; a duration past 64 bits of nanoseconds stays at that limit.
    #[test]
    fn a_generation_duration_is_whole_milliseconds_saturating() {
        for (seconds, nanos, expected) in [
            (2, 345678901, 2345),
            (-1, 999999, -999),
            (0, 999999, 0),
            (i64::MAX, 5, 9223372036854),
            (i64::MIN, 0, -9223372036854),
        ] {
            assert_eq!(duration_millis(&pbjson_types::Duration { seconds, nanos }), expected, "{seconds} {nanos}");
        }
    }

    #[test]
    fn licenses_are_named_by_id_then_name_then_expression() {
        let license = |found: Option<license::License>, url: Option<&str>| LicenseChoice {
            choice: Some(license_choice::Choice::License(License {
                license: found,
                url: url.map(String::from),
                ..Default::default()
            })),
        };
        let expression = |text: &str| LicenseChoice { choice: Some(license_choice::Choice::Expression(text.into())) };
        let comp = Component {
            name: "libbar".into(),
            licenses: vec![
                license(Some(license::License::Id("MIT".into())), None),
                license(Some(license::License::Name("Custom".into())), None),
                expression("MIT OR Apache-2.0"),
                // A license with neither id nor name still takes its place.
                license(None, Some("http://x")),
                expression(""),
                LicenseChoice { choice: None },
            ],
            ..Default::default()
        };
        let (_, components, _) = sbom_rows("t", now(), &with_bom(Bom { components: vec![comp], ..Default::default() }));
        assert_eq!(components[0].licenses, ["MIT", "Custom", "MIT OR Apache-2.0", ""]);
    }

    #[test]
    fn optional_strings_that_were_set_to_nothing_are_empty_not_absent() {
        let comp = Component {
            name: "libbar".into(),
            purl: Some(String::new()),
            description: Some("a <b> & \"c\"".into()),
            scope: Some(Scope::Optional as i32),
            hashes: vec![
                Hash { alg: HashAlg::Sha256 as i32, value: "first".into() },
                Hash { alg: HashAlg::Sha256 as i32, value: "last wins".into() },
                Hash { alg: 77, value: "unknown".into() },
            ],
            external_references: vec![
                ExternalReference {
                    r#type: ExternalReferenceType::Vcs as i32,
                    url: "https://x/<y>".into(),
                    ..Default::default()
                },
                ExternalReference { url: "u2".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let (_, components, _) = sbom_rows("t", now(), &with_bom(Bom { components: vec![comp], ..Default::default() }));
        let row = &components[0];
        assert_eq!(row.purl.as_deref(), Some(""));
        assert_eq!(row.description.as_deref(), Some("a <b> & \"c\""));
        assert_eq!(row.cpe, None);
        assert_eq!(row.scope, "SCOPE_OPTIONAL");
        assert_eq!(row.bom_ref, "");
        assert_eq!(row.hashes["HASH_ALG_SHA_256"], "last wins");
        assert_eq!(row.hashes["77"], "unknown");
        assert_eq!(
            row.external_references,
            r#"[{"type":"EXTERNAL_REFERENCE_TYPE_VCS","url":"https://x/<y>"},{"url":"u2"}]"#
        );
    }

    #[test]
    fn a_vulnerabilitys_analysis_dates_and_ratings_reach_its_row() {
        let analysis = VulnerabilityAnalysis {
            state: Some(ImpactAnalysisState::NotAffected as i32),
            detail: Some("not reachable".into()),
            response: vec![VulnerabilityResponse::Update as i32, 42],
            ..Default::default()
        };
        let vulnerability = Vulnerability {
            id: Some("CVE-1".into()),
            detail: Some(String::new()),
            analysis: Some(analysis),
            created: Some(pbjson_types::Timestamp { seconds: 1700000000, nanos: 0 }),
            updated: Some(pbjson_types::Timestamp { seconds: -1, nanos: 500 }),
            source: Some(Source { name: None, url: Some(String::new()) }),
            ratings: vec![
                VulnerabilityRating { score: Some(7.5), severity: Some(Severity::High as i32), ..Default::default() },
                VulnerabilityRating { score: Some(9.0), ..Default::default() },
            ],
            ..Default::default()
        };
        let bare = Vulnerability { analysis: Some(VulnerabilityAnalysis::default()), ..Default::default() };
        let bom = Bom { vulnerabilities: vec![vulnerability, bare], ..Default::default() };
        let (_, _, rows) = sbom_rows("t", now(), &with_bom(bom));

        assert_eq!(rows.len(), 2);
        let (full, empty) = (&rows[0], &rows[1]);
        assert_eq!(full.analysis_state, "IMPACT_ANALYSIS_STATE_NOT_AFFECTED");
        assert_eq!(full.analysis_justification, "IMPACT_ANALYSIS_JUSTIFICATION_NULL");
        assert_eq!(full.analysis_response, ["VULNERABILITY_RESPONSE_UPDATE", "42"]);
        assert_eq!(full.analysis_detail, "not reachable");
        assert_eq!(full.created, Some(at("2023-11-14T22:13:20Z")));
        assert_eq!(full.updated, Some(at("1969-12-31T23:59:59.0000005Z")));
        assert_eq!(full.published, None);
        assert_eq!((full.detail.as_deref(), full.description.as_deref()), (Some(""), None));
        assert_eq!((full.source_name.as_deref(), full.source_url.as_deref()), (None, Some("")));
        // serde writes the float 9 as 9.0; the F# server wrote 9. The same JSON number.
        assert_eq!(full.ratings, r#"[{"score":7.5,"severity":"SEVERITY_HIGH"},{"score":9.0}]"#);

        // An analysis that says nothing still names its zero values.
        assert_eq!(empty.analysis_state, "IMPACT_ANALYSIS_STATE_NULL");
        assert_eq!(empty.id, "");
        assert!(empty.analysis_response.is_empty());
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    /// Bodies (hex) and the layout the Go intake noted for them: these were produced by running its
    /// wire walk on the same bytes.
    #[test]
    fn the_wire_layout_lists_top_level_fields_in_order_runs_folded() {
        for (body, expected) in [
            ("", ""),
            ("0801", "1:varint"),
            ("08011002", "1:varint 2:varint"),
            ("0801080208031205616263646512001a00", "1:varintx3 2:bytesx2 3:bytes"),
            ("0d01020304", "1:fixed32"),
            ("090102030405060708", "1:fixed64"),
            ("0d010203", "malformed"),
            ("09010203", "malformed"),
            ("1203616263", "2:bytes"),
            ("12056162", "malformed"),
            ("0b0801100c0c", "1:type3"),
            ("0b08010c0b08020c", "1:type3x2"),
            ("0b0801", "malformed"),
            ("0b08011c", "malformed"),
            ("0b13081c140c", "1:type3"),
            ("0c", "malformed"),
            ("0e00", "malformed"),
            ("0f00", "malformed"),
            ("00", "malformed"),
            ("0001", "malformed"),
            ("08", "malformed"),
            ("0880", "malformed"),
            ("08ffffffffffffffffff01", "1:varint"),
            // Ten bytes whose last holds more than the one bit that fits: prost refuses it, where
            // the F# server's protobuf library dropped the extra bits and read on.
            ("08ffffffffffffffffff02", "malformed"),
            ("08ffffffffffffffffffff01", "malformed"),
            ("f8ffffff0701", "268435455:varint"),
            ("f8ffffff0f01", "536870911:varint"),
            ("8000", "malformed"),
            ("8080808080808080800100", "malformed"),
            ("3a0208013a0208024a01614a0162", "7:bytesx2 9:bytesx2"),
            ("1a0208011a020802220022000801", "3:bytesx2 4:bytesx2 1:varint"),
        ] {
            assert_eq!(wire_layout(&hex(body)).as_deref().unwrap_or("malformed"), expected, "{body}");
        }
    }

    /// The protobuf library stops following nested groups at its recursion limit, so hostile
    /// nesting is malformed, not a stack overflow.
    #[test]
    fn nested_groups_are_followed_as_deep_as_the_protobuf_library_follows_them() {
        for (depth, expected) in [(50, "1:type3"), (10002, "malformed"), (200000, "malformed")] {
            let body: Vec<u8> = [vec![0x0b; depth], vec![0x0c; depth]].concat();
            assert_eq!(wire_layout(&body).as_deref().unwrap_or("malformed"), expected, "{depth}");
        }
    }

    /// What the agent's data security check builds for one Postgres table
    /// (pkg/collector/sharedlibrary/rustchecks/checks/datasecurity, result.rs).
    pub fn agent_scan() -> SdsResultPayload {
        let column = |name: &str, data_type: &str| postgres_table::ScannedColumn {
            name: name.into(),
            data_type: data_type.into(),
        };
        let table = PostgresTable {
            database_cluster_name: "cluster".into(),
            database_instance_name: "inst".into(),
            database_host_name: "pg1".into(),
            database_name: "shop".into(),
            schema_name: "public".into(),
            table_name: "users".into(),
            scanned_row_count: 2,
            scanned_columns: vec![column("email", "text"), column("name", "varchar")],
            ..Default::default()
        };
        let result = ScanResult {
            location: Some(Location { scan_location: Some(ScanLocation::PostgresTable(table)), ..Default::default() }),
            scan_metadata: Some(ScanMetadata {
                scan_task_metadata: Some(scan_metadata::ScanTaskMetadata {
                    task_id: "task-1".into(),
                    sub_task_id: "sub-1".into(),
                    status: Status::Success as i32,
                    ..Default::default()
                }),
            }),
            table_matches: vec![TableMatch {
                rule_id: "email".into(),
                column_name: "email".into(),
                count_matched_rows: 2,
                count_matches: 3,
                ..Default::default()
            }],
            ..Default::default()
        };
        SdsResultPayload {
            timestamp: 1_790_877_420_109,
            resource: Some(Resource { r#type: "postgres_table".into(), name: "cluster.shop.public.users".into() }),
            scanning_source: Some(ScanningSource {
                source: Some(scanning_source::Source::Agent(scanning_source::Agent::default())),
            }),
            rule_ids: vec!["email".into()],
            scan_results: vec![result],
            ..Default::default()
        }
    }

    #[test]
    fn a_scan_of_a_postgres_table_is_one_scan_one_result_and_one_row_per_rule_that_matched() {
        let SdsRows { scan, results, matches } = sds_rows("t", now(), Uuid::new_v4(), &agent_scan());
        assert_eq!(
            [scan.resource_type.as_str(), &scan.resource_name, &scan.scanning_source],
            ["postgres_table", "cluster.shop.public.users", "agent"]
        );
        assert_eq!(scan.timestamp, Some(at("2026-10-01T17:57:00.109Z")));
        assert_eq!(scan.rule_ids, ["email"]);
        // No statistics were sent: unknown, not zero.
        assert_eq!(scan.files_scanned, None);
        assert_eq!(scan.result_count, 1);

        let result = &results[0];
        assert_eq!(result.scan_id, scan.scan_id);
        assert_eq!(
            [
                result.location_kind.as_str(),
                &result.database_name,
                &result.schema_name,
                &result.table_name,
                &result.path
            ],
            ["postgres_table", "shop", "public", "users", ""]
        );
        assert_eq!(result.scanned_row_count, Some(2));
        assert_eq!(result.scanned_column_names, ["email", "name"]);
        assert_eq!(result.scanned_column_types, ["text", "varchar"]);
        assert_eq!(
            [result.task_id.as_str(), &result.sub_task_id, &result.task_status, &result.failure_reason],
            ["task-1", "sub-1", "SUCCESS", ""]
        );
        assert_eq!(result.task_started_at, None);
        assert_eq!(value(&result.location)["postgresTable"]["databaseHostName"], "pg1");
        assert_eq!((result.match_count, result.table_match_count), (0, 1));

        let found = &matches[0];
        assert_eq!(
            (
                found.kind.as_str(),
                found.rule_id.as_str(),
                found.column_name.as_str(),
                found.count_matched_rows,
                found.count_matches
            ),
            ("table_match", "email", "email", 2, 3)
        );
        assert_eq!((found.scan_id, found.result_index, found.table_name.as_str()), (scan.scan_id, 0, "users"));
    }

    #[test]
    fn a_match_in_a_file_keeps_where_it_was_found_and_a_clean_location_still_gets_its_result_row() {
        let hit = ScanMatch {
            rule_id: "aws-key".into(),
            start_index: 4,
            end_index: 24,
            sample: "AKIA…".into(),
            line: Some(7),
            ..Default::default()
        };
        let dirty = ScanResult {
            location: Some(Location {
                scan_location: Some(ScanLocation::S3File(S3File { path: "s3://b/a.txt".into(), ..Default::default() })),
                ..Default::default()
            }),
            matches: vec![hit],
            ..Default::default()
        };
        let clean = ScanResult {
            location: Some(Location {
                scan_location_type: Some(ScanLocationType::Path("/old/style".into())),
                ..Default::default()
            }),
            ..Default::default()
        };
        let payload = SdsResultPayload {
            resource: Some(Resource { r#type: "s3_bucket".into(), name: "b".into() }),
            scan_stats: Some(ScanStats { files_scanned: 2, ..Default::default() }),
            scanning_source: Some(ScanningSource {
                source: Some(scanning_source::Source::Agentless(scanning_source::Agentless {
                    version: "1.2".into(),
                    region: "eu".into(),
                })),
            }),
            scan_results: vec![dirty, clean],
            rules: [(
                "aws-key".to_string(),
                RuleInfo {
                    id: "aws-key".into(),
                    name: "AWS key".into(),
                    priority: "high".into(),
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        };
        let SdsRows { scan, results, matches } = sds_rows("t", DateTime::UNIX_EPOCH, Uuid::nil(), &payload);
        assert_eq!(scan.timestamp, None);
        assert_eq!(
            [scan.scanning_source.as_str(), &scan.source_version, &scan.source_region],
            ["agentless", "1.2", "eu"]
        );
        assert_eq!((scan.files_scanned, scan.total_files_found), (Some(2), Some(0)));
        assert!(scan.rules.starts_with("{\"aws-key\":{"));

        let shown: Vec<(&str, &str, u32)> =
            results.iter().map(|r| (r.location_kind.as_str(), r.path.as_str(), r.match_count)).collect();
        assert_eq!(shown, [("s3_file", "s3://b/a.txt", 1), ("path", "/old/style", 0)]);

        let found = &matches[0];
        assert_eq!(matches.len(), 1);
        assert_eq!(
            (found.kind.as_str(), found.rule_id.as_str(), found.sample.as_str(), found.start_index, found.end_index),
            ("match", "aws-key", "AKIA…", 4, 24)
        );
        assert_eq!((found.line, found.row, found.result_index), (Some(7), None, 0));
    }
}
