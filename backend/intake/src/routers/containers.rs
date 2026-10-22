//! contlcycle-intake.<site>, contimage-intake.<site> — containers.
//!
//!   agent config: none. Event platform pipelines have no dd_url, only
//!   container_lifecycle.enabled / container_image.enabled, and their additional_endpoints ADD a
//!   recipient rather than replace one. DD_SITE is the only way to point them here.
//!
//! Both carry protobuf. Lifecycle events land in container_events, the image inventory in
//! container_images. A body that does not decode, or decodes to nothing, goes to raw_payloads.

use chrono::{DateTime, Utc};
use ninjapaws_proto::datadog::contimage::{ContainerImage, ContainerImagePayload};
use ninjapaws_proto::datadog::contlcycle::{
    ContainerKind, ContainerStateKind, ContainerStateTransition, ContainerStateValue, Event, EventsPayload,
    MissedIntermediate, ObjectKind, PodStateTransition, PodStatusField, PodStatusValue, Precision, container_event,
    event, pod_status_value,
};
use ninjapaws_storage::rows::Tags;
use ninjapaws_storage::rows::containers::{ContainerEvent as ContainerEventRow, ContainerImage as ContainerImageRow};
use ninjapaws_storage::rows::metrics::MetricPoint;
use prost::Message;
use tako::types::Response;

use crate::server::{Body, Ctx, accepted};
use crate::wire::{enum_name, tags, text, time};

// ---- lifecycle events ----

/// A container state as one label: the kind, then reason, exit code and signal when the value
/// carries them.
fn container_state_label(state: Option<&ContainerStateValue>) -> String {
    let Some(state) = state else {
        return "-".into();
    };
    let reason = state.reason.as_ref().map(|r| format!("/{r}")).unwrap_or_default();
    let exit_code = state.exit_code.map(|c| format!(" exit={c}")).unwrap_or_default();
    let signal = state.signal.map(|s| format!(" signal={s}")).unwrap_or_default();
    format!("{}{reason}{exit_code}{signal}", enum_name(state.kind, ContainerStateKind::as_str_name))
}

/// A pod status as one label: a phase, or a condition as type=status/reason.
fn pod_status_label(status: Option<&PodStatusValue>) -> String {
    match status.and_then(|s| s.value.as_ref()) {
        Some(pod_status_value::Value::Condition(condition)) => {
            let reason = condition.reason.as_ref().map(|r| format!("/{r}")).unwrap_or_default();
            format!("{}={}{reason}", condition.r#type, condition.status)
        }
        Some(pod_status_value::Value::Phase(phase)) if !phase.is_empty() => phase.clone(),
        _ => "-".into(),
    }
}

/// A pod status spread over its columns. A status is a phase OR a condition; variant says which,
/// so a reader knows which columns mean something.
#[derive(Default)]
struct PodStatusColumns {
    variant: String,
    phase: Option<String>,
    condition_type: Option<String>,
    condition_status: Option<String>,
    condition_reason: Option<String>,
    condition_message: Option<String>,
}

fn pod_status_columns(status: Option<&PodStatusValue>) -> PodStatusColumns {
    match status.and_then(|s| s.value.as_ref()) {
        Some(pod_status_value::Value::Condition(condition)) => PodStatusColumns {
            variant: "condition".into(),
            condition_type: Some(condition.r#type.clone()),
            condition_status: Some(condition.status.clone()),
            condition_reason: condition.reason.clone(),
            condition_message: condition.message.clone(),
            ..Default::default()
        },
        Some(pod_status_value::Value::Phase(phase)) => {
            PodStatusColumns { variant: "phase".into(), phase: Some(phase.clone()), ..Default::default() }
        }
        None => PodStatusColumns::default(),
    }
}

fn with_container_transition(transition: Option<&ContainerStateTransition>, row: &mut ContainerEventRow) {
    let Some(transition) = transition else { return };
    let was = transition.last_observed_state.as_ref();
    let became = transition.new_state.as_ref();
    let kind = |state: Option<&ContainerStateValue>| {
        state.map(|s| enum_name(s.kind, ContainerStateKind::as_str_name)).unwrap_or_default()
    };

    row.old_state = container_state_label(was);
    row.new_state = container_state_label(became);
    row.transition_at = time::optional_seconds(transition.transition_timestamp);
    row.container_kind = enum_name(transition.container_kind, ContainerKind::as_str_name);
    row.precision = enum_name(transition.precision, Precision::as_str_name);
    row.missed_intermediate = enum_name(transition.missed_intermediate, MissedIntermediate::as_str_name);
    row.old_state_kind = kind(was);
    row.old_reason = was.and_then(|s| s.reason.clone());
    row.old_exit_code = was.and_then(|s| s.exit_code);
    row.old_signal = was.and_then(|s| s.signal);
    row.new_state_kind = kind(became);
    row.new_reason = became.and_then(|s| s.reason.clone());
    row.new_exit_code = became.and_then(|s| s.exit_code);
    row.new_signal = became.and_then(|s| s.signal);
}

fn with_pod_transition(transition: Option<&PodStateTransition>, row: &mut ContainerEventRow) {
    let Some(transition) = transition else { return };
    let was = pod_status_columns(transition.last_observed_state.as_ref());
    let became = pod_status_columns(transition.new_state.as_ref());

    row.old_state = pod_status_label(transition.last_observed_state.as_ref());
    row.new_state = pod_status_label(transition.new_state.as_ref());
    row.transition_at = time::optional_seconds(transition.transition_timestamp);
    row.pod_status_field = enum_name(transition.field, PodStatusField::as_str_name);
    row.precision = enum_name(transition.precision, Precision::as_str_name);
    row.missed_intermediate = enum_name(transition.missed_intermediate, MissedIntermediate::as_str_name);
    row.old_state_variant = was.variant;
    row.old_phase = was.phase;
    row.old_condition_type = was.condition_type;
    row.old_condition_status = was.condition_status;
    row.old_condition_reason = was.condition_reason;
    row.old_condition_message = was.condition_message;
    row.new_state_variant = became.variant;
    row.new_phase = became.phase;
    row.new_condition_type = became.condition_type;
    row.new_condition_status = became.condition_status;
    row.new_condition_reason = became.condition_reason;
    row.new_condition_message = became.condition_message;
}

/// One row per event. The envelope (host, cluster, object kind) repeats on every row; each kind
/// of event fills only its own identity column.
///
/// `now` is the row's timestamp: the payload does not say when it was sent, only when things were
/// created, exited or changed, and those have columns of their own.
pub fn event_rows(payload: &EventsPayload, tenant: &str, now: DateTime<Utc>) -> Vec<ContainerEventRow> {
    let envelope = ContainerEventRow {
        tenant_id: tenant.into(),
        timestamp: now,
        host: payload.host.clone(),
        cluster_id: payload.cluster_id.clone(),
        object_kind: enum_name(payload.object_kind, ObjectKind::as_str_name),
        payload_version: payload.version.clone(),
        event_variant: "unknown".into(),
        ..Default::default()
    };

    let to_row = |event: &Event| {
        let mut row = ContainerEventRow {
            event_type: enum_name(event.event_type, event::EventType::as_str_name),
            ..envelope.clone()
        };
        match &event.typed_event {
            Some(event::TypedEvent::Container(container)) => {
                let owner = container.owner.as_ref();
                row.event_variant = "container".into();
                row.container_id = container.container_id.clone();
                row.container_name = container.container_name.clone().unwrap_or_default();
                row.container_name_present = text::flag(container.container_name.is_some());
                row.source = container.source.clone();
                // Presence, not value: no exit code is NULL, distinct from an honest exit 0. OOM
                // kill is 137.
                row.exit_code = container.optional_exit_code.map(|container_event::OptionalExitCode::ExitCode(c)| c);
                row.created_at = container.optional_creation_timestamp.and_then(
                    |container_event::OptionalCreationTimestamp::CreationTimestamp(t)| time::optional_seconds(t),
                );
                row.exited_at = container
                    .optional_exit_timestamp
                    .and_then(|container_event::OptionalExitTimestamp::ExitTimestamp(t)| time::optional_seconds(t));
                row.owner_type = owner.map(|o| enum_name(o.owner_type, ObjectKind::as_str_name)).unwrap_or_default();
                row.owner_uid = owner.map(|o| o.owner_uid.clone()).unwrap_or_default();
                with_container_transition(container.transition.as_ref(), &mut row);
            }
            Some(event::TypedEvent::Pod(pod)) => {
                row.event_variant = "pod".into();
                row.pod_uid = pod.pod_uid.clone();
                row.source = pod.source.clone();
                row.created_at = pod.creation_timestamp.and_then(time::optional_seconds);
                row.exited_at = pod.exit_timestamp.and_then(time::optional_seconds);
                with_pod_transition(pod.transition.as_ref(), &mut row);
            }
            Some(event::TypedEvent::Task(task)) => {
                row.event_variant = "task".into();
                row.task_arn = task.task_arn.clone();
                row.source = task.source.clone();
                row.exited_at = task.exit_timestamp.and_then(time::optional_seconds);
            }
            // No typed detail, or a kind newer than this schema. A Delete with no detail is still
            // a Delete, so the row stands on the envelope alone.
            None => {}
        }
        row
    };

    payload.events.iter().map(to_row).collect()
}

// ---- image inventory ----

/// A protobuf Timestamp for a nullable column. Not after 1970 is None, like an absent one: a
/// missing build date must not become a real one.
fn timestamp(value: Option<&pbjson_types::Timestamp>) -> Option<DateTime<Utc>> {
    value.filter(|t| t.seconds > 0).and_then(|t| time::of_timestamp(Some(t)))
}

/// The rows of one inventory, and how many images were skipped for having neither a digest nor
/// an id.
///
/// The table replaces on the image key. A missing digest is normal (built locally, loaded from a
/// tarball, pulled moments ago), so the image id stands in. An image with neither is skipped:
/// keyed on "" they would all collapse into one row, which loses more.
pub fn image_rows(
    payload: &ContainerImagePayload,
    tenant: &str,
    now: DateTime<Utc>,
) -> (Vec<ContainerImageRow>, usize) {
    let to_row = |image: &ContainerImage| {
        let (key, source) = if !image.digest.is_empty() { (&image.digest, "digest") } else { (&image.id, "image_id") };
        if key.is_empty() {
            return None;
        }
        let layers = &image.layers;
        let histories: Vec<_> = layers.iter().map(|layer| layer.history.as_ref()).collect();
        let history_text = |read: fn(&_) -> &String| -> Vec<String> {
            histories.iter().map(|h| h.map(|h| read(h).clone()).unwrap_or_default()).collect()
        };

        // Both size columns are unsigned and the wire is signed. A negative size cast blindly
        // becomes ~18 exabytes and poisons every SUM over the table, so it is clamped and the
        // clamp is recorded. For layers the flag rises on ANY negative one: inside a
        // still-positive sum it is just as wrong and far quieter.
        let layer_bytes: i64 = layers.iter().map(|layer| layer.size).sum();
        let os = image.os.as_ref();

        Some(ContainerImageRow {
            tenant_id: tenant.into(),
            collected_at: now,
            host: payload.host.clone(),
            image_key: key.clone(),
            identity_source: source.into(),
            image_id: image.id.clone(),
            digest: image.digest.clone(),
            name: image.name.clone(),
            short_name: image.short_name.clone(),
            registry: image.registry.clone(),
            repo_tags: image.repo_tags.clone(),
            repo_digests: image.repo_digests.clone(),
            size_bytes: image.size.max(0) as u64,
            os_name: os.map(|o| o.name.clone()).unwrap_or_default(),
            os_version: os.map(|o| o.version.clone()).unwrap_or_default(),
            architecture: os.map(|o| o.architecture.clone()).unwrap_or_default(),
            layer_count: layers.len() as u32,
            layer_bytes: layer_bytes.max(0) as u64,
            built_at: timestamp(image.built_at.as_ref()),
            published_at: timestamp(image.published_at.as_ref()),
            dd_tags: tags::to_multi_map(&image.dd_tags),
            payload_version: payload.version.clone(),
            source: payload.source.clone(),
            layer_media_types: layers.iter().map(|layer| layer.media_type.clone()).collect(),
            layer_digests: layers.iter().map(|layer| layer.digest.clone()).collect(),
            layer_sizes: layers.iter().map(|layer| layer.size).collect(),
            layer_urls: layers.iter().map(|layer| layer.urls.clone()).collect(),
            layer_history_created: histories.iter().map(|h| h.and_then(|h| timestamp(h.created.as_ref()))).collect(),
            layer_history_created_by: history_text(|h| &h.created_by),
            layer_history_author: history_text(|h| &h.author),
            layer_history_comment: history_text(|h| &h.comment),
            layer_history_empty_layer: histories.iter().map(|h| text::flag(h.is_some_and(|h| h.empty_layer))).collect(),
            size_negative: text::flag(image.size < 0),
            layer_size_negative: text::flag(layers.iter().any(|layer| layer.size < 0)),
        })
    };

    let rows: Vec<ContainerImageRow> = payload.images.iter().filter_map(to_row).collect();
    let skipped = payload.images.len() - rows.len();
    (rows, skipped)
}

// ---- handlers ----

// Protobuf carries no type marker and skips fields it does not know, so a payload meant for
// another endpoint decodes "successfully" into nothing. An empty decode is the one hint that the
// wrong message arrived; both handlers keep such a body raw.

/// Container, pod and task start/stop/OOM events. One payload covers one kind of object, named by
/// object_kind.
pub async fn handle_lifecycle(ctx: Ctx, body: Body) -> Response {
    match EventsPayload::decode(body.as_ref()) {
        Err(e) => ctx.raw("contlcycle", "decode_error", &e.to_string(), &body),
        Ok(payload) if payload.events.is_empty() => {
            ctx.raw("contlcycle", "unexpected_shape", "EventsPayload decoded with no events", &body)
        }
        Ok(payload) => ctx.write(event_rows(&payload, ctx.tenant(), Utc::now())),
    }
    accepted()
}

/// The container image inventory.
pub async fn handle_images(ctx: Ctx, body: Body) -> Response {
    match ContainerImagePayload::decode(body.as_ref()) {
        Err(e) => ctx.raw("contimage", "decode_error", &e.to_string(), &body),
        Ok(payload) if payload.images.is_empty() => {
            ctx.raw("contimage", "unexpected_shape", "ContainerImagePayload decoded with no images", &body)
        }
        Ok(payload) => {
            let (rows, skipped) = image_rows(&payload, ctx.tenant(), Utc::now());
            if skipped > 0 {
                tracing::warn!(
                    "[contimage] {skipped} of {} images skipped: neither digest nor image id, so nothing to key on",
                    payload.images.len()
                );
                // The metric is what makes an incomplete inventory noticeable without going
                // looking for a log line.
                let reason = Tags::from([("reason".to_string(), vec!["no_identity".to_string()])]);
                ctx.write(vec![MetricPoint {
                    host: payload.host.clone(),
                    metric_type: "COUNT".into(),
                    source_type: "ninjacat".into(),
                    tags: reason,
                    ..MetricPoint::new(ctx.tenant(), Utc::now(), "ninjacat.intake.images.skipped", skipped as f64)
                }]);
            }
            ctx.write(rows);
        }
    }
    accepted()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use ninjapaws_proto::datadog::contimage::container_image::{
        ContainerImageLayer, OperatingSystem, container_image_layer,
    };
    use ninjapaws_proto::datadog::contlcycle::{
        ConditionValue, ContainerEvent, PodEvent, TaskEvent, container_event::Owner, event::EventType,
    };
    use pbjson_types::Timestamp;

    use super::*;

    /// A fixed conversion time, so a test can tell the sender's value from the receive time: the
    /// two things an absent timestamp must never become.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 21, 12, 0, 0).unwrap()
    }

    fn at(seconds: i64) -> Option<DateTime<Utc>> {
        Some(Utc.timestamp_opt(seconds, 0).unwrap())
    }

    fn lifecycle(events: Vec<Event>) -> EventsPayload {
        EventsPayload { host: "node1".into(), events, ..Default::default() }
    }

    fn typed(event_type: EventType, typed: event::TypedEvent) -> Event {
        Event { event_type: event_type as i32, typed_event: Some(typed) }
    }

    fn container_row(container: ContainerEvent) -> ContainerEventRow {
        let rows = event_rows(
            &lifecycle(vec![typed(EventType::Delete, event::TypedEvent::Container(container))]),
            "t1",
            now(),
        );
        assert_eq!(rows.len(), 1);
        rows.into_iter().next().unwrap()
    }

    fn container(id: &str) -> ContainerEvent {
        ContainerEvent { container_id: id.into(), ..Default::default() }
    }

    fn exit_code(code: i32) -> Option<container_event::OptionalExitCode> {
        Some(container_event::OptionalExitCode::ExitCode(code))
    }

    fn images(list: Vec<ContainerImage>) -> ContainerImagePayload {
        ContainerImagePayload { host: "node1".into(), images: list, ..Default::default() }
    }

    fn image(id: &str) -> ContainerImage {
        ContainerImage { id: id.into(), ..Default::default() }
    }

    fn layer(size: i64) -> ContainerImageLayer {
        ContainerImageLayer { size, ..Default::default() }
    }

    fn with_layers(id: &str, sizes: &[i64]) -> ContainerImage {
        ContainerImage { layers: sizes.iter().map(|s| layer(*s)).collect(), ..image(id) }
    }

    // ---- lifecycle events ----

    #[test]
    fn no_exit_code_is_none_exit_0_is_some_0_an_oom_kill_is_some_137() {
        assert_eq!(container_row(container("a")).exit_code, None);
        assert_eq!(
            container_row(ContainerEvent { optional_exit_code: exit_code(0), ..container("b") }).exit_code,
            Some(0)
        );
        assert_eq!(
            container_row(ContainerEvent { optional_exit_code: exit_code(137), ..container("c") }).exit_code,
            Some(137)
        );
    }

    #[test]
    fn an_explicit_exit_0_is_still_there_after_the_wire() {
        let sent = lifecycle(vec![
            typed(
                EventType::Delete,
                event::TypedEvent::Container(ContainerEvent { optional_exit_code: exit_code(0), ..container("clean") }),
            ),
            typed(EventType::Delete, event::TypedEvent::Container(container("unknown"))),
        ]);
        let rows = event_rows(&EventsPayload::decode(sent.encode_to_vec().as_slice()).unwrap(), "t1", now());
        assert_eq!(rows.iter().map(|r| r.exit_code).collect::<Vec<_>>(), [Some(0), None]);
    }

    #[test]
    fn absent_timestamps_stay_none_never_1970_never_the_receive_time() {
        // A transition stamped 0 is "the agent did not say when", not the epoch.
        let row = container_row(ContainerEvent { transition: Some(Default::default()), ..container("a") });
        assert_eq!((row.created_at, row.exited_at, row.transition_at), (None, None, None));
        assert_eq!(row.timestamp, now());
    }

    #[test]
    fn timestamps_the_sender_supplied_are_kept() {
        let row = container_row(ContainerEvent {
            optional_creation_timestamp: Some(container_event::OptionalCreationTimestamp::CreationTimestamp(
                1700000000,
            )),
            optional_exit_timestamp: Some(container_event::OptionalExitTimestamp::ExitTimestamp(1700000600)),
            transition: Some(ContainerStateTransition { transition_timestamp: 1700000300, ..Default::default() }),
            ..container("a")
        });
        assert_eq!(row.created_at, at(1700000000));
        assert_eq!(row.exited_at, at(1700000600));
        assert_eq!(row.transition_at, at(1700000300));
    }

    #[test]
    fn each_kind_of_event_fills_only_its_own_identity_column() {
        let pod = PodEvent {
            pod_uid: "pod-1".into(),
            source: "kubelet".into(),
            exit_timestamp: Some(1700000600),
            ..Default::default()
        };
        let task =
            TaskEvent { task_arn: "arn:aws:ecs:task/1".into(), source: "ecs".into(), exit_timestamp: Some(1700000600) };
        let payload = EventsPayload {
            cluster_id: "cl-1".into(),
            object_kind: ObjectKind::Pod as i32,
            ..lifecycle(vec![
                typed(EventType::Delete, event::TypedEvent::Pod(pod)),
                typed(EventType::Delete, event::TypedEvent::Task(task)),
            ])
        };
        let rows = event_rows(&payload, "t1", now());
        assert_eq!(rows.len(), 2);

        let pod = &rows[0];
        assert_eq!(
            (pod.event_variant.as_str(), pod.pod_uid.as_str(), pod.source.as_str()),
            ("pod", "pod-1", "kubelet")
        );
        assert_eq!((pod.container_id.as_str(), pod.container_name.as_str(), pod.task_arn.as_str()), ("", "", ""));
        assert_eq!((pod.host.as_str(), pod.cluster_id.as_str()), ("node1", "cl-1"));
        assert_eq!((pod.object_kind.as_str(), pod.event_type.as_str()), ("Pod", "Delete"));
        assert_eq!(pod.exited_at, at(1700000600));

        let task = &rows[1];
        assert_eq!(
            (task.event_variant.as_str(), task.task_arn.as_str(), task.source.as_str()),
            ("task", "arn:aws:ecs:task/1", "ecs")
        );
        assert_eq!((task.container_id.as_str(), task.pod_uid.as_str()), ("", ""));
    }

    #[test]
    fn an_event_with_no_typed_detail_still_yields_a_row_marked_unknown() {
        let rows = event_rows(
            &lifecycle(vec![Event { event_type: EventType::Create as i32, typed_event: None }]),
            "t1",
            now(),
        );
        let row = &rows[0];
        assert_eq!(rows.len(), 1);
        assert_eq!((row.event_type.as_str(), row.event_variant.as_str()), ("Create", "unknown"));
        assert_eq!((row.host.as_str(), row.tenant_id.as_str()), ("node1", "t1"));
        assert_eq!((row.container_id.as_str(), row.pod_uid.as_str(), row.task_arn.as_str()), ("", "", ""));
        assert_eq!(row.exit_code, None);
    }

    #[test]
    fn empty_sub_messages_convert_without_failing() {
        let payload = lifecycle(vec![
            Event {
                typed_event: Some(event::TypedEvent::Container(ContainerEvent {
                    transition: Some(Default::default()),
                    ..Default::default()
                })),
                ..Default::default()
            },
            Event {
                typed_event: Some(event::TypedEvent::Pod(PodEvent {
                    transition: Some(Default::default()),
                    ..Default::default()
                })),
                ..Default::default()
            },
            Event { typed_event: Some(event::TypedEvent::Task(TaskEvent::default())), ..Default::default() },
            Event::default(),
        ]);
        let rows = event_rows(&payload, "t1", now());
        assert_eq!(
            rows.iter().map(|r| r.event_variant.as_str()).collect::<Vec<_>>(),
            ["container", "pod", "task", "unknown"]
        );
        // A transition with no states: the label says so, the columns stay empty.
        assert_eq!((rows[0].old_state.as_str(), rows[0].old_state_kind.as_str()), ("-", ""));
        assert_eq!((rows[1].new_state.as_str(), rows[1].new_state_variant.as_str()), ("-", ""));
    }

    #[test]
    fn a_container_event_carries_its_owner_and_its_transition() {
        let row = container_row(ContainerEvent {
            owner: Some(Owner { owner_type: ObjectKind::Pod as i32, owner_uid: "pod-9".into() }),
            transition: Some(ContainerStateTransition {
                last_observed_state: Some(ContainerStateValue {
                    kind: ContainerStateKind::Running as i32,
                    ..Default::default()
                }),
                new_state: Some(ContainerStateValue {
                    kind: ContainerStateKind::Terminated as i32,
                    reason: Some("OOMKilled".into()),
                    ..Default::default()
                }),
                transition_timestamp: 1700000300,
                ..Default::default()
            }),
            ..container("a")
        });
        assert_eq!((row.owner_type.as_str(), row.owner_uid.as_str()), ("Pod", "pod-9"));
        assert_eq!(row.old_state, "CONTAINER_STATE_KIND_RUNNING");
        assert_eq!(row.new_state, "CONTAINER_STATE_KIND_TERMINATED/OOMKilled");
        assert_eq!(row.transition_at, at(1700000300));
    }

    #[test]
    fn an_oom_kill_is_stored_as_columns_beside_the_label() {
        let transition = ContainerStateTransition {
            container_kind: ContainerKind::Regular as i32,
            precision: Precision::Exact as i32,
            missed_intermediate: MissedIntermediate::Proven as i32,
            transition_timestamp: 1700000000,
            last_observed_state: Some(ContainerStateValue {
                kind: ContainerStateKind::Running as i32,
                ..Default::default()
            }),
            new_state: Some(ContainerStateValue {
                kind: ContainerStateKind::Terminated as i32,
                reason: Some("OOMKilled".into()),
                exit_code: Some(137),
                signal: Some(9),
            }),
        };
        let event = ContainerEvent { transition: Some(transition), ..container("containerd://abc") };
        let payload = EventsPayload {
            version: "v1".into(),
            ..lifecycle(vec![typed(EventType::Transition, event::TypedEvent::Container(event))])
        };
        let row = &event_rows(&payload, "t", now())[0];

        assert_eq!((row.payload_version.as_str(), row.event_variant.as_str()), ("v1", "container"));
        assert_eq!(row.container_kind, "CONTAINER_KIND_REGULAR");
        assert_eq!(row.precision, "PRECISION_EXACT");
        assert_eq!(row.missed_intermediate, "MISSED_INTERMEDIATE_PROVEN");
        assert_eq!(row.old_state_kind, "CONTAINER_STATE_KIND_RUNNING");
        assert_eq!(row.new_state_kind, "CONTAINER_STATE_KIND_TERMINATED");
        assert_eq!(
            (row.new_reason.as_deref(), row.new_exit_code, row.new_signal),
            (Some("OOMKilled"), Some(137), Some(9))
        );
        // RUNNING carried no reason, code or signal, and absent must stay absent rather than read
        // as "terminated cleanly with no reason".
        assert_eq!((row.old_reason.as_deref(), row.old_exit_code, row.old_signal), (None, None, None));
        assert_eq!(row.new_state, "CONTAINER_STATE_KIND_TERMINATED/OOMKilled exit=137 signal=9");
    }

    #[test]
    fn a_pod_condition_keeps_its_message_and_the_row_says_which_kind_of_status_it_holds() {
        let message = "0/3 nodes are available: insufficient memory";
        let condition = ConditionValue {
            r#type: "PodScheduled".into(),
            status: "False".into(),
            reason: Some("Unschedulable".into()),
            message: Some(message.into()),
        };
        let pod = PodEvent {
            pod_uid: "pod-uid-1".into(),
            transition: Some(PodStateTransition {
                field: PodStatusField::Condition as i32,
                last_observed_state: Some(PodStatusValue {
                    value: Some(pod_status_value::Value::Phase("Pending".into())),
                }),
                new_state: Some(PodStatusValue { value: Some(pod_status_value::Value::Condition(condition)) }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let row =
            &event_rows(&lifecycle(vec![typed(EventType::Transition, event::TypedEvent::Pod(pod))]), "t", now())[0];

        assert_eq!((row.event_variant.as_str(), row.pod_uid.as_str()), ("pod", "pod-uid-1"));
        assert_eq!(row.pod_status_field, "POD_STATUS_FIELD_CONDITION");
        assert_eq!((row.old_state_variant.as_str(), row.new_state_variant.as_str()), ("phase", "condition"));
        assert_eq!((row.old_phase.as_deref(), row.old_condition_type.as_deref()), (Some("Pending"), None));
        assert_eq!((row.new_phase.as_deref(), row.new_condition_type.as_deref()), (None, Some("PodScheduled")));
        assert_eq!(row.new_condition_status.as_deref(), Some("False"));
        assert_eq!(row.new_condition_reason.as_deref(), Some("Unschedulable"));
        assert_eq!(row.new_condition_message.as_deref(), Some(message));
        assert_eq!((row.old_state.as_str(), row.new_state.as_str()), ("Pending", "PodScheduled=False/Unschedulable"));
    }

    #[test]
    fn a_container_name_set_to_empty_is_not_the_same_as_one_never_set() {
        assert_eq!(container_row(container("a")).container_name_present, 0);
        assert_eq!(
            container_row(ContainerEvent { container_name: Some("".into()), ..container("b") }).container_name_present,
            1
        );
        assert_eq!(
            container_row(ContainerEvent { container_name: Some("app".into()), ..container("c") })
                .container_name_present,
            1
        );
    }

    #[test]
    fn an_enum_value_this_schema_has_no_name_for_is_stored_as_its_number() {
        let payload = EventsPayload { object_kind: 7, ..lifecycle(vec![Event { event_type: 42, typed_event: None }]) };
        let row = &event_rows(&payload, "t", now())[0];
        assert_eq!((row.event_type.as_str(), row.object_kind.as_str()), ("42", "7"));
    }

    // ---- image inventory ----

    #[test]
    fn an_image_row_is_keyed_on_its_digest_and_an_image_with_no_identity_is_skipped() {
        let keyed = ContainerImage {
            name: "registry.example.com/app".into(),
            short_name: "app".into(),
            registry: "registry.example.com".into(),
            digest: "sha256:abc".into(),
            size: 1234,
            os: Some(OperatingSystem { name: "linux".into(), architecture: "amd64".into(), ..Default::default() }),
            built_at: Some(Timestamp { seconds: 1700000000, nanos: 0 }),
            repo_tags: vec!["app:1.0".into()],
            layers: vec![layer(100), layer(250)],
            ..image("img-1")
        };
        let (rows, skipped) = image_rows(
            &images(vec![ContainerImage { name: "no-digest".into(), ..Default::default() }, keyed]),
            "t1",
            now(),
        );

        assert_eq!((rows.len(), skipped), (1, 1));
        let row = &rows[0];
        assert_eq!((row.digest.as_str(), row.host.as_str(), row.tenant_id.as_str()), ("sha256:abc", "node1", "t1"));
        assert_eq!(row.collected_at, now());
        assert_eq!((row.layer_count, row.layer_bytes, row.size_bytes), (2, 350, 1234));
        assert_eq!((row.os_name.as_str(), row.architecture.as_str()), ("linux", "amd64"));
        assert_eq!(row.repo_tags, ["app:1.0"]);
        assert_eq!((row.built_at, row.published_at, row.source.as_deref()), (at(1700000000), None, None));
    }

    #[test]
    fn empty_payloads_convert_to_no_rows() {
        assert!(event_rows(&EventsPayload::default(), "t1", now()).is_empty());
        let (rows, skipped) = image_rows(&ContainerImagePayload::default(), "t1", now());
        assert!(rows.is_empty());
        assert_eq!(skipped, 0);
    }

    #[test]
    fn an_image_without_a_digest_is_keyed_on_its_id_and_the_row_says_so() {
        let payload = images(vec![
            ContainerImage { name: "pushed".into(), digest: "sha256:abc".into(), ..image("img-a") },
            ContainerImage { name: "built-locally".into(), ..image("img-b") },
            ContainerImage { name: "nothing-at-all".into(), ..Default::default() },
        ]);
        let (rows, skipped) = image_rows(&payload, "t1", now());
        let identity = |r: &ContainerImageRow| {
            (r.image_key.clone(), r.identity_source.clone(), r.digest.clone(), r.image_id.clone())
        };

        assert_eq!((rows.len(), skipped), (2, 1));
        assert_eq!(identity(&rows[0]), ("sha256:abc".into(), "digest".into(), "sha256:abc".into(), "img-a".into()));
        // The fallback fills the key; it does not forge a digest.
        assert_eq!(identity(&rows[1]), ("img-b".into(), "image_id".into(), "".into(), "img-b".into()));
    }

    #[test]
    fn layers_and_their_build_history_are_parallel_arrays_in_wire_order() {
        let first = ContainerImageLayer {
            media_type: "application/vnd.oci.image.layer.v1.tar+gzip".into(),
            digest: "sha256:layer1".into(),
            size: 100,
            urls: vec!["https://example.invalid/blob".into()],
            history: Some(container_image_layer::History {
                created: Some(Timestamp { seconds: 1767323045, nanos: 500_000_000 }),
                created_by: "RUN apk add curl".into(),
                author: "ci".into(),
                comment: "base".into(),
                empty_layer: false,
            }),
        };
        let second = ContainerImageLayer {
            digest: "sha256:layer2".into(),
            size: 200,
            history: Some(container_image_layer::History {
                created_by: "COPY . /app".into(),
                empty_layer: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let image = ContainerImage {
            name: "shop/app".into(),
            digest: "sha256:dig".into(),
            size: 300,
            layers: vec![first, second, layer(5)],
            ..image("sha256:img")
        };
        let payload =
            ContainerImagePayload { version: "v1".into(), source: Some("agent".into()), ..images(vec![image]) };
        let (rows, skipped) = image_rows(&payload, "t", now());

        assert_eq!((rows.len(), skipped), (1, 0));
        let row = &rows[0];
        assert_eq!((row.payload_version.as_str(), row.source.as_deref()), ("v1", Some("agent")));
        assert_eq!(row.layer_digests, ["sha256:layer1", "sha256:layer2", ""]);
        assert_eq!(row.layer_sizes, [100, 200, 5]);
        assert_eq!(row.layer_media_types[0], "application/vnd.oci.image.layer.v1.tar+gzip");
        assert_eq!(row.layer_urls, [vec!["https://example.invalid/blob".to_string()], vec![], vec![]]);
        // The third layer has no history at all: the arrays stay aligned.
        assert_eq!(row.layer_history_created_by, ["RUN apk add curl", "COPY . /app", ""]);
        assert_eq!(row.layer_history_author, ["ci", "", ""]);
        assert_eq!(row.layer_history_comment, ["base", "", ""]);
        assert_eq!(row.layer_history_empty_layer, [0, 1, 0]);
        // A missing date is None in its slot, not 1970.
        let created = Utc.timestamp_opt(1767323045, 500_000_000).unwrap();
        assert_eq!(row.layer_history_created, [Some(created), None, None]);
        assert_eq!((row.layer_count, row.layer_bytes), (3, 305));
    }

    #[test]
    fn a_timestamp_that_is_not_after_1970_or_not_valid_is_none() {
        let built_at = |seconds, nanos| {
            let (rows, _) = image_rows(
                &images(vec![ContainerImage { built_at: Some(Timestamp { seconds, nanos }), ..image("a") }]),
                "t",
                now(),
            );
            rows[0].built_at
        };
        assert_eq!(built_at(0, 5), None);
        assert_eq!(built_at(-1, 0), None);
        assert_eq!(built_at(1600000000, -1), None);
        assert_eq!(built_at(253402300800, 0), None);
        assert_eq!(built_at(1600000000, 0), at(1600000000));
    }

    #[test]
    fn a_negative_image_size_is_clamped_to_zero_and_flagged() {
        let (rows, _) = image_rows(
            &images(vec![ContainerImage { size: -1, ..image("a") }, ContainerImage { size: 42, ..image("b") }]),
            "t",
            now(),
        );
        assert_eq!((rows[0].size_bytes, rows[0].size_negative), (0, 1));
        assert_eq!((rows[1].size_bytes, rows[1].size_negative), (42, 0));
    }

    #[test]
    fn one_negative_layer_size_flags_the_total_and_a_negative_total_is_clamped() {
        let payload = images(vec![
            // The sum stays positive, but one layer lied: only the flag says so.
            with_layers("a", &[500, -1]),
            // The sum goes negative: cast as is it would be ~18 exabytes.
            with_layers("b", &[-700, 100]),
            with_layers("c", &[10, 20]),
        ]);
        let (rows, _) = image_rows(&payload, "t", now());
        assert_eq!((rows[0].layer_bytes, rows[0].layer_size_negative), (499, 1));
        assert_eq!((rows[1].layer_bytes, rows[1].layer_size_negative), (0, 1));
        assert_eq!((rows[2].layer_bytes, rows[2].layer_size_negative), (30, 0));
        // The clamp hides nothing: the signed values arrive as they were sent.
        assert_eq!(rows[1].layer_sizes[0], -700);
    }
}
