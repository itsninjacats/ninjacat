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

/// One "which of these build ids do you hold?" from the profiler's symbol
/// uploader. BuildIDs keeps its order: the reply is positional against it.
type SymbolQueryRow =
    { TenantID: string
      At: DateTime
      Arch: string
      BuildIDs: string[]
      Resource: string }

/// One Private Action Runner identity. The table is a ReplacingMergeTree on
/// (tenant_id, runner_id).
type RunnerEnrollmentRow =
    { TenantID: string
      RunnerID: string
      EnrolledAt: DateTime
      Name: string
      Modes: string[]
      Host: string
      /// The whole PEM block: without it no signature of the runner's can
      /// ever be verified.
      PublicKeyPEM: string
      AgentHostname: string
      OrchClusterID: string
      AgentFlavor: string
      /// What we answered with. The derivation from the tenant is ours, so
      /// only this row remembers it if the derivation changes.
      OrgID: int64
      /// The request's attributes as JSON, for a native JSON column.
      Attributes: string }

/// The outcome of one action. Outcome is "succeed_task" or "fail_task".
type RunnerTaskUpdateRow =
    { TenantID: string
      At: DateTime
      Outcome: string
      TaskID: string
      ActionFQN: string
      JobID: string
      Client: string
      Branch: string
      Outputs: string
      /// A numeric enum on the runner's side; a succeeded task sends none.
      ErrorCode: int64 option
      ErrorDetails: string
      APIError: string
      /// Undeclared attributes, values JSON-encoded; "payload."-prefixed
      /// when they came from the nested object.
      Extra: Map<string, string> }

type RunnerHeartbeatRow =
    { TenantID: string
      At: DateTime
      TaskID: string
      ActionFQN: string
      JobID: string
      Client: string }

/// One idle poll for work: the only liveness signal of a runner with no
/// tasks. Both timestamps stay strings: their format is undocumented.
type RunnerDequeueRow =
    { TenantID: string
      At: DateTime
      RunnerStartedAt: string
      LastTaskReceivedAt: string
      Version: string
      Modes: string }

/// A connection an action runner may use to reach a third-party system.
/// SENSITIVE: Credentials holds that integration's secrets verbatim.
type ActionConnectionRow =
    { TenantID: string
      At: DateTime
      Name: string
      RunnerID: string
      Tags: Map<string, string[]>
      IntegrationType: string
      Credentials: string
      Extra: Map<string, string> }

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

module SymbolQueries =
    let table: Table<SymbolQueryRow> =
        { Table.create
              "storage_symbol_queries"
              "symbol_queries"
              [ "tenant_id"; "at"; "arch"; "build_ids"; "resource" ]
              (fun (r: SymbolQueryRow) -> [| r.TenantID; r.At; r.Arch; r.BuildIDs; r.Resource |])
          with
              MaxRows = 50
              FlushInterval = TimeSpan.FromSeconds 15.0
              BufferLimit = 1_000
              MaxInFlight = 1 }

module RunnerEnrollments =
    let table: Table<RunnerEnrollmentRow> =
        { Table.create
              "storage_runner_enrollments"
              "runner_enrollments"
              [ "tenant_id"; "runner_id"; "enrolled_at"; "name"; "modes"; "host"; "public_key_pem"
                "agent_hostname"; "orch_cluster_id"; "agent_flavor"; "org_id"; "attributes" ]
              (fun (r: RunnerEnrollmentRow) ->
                  [| r.TenantID; r.RunnerID; r.EnrolledAt; r.Name; r.Modes; r.Host; r.PublicKeyPEM
                     r.AgentHostname; r.OrchClusterID; r.AgentFlavor; r.OrgID; Col.jsonObject r.Attributes |])
          with
              MaxRows = 100
              FlushInterval = TimeSpan.FromSeconds 10.0 }

module RunnerTaskUpdates =
    let table: Table<RunnerTaskUpdateRow> =
        { Table.create
              "storage_runner_task_updates"
              "runner_task_updates"
              [ "tenant_id"; "at"; "outcome"; "task_id"; "action_fqn"; "job_id"; "client"
                "branch"; "outputs"; "error_code"; "error_details"; "api_error"; "extra" ]
              (fun (r: RunnerTaskUpdateRow) ->
                  [| r.TenantID; r.At; r.Outcome; r.TaskID; r.ActionFQN; r.JobID; r.Client
                     r.Branch; r.Outputs; Col.opt r.ErrorCode; r.ErrorDetails; r.APIError; r.Extra |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }

module RunnerHeartbeats =
    let table: Table<RunnerHeartbeatRow> =
        { Table.create
              "storage_runner_heartbeats"
              "runner_heartbeats"
              [ "tenant_id"; "at"; "task_id"; "action_fqn"; "job_id"; "client" ]
              (fun (r: RunnerHeartbeatRow) -> [| r.TenantID; r.At; r.TaskID; r.ActionFQN; r.JobID; r.Client |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module RunnerDequeues =
    let table: Table<RunnerDequeueRow> =
        { Table.create
              "storage_runner_dequeues"
              "runner_dequeues"
              [ "tenant_id"; "at"; "runner_started_at"; "last_task_received_at"; "version"; "modes" ]
              (fun (r: RunnerDequeueRow) -> [| r.TenantID; r.At; r.RunnerStartedAt; r.LastTaskReceivedAt; r.Version; r.Modes |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module ActionConnections =
    let table: Table<ActionConnectionRow> =
        { Table.create
              "storage_action_connections"
              "action_connections"
              [ "tenant_id"; "at"; "name"; "runner_id"; "tags"; "integration_type"; "credentials"; "extra" ]
              (fun (r: ActionConnectionRow) ->
                  [| r.TenantID; r.At; r.Name; r.RunnerID; r.Tags; r.IntegrationType; r.Credentials; r.Extra |])
          with
              MaxRows = 50
              FlushInterval = TimeSpan.FromSeconds 15.0
              BufferLimit = 1_000
              MaxInFlight = 1 }
