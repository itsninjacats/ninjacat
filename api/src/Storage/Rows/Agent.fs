/// What the agent says about itself and its host on api.<site>, beside its
/// telemetry: its metadata, its checks, the tags of hosts it does not run on.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// Who sent a sketch batch: CommonMetadata without its api_key, which is a
/// credential and has no column.
type AgentBatchMetadataRow =
    { TenantID: string
      ReceivedAt: DateTime
      Intake: string
      AgentVersion: string
      Timezone: string
      /// The agent's own clock at send time, in seconds. Against ReceivedAt
      /// it measures clock skew.
      CurrentEpoch: float
      InternalIP: string
      PublicIP: string }

/// One entry of the V5 collector's agent_checks list. The wire shape is a
/// positional array with no key names: the five documented positions are
/// columns, anything after them is kept as a JSON array.
type AgentCheckRow =
    { TenantID: string
      ReceivedAt: DateTime
      Hostname: string
      AgentVersion: string
      UUID: string
      CheckName: string
      SourceType: string
      InstanceID: string
      /// None when the array stopped before position 3: status 0 means OK,
      /// and a truncated entry must not read as healthy.
      Status: int64 option
      Message: string
      PositionalExtra: string
      Meta: string }

/// Tags an agent reported for a host it is NOT running on, one row per
/// (host, source).
type ExternalHostTagsRow =
    { TenantID: string
      ReceivedAt: DateTime
      Host: string
      Source: string
      Tags: Map<string, string[]> }

/// One variant of /api/v1/metadata: the shared envelope as columns, the
/// variant's own body as JSON text.
type AgentMetadataRow =
    { TenantID: string
      ReceivedAt: DateTime
      Variant: string
      Hostname: string
      ClusterName: string
      ClusterID: string
      /// The envelope's own timestamp; optional, and 1970 is not an answer.
      Timestamp: DateTime option
      UUID: string
      Payload: string
      EnvelopeExtra: Map<string, string> }

/// One /api/v2/intake-key exchange. ProofFingerprint is the SHA-256 of the
/// delegated-auth proof, never the proof.
type DelegatedAuthRow =
    { TenantID: string
      At: DateTime
      Scheme: string
      ProofFingerprint: string
      APIKeyID: string }

/// The processes of one name on a host, summed: what the agent's resources
/// check reports beside the host metadata.
type ProcessGroupRow =
    { TenantID: string
      /// The agent's clock when it took the snapshot.
      Timestamp: DateTime
      Host: string
      Name: string
      Usernames: string[]
      ProcessCount: uint32
      CPUPct: float
      MemPct: float
      VMS: uint64
      RSS: uint64 }

module ProcessGroups =
    let table: Table<ProcessGroupRow> =
        { Table.create
              "storage_process_groups"
              "process_groups"
              [ "tenant_id"; "timestamp"; "host"; "name"; "usernames"; "process_count"; "cpu_pct"; "mem_pct"; "vms"; "rss" ]
              (fun (r: ProcessGroupRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.Name; r.Usernames; r.ProcessCount; r.CPUPct; r.MemPct; r.VMS; r.RSS |])
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 10.0 }

module AgentBatchMetadata =
    let table: Table<AgentBatchMetadataRow> =
        { Table.create
              "storage_agent_batch_metadata"
              "agent_batch_metadata"
              [ "tenant_id"; "received_at"; "intake"; "agent_version"; "timezone"; "current_epoch"
                "internal_ip"; "public_ip" ]
              (fun (r: AgentBatchMetadataRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Intake; r.AgentVersion; r.Timezone; r.CurrentEpoch
                     r.InternalIP; r.PublicIP |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module AgentChecks =
    let table: Table<AgentCheckRow> =
        { Table.create
              "storage_agent_checks"
              "agent_checks"
              [ "tenant_id"; "received_at"; "hostname"; "agent_version"; "uuid"
                "check_name"; "source_type"; "instance_id"; "status"; "message"
                "positional_extra"; "meta" ]
              (fun (r: AgentCheckRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Hostname; r.AgentVersion; r.UUID
                     r.CheckName; r.SourceType; r.InstanceID; Col.opt r.Status; r.Message
                     r.PositionalExtra; r.Meta |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module ExternalHostTags =
    let table: Table<ExternalHostTagsRow> =
        { Table.create
              "storage_external_host_tags"
              "external_host_tags"
              [ "tenant_id"; "received_at"; "host"; "source"; "tags" ]
              (fun (r: ExternalHostTagsRow) -> [| r.TenantID; r.ReceivedAt; r.Host; r.Source; r.Tags |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module AgentMetadata =
    /// A row is a whole inventory document, so the counts stand for megabytes.
    let table: Table<AgentMetadataRow> =
        { Table.create
              "storage_agent_metadata"
              "agent_metadata"
              [ "tenant_id"; "received_at"; "variant"; "hostname"; "cluster_name"; "cluster_id"
                "timestamp"; "uuid"; "payload"; "envelope_extra" ]
              (fun (r: AgentMetadataRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Variant; r.Hostname; r.ClusterName; r.ClusterID
                     Col.opt r.Timestamp; r.UUID; r.Payload; r.EnvelopeExtra |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

module DelegatedAuth =
    let table: Table<DelegatedAuthRow> =
        { Table.create
              "storage_delegated_auth_requests"
              "delegated_auth_requests"
              [ "tenant_id"; "at"; "scheme"; "proof_fingerprint"; "api_key_id" ]
              (fun (r: DelegatedAuthRow) -> [| r.TenantID; r.At; r.Scheme; r.ProofFingerprint; r.APIKeyID |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }
