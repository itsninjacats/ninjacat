/// The Private Action Runner: its enrolment, heartbeats and task traffic.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

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
