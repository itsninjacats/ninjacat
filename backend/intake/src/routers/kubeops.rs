//! kubeops-intake.<site>, orchestrator.<site> — Kubernetes.
//!
//!   agent config: orchestrator_explorer.orchestrator_dd_url
//!                 / DD_ORCHESTRATOR_EXPLORER_ORCHESTRATOR_DD_URL
//!
//! Sent by the CLUSTER agent, not the node agent. orch and orchmanif carry the same frame as the
//! process agent's /api/v1/collector, with other message types (41..88, 200). kubeactions is
//! different: event platform, JSON.
//!
//! Object collections land in k8s_resources, raw manifests in k8s_manifests, the cluster summary
//! in k8s_cluster, ECS tasks in ecs_tasks, action results in k8s_actions. A frame that does not
//! decode, and a message that is not an orchestrator one, goes to raw_payloads.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::Engine;
use chrono::{DateTime, Utc};
use ninjapaws_proto::datadog::process_agent::*;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::ecs::EcsTask as EcsTaskRow;
use ninjapaws_storage::rows::k8s::{K8sAction, K8sCluster, K8sManifest, K8sResource};
use serde_json::Value;
use tako::types::Response;

use crate::fields::{self, Fields, Mismatches};
use crate::json;
use crate::process_frame::{self, Body, Header};
use crate::server::{self, Ctx, accepted};
use crate::wire::{enum_name, tags, text, time, version_spread};

/// The frame header as the four columns every orchestrator row carries.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameColumns {
    pub org_id: i32,
    pub subscription_id: u8,
    /// The header's raw integer; None when the wire said 0, "not provided".
    pub timestamp: Option<i64>,
    pub encoding: String,
}

/// An unknown encoding keeps its number rather than becoming "".
pub fn encoding_name(encoding: u8) -> String {
    match encoding {
        0 => "protobuf".into(),
        1 => "json".into(),
        2 => "zstd_protobuf".into(),
        4 => "zstd1x_protobuf".into(),
        5 => "zstd_protobuf_nocgo".into(),
        other => format!("unknown_{other}"),
    }
}

pub fn frame_columns(header: &Header) -> FrameColumns {
    FrameColumns {
        org_id: header.org_id,
        subscription_id: header.subscription_id,
        timestamp: (header.timestamp != 0).then_some(header.timestamp),
        encoding: encoding_name(header.encoding),
    }
}

/// Unix seconds for a nullable column: 0 means the sender did not supply the time, and it must
/// never become 1970.
fn seconds(value: i64) -> Option<DateTime<Utc>> {
    time::optional_seconds(true, value)
}

/// major.minor.patch, with the prerelease tag when set.
fn agent_version_text(version: Option<&AgentVersion>) -> String {
    let Some(version) = version else {
        return String::new();
    };
    let numbers = format!("{}.{}.{}", version.major, version.minor, version.patch);
    if version.pre.is_empty() { numbers } else { format!("{numbers}-{}", version.pre) }
}

fn to_map<V: Clone>(field: &HashMap<String, V>) -> BTreeMap<String, V> {
    field.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn counts(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(name, value)| (name.to_string(), *value)).collect()
}

fn metric_values(metrics: Option<&ResourceMetrics>) -> BTreeMap<String, f64> {
    metrics.map(|m| to_map(&m.metric_values)).unwrap_or_default()
}

// ---- object collections → k8s_resources ----

// The 24 kinds share a shape but no type: every collector message has the same envelope and one
// list of objects, and every object has metadata and tags. What differs is written out per kind
// below, plainly, so the compiler checks every field name.

/// One condition of an object. Each kind declares a condition message of its own; a time a kind
/// does not have is 0.
struct Condition {
    kind: String,
    status: String,
    reason: String,
    message: String,
    last_transition: i64,
    last_update: i64,
    last_probe: i64,
}

fn condition(kind: &str, status: &str, reason: &str, message: &str, last_transition: i64) -> Condition {
    Condition {
        kind: kind.into(),
        status: status.into(),
        reason: reason.into(),
        message: message.into(),
        last_transition,
        last_update: 0,
        last_probe: 0,
    }
}

/// The kinds whose numbers and typed extras have columns of their own.
enum Kind<'a> {
    Pod(&'a Pod),
    Deployment(&'a Deployment),
    ReplicaSet(&'a ReplicaSet),
    DaemonSet(&'a DaemonSet),
    StatefulSet(&'a StatefulSet),
    Node(&'a Node),
    Namespace(&'a Namespace),
    Service(&'a Service),
    Role(&'a Role),
    ClusterRole(&'a ClusterRole),
    RoleBinding(&'a RoleBinding),
    ClusterRoleBinding(&'a ClusterRoleBinding),
    Job(&'a Job),
    CronJob(&'a CronJob),
    HorizontalPodAutoscaler(&'a HorizontalPodAutoscaler),
    PodDisruptionBudget(&'a PodDisruptionBudget),
    Other,
}

/// What a row takes from an object besides the columns of its kind.
struct ObjectParts<'a> {
    kind: Kind<'a>,
    /// The whole object as JSON.
    object: String,
    metadata: Option<&'a Metadata>,
    tags: &'a [String],
    conditions: Vec<Condition>,
    /// The agent's own one-line summary of what is wrong, where it sends one.
    condition_message: String,
    resource_requirements: &'a [ResourceRequirements],
    /// None for the kinds that carry no computed metrics.
    metrics: Option<&'a ResourceMetrics>,
}

/// An object with metadata and tags and nothing else in common. One without metadata gets no row,
/// so its JSON is not written.
fn plain_parts<'a, T: prost::Message + prost::Name>(
    kind: Kind<'a>,
    object: &'a T,
    metadata: Option<&'a Metadata>,
    tags: &'a [String],
) -> ObjectParts<'a> {
    ObjectParts {
        kind,
        object: if metadata.is_some() { json::proto::message(Some(object)) } else { String::new() },
        metadata,
        tags,
        conditions: Vec::new(),
        condition_message: String::new(),
        resource_requirements: &[],
        metrics: None,
    }
}

fn pod_parts(pod: &Pod) -> ObjectParts<'_> {
    ObjectParts {
        conditions: pod
            .conditions
            .iter()
            .map(|c| Condition {
                last_probe: c.last_probe_time,
                ..condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time)
            })
            .collect(),
        condition_message: pod.condition_message.clone(),
        resource_requirements: &pod.resource_requirements,
        metrics: pod.metrics.as_ref(),
        ..plain_parts(Kind::Pod(pod), pod, pod.metadata.as_ref(), &pod.tags)
    }
}

fn pod_disruption_budget_parts(budget: &PodDisruptionBudget) -> ObjectParts<'_> {
    ObjectParts {
        conditions: budget
            .status
            .iter()
            .flat_map(|s| &s.conditions)
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        ..plain_parts(Kind::PodDisruptionBudget(budget), budget, budget.metadata.as_ref(), &budget.tags)
    }
}

fn replica_set_parts(replica_set: &ReplicaSet) -> ObjectParts<'_> {
    ObjectParts {
        conditions: replica_set
            .conditions
            .iter()
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        resource_requirements: &replica_set.resource_requirements,
        metrics: replica_set.metrics.as_ref(),
        ..plain_parts(Kind::ReplicaSet(replica_set), replica_set, replica_set.metadata.as_ref(), &replica_set.tags)
    }
}

fn deployment_parts(deployment: &Deployment) -> ObjectParts<'_> {
    ObjectParts {
        conditions: deployment
            .conditions
            .iter()
            .map(|c| Condition {
                last_update: c.last_update_time,
                ..condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time)
            })
            .collect(),
        condition_message: deployment.condition_message.clone(),
        resource_requirements: &deployment.resource_requirements,
        metrics: deployment.metrics.as_ref(),
        ..plain_parts(Kind::Deployment(deployment), deployment, deployment.metadata.as_ref(), &deployment.tags)
    }
}

fn service_parts(service: &Service) -> ObjectParts<'_> {
    ObjectParts {
        metrics: service.metrics.as_ref(),
        ..plain_parts(Kind::Service(service), service, service.metadata.as_ref(), &service.tags)
    }
}

fn node_parts(node: &Node) -> ObjectParts<'_> {
    ObjectParts {
        conditions: node
            .status
            .iter()
            .flat_map(|s| &s.conditions)
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        metrics: node.metrics.as_ref(),
        ..plain_parts(Kind::Node(node), node, node.metadata.as_ref(), &node.tags)
    }
}

fn namespace_parts(namespace: &Namespace) -> ObjectParts<'_> {
    ObjectParts {
        conditions: namespace
            .conditions
            .iter()
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        condition_message: namespace.condition_message.clone(),
        ..plain_parts(Kind::Namespace(namespace), namespace, namespace.metadata.as_ref(), &namespace.tags)
    }
}

fn job_parts(job: &Job) -> ObjectParts<'_> {
    ObjectParts {
        conditions: job
            .conditions
            .iter()
            .map(|c| Condition {
                last_probe: c.last_probe_time,
                ..condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time)
            })
            .collect(),
        condition_message: job.status.as_ref().map(|s| s.condition_message.clone()).unwrap_or_default(),
        resource_requirements: job.spec.as_ref().map_or(&[], |s| &s.resource_requirements),
        ..plain_parts(Kind::Job(job), job, job.metadata.as_ref(), &job.tags)
    }
}

fn cron_job_parts(cron_job: &CronJob) -> ObjectParts<'_> {
    ObjectParts {
        resource_requirements: cron_job.spec.as_ref().map_or(&[], |s| &s.resource_requirements),
        ..plain_parts(Kind::CronJob(cron_job), cron_job, cron_job.metadata.as_ref(), &cron_job.tags)
    }
}

fn daemon_set_parts(daemon_set: &DaemonSet) -> ObjectParts<'_> {
    ObjectParts {
        conditions: daemon_set
            .conditions
            .iter()
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        resource_requirements: daemon_set.spec.as_ref().map_or(&[], |s| &s.resource_requirements),
        metrics: daemon_set.metrics.as_ref(),
        ..plain_parts(Kind::DaemonSet(daemon_set), daemon_set, daemon_set.metadata.as_ref(), &daemon_set.tags)
    }
}

fn stateful_set_parts(stateful_set: &StatefulSet) -> ObjectParts<'_> {
    ObjectParts {
        conditions: stateful_set
            .conditions
            .iter()
            .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        resource_requirements: stateful_set.spec.as_ref().map_or(&[], |s| &s.resource_requirements),
        metrics: stateful_set.metrics.as_ref(),
        ..plain_parts(Kind::StatefulSet(stateful_set), stateful_set, stateful_set.metadata.as_ref(), &stateful_set.tags)
    }
}

fn persistent_volume_claim_parts(claim: &PersistentVolumeClaim) -> ObjectParts<'_> {
    ObjectParts {
        conditions: claim
            .status
            .iter()
            .flat_map(|s| &s.conditions)
            .map(|c| Condition {
                last_probe: c.last_probe_time,
                ..condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time)
            })
            .collect(),
        ..plain_parts(Kind::Other, claim, claim.metadata.as_ref(), &claim.tags)
    }
}

fn cluster_role_parts(role: &ClusterRole) -> ObjectParts<'_> {
    ObjectParts {
        metrics: role.metrics.as_ref(),
        ..plain_parts(Kind::ClusterRole(role), role, role.metadata.as_ref(), &role.tags)
    }
}

/// A VerticalPodAutoscaler has two condition lists, on the object and in its status, and they
/// are different messages; the nested one is what a real autoscaler fills. Both are read, the
/// object's first.
fn vertical_pod_autoscaler_parts(autoscaler: &VerticalPodAutoscaler) -> ObjectParts<'_> {
    let own = autoscaler
        .conditions
        .iter()
        .map(|c| condition(&c.r#type, &c.status, &c.reason, &c.message, c.last_transition_time));
    let nested = autoscaler
        .status
        .iter()
        .flat_map(|s| &s.conditions)
        .map(|c| condition(&c.condition_type, &c.condition_status, &c.reason, &c.message, c.last_transition_time));
    ObjectParts {
        conditions: own.chain(nested).collect(),
        ..plain_parts(Kind::Other, autoscaler, autoscaler.metadata.as_ref(), &autoscaler.tags)
    }
}

fn horizontal_pod_autoscaler_parts(autoscaler: &HorizontalPodAutoscaler) -> ObjectParts<'_> {
    ObjectParts {
        conditions: autoscaler
            .conditions
            .iter()
            .map(|c| condition(&c.condition_type, &c.condition_status, &c.reason, &c.message, c.last_transition_time))
            .collect(),
        ..plain_parts(
            Kind::HorizontalPodAutoscaler(autoscaler),
            autoscaler,
            autoscaler.metadata.as_ref(),
            &autoscaler.tags,
        )
    }
}

/// The columns every kind can fill, from the parts read above.
fn with_common_fields(parts: &ObjectParts, row: &mut K8sResource) {
    let conditions = &parts.conditions;
    row.condition_types = conditions.iter().map(|c| c.kind.clone()).collect();
    row.condition_statuses = conditions.iter().map(|c| c.status.clone()).collect();
    row.condition_reasons = conditions.iter().map(|c| c.reason.clone()).collect();
    row.condition_messages = conditions.iter().map(|c| c.message.clone()).collect();
    row.condition_last_transition = conditions.iter().map(|c| seconds(c.last_transition)).collect();
    row.condition_last_update = conditions.iter().map(|c| seconds(c.last_update)).collect();
    row.condition_last_probe = conditions.iter().map(|c| seconds(c.last_probe)).collect();
    row.condition_message = parts.condition_message.clone();
    row.resource_requirements = json::proto::messages(parts.resource_requirements);
    row.metrics = metric_values(parts.metrics);
}

/// The numbers and typed extras of the kinds people ask about by name. Every kind names its
/// counters differently, so this is the part that cannot be read generically. A kind with no case
/// here loses nothing: its whole message is in the object column.
fn with_kind_fields(kind: &Kind, row: &mut K8sResource) {
    match kind {
        Kind::Pod(pod) => {
            // Ready and restarts are sums over the container statuses; the pod has no field for
            // them. Regular containers first, then init containers, in one set of arrays: every
            // question about a container is the same question for both.
            let regular = &pod.container_statuses;
            let init = &pod.init_container_statuses;
            let all: Vec<&ContainerStatus> = regular.iter().chain(init).collect();
            let ready_count = |statuses: &[ContainerStatus]| statuses.iter().filter(|s| s.ready).count() as i32;
            let restarts = |statuses: &[ContainerStatus]| statuses.iter().map(|s| s.restart_count).sum::<i32>();

            row.node_name = pod.node_name.clone();
            row.phase = pod.phase.clone();
            row.status = pod.status.clone();
            // For a pod, ready/desired is containers ready / containers total.
            row.ready = ready_count(regular);
            row.desired = regular.len() as i32;
            row.counts = counts(&[
                ("restarts", i64::from(restarts(regular))),
                ("ready_containers", i64::from(ready_count(regular))),
                ("total_containers", regular.len() as i64),
                ("init_restarts", i64::from(restarts(init))),
                ("ready_init_containers", i64::from(ready_count(init))),
                ("total_init_containers", init.len() as i64),
                // The agent's own aggregate, beside the recomputed one: if they ever disagree,
                // that is worth seeing.
                ("pod_restart_count", i64::from(pod.restart_count)),
            ]);
            row.pod_ip = pod.ip.clone();
            row.nominated_node_name = pod.nominated_node_name.clone();
            row.qos_class = pod.qos_class.clone();
            row.priority_class = pod.priority_class.clone();
            row.start_time = seconds(pod.start_time);
            row.scheduled_time = seconds(pod.scheduled_time);
            if let Some(host) = pod.host.as_ref().filter(|h| !h.name.is_empty()) {
                row.host_name = host.name.clone();
            }
            row.container_names = all.iter().map(|s| s.name.clone()).collect();
            row.container_ids = all.iter().map(|s| s.container_id.clone()).collect();
            row.container_ready = all.iter().map(|s| text::flag(s.ready)).collect();
            row.container_restarts = all.iter().map(|s| s.restart_count).collect();
            row.container_states = all.iter().map(|s| s.state.clone()).collect();
            row.container_messages = all.iter().map(|s| s.message.clone()).collect();
            row.container_images = all.iter().map(|s| s.image.clone()).collect();
            row.container_image_ids = all.iter().map(|s| s.image_id.clone()).collect();
            row.container_is_init = regular.iter().map(|_| 0).chain(init.iter().map(|_| 1)).collect();
        }
        Kind::Deployment(deployment) => {
            row.ready = deployment.ready_replicas;
            row.desired = deployment.replicas_desired;
            row.available = deployment.available_replicas;
            row.counts = counts(&[
                ("ready", i64::from(deployment.ready_replicas)),
                ("desired", i64::from(deployment.replicas_desired)),
                ("updated", i64::from(deployment.updated_replicas)),
                ("available", i64::from(deployment.available_replicas)),
                ("unavailable", i64::from(deployment.unavailable_replicas)),
                // The observed total: during a rollout it differs from desired, and that
                // difference is the rollout.
                ("replicas", i64::from(deployment.replicas)),
            ]);
        }
        Kind::ReplicaSet(replica_set) => {
            row.ready = replica_set.ready_replicas;
            row.desired = replica_set.replicas_desired;
            row.available = replica_set.available_replicas;
            row.counts = counts(&[
                ("ready", i64::from(replica_set.ready_replicas)),
                ("desired", i64::from(replica_set.replicas_desired)),
                ("available", i64::from(replica_set.available_replicas)),
                ("replicas", i64::from(replica_set.replicas)),
                ("fully_labeled", i64::from(replica_set.fully_labeled_replicas)),
            ]);
        }
        Kind::DaemonSet(daemon_set) => {
            let none = DaemonSetStatus::default();
            let status = daemon_set.status.as_ref().unwrap_or(&none);
            row.ready = status.number_ready;
            row.desired = status.desired_number_scheduled;
            row.available = status.number_available;
            row.counts = counts(&[
                ("ready", i64::from(status.number_ready)),
                ("desired", i64::from(status.desired_number_scheduled)),
                ("current", i64::from(status.current_number_scheduled)),
                ("updated", i64::from(status.updated_number_scheduled)),
                ("available", i64::from(status.number_available)),
                ("misscheduled", i64::from(status.number_misscheduled)),
                // The one that says a rollout is stuck.
                ("unavailable", i64::from(status.number_unavailable)),
            ]);
        }
        Kind::StatefulSet(stateful_set) => {
            // Desired lives in the spec here, unlike the other workloads.
            let (no_status, no_spec) = (StatefulSetStatus::default(), StatefulSetSpec::default());
            let status = stateful_set.status.as_ref().unwrap_or(&no_status);
            let spec = stateful_set.spec.as_ref().unwrap_or(&no_spec);
            row.ready = status.ready_replicas;
            row.desired = spec.desired_replicas;
            row.counts = counts(&[
                ("ready", i64::from(status.ready_replicas)),
                ("desired", i64::from(spec.desired_replicas)),
                ("current", i64::from(status.current_replicas)),
                ("updated", i64::from(status.updated_replicas)),
                ("replicas", i64::from(status.replicas)),
                ("partition", i64::from(spec.partition)),
            ]);
        }
        Kind::Node(node) => {
            let none = NodeStatus::default();
            let status = node.status.as_ref().unwrap_or(&none);
            // "Ready", "NotReady": the one status a node has.
            row.status = status.status.clone();
            row.pod_cidr = node.pod_cidr.clone();
            row.pod_cidrs = node.pod_cid_rs.clone();
            row.unschedulable = text::flag(node.unschedulable);
            row.provider_id = node.provider_id.clone();
            row.node_roles = node.roles.clone();
            row.taints = json::proto::messages(&node.taints);
            row.capacity = to_map(&status.capacity);
            row.allocatable = to_map(&status.allocatable);
            row.node_addresses = if status.node_addresses.is_empty() {
                String::new()
            } else {
                serde_json::to_string(&to_map(&status.node_addresses)).unwrap()
            };
            row.kubelet_version = status.kubelet_version.clone();
            row.kube_proxy_version = status.kube_proxy_version.clone();
            row.operating_system = status.operating_system.clone();
            row.architecture = status.architecture.clone();
            row.kernel_version = status.kernel_version.clone();
            row.os_image = status.os_image.clone();
            row.container_runtime_version = status.container_runtime_version.clone();
            if let Some(host) = node.host.as_ref().filter(|h| !h.name.is_empty()) {
                row.host_name = host.name.clone();
            }
            // The images themselves are the contimage intake's job.
            row.counts = counts(&[("taints", node.taints.len() as i64), ("images", status.images.len() as i64)]);
        }
        Kind::Namespace(namespace) => row.status = namespace.status.clone(),
        Kind::Service(service) => {
            let none = ServiceSpec::default();
            let spec = service.spec.as_ref().unwrap_or(&none);
            row.service_type = spec.r#type.clone();
            row.cluster_ip = spec.cluster_ip.clone();
            row.service_ports = json::proto::messages(&spec.ports);
            row.counts = counts(&[("ports", spec.ports.len() as i64)]);
        }
        Kind::Role(role) => {
            row.rbac_rules = json::proto::messages(&role.rules);
            row.counts = counts(&[("rules", role.rules.len() as i64)]);
        }
        Kind::ClusterRole(role) => {
            row.rbac_rules = json::proto::messages(&role.rules);
            row.counts = counts(&[
                ("rules", role.rules.len() as i64),
                ("aggregation_rules", role.aggregation_rules.len() as i64),
            ]);
        }
        Kind::RoleBinding(binding) => {
            row.rbac_subjects = json::proto::messages(&binding.subjects);
            row.rbac_role_ref = json::proto::message(binding.role_ref.as_ref());
            row.counts = counts(&[("subjects", binding.subjects.len() as i64)]);
        }
        Kind::ClusterRoleBinding(binding) => {
            row.rbac_subjects = json::proto::messages(&binding.subjects);
            row.rbac_role_ref = json::proto::message(binding.role_ref.as_ref());
            row.counts = counts(&[("subjects", binding.subjects.len() as i64)]);
        }
        Kind::Job(job) => {
            let (no_status, no_spec) = (JobStatus::default(), JobSpec::default());
            let status = job.status.as_ref().unwrap_or(&no_status);
            let spec = job.spec.as_ref().unwrap_or(&no_spec);
            row.counts = counts(&[
                ("active", i64::from(status.active)),
                ("succeeded", i64::from(status.succeeded)),
                ("failed", i64::from(status.failed)),
                ("parallelism", i64::from(spec.parallelism)),
                ("completions", i64::from(spec.completions)),
                ("backoff_limit", i64::from(spec.backoff_limit)),
                ("active_deadline_seconds", spec.active_deadline_seconds),
            ]);
            row.start_time = seconds(status.start_time);
        }
        Kind::CronJob(cron_job) => {
            let (no_status, no_spec) = (CronJobStatus::default(), CronJobSpec::default());
            let status = cron_job.status.as_ref().unwrap_or(&no_status);
            let spec = cron_job.spec.as_ref().unwrap_or(&no_spec);
            row.counts = counts(&[
                ("active_jobs", status.active.len() as i64),
                ("suspend", i64::from(text::flag(spec.suspend))),
                ("successful_jobs_history_limit", i64::from(spec.successful_jobs_history_limit)),
                ("failed_jobs_history_limit", i64::from(spec.failed_jobs_history_limit)),
                ("starting_deadline_seconds", spec.starting_deadline_seconds),
            ]);
        }
        Kind::HorizontalPodAutoscaler(autoscaler) => {
            let (no_status, no_spec) =
                (HorizontalPodAutoscalerStatus::default(), HorizontalPodAutoscalerSpec::default());
            let status = autoscaler.status.as_ref().unwrap_or(&no_status);
            let spec = autoscaler.spec.as_ref().unwrap_or(&no_spec);
            row.ready = status.current_replicas;
            row.desired = status.desired_replicas;
            row.counts = counts(&[
                ("min_replicas", i64::from(spec.min_replicas)),
                ("max_replicas", i64::from(spec.max_replicas)),
                ("current_replicas", i64::from(status.current_replicas)),
                ("desired_replicas", i64::from(status.desired_replicas)),
            ]);
        }
        Kind::PodDisruptionBudget(budget) => {
            let none = PodDisruptionBudgetStatus::default();
            let status = budget.status.as_ref().unwrap_or(&none);
            row.counts = counts(&[
                // The number a budget exists to produce: how many pods may be evicted right now.
                ("disruptions_allowed", i64::from(status.disruptions_allowed)),
                ("current_healthy", i64::from(status.current_healthy)),
                ("desired_healthy", i64::from(status.desired_healthy)),
                ("expected_pods", i64::from(status.expected_pods)),
                ("disrupted_pods", status.disrupted_pods.len() as i64),
            ]);
        }
        Kind::Other => {}
    }
}

/// The envelope of one collector message: what every row of the frame repeats.
struct Envelope<'a> {
    kind: &'a str,
    cluster_name: &'a str,
    cluster_id: &'a str,
    group_id: i32,
    group_size: i32,
    host_name: &'a str,
    tags: &'a [String],
    agent_version: Option<&'a AgentVersion>,
}

impl<'a> Envelope<'a> {
    fn new(
        kind: &'a str,
        cluster_name: &'a str,
        cluster_id: &'a str,
        group_id: i32,
        group_size: i32,
        tags: &'a [String],
        agent_version: Option<&'a AgentVersion>,
    ) -> Self {
        Envelope { kind, cluster_name, cluster_id, group_id, group_size, host_name: "", tags, agent_version }
    }
}

/// The rows of one collector message: the envelope's columns on every row, then each object's
/// own.
///
/// An object without Metadata is skipped: with no namespace, name or uid the row could never be
/// queried or joined.
fn rows_of<'a>(
    tenant: &str,
    now: DateTime<Utc>,
    frame: &FrameColumns,
    envelope: Envelope,
    // The message as JSON without its object list: the list is the rows.
    bare: String,
    objects: impl Iterator<Item = ObjectParts<'a>>,
) -> Vec<K8sResource> {
    // The same on every row of the frame.
    let shared = K8sResource {
        tenant_id: tenant.into(),
        // The sender leaves the frame's timestamp unset, so arrival is the collection time there
        // is.
        collected_at: now,
        cluster_id: envelope.cluster_id.into(),
        cluster_name: envelope.cluster_name.into(),
        kind: envelope.kind.into(),
        agent_version: agent_version_text(envelope.agent_version),
        group_id: envelope.group_id,
        group_size: envelope.group_size,
        org_id: frame.org_id,
        subscription_id: frame.subscription_id,
        header_timestamp: frame.timestamp,
        encoding: frame.encoding.clone(),
        host_name: envelope.host_name.into(),
        envelope: bare,
        ..Default::default()
    };

    let to_row = |parts: ObjectParts, metadata: &Metadata| {
        let owners = &metadata.owner_references;
        let mut row = K8sResource {
            namespace: metadata.namespace.clone(),
            name: metadata.name.clone(),
            uid: metadata.uid.clone(),
            resource_version: metadata.resource_version.clone(),
            // Kubernetes allows several owners but in practice writes one controller; the first
            // is the convenience column.
            owner_kind: owners.first().map(|o| o.kind.clone()).unwrap_or_default(),
            owner_name: owners.first().map(|o| o.name.clone()).unwrap_or_default(),
            owner_kinds: owners.iter().map(|o| o.kind.clone()).collect(),
            owner_names: owners.iter().map(|o| o.name.clone()).collect(),
            owner_uids: owners.iter().map(|o| o.uid.clone()).collect(),
            // Labels and annotations travel as "key:value" like tags, but Kubernetes forbids
            // duplicate keys.
            labels: tags::to_map(&metadata.labels),
            annotations: tags::to_map(&metadata.annotations),
            // The frame's tags apply to every object, the object's own on top.
            tags: tags::to_multi_map(envelope.tags.iter().chain(parts.tags)),
            creation_timestamp: seconds(metadata.creation_timestamp),
            deletion_timestamp: seconds(metadata.deletion_timestamp),
            // Only meaningful while the object is being deleted, and 0 is then a real value
            // (force delete), not absence.
            deletion_grace_period_seconds: (metadata.deletion_timestamp != 0)
                .then_some(metadata.deletion_grace_period_seconds),
            finalizers: metadata.finalizers.clone(),
            object: parts.object.clone(),
            ..shared.clone()
        };
        with_common_fields(&parts, &mut row);
        with_kind_fields(&parts.kind, &mut row);
        row
    };

    objects.filter_map(|parts| parts.metadata.map(|metadata| to_row(parts, metadata))).collect()
}

/// One row per object of a collector message; none for a message that is not a collection of
/// Kubernetes objects. Each arm takes the object list out of the message, so what remains of it
/// is the envelope.
pub fn resource_rows(tenant: &str, now: DateTime<Utc>, frame: &FrameColumns, body: Body) -> Vec<K8sResource> {
    match body {
        Body::Pod(mut c) => {
            let objects = std::mem::take(&mut c.pods);
            let envelope = Envelope {
                host_name: &c.host_name,
                ..Envelope::new(
                    "Pod",
                    &c.cluster_name,
                    &c.cluster_id,
                    c.group_id,
                    c.group_size,
                    &c.tags,
                    c.agent_version.as_ref(),
                )
            };
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(pod_parts))
        }
        Body::PodDisruptionBudget(mut c) => {
            let objects = std::mem::take(&mut c.pod_disruption_budgets);
            let envelope = Envelope::new(
                "PodDisruptionBudget",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(pod_disruption_budget_parts),
            )
        }
        Body::ReplicaSet(mut c) => {
            let objects = std::mem::take(&mut c.replica_sets);
            let envelope = Envelope::new(
                "ReplicaSet",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(replica_set_parts))
        }
        Body::Deployment(mut c) => {
            let objects = std::mem::take(&mut c.deployments);
            let envelope = Envelope::new(
                "Deployment",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(deployment_parts))
        }
        Body::Service(mut c) => {
            let objects = std::mem::take(&mut c.services);
            let envelope = Envelope::new(
                "Service",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(service_parts))
        }
        Body::Node(mut c) => {
            let objects = std::mem::take(&mut c.nodes);
            let envelope = Envelope::new(
                "Node",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(node_parts))
        }
        Body::Namespace(mut c) => {
            let objects = std::mem::take(&mut c.namespaces);
            let envelope = Envelope::new(
                "Namespace",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(namespace_parts))
        }
        Body::Job(mut c) => {
            let objects = std::mem::take(&mut c.jobs);
            let envelope = Envelope::new(
                "Job",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(job_parts))
        }
        Body::CronJob(mut c) => {
            let objects = std::mem::take(&mut c.cron_jobs);
            let envelope = Envelope::new(
                "CronJob",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(cron_job_parts))
        }
        Body::DaemonSet(mut c) => {
            let objects = std::mem::take(&mut c.daemon_sets);
            let envelope = Envelope::new(
                "DaemonSet",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(tenant, now, frame, envelope, json::proto::message(Some(&c)), objects.iter().map(daemon_set_parts))
        }
        Body::StatefulSet(mut c) => {
            let objects = std::mem::take(&mut c.stateful_sets);
            let envelope = Envelope::new(
                "StatefulSet",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(stateful_set_parts),
            )
        }
        Body::PersistentVolume(mut c) => {
            let objects = std::mem::take(&mut c.persistent_volumes);
            let envelope = Envelope::new(
                "PersistentVolume",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::PersistentVolumeClaim(mut c) => {
            let objects = std::mem::take(&mut c.persistent_volume_claims);
            let envelope = Envelope::new(
                "PersistentVolumeClaim",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(persistent_volume_claim_parts),
            )
        }
        Body::Role(mut c) => {
            let objects = std::mem::take(&mut c.roles);
            let envelope = Envelope::new(
                "Role",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Role(o), o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::RoleBinding(mut c) => {
            let objects = std::mem::take(&mut c.role_bindings);
            let envelope = Envelope::new(
                "RoleBinding",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::RoleBinding(o), o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::ClusterRole(mut c) => {
            let objects = std::mem::take(&mut c.cluster_roles);
            let envelope = Envelope::new(
                "ClusterRole",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(cluster_role_parts),
            )
        }
        Body::ClusterRoleBinding(mut c) => {
            let objects = std::mem::take(&mut c.cluster_role_bindings);
            let envelope = Envelope::new(
                "ClusterRoleBinding",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::ClusterRoleBinding(o), o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::ServiceAccount(mut c) => {
            let objects = std::mem::take(&mut c.service_accounts);
            let envelope = Envelope::new(
                "ServiceAccount",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::Ingress(mut c) => {
            let objects = std::mem::take(&mut c.ingresses);
            let envelope = Envelope::new(
                "Ingress",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::VerticalPodAutoscaler(mut c) => {
            let objects = std::mem::take(&mut c.vertical_pod_autoscalers);
            let envelope = Envelope::new(
                "VerticalPodAutoscaler",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(vertical_pod_autoscaler_parts),
            )
        }
        Body::HorizontalPodAutoscaler(mut c) => {
            let objects = std::mem::take(&mut c.horizontal_pod_autoscalers);
            let envelope = Envelope::new(
                "HorizontalPodAutoscaler",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(horizontal_pod_autoscaler_parts),
            )
        }
        Body::NetworkPolicy(mut c) => {
            let objects = std::mem::take(&mut c.network_policies);
            let envelope = Envelope::new(
                "NetworkPolicy",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::LimitRange(mut c) => {
            let objects = std::mem::take(&mut c.limit_ranges);
            let envelope = Envelope::new(
                "LimitRange",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        Body::StorageClass(mut c) => {
            let objects = std::mem::take(&mut c.storage_classes);
            let envelope = Envelope::new(
                "StorageClass",
                &c.cluster_name,
                &c.cluster_id,
                c.group_id,
                c.group_size,
                &c.tags,
                c.agent_version.as_ref(),
            );
            rows_of(
                tenant,
                now,
                frame,
                envelope,
                json::proto::message(Some(&c)),
                objects.iter().map(|o| plain_parts(Kind::Other, o, o.metadata.as_ref(), &o.tags)),
            )
        }
        _ => Vec::new(),
    }
}

// ---- manifests → k8s_manifests ----

/// One row per manifest, its content stored whole. `collector` is None for a CRD or CR frame
/// whose inner envelope the agent left out.
///
/// Tags come from three places: the frame envelope, the CRD/CR wrapper around it, and the
/// manifest itself. They are merged, because a query for env:prod does not care who attached the
/// tag, and tag_sources keeps who did.
pub fn manifest_rows(
    tenant: &str,
    now: DateTime<Utc>,
    frame: &FrameColumns,
    collector: Option<CollectorManifest>,
    wrapper_tags: &[String],
) -> Vec<K8sManifest> {
    let Some(mut collector) = collector else {
        return Vec::new();
    };
    // Without the manifest list: the list is the rows.
    let manifests = std::mem::take(&mut collector.manifests);
    let envelope = json::proto::message(Some(&collector));
    let envelope_tags = &collector.tags;

    let to_row = |manifest: Manifest| {
        let sources: [(&str, &[String]); 3] =
            [("envelope", envelope_tags), ("wrapper", wrapper_tags), ("manifest", &manifest.tags)];
        let tag_sources = sources
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(source, tags)| (source.to_string(), tags.to_vec()));
        K8sManifest {
            tenant_id: tenant.into(),
            collected_at: now,
            cluster_id: collector.cluster_id.clone(),
            cluster_name: collector.cluster_name.clone(),
            uid: manifest.uid.clone(),
            kind: manifest.kind.clone(),
            api_version: manifest.api_version.clone(),
            resource_version: manifest.resource_version.clone(),
            content_type: manifest.content_type.clone(),
            is_terminated: text::flag(manifest.is_terminated),
            org_id: frame.org_id,
            subscription_id: frame.subscription_id,
            header_timestamp: frame.timestamp,
            encoding: frame.encoding.clone(),
            group_id: collector.group_id,
            group_size: collector.group_size,
            host_name: collector.host_name.clone(),
            agent_version: agent_version_text(collector.agent_version.as_ref()),
            origin_collector: enum_name(collector.origin_collector, OriginCollector::as_str_name),
            tags: tags::to_multi_map(envelope_tags.iter().chain(wrapper_tags).chain(&manifest.tags)),
            tag_sources: tag_sources.collect(),
            node_name: manifest.node_name.clone(),
            r#type: manifest.r#type,
            version: manifest.version.clone(),
            extra_attributes: to_map(&manifest.extra_attributes),
            content_is_utf8: text::flag(std::str::from_utf8(&manifest.content).is_ok()),
            envelope: envelope.clone(),
            manifest_host: json::proto::message(manifest.host.as_ref()),
            content: Blob(manifest.content),
        }
    };

    manifests.into_iter().map(to_row).collect()
}

// ---- the cluster summary → k8s_cluster ----

/// The one row of a CollectorCluster, or none when the cluster itself is missing from it.
pub fn cluster_rows(
    tenant: &str,
    now: DateTime<Utc>,
    frame: &FrameColumns,
    collector: &CollectorCluster,
) -> Vec<K8sCluster> {
    let Some(cluster) = &collector.cluster else {
        return Vec::new();
    };
    let nodes = &cluster.nodes_info;
    let versions = |spread: &HashMap<String, i32>| version_spread(spread.iter().map(|(v, n)| (v.clone(), *n)));

    vec![K8sCluster {
        tenant_id: tenant.into(),
        collected_at: now,
        cluster_id: collector.cluster_id.clone(),
        cluster_name: collector.cluster_name.clone(),
        // A count cannot be negative, and cast as is it would be 4 billion nodes.
        node_count: cluster.node_count.max(0) as u32,
        pod_capacity: cluster.pod_capacity,
        pod_allocatable: cluster.pod_allocatable,
        cpu_capacity: cluster.cpu_capacity,
        cpu_allocatable: cluster.cpu_allocatable,
        memory_capacity: cluster.memory_capacity,
        memory_allocatable: cluster.memory_allocatable,
        kubelet_versions: versions(&cluster.kubelet_versions),
        apiserver_versions: versions(&cluster.api_server_versions),
        org_id: frame.org_id,
        subscription_id: frame.subscription_id,
        header_timestamp: frame.timestamp,
        encoding: frame.encoding.clone(),
        group_id: collector.group_id,
        group_size: collector.group_size,
        agent_version: agent_version_text(collector.agent_version.as_ref()),
        // Not merged: the frame's tags and the cluster object's own are different fields, and
        // who attached a tag is worth keeping.
        tags: tags::to_multi_map(&collector.tags),
        cluster_tags: tags::to_multi_map(&cluster.tags),
        resource_version: cluster.resource_version.clone(),
        creation_timestamp: seconds(cluster.creation_timestamp),
        metrics: metric_values(cluster.metrics.as_ref()),
        extended_resources_capacity: to_map(&cluster.extended_resources_capacity),
        extended_resources_allocatable: to_map(&cluster.extended_resources_allocatable),
        nodes_name: nodes.iter().map(|n| n.name.clone()).collect(),
        nodes_region: nodes.iter().map(|n| n.region.clone()).collect(),
        nodes_instance_type: nodes.iter().map(|n| n.instance_type.clone()).collect(),
        nodes_os: nodes.iter().map(|n| n.operating_system.clone()).collect(),
        nodes_os_image: nodes.iter().map(|n| n.operating_system_image.clone()).collect(),
        nodes_architecture: nodes.iter().map(|n| n.architecture.clone()).collect(),
        nodes_kernel_version: nodes.iter().map(|n| n.kernel_version.clone()).collect(),
        nodes_container_runtime_version: nodes.iter().map(|n| n.container_runtime_version.clone()).collect(),
        nodes_kubelet_version: nodes.iter().map(|n| n.kubelet_version.clone()).collect(),
        nodes_allocatable: nodes.iter().map(|n| to_map(&n.resource_allocatable)).collect(),
        nodes_capacity: nodes.iter().map(|n| to_map(&n.resource_capacity)).collect(),
    }]
}

// ---- ECS tasks → ecs_tasks ----

/// One row per task. The ARN is the identity: a task has no Kubernetes metadata.
pub fn ecs_task_rows(
    tenant: &str,
    now: DateTime<Utc>,
    frame: &FrameColumns,
    mut collector: CollectorEcsTask,
) -> Vec<EcsTaskRow> {
    let envelope_tags = tags::to_multi_map(&collector.tags);
    // The envelope has a Host and an Info of its own (the machine and agent that reported the
    // pass) and no column holds either.
    let tasks = std::mem::take(&mut collector.tasks);
    let envelope = json::proto::message(Some(&collector));

    let to_row = |task: &EcsTask| EcsTaskRow {
        tenant_id: tenant.into(),
        collected_at: now,
        org_id: frame.org_id,
        subscription_id: frame.subscription_id,
        header_timestamp: frame.timestamp,
        encoding: frame.encoding.clone(),
        aws_account_id: collector.aws_account_id,
        cluster_id: collector.cluster_id.clone(),
        cluster_name: collector.cluster_name.clone(),
        region: collector.region.clone(),
        group_id: collector.group_id,
        group_size: collector.group_size,
        host_name: collector.host_name.clone(),
        agent_version: agent_version_text(collector.agent_version.as_ref()),
        envelope_tags: envelope_tags.clone(),
        arn: task.arn.clone(),
        resource_version: task.resource_version.clone(),
        launch_type: task.launch_type.clone(),
        desired_status: task.desired_status.clone(),
        known_status: task.known_status.clone(),
        family: task.family.clone(),
        version: task.version.clone(),
        availability_zone: task.availability_zone.clone(),
        service_name: task.service_name.clone(),
        vpc_id: task.vpc_id.clone(),
        container_instance_arn: task.container_instance_arn.clone(),
        daemon_name: task.daemon_name.clone(),
        task_host_name: task.host.as_ref().map(|h| h.name.clone()).unwrap_or_default(),
        limits: to_map(&task.limits),
        ephemeral_storage_metrics: to_map(&task.ephemeral_storage_metrics),
        // A task still pulling has no pull_stopped_at.
        pull_started_at: seconds(task.pull_started_at),
        pull_stopped_at: seconds(task.pull_stopped_at),
        execution_stopped_at: seconds(task.execution_stopped_at),
        containers: json::proto::messages(&task.containers),
        container_count: task.containers.len() as u32,
        tags: tags::to_multi_map(&task.tags),
        ecs_tags: tags::to_multi_map(&task.ecs_tags),
        container_instance_tags: tags::to_multi_map(&task.container_instance_tags),
        envelope: envelope.clone(),
        task_host: json::proto::message(task.host.as_ref()),
    };

    tasks.iter().map(to_row).collect()
}

// ---- the orchestrator handlers ----

pub async fn handle_frame(label: &str, ctx: Ctx, body: server::Body) -> Response {
    let tenant = ctx.tenant();
    match process_frame::decode(&body) {
        Err(e) => ctx.raw("orchestrator", "decode_error", &format!("{label}: {e}"), &body),
        Ok(decoded) => {
            if std::env::var("NINJACAT_DUMP_K8S").is_ok_and(|v| v == "true") {
                // The whole message: how to find out what a real cluster sends.
                tracing::info!("[{label}] {:?}", decoded.body);
            }
            let frame = frame_columns(&decoded.header);
            let now = Utc::now();
            match decoded.body {
                Body::Cluster(collector) => ctx.write(cluster_rows(tenant, now, &frame, &collector)),
                Body::Manifest(collector) => ctx.write(manifest_rows(tenant, now, &frame, Some(collector), &[])),
                // The CRD and CR wrappers hold an inner CollectorManifest that may be missing,
                // and tags of their own beside the inner one's.
                Body::ManifestCrd(wrapper) => {
                    ctx.write(manifest_rows(tenant, now, &frame, wrapper.manifest, &wrapper.tags))
                }
                Body::ManifestCr(wrapper) => {
                    ctx.write(manifest_rows(tenant, now, &frame, wrapper.manifest, &wrapper.tags))
                }
                Body::EcsTask(collector) => ctx.write(ecs_task_rows(tenant, now, &frame, collector)),
                // The process agent's own messages belong on /api/v1/collector. Here they mean a
                // misconfigured agent: the frame decoded, but this router has nowhere to put it.
                other @ (Body::Proc(_)
                | Body::Connections(_)
                | Body::Res(_)
                | Body::RealTime(_)
                | Body::Container(_)
                | Body::ContainerRealTime(_)
                | Body::ProcDiscovery(_)
                | Body::ProcEvent(_)) => ctx.raw(
                    "orchestrator",
                    "unexpected_shape",
                    &format!("{label}: {} on an orchestrator intake", other.name()),
                    &body,
                ),
                collection => ctx.write(resource_rows(tenant, now, &frame, collection)),
            }
        }
    }
    accepted()
}

// ---- kubeactions → k8s_actions ----

/// One report on a remote action, as the cluster agent's ActionResultEvent
/// (pkg/clusteragent/kubeactions/reporter.go) sends it.
///
/// event_type is action_received, action_progress or action_executed; status is success, failed,
/// skipped, expired, claimed or in_progress; timestamp is RFC 3339, kept as the text it came as.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionEvent {
    pub action_id: String,
    pub org_id: i64,
    pub event_type: String,
    pub status: String,
    pub action_type: String,
    pub cluster_id: String,
    pub resource_id: String,
    pub requested_by: String,
    pub timestamp: String,
    pub message: String,
    /// What the executor attached, base64 on the wire.
    pub payloads: BTreeMap<String, Vec<u8>>,
    pub cluster_name: String,
    pub resource_kind: String,
    pub resource_name: String,
    pub resource_namespace: String,
    /// Keys the event carried that are not among the known ones, so a newer agent loses nothing
    /// here.
    pub extra: BTreeMap<String, Value>,
}

/// The JSON keys of an ActionEvent's own fields.
pub const ACTION_KNOWN_KEYS: [&str; 15] = [
    "action_id",
    "org_id",
    "event_type",
    "status",
    "action_type",
    "cluster_id",
    "resource_id",
    "requested_by",
    "timestamp",
    "message",
    "payloads",
    "cluster_name",
    "resource_kind",
    "resource_name",
    "resource_namespace",
];

/// Base64 with padding, in the standard alphabet; line breaks are allowed and nothing else.
fn try_base64(text: &str) -> Option<Vec<u8>> {
    let joined = text.replace(['\r', '\n'], "");
    base64::engine::general_purpose::STANDARD.decode(joined).ok()
}

/// `payloads`: a name → base64 map. A null value is an empty attachment.
fn attachments(payloads: &Fields) -> BTreeMap<String, Vec<u8>> {
    let mut attached = BTreeMap::new();
    for (name, value) in payloads.members() {
        match &value {
            Value::Null => {
                attached.insert(name, Vec::new());
            }
            Value::String(text) => match try_base64(text) {
                Some(bytes) => {
                    attached.insert(name, bytes);
                }
                None => payloads.invalid(&name, "not base64"),
            },
            other => payloads.refuse(&name, "base64 text", other),
        }
    }
    attached
}

/// One event, `path` being its place in the batch. A member of the wrong type refuses the whole
/// event; a member that is not a known one is kept in extra.
pub fn decode_action(path: &str, raw: &Value) -> Result<ActionEvent, String> {
    let bad = Mismatches::default();
    let fields = fields::fields(&bad, path, raw);
    let known: BTreeSet<&str> = ACTION_KNOWN_KEYS.into_iter().collect();

    let event = ActionEvent {
        action_id: fields.string("action_id"),
        org_id: fields.int64("org_id"),
        event_type: fields.string("event_type"),
        status: fields.string("status"),
        action_type: fields.string("action_type"),
        cluster_id: fields.string("cluster_id"),
        resource_id: fields.string("resource_id"),
        requested_by: fields.string("requested_by"),
        timestamp: fields.string("timestamp"),
        message: fields.string("message"),
        payloads: attachments(&fields.object("payloads")),
        cluster_name: fields.string("cluster_name"),
        resource_kind: fields.string("resource_kind"),
        resource_name: fields.string("resource_name"),
        resource_namespace: fields.string("resource_namespace"),
        extra: json::members(raw).into_iter().filter(|(name, _)| !known.contains(name.as_str())).collect(),
    };

    match bad.into_inner().into_iter().next() {
        Some(first) => Err(first),
        None => Ok(event),
    }
}

/// An empty or unparseable timestamp means "not provided": the row gets the arrival time, never
/// 1970, and timestamp_raw keeps what was sent.
pub fn action_row(tenant: &str, arrival: DateTime<Utc>, event: ActionEvent) -> K8sAction {
    K8sAction {
        tenant_id: tenant.into(),
        timestamp: time::try_rfc3339(&event.timestamp).unwrap_or(arrival),
        action_id: event.action_id,
        org_id: event.org_id,
        event_type: event.event_type,
        status: event.status,
        action_type: event.action_type,
        cluster_id: event.cluster_id,
        cluster_name: event.cluster_name,
        resource_id: event.resource_id,
        resource_kind: event.resource_kind,
        resource_name: event.resource_name,
        resource_namespace: event.resource_namespace,
        requested_by: event.requested_by,
        message: event.message,
        // A column of their own, sorted, so filtering on a name needs no JSON parsing.
        extra_keys: event.extra.keys().cloned().collect(),
        payloads: event.payloads.into_iter().map(|(name, bytes)| (name, Blob(bytes))).collect(),
        extra: if event.extra.is_empty() { String::new() } else { json::compact_object(&event.extra) },
        timestamp_raw: event.timestamp,
    }
}

/// The cluster agent's reports on the actions it ran: the other half of remote configuration. One
/// event when an action is received, one when it is executed. Event platform track
/// "kubeactions": a JSON array of events.
pub async fn handle_actions(ctx: Ctx, body: server::Body) -> Response {
    match json::try_parse_array(&body) {
        Err(e) => ctx.raw("kubeactions", "decode_error", &e, &body),
        Ok(items) => {
            let text = json::raw::text(&body);
            let sent = json::raw::elements(&text);
            let mut events = Vec::new();
            for (i, raw) in items.iter().enumerate() {
                match decode_action(&format!("[{i}]"), raw) {
                    Ok(event) => events.push(event),
                    // One bad element must not cost the batch, nor itself: its own JSON is kept,
                    // as it was sent, while the rest become rows.
                    Err(e) => {
                        ctx.raw("kubeactions", "decode_error", &e, sent.get(i).copied().unwrap_or_default().as_bytes())
                    }
                }
            }
            let arrival = Utc::now();
            ctx.write(events.into_iter().map(|event| action_row(ctx.tenant(), arrival, event)).collect());
        }
    }
    accepted()
}

/// One converter serves some 24 kinds of object, so these pin what it must keep for all of them:
/// the kind comes from the message, identity from Metadata, and the envelope lands on every row.
#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 21, 12, 0, 0).unwrap()
    }

    fn at(seconds: i64) -> Option<DateTime<Utc>> {
        Some(Utc.timestamp_opt(seconds, 0).unwrap())
    }

    /// A header with every field set, so a test can tell "the converter copied the header" from
    /// "the header happened to be zero".
    fn frame() -> FrameColumns {
        FrameColumns { org_id: 77, subscription_id: 3, timestamp: Some(1700000000), encoding: "protobuf".into() }
    }

    fn metadata(namespace: &str, name: &str, uid: &str) -> Option<Metadata> {
        Some(Metadata { namespace: namespace.into(), name: name.into(), uid: uid.into(), ..Default::default() })
    }

    fn same_json(expected: &str, actual: &str) {
        let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap();
        assert_eq!(parse(expected), parse(actual), "{actual}");
    }

    fn rows(body: Body) -> Vec<K8sResource> {
        resource_rows("t", now(), &frame(), body)
    }

    fn row(body: Body) -> K8sResource {
        let mut all = rows(body);
        assert_eq!(all.len(), 1);
        all.remove(0)
    }

    fn pods(pods: Vec<Pod>) -> Vec<K8sResource> {
        rows(Body::Pod(CollectorPod { pods, ..Default::default() }))
    }

    fn pod_row(pod: Pod) -> K8sResource {
        row(Body::Pod(CollectorPod { pods: vec![pod], ..Default::default() }))
    }

    fn decoded(text: &str) -> ActionEvent {
        decode_action("", &serde_json::from_str(text).unwrap()).unwrap()
    }

    fn refused(text: &str) -> bool {
        decode_action("", &serde_json::from_str(text).unwrap()).is_err()
    }

    fn map<V: Clone>(pairs: &[(&str, V)]) -> BTreeMap<String, V> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn agent_version(major: i64, minor: i64, patch: i64, pre: &str) -> Option<AgentVersion> {
        Some(AgentVersion { major, minor, patch, pre: pre.into(), ..Default::default() })
    }

    // ---- object collections ----

    #[test]
    fn a_pod_row_has_its_kind_identity_and_first_owner_and_the_envelope_of_its_frame() {
        let mut meta =
            Metadata { resource_version: "42".into(), ..metadata("default", "web-abc12", "uid-pod").unwrap() };
        meta.owner_references.push(OwnerReference {
            kind: "ReplicaSet".into(),
            name: "web".into(),
            ..Default::default()
        });
        let collector = CollectorPod {
            cluster_name: "prod".into(),
            cluster_id: "cid-1".into(),
            group_id: 3,
            group_size: 7,
            pods: vec![Pod { metadata: Some(meta), ..Default::default() }],
            ..Default::default()
        };
        let mut all = resource_rows("tenant-1", now(), &frame(), Body::Pod(collector));
        let row = all.remove(0);

        assert_eq!(row.kind, "Pod");
        assert_eq!((row.namespace.as_str(), row.name.as_str(), row.uid.as_str()), ("default", "web-abc12", "uid-pod"));
        assert_eq!((row.owner_kind.as_str(), row.owner_name.as_str()), ("ReplicaSet", "web"));
        assert_eq!(row.resource_version, "42");
        assert_eq!(
            (row.tenant_id.as_str(), row.cluster_name.as_str(), row.cluster_id.as_str()),
            ("tenant-1", "prod", "cid-1")
        );
        assert_eq!((row.group_id, row.group_size), (3, 7));
        assert_eq!(row.collected_at, now());
    }

    #[test]
    fn a_deployment_row_has_its_kind_and_identity_and_no_owner_when_it_has_none() {
        let collector = CollectorDeployment {
            cluster_name: "prod".into(),
            cluster_id: "cid-1".into(),
            group_id: 3,
            group_size: 7,
            deployments: vec![Deployment { metadata: metadata("shop", "checkout", "uid-dep"), ..Default::default() }],
            ..Default::default()
        };
        let row = row(Body::Deployment(collector));
        assert_eq!(row.kind, "Deployment");
        assert_eq!((row.namespace.as_str(), row.name.as_str(), row.uid.as_str()), ("shop", "checkout", "uid-dep"));
        assert_eq!((row.owner_kind.as_str(), row.owner_name.as_str()), ("", ""));
        assert!(row.owner_kinds.is_empty());
        assert_eq!(
            (row.cluster_name.as_str(), row.cluster_id.as_str(), row.group_id, row.group_size),
            ("prod", "cid-1", 3, 7)
        );
    }

    #[test]
    fn pod_numbers_are_sums_over_its_container_statuses() {
        let status = |name: &str, ready, restart_count| ContainerStatus {
            name: name.into(),
            ready,
            restart_count,
            ..Default::default()
        };
        let row = pod_row(Pod {
            metadata: metadata("ns", "p", "u"),
            node_name: "node-1".into(),
            phase: "Running".into(),
            status: "CrashLoopBackOff".into(),
            container_statuses: vec![status("app", true, 4), status("sidecar", true, 1), status("init-ish", false, 0)],
            ..Default::default()
        });
        assert_eq!(
            (row.node_name.as_str(), row.phase.as_str(), row.status.as_str()),
            ("node-1", "Running", "CrashLoopBackOff")
        );
        assert_eq!((row.ready, row.desired), (2, 3));
        assert_eq!(
            row.counts,
            map(&[
                ("restarts", 5),
                ("ready_containers", 2),
                ("total_containers", 3),
                ("init_restarts", 0),
                ("ready_init_containers", 0),
                ("total_init_containers", 0),
                ("pod_restart_count", 0)
            ])
        );
    }

    #[test]
    fn deployment_numbers_come_from_its_own_fields_under_stable_keys() {
        let deployment = Deployment {
            metadata: metadata("ns", "d", "u"),
            replicas_desired: 5,
            ready_replicas: 3,
            updated_replicas: 4,
            available_replicas: 3,
            unavailable_replicas: 2,
            ..Default::default()
        };
        let row = row(Body::Deployment(CollectorDeployment { deployments: vec![deployment], ..Default::default() }));
        assert_eq!((row.ready, row.desired, row.available), (3, 5, 3));
        assert_eq!(
            row.counts,
            map(&[("ready", 3), ("desired", 5), ("updated", 4), ("available", 3), ("unavailable", 2), ("replicas", 0)])
        );
    }

    #[test]
    fn an_object_without_metadata_has_no_identity_and_gets_no_row() {
        let rows = pods(vec![
            Pod { node_name: "metadata omitted on the wire".into(), ..Default::default() },
            Pod { metadata: metadata("ns", "ok", "u"), ..Default::default() },
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "ok");
    }

    #[test]
    fn every_row_carries_the_frame_header_and_an_unset_header_timestamp_is_none() {
        let row = pod_row(Pod { metadata: metadata("", "p", "u"), ..Default::default() });
        assert_eq!((row.org_id, row.subscription_id, row.encoding.as_str()), (77, 3, "protobuf"));
        // The raw integer off the wire: the frame format gives it no unit.
        assert_eq!(row.header_timestamp, Some(1700000000));
        let unset = Header { version: 3, message_type: 41, ..Default::default() };
        assert_eq!(frame_columns(&unset).timestamp, None);
    }

    #[test]
    fn a_frame_encoding_is_stored_by_name_an_unknown_one_by_number() {
        let names: Vec<String> = (0..=5).map(encoding_name).collect();
        assert_eq!(names, ["protobuf", "json", "zstd_protobuf", "unknown_3", "zstd1x_protobuf", "zstd_protobuf_nocgo"]);
    }

    #[test]
    fn metadata_reaches_the_row_in_full_dates_finalizers_and_every_owner() {
        let owner = |kind: &str, name: &str, uid: &str| OwnerReference {
            kind: kind.into(),
            name: name.into(),
            uid: uid.into(),
        };
        let meta = Metadata {
            name: "web-abc".into(),
            uid: "uid-1".into(),
            creation_timestamp: 1600000000,
            deletion_timestamp: 1600003600,
            deletion_grace_period_seconds: 30,
            finalizers: vec!["kubernetes.io/pvc-protection".into()],
            owner_references: vec![owner("ReplicaSet", "web", "uid-rs"), owner("Foo", "bar", "uid-foo")],
            ..Default::default()
        };
        let row = pod_row(Pod { metadata: Some(meta), ..Default::default() });

        assert_eq!((row.creation_timestamp, row.deletion_timestamp), (at(1600000000), at(1600003600)));
        assert_eq!(row.deletion_grace_period_seconds, Some(30));
        assert_eq!(row.finalizers, ["kubernetes.io/pvc-protection"]);
        assert_eq!((row.owner_kind.as_str(), row.owner_name.as_str()), ("ReplicaSet", "web"));
        assert_eq!(row.owner_kinds, ["ReplicaSet", "Foo"]);
        assert_eq!(row.owner_names, ["web", "bar"]);
        // The uid is the only thing a join can use.
        assert_eq!(row.owner_uids, ["uid-rs", "uid-foo"]);
    }

    #[test]
    fn a_grace_period_exists_only_while_an_object_is_being_deleted() {
        let live = pod_row(Pod { metadata: metadata("", "p", "u"), ..Default::default() });
        assert_eq!(
            (live.deletion_grace_period_seconds, live.deletion_timestamp, live.creation_timestamp),
            (None, None, None)
        );

        // For an object that IS being deleted, 0 is a force delete: a real value.
        let meta = Metadata { deletion_timestamp: 1600003600, ..metadata("", "p", "u").unwrap() };
        assert_eq!(pod_row(Pod { metadata: Some(meta), ..Default::default() }).deletion_grace_period_seconds, Some(0));
    }

    #[test]
    fn labels_are_single_valued_tags_are_a_multiset_of_the_frames_and_the_objects() {
        let meta = Metadata {
            labels: vec!["app:web".into(), "app:api".into(), "tier".into()],
            annotations: vec!["note:a:b".into()],
            ..metadata("", "p", "u").unwrap()
        };
        let pod = Pod { metadata: Some(meta), tags: vec!["env:stage".into()], ..Default::default() };
        let collector =
            CollectorPod { tags: vec!["env:prod".into(), "team".into()], pods: vec![pod], ..Default::default() };
        let row = row(Body::Pod(collector));

        assert_eq!(row.labels, map(&[("app", "api".to_string()), ("tier", String::new())]));
        assert_eq!(row.annotations, map(&[("note", "a:b".to_string())]));
        assert_eq!(
            row.tags,
            map(&[("env", vec!["prod".to_string(), "stage".to_string()]), ("team", vec![String::new()])])
        );
    }

    #[test]
    fn a_pod_keeps_each_containers_status_regular_containers_first_then_init_containers() {
        let mut requirements = ResourceRequirements { name: "app".into(), ..Default::default() };
        requirements.requests.insert("cpu".into(), 100);
        requirements.limits.insert("memory".into(), 536870912);
        let mut metrics = ResourceMetrics::default();
        metrics.metric_values.insert("cpu.usage".into(), 0.42);
        let pod = Pod {
            metadata: metadata("shop", "checkout-1", "u"),
            ip: "10.4.2.9".into(),
            nominated_node_name: "node-9".into(),
            qos_class: "Burstable".into(),
            priority_class: "high".into(),
            start_time: 1600000100,
            scheduled_time: 1600000050,
            host: Some(Host { name: "cluster-agent-1".into(), ..Default::default() }),
            metrics: Some(metrics),
            container_statuses: vec![
                ContainerStatus {
                    name: "app".into(),
                    container_id: "containerd://aaa".into(),
                    ready: true,
                    state: "running".into(),
                    image: "shop/app:1.2".into(),
                    image_id: "sha256:aaa".into(),
                    ..Default::default()
                },
                ContainerStatus {
                    name: "sidecar".into(),
                    container_id: "containerd://bbb".into(),
                    restart_count: 7,
                    state: "waiting".into(),
                    message: "back-off 5m0s restarting failed container".into(),
                    image: "envoy:1.29".into(),
                    image_id: "sha256:bbb".into(),
                    ..Default::default()
                },
            ],
            init_container_statuses: vec![ContainerStatus {
                name: "wait-for-db".into(),
                container_id: "containerd://ccc".into(),
                ready: true,
                state: "terminated".into(),
                ..Default::default()
            }],
            conditions: vec![PodCondition {
                r#type: "Ready".into(),
                status: "False".into(),
                reason: "ContainersNotReady".into(),
                message: "containers with unready status: [sidecar]".into(),
                last_transition_time: 1600000200,
                last_probe_time: 1600000300,
            }],
            resource_requirements: vec![requirements],
            ..Default::default()
        };
        let row = pod_row(pod);

        assert_eq!(
            (
                row.pod_ip.as_str(),
                row.nominated_node_name.as_str(),
                row.qos_class.as_str(),
                row.priority_class.as_str()
            ),
            ("10.4.2.9", "node-9", "Burstable", "high")
        );
        assert_eq!((row.start_time, row.scheduled_time), (at(1600000100), at(1600000050)));
        // The reporting host, which the pod names itself.
        assert_eq!(row.host_name, "cluster-agent-1");
        assert_eq!(row.container_names, ["app", "sidecar", "wait-for-db"]);
        assert_eq!(row.container_ids, ["containerd://aaa", "containerd://bbb", "containerd://ccc"]);
        assert_eq!(row.container_is_init, [0, 0, 1]);
        assert_eq!(row.container_ready, [1, 0, 1]);
        assert_eq!(row.container_restarts, [0, 7, 0]);
        assert_eq!(row.container_states, ["running", "waiting", "terminated"]);
        // The line an operator is after.
        assert_eq!(row.container_messages[1], "back-off 5m0s restarting failed container");
        assert_eq!(
            (row.container_images[1].as_str(), row.container_image_ids[1].as_str()),
            ("envoy:1.29", "sha256:bbb")
        );
        assert_eq!(row.counts["ready_init_containers"], 1);
        assert_eq!(row.condition_types, ["Ready"]);
        assert_eq!(row.condition_statuses, ["False"]);
        assert_eq!(row.condition_reasons, ["ContainersNotReady"]);
        assert_eq!(row.condition_messages, ["containers with unready status: [sidecar]"]);
        assert_eq!(row.condition_last_transition, [at(1600000200)]);
        assert_eq!(row.condition_last_probe, [at(1600000300)]);
        // A pod condition has no lastUpdateTime: the slot is None, not a made-up date.
        assert_eq!(row.condition_last_update, [None]);
        same_json(
            r#"[{"limits":{"memory":"536870912"},"requests":{"cpu":"100"},"name":"app"}]"#,
            &row.resource_requirements,
        );
        assert_eq!(row.metrics, map(&[("cpu.usage", 0.42)]));
    }

    #[test]
    fn a_node_keeps_its_cordon_flag_taints_capacity_versions_and_the_conditions_nested_in_its_status() {
        let mut status = NodeStatus {
            status: "Ready,SchedulingDisabled".into(),
            kubelet_version: "v1.29.4".into(),
            kube_proxy_version: "v1.29.4".into(),
            operating_system: "linux".into(),
            architecture: "amd64".into(),
            kernel_version: "6.1.0".into(),
            os_image: "Amazon Linux 2".into(),
            container_runtime_version: "containerd://1.7.13".into(),
            conditions: vec![NodeCondition {
                r#type: "MemoryPressure".into(),
                status: "False".into(),
                reason: "KubeletHasSufficientMemory".into(),
                last_transition_time: 1600000500,
                ..Default::default()
            }],
            ..Default::default()
        };
        status.capacity.insert("cpu".into(), 8000);
        status.capacity.insert("memory".into(), 33554432000);
        status.allocatable.insert("cpu".into(), 7800);
        status.node_addresses.insert("InternalIP".into(), "10.0.1.7".into());
        let node = Node {
            metadata: metadata("", "node-1", "u"),
            pod_cidr: "10.244.1.0/24".into(),
            pod_cid_rs: vec!["10.244.1.0/24".into()],
            unschedulable: true,
            provider_id: "aws:///eu-west-1a/i-0abc".into(),
            roles: vec!["worker".into()],
            taints: vec![Taint {
                key: "node.kubernetes.io/unreachable".into(),
                effect: "NoExecute".into(),
                time_added: 1600000000,
                ..Default::default()
            }],
            status: Some(status),
            ..Default::default()
        };
        let row = row(Body::Node(CollectorNode { nodes: vec![node], ..Default::default() }));

        assert_eq!(
            (row.kind.as_str(), row.status.as_str(), row.unschedulable),
            ("Node", "Ready,SchedulingDisabled", 1)
        );
        assert_eq!(row.pod_cidr, "10.244.1.0/24");
        assert_eq!(row.pod_cidrs, ["10.244.1.0/24"]);
        assert_eq!(row.provider_id, "aws:///eu-west-1a/i-0abc");
        assert_eq!(row.node_roles, ["worker"]);
        same_json(
            r#"[{"key":"node.kubernetes.io/unreachable","effect":"NoExecute","timeAdded":"1600000000"}]"#,
            &row.taints,
        );
        assert_eq!(row.capacity, map(&[("cpu", 8000), ("memory", 33554432000)]));
        assert_eq!(row.allocatable, map(&[("cpu", 7800)]));
        assert_eq!(row.node_addresses, r#"{"InternalIP":"10.0.1.7"}"#);
        assert_eq!((row.kubelet_version.as_str(), row.kube_proxy_version.as_str()), ("v1.29.4", "v1.29.4"));
        assert_eq!(
            (row.operating_system.as_str(), row.architecture.as_str(), row.kernel_version.as_str()),
            ("linux", "amd64", "6.1.0")
        );
        assert_eq!(
            (row.os_image.as_str(), row.container_runtime_version.as_str()),
            ("Amazon Linux 2", "containerd://1.7.13")
        );
        assert_eq!(row.condition_types, ["MemoryPressure"]);
        assert_eq!(row.condition_last_transition, [at(1600000500)]);
        assert_eq!(row.counts, map(&[("taints", 1), ("images", 0)]));
    }

    #[test]
    fn an_autoscaler_keeps_its_replica_numbers_and_its_conditions_under_their_other_field_names() {
        let autoscaler = HorizontalPodAutoscaler {
            metadata: metadata("shop", "checkout", "u"),
            spec: Some(HorizontalPodAutoscalerSpec { min_replicas: 2, max_replicas: 20, ..Default::default() }),
            status: Some(HorizontalPodAutoscalerStatus {
                current_replicas: 6,
                desired_replicas: 11,
                ..Default::default()
            }),
            conditions: vec![HorizontalPodAutoscalerCondition {
                condition_type: "ScalingLimited".into(),
                condition_status: "True".into(),
                reason: "TooManyReplicas".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let row = row(Body::HorizontalPodAutoscaler(CollectorHorizontalPodAutoscaler {
            horizontal_pod_autoscalers: vec![autoscaler],
            ..Default::default()
        }));
        assert_eq!(row.kind, "HorizontalPodAutoscaler");
        assert_eq!((row.ready, row.desired), (6, 11));
        assert_eq!(
            row.counts,
            map(&[("min_replicas", 2), ("max_replicas", 20), ("current_replicas", 6), ("desired_replicas", 11)])
        );
        assert_eq!(row.condition_types, ["ScalingLimited"]);
        assert_eq!(row.condition_statuses, ["True"]);
        assert_eq!(row.condition_reasons, ["TooManyReplicas"]);
    }

    #[test]
    fn each_workload_kind_stores_its_own_numbers_under_stable_keys() {
        let meta = || metadata("ns", "x", "u");

        let replica_set = row(Body::ReplicaSet(CollectorReplicaSet {
            replica_sets: vec![ReplicaSet {
                metadata: meta(),
                replicas_desired: 5,
                replicas: 4,
                fully_labeled_replicas: 4,
                ready_replicas: 3,
                available_replicas: 2,
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!((replica_set.ready, replica_set.desired, replica_set.available), (3, 5, 2));
        assert_eq!(
            replica_set.counts,
            map(&[("ready", 3), ("desired", 5), ("available", 2), ("replicas", 4), ("fully_labeled", 4)])
        );

        let daemon_set = row(Body::DaemonSet(CollectorDaemonSet {
            daemon_sets: vec![DaemonSet {
                metadata: meta(),
                status: Some(DaemonSetStatus {
                    current_number_scheduled: 6,
                    number_misscheduled: 1,
                    desired_number_scheduled: 7,
                    number_ready: 5,
                    updated_number_scheduled: 4,
                    number_available: 3,
                    number_unavailable: 2,
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!((daemon_set.ready, daemon_set.desired, daemon_set.available), (5, 7, 3));
        assert_eq!(
            daemon_set.counts,
            map(&[
                ("ready", 5),
                ("desired", 7),
                ("current", 6),
                ("updated", 4),
                ("available", 3),
                ("misscheduled", 1),
                ("unavailable", 2)
            ])
        );

        // A stateful set's desired count is in its spec, not its status.
        let stateful_set = row(Body::StatefulSet(CollectorStatefulSet {
            stateful_sets: vec![StatefulSet {
                metadata: meta(),
                spec: Some(StatefulSetSpec { desired_replicas: 3, partition: 1, ..Default::default() }),
                status: Some(StatefulSetStatus {
                    replicas: 3,
                    ready_replicas: 2,
                    current_replicas: 2,
                    updated_replicas: 1,
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!((stateful_set.ready, stateful_set.desired, stateful_set.available), (2, 3, 0));
        assert_eq!(
            stateful_set.counts,
            map(&[("ready", 2), ("desired", 3), ("current", 2), ("updated", 1), ("replicas", 3), ("partition", 1)])
        );

        let job = row(Body::Job(CollectorJob {
            jobs: vec![Job {
                metadata: meta(),
                spec: Some(JobSpec {
                    parallelism: 2,
                    completions: 4,
                    backoff_limit: 6,
                    active_deadline_seconds: 600,
                    ..Default::default()
                }),
                status: Some(JobStatus {
                    active: 1,
                    succeeded: 3,
                    failed: 1,
                    start_time: 1600000000,
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!(job.start_time, at(1600000000));
        assert_eq!(
            job.counts,
            map(&[
                ("active", 1),
                ("succeeded", 3),
                ("failed", 1),
                ("parallelism", 2),
                ("completions", 4),
                ("backoff_limit", 6),
                ("active_deadline_seconds", 600)
            ])
        );

        let cron_job = row(Body::CronJob(CollectorCronJob {
            cron_jobs: vec![CronJob {
                metadata: meta(),
                spec: Some(CronJobSpec {
                    suspend: true,
                    successful_jobs_history_limit: 3,
                    failed_jobs_history_limit: 1,
                    ..Default::default()
                }),
                status: Some(CronJobStatus {
                    active: vec![ObjectReference { name: "run-1".into(), ..Default::default() }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!(
            cron_job.counts,
            map(&[
                ("active_jobs", 1),
                ("suspend", 1),
                ("successful_jobs_history_limit", 3),
                ("failed_jobs_history_limit", 1),
                ("starting_deadline_seconds", 0)
            ])
        );

        let mut budget_status = PodDisruptionBudgetStatus {
            disruptions_allowed: 1,
            current_healthy: 3,
            desired_healthy: 2,
            expected_pods: 3,
            ..Default::default()
        };
        budget_status.disrupted_pods.insert("pod-1".into(), 1600000000);
        let budget = row(Body::PodDisruptionBudget(CollectorPodDisruptionBudget {
            pod_disruption_budgets: vec![PodDisruptionBudget {
                metadata: meta(),
                status: Some(budget_status),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!(
            budget.counts,
            map(&[
                ("disruptions_allowed", 1),
                ("current_healthy", 3),
                ("desired_healthy", 2),
                ("expected_pods", 3),
                ("disrupted_pods", 1)
            ])
        );

        let service = row(Body::Service(CollectorService {
            services: vec![Service {
                metadata: meta(),
                spec: Some(ServiceSpec {
                    r#type: "ClusterIP".into(),
                    cluster_ip: "10.96.0.10".into(),
                    ports: vec![ServicePort {
                        name: "http".into(),
                        protocol: "TCP".into(),
                        port: 80,
                        target_port: "8080".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!((service.service_type.as_str(), service.cluster_ip.as_str()), ("ClusterIP", "10.96.0.10"));
        same_json(r#"[{"name":"http","protocol":"TCP","port":80,"targetPort":"8080"}]"#, &service.service_ports);
        assert_eq!(service.counts, map(&[("ports", 1)]));

        let namespace = row(Body::Namespace(CollectorNamespace {
            namespaces: vec![Namespace { metadata: meta(), status: "Terminating".into(), ..Default::default() }],
            ..Default::default()
        }));
        assert_eq!(namespace.status, "Terminating");

        // A kind whose status or spec is missing reads as zeros, not a failure.
        let bare = row(Body::DaemonSet(CollectorDaemonSet {
            daemon_sets: vec![DaemonSet { metadata: meta(), ..Default::default() }],
            ..Default::default()
        }));
        assert_eq!(bare.counts["ready"], 0);
    }

    #[test]
    fn a_role_keeps_the_permissions_it_grants() {
        let rule = PolicyRule {
            api_groups: vec!["".into()],
            resources: vec!["secrets".into()],
            verbs: vec!["get".into(), "list".into()],
            ..Default::default()
        };
        let roles = rows(Body::Role(CollectorRole {
            roles: vec![
                Role {
                    metadata: metadata("kube-system", "secret-reader", "u"),
                    rules: vec![rule],
                    ..Default::default()
                },
                Role { metadata: metadata("kube-system", "empty", "u2"), ..Default::default() },
            ],
            ..Default::default()
        }));
        assert_eq!(roles[0].counts["rules"], 1);
        same_json(r#"[{"verbs":["get","list"],"apiGroups":[""],"resources":["secrets"]}]"#, &roles[0].rbac_rules);
        // A role with no rules.
        assert_eq!(roles[1].rbac_rules, "");
    }

    #[test]
    fn a_role_binding_keeps_its_subjects_and_the_role_it_refers_to() {
        let binding = RoleBinding {
            metadata: metadata("shop", "readers", "u"),
            role_ref: Some(TypedLocalObjectReference {
                kind: "Role".into(),
                name: "secret-reader".into(),
                ..Default::default()
            }),
            subjects: vec![Subject {
                kind: "ServiceAccount".into(),
                name: "ci".into(),
                namespace: "shop".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let bindings = rows(Body::RoleBinding(CollectorRoleBinding {
            role_bindings: vec![
                binding,
                RoleBinding { metadata: metadata("shop", "empty", "u2"), ..Default::default() },
            ],
            ..Default::default()
        }));
        same_json(r#"[{"kind":"ServiceAccount","name":"ci","namespace":"shop"}]"#, &bindings[0].rbac_subjects);
        same_json(r#"{"kind":"Role","name":"secret-reader"}"#, &bindings[0].rbac_role_ref);
        assert_eq!(bindings[0].counts, map(&[("subjects", 1)]));
        assert_eq!((bindings[1].rbac_subjects.as_str(), bindings[1].rbac_role_ref.as_str()), ("", ""));
    }

    #[test]
    fn a_kind_with_no_column_of_its_own_is_kept_whole_in_the_object_column() {
        let mut storage_class = StorageClass {
            metadata: metadata("", "gp3", "u"),
            provisioner: "ebs.csi.aws.com".into(),
            reclaim_policy: "Delete".into(),
            volume_binding_mode: "WaitForFirstConsumer".into(),
            ..Default::default()
        };
        storage_class.parameters.insert("type".into(), "gp3".into());
        storage_class.parameters.insert("encrypted".into(), "true".into());
        let row = row(Body::StorageClass(CollectorStorageClass {
            cluster_name: "prod".into(),
            storage_classes: vec![storage_class],
            ..Default::default()
        }));

        assert_eq!(row.kind, "StorageClass");
        for expected in ["ebs.csi.aws.com", "gp3", "WaitForFirstConsumer", "Delete", "encrypted"] {
            assert!(row.object.contains(expected), "{expected} in {}", row.object);
        }
        // The envelope must not repeat the object list: that would square the payload.
        same_json(r#"{"clusterName":"prod"}"#, &row.envelope);
    }

    #[test]
    fn the_json_columns_hold_protobufs_own_json() {
        // proto3's JSON mapping, as the protobuf library writes it: values at their default left
        // out, 64-bit integers as strings, enums by name, bytes as base64, an empty message still
        // present.
        let mut requirements = ResourceRequirements { name: "app".into(), r#type: 1, ..Default::default() };
        requirements.requests.insert("memory".into(), 9007199254740993);
        requirements.requests.insert("cpu".into(), 100);
        let mut metrics = ResourceMetrics::default();
        metrics.metric_values.insert("cpu.usage".into(), 0.42);
        #[allow(deprecated)]
        let pod = Pod {
            metadata: Some(Metadata { labels: vec!["app:web".into()], ..metadata("shop", "web-1", "u-1").unwrap() }),
            ip: "10.0.0.1".into(),
            qos_class: "Burstable".into(),
            metrics: Some(metrics),
            host: Some(Host { id: 9007199254740993, name: "node-1".into(), ..Default::default() }),
            node_affinity: Some(NodeAffinity::default()),
            yaml: vec![0xff, 0x00],
            container_statuses: vec![ContainerStatus {
                name: "app".into(),
                ready: true,
                restart_count: 2,
                ..Default::default()
            }],
            resource_requirements: vec![requirements],
            ..Default::default()
        };
        let collector = CollectorPod {
            host_name: "cluster-agent-1".into(),
            cluster_name: "prod".into(),
            cluster_id: "cid".into(),
            group_id: 1,
            group_size: 2,
            is_terminated: true,
            info: Some(SystemInfo { uuid: "sys-uuid".into(), total_memory: 9007199254740993, ..Default::default() }),
            agent_version: agent_version(7, 55, 1, ""),
            tags: vec!["env:prod".into()],
            pods: vec![pod],
            ..Default::default()
        };
        let row = row(Body::Pod(collector));

        same_json(
            r#"{"metadata":{"name":"web-1","namespace":"shop","uid":"u-1","labels":["app:web"]},"IP":"10.0.0.1","containerStatuses":[{"name":"app","ready":true,"restartCount":2}],"yaml":"/wA=","host":{"id":"9007199254740993","name":"node-1"},"resourceRequirements":[{"requests":{"cpu":"100","memory":"9007199254740993"},"name":"app","type":"container"}],"QOSClass":"Burstable","metrics":{"metricValues":{"cpu.usage":0.42}},"nodeAffinity":{}}"#,
            &row.object,
        );
        // The envelope keeps the extras no column holds: the cluster agent's own system info, and
        // the flag that says this is the terminated-pods batch.
        same_json(
            r#"{"hostName":"cluster-agent-1","clusterName":"prod","clusterId":"cid","groupId":1,"groupSize":2,"tags":["env:prod"],"info":{"uuid":"sys-uuid","totalMemory":"9007199254740993"},"isTerminated":true,"agentVersion":{"Major":"7","Minor":"55","Patch":"1"}}"#,
            &row.envelope,
        );
        same_json(
            r#"[{"requests":{"cpu":"100","memory":"9007199254740993"},"name":"app","type":"container"}]"#,
            &row.resource_requirements,
        );
        assert_eq!(row.agent_version, "7.55.1");
        // The pod names its own host, which wins over the envelope's.
        assert_eq!(row.host_name, "node-1");
    }

    #[test]
    fn a_number_json_has_no_form_for_is_written_as_text_and_the_row_is_kept() {
        let mut metrics = ResourceMetrics::default();
        metrics.metric_values.insert("nan".into(), f64::NAN);
        let row = pod_row(Pod { metadata: metadata("", "p", "u"), metrics: Some(metrics), ..Default::default() });
        assert!(row.object.contains("\"NaN\""), "{}", row.object);
        assert!(row.metrics["nan"].is_nan());
    }

    #[test]
    fn an_enum_value_the_schema_does_not_have_is_written_as_its_number() {
        let requirements = ResourceRequirements { name: "app".into(), r#type: 42, ..Default::default() };
        let row = pod_row(Pod {
            metadata: metadata("", "p", "u"),
            resource_requirements: vec![requirements],
            ..Default::default()
        });
        same_json(r#"[{"name":"app","type":42}]"#, &row.resource_requirements);
    }

    #[test]
    fn a_vertical_autoscalers_conditions_are_read_from_its_status() {
        // The list nested in status is the one a real autoscaler fills, and its fields are named
        // condition_type/condition_status.
        let status = VerticalPodAutoscalerStatus {
            conditions: vec![VpaCondition {
                condition_type: "RecommendationProvided".into(),
                condition_status: "True".into(),
                message: "recommendation computed".into(),
                last_transition_time: 1700000000,
                ..Default::default()
            }],
            ..Default::default()
        };
        let row = row(Body::VerticalPodAutoscaler(CollectorVerticalPodAutoscaler {
            cluster_id: "cid".into(),
            vertical_pod_autoscalers: vec![VerticalPodAutoscaler {
                metadata: metadata("shop", "vpa-1", "u-1"),
                status: Some(status),
                ..Default::default()
            }],
            ..Default::default()
        }));
        assert_eq!(row.condition_types, ["RecommendationProvided"]);
        assert_eq!(row.condition_statuses, ["True"]);
        assert_eq!(row.condition_messages, ["recommendation computed"]);
        assert_eq!(row.condition_last_transition, [at(1700000000)]);
    }

    #[test]
    fn a_vertical_autoscaler_with_conditions_in_both_places_keeps_both_the_objects_first() {
        let status = VerticalPodAutoscalerStatus {
            conditions: vec![VpaCondition {
                condition_type: "Nested".into(),
                condition_status: "False".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let autoscaler = VerticalPodAutoscaler {
            metadata: metadata("", "vpa-1", "u-1"),
            status: Some(status),
            conditions: vec![VerticalPodAutoscalerCondition {
                r#type: "TopLevel".into(),
                status: "True".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let row = row(Body::VerticalPodAutoscaler(CollectorVerticalPodAutoscaler {
            vertical_pod_autoscalers: vec![autoscaler],
            ..Default::default()
        }));
        assert_eq!(row.condition_types, ["TopLevel", "Nested"]);
        assert_eq!(row.condition_statuses, ["True", "False"]);
    }

    // ---- the cluster summary ----

    #[test]
    fn the_cluster_row_keeps_both_tag_sets_apart_and_the_per_node_breakdown() {
        let mut metrics = ResourceMetrics::default();
        metrics.metric_values.insert("nodes.ready".into(), 2.0);
        let mut first = ClusterNodeInfo {
            name: "node-1".into(),
            region: "eu-west-1".into(),
            instance_type: "m6i.large".into(),
            kubelet_version: "v1.29.4".into(),
            ..Default::default()
        };
        first.resource_capacity.insert("cpu".into(), "2".into());
        let second = ClusterNodeInfo {
            name: "node-2".into(),
            region: "eu-west-1".into(),
            instance_type: "m6i.xlarge".into(),
            kubelet_version: "v1.28.9".into(),
            ..Default::default()
        };
        let mut cluster = Cluster {
            node_count: 2,
            resource_version: "9912".into(),
            creation_timestamp: 1600000000,
            metrics: Some(metrics),
            tags: vec!["kube_cluster_name:prod".into()],
            nodes_info: vec![first, second],
            ..Default::default()
        };
        cluster.kubelet_versions.insert("v1.29.4".into(), 1);
        cluster.kubelet_versions.insert("v1.28.9".into(), -1);
        cluster.extended_resources_capacity.insert("nvidia.com/gpu".into(), 4);
        let collector = CollectorCluster {
            cluster_name: "prod".into(),
            cluster_id: "cid".into(),
            group_id: 1,
            group_size: 2,
            agent_version: agent_version(7, 55, 1, "rc.1"),
            cluster: Some(cluster),
            tags: vec!["env:prod".into(), "team:infra".into()],
        };
        let rows = cluster_rows("t", now(), &frame(), &collector);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];

        assert_eq!((row.org_id, row.encoding.as_str()), (77, "protobuf"));
        assert_eq!((row.group_id, row.group_size, row.agent_version.as_str()), (1, 2, "7.55.1-rc.1"));
        assert_eq!(row.node_count, 2);
        assert_eq!(row.tags, map(&[("env", vec!["prod".to_string()]), ("team", vec!["infra".to_string()])]));
        assert_eq!(row.cluster_tags, map(&[("kube_cluster_name", vec!["prod".to_string()])]));
        assert_eq!(row.resource_version, "9912");
        assert_eq!(row.creation_timestamp, at(1600000000));
        assert_eq!(row.metrics, map(&[("nodes.ready", 2.0)]));
        assert_eq!(row.extended_resources_capacity, map(&[("nvidia.com/gpu", 4)]));
        // A negative node count cannot be real, and must not wrap to billions.
        assert_eq!(row.kubelet_versions, map(&[("v1.29.4", 1), ("v1.28.9", 0)]));
        assert_eq!(row.nodes_name, ["node-1", "node-2"]);
        assert_eq!(row.nodes_kubelet_version, ["v1.29.4", "v1.28.9"]);
        assert_eq!(row.nodes_capacity, [map(&[("cpu", "2".to_string())]), BTreeMap::new()]);
        assert_eq!(row.nodes_allocatable, [BTreeMap::new(), BTreeMap::new()]);
    }

    #[test]
    fn a_cluster_frame_with_no_cluster_in_it_gives_no_row() {
        let collector = CollectorCluster { cluster_name: "prod".into(), ..Default::default() };
        assert!(cluster_rows("t", now(), &frame(), &collector).is_empty());
    }

    // ---- manifests ----

    #[test]
    fn a_crd_or_cr_frame_whose_inner_envelope_is_missing_gives_no_rows() {
        assert!(manifest_rows("t", now(), &frame(), None, &[]).is_empty());
        assert!(manifest_rows("t", now(), &frame(), CollectorManifestCrd::default().manifest, &[]).is_empty());
    }

    #[test]
    fn a_manifest_row_holds_the_document_as_it_was_collected() {
        let collector = CollectorManifest {
            cluster_name: "prod".into(),
            cluster_id: "cid".into(),
            manifests: vec![Manifest {
                uid: "u1".into(),
                kind: "ConfigMap".into(),
                api_version: "v1".into(),
                resource_version: "7".into(),
                content: b"kind: ConfigMap".to_vec(),
                content_type: "yaml".into(),
                is_terminated: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let rows = manifest_rows("t", now(), &frame(), Some(collector), &[]);
        let row = &rows[0];
        assert_eq!(
            (row.uid.as_str(), row.kind.as_str(), row.api_version.as_str(), row.resource_version.as_str()),
            ("u1", "ConfigMap", "v1", "7")
        );
        assert_eq!(row.content.0, b"kind: ConfigMap");
        assert_eq!((row.content_type.as_str(), row.is_terminated, row.content_is_utf8), ("yaml", 1, 1));
        assert_eq!((row.cluster_name.as_str(), row.cluster_id.as_str()), ("prod", "cid"));
        // A manifest with no Host leaves the column empty, not "null".
        assert_eq!(row.manifest_host, "");
    }

    #[test]
    fn manifest_tags_merge_the_frames_the_wrappers_and_the_manifests_and_remember_which_was_which() {
        let mut manifest = Manifest {
            uid: "u1".into(),
            kind: "DatadogAgent".into(),
            api_version: "datadoghq.com/v2alpha1".into(),
            r#type: 81,
            version: "v1".into(),
            node_name: "node-3".into(),
            content: b"apiVersion: datadoghq.com/v2alpha1".to_vec(),
            content_type: "json".into(),
            tags: vec!["team:infra".into()],
            ..Default::default()
        };
        manifest.extra_attributes.insert("crd_group".into(), "datadoghq.com".into());
        let collector = CollectorManifest {
            cluster_name: "prod".into(),
            cluster_id: "cid".into(),
            group_id: 4,
            group_size: 8,
            host_name: "cluster-agent-1".into(),
            agent_version: agent_version(7, 55, 1, ""),
            origin_collector: OriginCollector::DatadogAgent as i32,
            tags: vec!["env:prod".into()],
            manifests: vec![manifest],
            ..Default::default()
        };
        let rows = manifest_rows("t", now(), &frame(), Some(collector), &["source:crd".into()]);
        let row = &rows[0];

        assert_eq!(
            (row.group_id, row.group_size, row.host_name.as_str(), row.agent_version.as_str()),
            (4, 8, "cluster-agent-1", "7.55.1")
        );
        assert_eq!((row.origin_collector.as_str(), row.org_id), ("datadogAgent", 77));
        let one = |v: &str| vec![v.to_string()];
        assert_eq!(row.tags, map(&[("env", one("prod")), ("source", one("crd")), ("team", one("infra"))]));
        assert_eq!(
            row.tag_sources,
            map(&[("envelope", one("env:prod")), ("wrapper", one("source:crd")), ("manifest", one("team:infra"))])
        );
        assert_eq!((row.node_name.as_str(), row.r#type, row.version.as_str()), ("node-3", 81, "v1"));
        assert_eq!(row.extra_attributes, map(&[("crd_group", "datadoghq.com".to_string())]));
        assert_eq!(row.content_is_utf8, 1);
    }

    #[test]
    fn content_that_is_not_utf8_is_kept_byte_for_byte_and_flagged() {
        let collector = CollectorManifest {
            manifests: vec![Manifest { uid: "u".into(), content: vec![0xff, 0xfe, 0x00], ..Default::default() }],
            ..Default::default()
        };
        let rows = manifest_rows("t", now(), &frame(), Some(collector), &[]);
        assert_eq!(rows[0].content_is_utf8, 0);
        assert_eq!(rows[0].content.0, [0xff, 0xfe, 0x00]);
        // A source with no tags is left out rather than listed empty.
        assert!(rows[0].tag_sources.is_empty());
    }

    #[test]
    fn a_manifest_row_keeps_the_senders_system_info_and_the_host_the_object_was_seen_on() {
        let collector = CollectorManifest {
            cluster_id: "cid".into(),
            system_info: Some(SystemInfo {
                uuid: "agent-uuid-1".into(),
                os: Some(OsInfo { name: "linux".into(), platform: "ubuntu".into(), ..Default::default() }),
                total_memory: 8589934592,
                ..Default::default()
            }),
            manifests: vec![Manifest {
                uid: "u-1".into(),
                kind: "Pod".into(),
                content: b"{}".to_vec(),
                host: Some(Host { id: 42, org_id: 7, name: "node-1".into(), num_cpus: 8, ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let rows = manifest_rows("t", now(), &frame(), Some(collector), &[]);
        // Without the manifest list: the list is the rows.
        same_json(
            r#"{"clusterId":"cid","systemInfo":{"uuid":"agent-uuid-1","os":{"name":"linux","platform":"ubuntu"},"totalMemory":"8589934592"}}"#,
            &rows[0].envelope,
        );
        same_json(r#"{"id":"42","orgId":7,"name":"node-1","numCpus":8}"#, &rows[0].manifest_host);
    }

    // ---- ECS tasks ----

    #[test]
    fn an_ecs_task_row_is_keyed_on_its_arn_and_keeps_its_four_tag_sets_apart() {
        let mut task = EcsTask {
            arn: "arn:aws:ecs:eu-west-1:123:task/abc".into(),
            resource_version: "rv-1".into(),
            launch_type: "FARGATE".into(),
            desired_status: "RUNNING".into(),
            known_status: "RUNNING".into(),
            family: "checkout".into(),
            version: "3".into(),
            availability_zone: "eu-west-1a".into(),
            service_name: "checkout-svc".into(),
            vpc_id: "vpc-1".into(),
            container_instance_arn: "arn:aws:ecs:...:container-instance/xyz".into(),
            daemon_name: "datadog-agent".into(),
            host: Some(Host { name: "ip-10-0-1-7".into(), ..Default::default() }),
            pull_started_at: 1600000000,
            pull_stopped_at: 1600000030,
            containers: vec![EcsContainer { name: "app".into(), docker_id: "d1".into(), ..Default::default() }],
            tags: vec!["service:checkout".into()],
            ecs_tags: vec!["ecs:tag".into()],
            container_instance_tags: vec!["instance:tag".into()],
            ..Default::default()
        };
        task.limits.insert("CPU".into(), 1024.0);
        task.limits.insert("Memory".into(), 2048.0);
        task.ephemeral_storage_metrics.insert("utilized".into(), 100);
        let collector = CollectorEcsTask {
            aws_account_id: 123456789012,
            cluster_name: "ecs-prod".into(),
            cluster_id: "ecs-cid".into(),
            region: "eu-west-1".into(),
            group_id: 2,
            group_size: 5,
            host_name: "ecs-agent-1".into(),
            agent_version: agent_version(7, 55, 1, ""),
            tags: vec!["env:prod".into()],
            tasks: vec![task],
            ..Default::default()
        };
        let rows = ecs_task_rows("t", now(), &frame(), collector);
        let row = &rows[0];
        let one = |k: &str, v: &str| map(&[(k, vec![v.to_string()])]);

        assert_eq!(
            (row.arn.as_str(), row.family.as_str(), row.launch_type.as_str()),
            ("arn:aws:ecs:eu-west-1:123:task/abc", "checkout", "FARGATE")
        );
        assert_eq!(row.aws_account_id, 123456789012);
        assert_eq!(
            (row.region.as_str(), row.host_name.as_str(), row.task_host_name.as_str()),
            ("eu-west-1", "ecs-agent-1", "ip-10-0-1-7")
        );
        assert_eq!(
            (
                row.cluster_name.as_str(),
                row.cluster_id.as_str(),
                row.group_id,
                row.group_size,
                row.agent_version.as_str()
            ),
            ("ecs-prod", "ecs-cid", 2, 5, "7.55.1")
        );
        assert_eq!(
            (row.org_id, row.subscription_id, row.header_timestamp, row.encoding.as_str()),
            (77, 3, Some(1700000000), "protobuf")
        );
        assert_eq!(row.limits, map(&[("CPU", 1024.0), ("Memory", 2048.0)]));
        assert_eq!(row.ephemeral_storage_metrics, map(&[("utilized", 100)]));
        // A task still running has no stop time, and unset must not become 1970.
        assert_eq!(
            (row.pull_started_at, row.pull_stopped_at, row.execution_stopped_at),
            (at(1600000000), at(1600000030), None)
        );
        assert_eq!(row.container_count, 1);
        same_json(r#"[{"dockerID":"d1","name":"app"}]"#, &row.containers);
        assert_eq!(row.envelope_tags, one("env", "prod"));
        assert_eq!(row.tags, one("service", "checkout"));
        assert_eq!(row.ecs_tags, one("ecs", "tag"));
        assert_eq!(row.container_instance_tags, one("instance", "tag"));
    }

    #[test]
    fn an_ecs_task_row_keeps_the_envelopes_own_host_and_info_and_the_tasks_host_in_full() {
        let collector = CollectorEcsTask {
            cluster_id: "ecs-cid".into(),
            host: Some(Host {
                id: 11,
                name: "ecs-collector".into(),
                all_tags: vec!["role:collector".into()],
                ..Default::default()
            }),
            info: Some(SystemInfo { uuid: "ecs-agent-uuid".into(), total_memory: 4294967296, ..Default::default() }),
            tasks: vec![EcsTask {
                arn: "arn:aws:ecs:eu-west-1:123:task/abc".into(),
                host: Some(Host { id: 99, org_id: 7, name: "ip-10-0-1-7".into(), num_cpus: 4, ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let rows = ecs_task_rows("t", now(), &frame(), collector);
        same_json(
            r#"{"clusterId":"ecs-cid","host":{"id":"11","name":"ecs-collector","allTags":["role:collector"]},"info":{"uuid":"ecs-agent-uuid","totalMemory":"4294967296"}}"#,
            &rows[0].envelope,
        );
        same_json(r#"{"id":"99","orgId":7,"name":"ip-10-0-1-7","numCpus":4}"#, &rows[0].task_host);
        assert_eq!(rows[0].task_host_name, "ip-10-0-1-7");
        // What the Go server stored for a task with no containers.
        assert_eq!(rows[0].containers, "");
    }

    // ---- kubeactions ----

    #[test]
    fn an_empty_or_unparseable_action_timestamp_becomes_the_arrival_time_never_1970() {
        let timestamp_of =
            |text: &str| action_row("t", now(), decoded(&json!({ "timestamp": text }).to_string())).timestamp;
        let utc = |h| Utc.with_ymd_and_hms(2026, 9, 20, h, 30, 0).unwrap();
        assert_eq!(timestamp_of(""), now());
        assert_eq!(timestamp_of("yesterday-ish"), now());
        assert_eq!(timestamp_of("2026-09-20T08:30:00Z"), utc(8));
        assert_eq!(timestamp_of("2026-09-20T10:30:00+02:00"), utc(8));
    }

    #[test]
    fn keys_an_action_event_does_not_declare_surface_in_the_row_sorted() {
        let event = decoded(
            r#"{"action_id": "a-1", "event_type": "action_executed", "status": "success", "zeta_field": 1, "alpha_field": {"nested": true}}"#,
        );
        let row = action_row("t", now(), event);
        assert_eq!(
            (row.action_id.as_str(), row.event_type.as_str(), row.status.as_str()),
            ("a-1", "action_executed", "success")
        );
        assert_eq!(row.extra_keys, ["alpha_field", "zeta_field"]);
        assert_eq!(row.extra, r#"{"alpha_field":{"nested":true},"zeta_field":1}"#);
    }

    #[test]
    fn an_action_row_keeps_its_attachments_its_undeclared_values_and_the_timestamp_as_it_arrived() {
        let event = decoded(
            r#"{"action_id": "a-1", "org_id": 9007199254740993, "status": "failed", "timestamp": "not-a-date",
                "payloads": {"stderr": "YmFk", "binary": "/wD+", "nothing": null}, "new_field": {"nested": 7}}"#,
        );
        let row = action_row("t", now(), event);
        assert_eq!(row.org_id, 9007199254740993);
        assert_eq!(row.payloads["stderr"].0, b"bad");
        assert_eq!(row.payloads["binary"].0, [0xff, 0x00, 0xfe]);
        assert!(row.payloads["nothing"].0.is_empty());
        assert_eq!(row.extra_keys, ["new_field"]);
        assert_eq!(row.extra, r#"{"new_field":{"nested":7}}"#);
        assert_eq!(row.timestamp_raw, "not-a-date");
        assert_eq!(row.timestamp, now());
    }

    #[test]
    fn an_action_row_with_nothing_extra_has_empty_extra_columns() {
        let row = action_row("t", now(), decoded(r#"{"action_id":"a-1"}"#));
        assert!(row.extra_keys.is_empty());
        assert_eq!(row.extra, "");
        assert!(row.payloads.is_empty());
    }

    #[test]
    fn an_action_event_exact_keys_the_last_of_two_null_as_absent_the_rest_in_extra() {
        assert_eq!(decoded(r#"{"action_id":"x","ACTION_ID":"y"}"#).action_id, "x");
        assert_eq!(decoded(r#"{"action_id":"x","action_id":"z"}"#).action_id, "z");
        // A key that is not exactly a known one fills nothing and is kept as extra.
        assert_eq!(decoded(r#"{"ACTION_ID":"y"}"#).extra.keys().collect::<Vec<_>>(), ["ACTION_ID"]);
        assert_eq!(decoded(r#"{"ACTION_ID":"y","actionid":"x","action-id":"x"}"#).action_id, "");

        assert_eq!(decoded(r#"{"action_id":"x","action_id":null}"#).action_id, "");
        assert_eq!(decoded(r#"{"org_id":null,"payloads":null}"#).org_id, 0);
        assert_eq!(decoded(r#"{"org_id":-5}"#).org_id, -5);

        // null and {} are events with nothing in them.
        assert_eq!(decoded("null").action_id, "");
        assert_eq!(decoded("{}").action_id, "");

        // A value of the wrong type refuses the whole event.
        for text in [
            r#"{"action_id":5}"#,
            r#"{"timestamp":123}"#,
            r#"{"org_id":"5"}"#,
            r#"{"org_id":5.0}"#,
            r#"{"org_id":1e3}"#,
            r#"{"org_id":9223372036854775808}"#,
            r#"{"payloads":"x"}"#,
            r#"{"action_id":"ok","org_id":"bad"}"#,
            "5",
            "[]",
            r#""text""#,
        ] {
            assert!(refused(text), "{text}");
        }
    }

    #[test]
    fn attachments_are_padded_standard_base64_line_breaks_allowed() {
        let attachment = |text: &str| decoded(&json!({ "payloads": { "a": text } }).to_string()).payloads["a"].clone();
        assert_eq!(attachment("YmE="), b"ba");
        assert_eq!(attachment("Ym\nFk"), b"bad");
        assert!(attachment("").is_empty());
        for text in [
            r#"{"payloads":{"a":"YmE"}}"#,
            r#"{"payloads":{"a":"Y mFk"}}"#,
            r#"{"payloads":{"a":"-_-_"}}"#,
            r#"{"payloads":{"a":5}}"#,
        ] {
            assert!(refused(text), "{text}");
        }
        // A null attachment is an empty one.
        assert!(decoded(r#"{"payloads":{"a":null}}"#).payloads["a"].is_empty());
        assert_eq!(
            decode_action("[3]", &json!({ "payloads": { "a": "YmE" } })).map(|_| ()),
            Err("[3].payloads.a: not base64".into())
        );
    }
}
