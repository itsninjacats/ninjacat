//! The envelope of the process-agent and of the orchestrator collector: a small binary header
//! that says which protobuf message follows and how it is encoded, then the message.
//!
//! Ported from github.com/DataDog/agent-payload, process/message.go.
//!
//! Copyright (c) 2017, Datadog, Inc. Licensed under the BSD 3-Clause License; see
//! THIRD_PARTY_NOTICES.md.

use ninjapaws_proto::datadog::process_agent::*;
use prost::Message;

use crate::body;
use crate::json;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    /// 0 protobuf, 1 JSON, 2 zstd 0.x (not supported), 4 and 5 zstd 1.x.
    pub encoding: u8,
    pub message_type: u8,
    pub subscription_id: u8,
    pub org_id: i32,
    pub timestamp: i64,
}

/// The message of a frame, by its type.
// One per request, so how large the largest variant is does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Proc(CollectorProc),
    Connections(CollectorConnections),
    Res(ResCollector),
    RealTime(CollectorRealTime),
    Container(CollectorContainer),
    ContainerRealTime(CollectorContainerRealTime),
    Pod(CollectorPod),
    ReplicaSet(CollectorReplicaSet),
    Deployment(CollectorDeployment),
    Service(CollectorService),
    Node(CollectorNode),
    Cluster(CollectorCluster),
    Job(CollectorJob),
    CronJob(CollectorCronJob),
    DaemonSet(CollectorDaemonSet),
    StatefulSet(CollectorStatefulSet),
    PersistentVolume(CollectorPersistentVolume),
    PersistentVolumeClaim(CollectorPersistentVolumeClaim),
    ProcDiscovery(CollectorProcDiscovery),
    Role(CollectorRole),
    RoleBinding(CollectorRoleBinding),
    ClusterRole(CollectorClusterRole),
    ClusterRoleBinding(CollectorClusterRoleBinding),
    ServiceAccount(CollectorServiceAccount),
    Ingress(CollectorIngress),
    ProcEvent(CollectorProcEvent),
    Namespace(CollectorNamespace),
    Manifest(CollectorManifest),
    ManifestCrd(CollectorManifestCrd),
    ManifestCr(CollectorManifestCr),
    VerticalPodAutoscaler(CollectorVerticalPodAutoscaler),
    HorizontalPodAutoscaler(CollectorHorizontalPodAutoscaler),
    NetworkPolicy(CollectorNetworkPolicy),
    LimitRange(CollectorLimitRange),
    StorageClass(CollectorStorageClass),
    PodDisruptionBudget(CollectorPodDisruptionBudget),
    EcsTask(CollectorEcsTask),
}

impl Body {
    /// The name the .proto gives the message.
    pub fn name(&self) -> &'static str {
        match self {
            Body::Proc(_) => "CollectorProc",
            Body::Connections(_) => "CollectorConnections",
            Body::Res(_) => "ResCollector",
            Body::RealTime(_) => "CollectorRealTime",
            Body::Container(_) => "CollectorContainer",
            Body::ContainerRealTime(_) => "CollectorContainerRealTime",
            Body::Pod(_) => "CollectorPod",
            Body::ReplicaSet(_) => "CollectorReplicaSet",
            Body::Deployment(_) => "CollectorDeployment",
            Body::Service(_) => "CollectorService",
            Body::Node(_) => "CollectorNode",
            Body::Cluster(_) => "CollectorCluster",
            Body::Job(_) => "CollectorJob",
            Body::CronJob(_) => "CollectorCronJob",
            Body::DaemonSet(_) => "CollectorDaemonSet",
            Body::StatefulSet(_) => "CollectorStatefulSet",
            Body::PersistentVolume(_) => "CollectorPersistentVolume",
            Body::PersistentVolumeClaim(_) => "CollectorPersistentVolumeClaim",
            Body::ProcDiscovery(_) => "CollectorProcDiscovery",
            Body::Role(_) => "CollectorRole",
            Body::RoleBinding(_) => "CollectorRoleBinding",
            Body::ClusterRole(_) => "CollectorClusterRole",
            Body::ClusterRoleBinding(_) => "CollectorClusterRoleBinding",
            Body::ServiceAccount(_) => "CollectorServiceAccount",
            Body::Ingress(_) => "CollectorIngress",
            Body::ProcEvent(_) => "CollectorProcEvent",
            Body::Namespace(_) => "CollectorNamespace",
            Body::Manifest(_) => "CollectorManifest",
            Body::ManifestCrd(_) => "CollectorManifestCRD",
            Body::ManifestCr(_) => "CollectorManifestCR",
            Body::VerticalPodAutoscaler(_) => "CollectorVerticalPodAutoscaler",
            Body::HorizontalPodAutoscaler(_) => "CollectorHorizontalPodAutoscaler",
            Body::NetworkPolicy(_) => "CollectorNetworkPolicy",
            Body::LimitRange(_) => "CollectorLimitRange",
            Body::StorageClass(_) => "CollectorStorageClass",
            Body::PodDisruptionBudget(_) => "CollectorPodDisruptionBudget",
            Body::EcsTask(_) => "CollectorECSTask",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub header: Header,
    pub body: Body,
}

/// The header and where the message starts. Three versions exist; each later one appends fields:
/// v1 is 4 bytes, v2 adds the org id (8), v3 adds the timestamp (16).
pub fn read_header(data: &[u8]) -> Result<(Header, usize), String> {
    if data.len() <= 4 {
        return Err(format!("invalid message length: {}", data.len()));
    }
    let header = Header {
        version: data[0],
        encoding: data[1],
        message_type: data[2],
        subscription_id: data[3],
        org_id: 0,
        timestamp: 0,
    };
    let org_id = || i32::from_le_bytes(data[4..8].try_into().unwrap());
    match data[0] {
        1 => Ok((header, 4)),
        2 if data.len() >= 8 => Ok((Header { org_id: org_id(), ..header }, 8)),
        3 if data.len() >= 16 => Ok((
            Header { org_id: org_id(), timestamp: i64::from_le_bytes(data[8..16].try_into().unwrap()), ..header },
            16,
        )),
        2 | 3 => Err("unexpected EOF".into()),
        version => Err(format!("invalid message version: {version}")),
    }
}

/// The message in the encoding the header names.
fn message<T: Message + Default + prost::Name>(encoding: u8, body: &[u8]) -> Result<T, String> {
    match encoding {
        0 => T::decode(body).map_err(|e| e.to_string()),
        1 => json::proto::try_parse(body),
        // Zstandard 0.x predates the format every current library reads. Agents have sent
        // encoding 4 or 5 for years. The caller keeps such a frame in raw_payloads.
        2 => Err("zstd 0.x frames (message encoding 2) are not supported".into()),
        4 | 5 => {
            let inflated =
                zstd::stream::read::Decoder::new(body).and_then(body::read_inflated).map_err(|e| e.to_string())?;
            T::decode(inflated.as_slice()).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown message encoding: {other}")),
    }
}

/// The whole frame: header and decoded message.
pub fn decode(data: &[u8]) -> Result<Frame, String> {
    let (header, offset) = read_header(data)?;
    let body = &data[offset..];
    let e = header.encoding;
    let decoded = match header.message_type {
        12 => message(e, body).map(Body::Proc),
        22 => message(e, body).map(Body::Connections),
        23 => message(e, body).map(Body::Res),
        27 => message(e, body).map(Body::RealTime),
        39 => message(e, body).map(Body::Container),
        40 => message(e, body).map(Body::ContainerRealTime),
        41 => message(e, body).map(Body::Pod),
        42 => message(e, body).map(Body::ReplicaSet),
        43 => message(e, body).map(Body::Deployment),
        44 => message(e, body).map(Body::Service),
        45 => message(e, body).map(Body::Node),
        46 => message(e, body).map(Body::Cluster),
        47 => message(e, body).map(Body::Job),
        48 => message(e, body).map(Body::CronJob),
        49 => message(e, body).map(Body::DaemonSet),
        50 => message(e, body).map(Body::StatefulSet),
        51 => message(e, body).map(Body::PersistentVolume),
        52 => message(e, body).map(Body::PersistentVolumeClaim),
        53 => message(e, body).map(Body::ProcDiscovery),
        54 => message(e, body).map(Body::Role),
        55 => message(e, body).map(Body::RoleBinding),
        56 => message(e, body).map(Body::ClusterRole),
        57 => message(e, body).map(Body::ClusterRoleBinding),
        58 => message(e, body).map(Body::ServiceAccount),
        59 => message(e, body).map(Body::Ingress),
        60 => message(e, body).map(Body::ProcEvent),
        61 => message(e, body).map(Body::Namespace),
        80 => message(e, body).map(Body::Manifest),
        81 => message(e, body).map(Body::ManifestCrd),
        82 => message(e, body).map(Body::ManifestCr),
        83 => message(e, body).map(Body::VerticalPodAutoscaler),
        84 => message(e, body).map(Body::HorizontalPodAutoscaler),
        85 => message(e, body).map(Body::NetworkPolicy),
        86 => message(e, body).map(Body::LimitRange),
        87 => message(e, body).map(Body::StorageClass),
        88 => message(e, body).map(Body::PodDisruptionBudget),
        200 => message(e, body).map(Body::EcsTask),
        other => return Err(format!("unhandled message type: {other}")),
    };
    decoded.map(|body| Frame { header, body })
}

/// A frame as the intake answers with one: header v3, plain protobuf.
pub fn encode(message_type: u8, timestamp: i64, body: &impl Message) -> Vec<u8> {
    let mut frame = vec![3, 0, message_type, 0, 0, 0, 0, 0];
    frame.extend_from_slice(&timestamp.to_le_bytes());
    body.encode(&mut frame).unwrap();
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames made by the agent's own encoder (agent-payload v5.0.207), one per encoding, each
    /// carrying the same CollectorProc.
    fn frame(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../api/tests/Intake.Tests/Fixtures/frames")
            .join(name);
        std::fs::read(path).unwrap()
    }

    fn assert_the_process(data: &[u8], encoding: u8) {
        let decoded = decode(data).unwrap();
        assert_eq!(decoded.header.version, 3);
        assert_eq!(decoded.header.encoding, encoding);
        assert_eq!(decoded.header.message_type, 12);
        assert_eq!(decoded.header.timestamp, 1790151330);
        let Body::Proc(proc) = decoded.body else { panic!("not a CollectorProc") };
        assert_eq!(proc.host_name, "host-a");
        assert_eq!(proc.network_id, "vpc-1");
        assert_eq!(proc.group_id, 7);
        assert_eq!(proc.processes.len(), 1);
        assert_eq!(proc.processes[0].pid, 42);
        assert_eq!(proc.processes[0].command.as_ref().unwrap().args, ["nginx", "-g"]);
    }

    #[test]
    fn plain_protobuf() {
        assert_the_process(&frame("frame_enc0.bin"), 0);
    }

    #[test]
    fn json() {
        assert_the_process(&frame("frame_enc1.bin"), 1);
    }

    #[test]
    fn zstd_1x_from_the_agents_cgo_build() {
        assert_the_process(&frame("frame_enc4.bin"), 4);
    }

    #[test]
    fn zstd_from_the_agents_pure_go_build() {
        assert_the_process(&frame("frame_enc5.bin"), 5);
    }

    /// No current library reads it; the frame is kept raw by the caller.
    #[test]
    fn zstd_0x_the_legacy_encoding_is_refused_by_name() {
        assert_eq!(
            decode(&frame("frame_enc2.bin")).map(|_| ()),
            Err("zstd 0.x frames (message encoding 2) are not supported".into())
        );
    }

    /// POST /api/v1/collector as a real datadog/agent 7.84.0 sent it (hostname "probe"): what
    /// current agents put on the wire is encoding 4.
    #[test]
    fn a_frame_from_a_real_agent() {
        let decoded = decode(&frame("frame_agent_7_84_0.bin")).unwrap();
        assert_eq!(decoded.header.encoding, 4);
        assert_eq!(decoded.header.message_type, 12);
        let Body::Proc(proc) = decoded.body else { panic!("not a CollectorProc") };
        assert_eq!(proc.host_name, "probe");
        assert!(
            proc.processes
                .iter()
                .any(|p| p.command.as_ref().is_some_and(|c| c.args.iter().any(|a| a.contains("agent"))))
        );
    }

    #[test]
    fn header_problems_are_named_as_the_agents_library_names_them() {
        let problem = |data: &[u8]| decode(data).map(|_| ()).unwrap_err();
        let padded = |head: &[u8], zeros: usize| [head, &vec![0; zeros]].concat();
        assert_eq!(problem(&[3, 0, 12]), "invalid message length: 3");
        assert_eq!(problem(&padded(&[9], 16)), "invalid message version: 9");
        assert_eq!(problem(&padded(&[3, 0, 99], 14)), "unhandled message type: 99");
        assert_eq!(problem(&padded(&[3, 3, 12], 14)), "unknown message encoding: 3");
    }

    #[test]
    fn what_encode_writes_decode_reads() {
        let body = ResCollector { message: "ok".into(), ..Default::default() };
        let decoded = decode(&encode(23, 5, &body)).unwrap();
        assert_eq!(decoded.header.message_type, 23);
        assert_eq!(decoded.header.timestamp, 5);
        assert_eq!(decoded.body, Body::Res(body));
    }
}
