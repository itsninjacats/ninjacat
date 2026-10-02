namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One ECS task in one collection pass. It arrives on the orchestrator
/// intake with the Kubernetes collectors but has a table of its own: a task
/// has no namespace, name or uid, the columns k8s_resources sorts by.
///
/// Four tag sets, kept apart: the frame envelope's, and the three ECS itself
/// keeps separate. Merging them would answer "is this env:prod" while
/// destroying "who said so".
type ECSTaskRow =
    { TenantID: string
      CollectedAt: DateTime
      // The frame header, as on every orchestrator row.
      OrgID: int32
      SubscriptionID: uint8
      HeaderTimestamp: int64 option
      Encoding: string
      // The envelope.
      AWSAccountID: int64
      ClusterID: string
      ClusterName: string
      Region: string
      GroupID: int32
      GroupSize: int32
      HostName: string
      AgentVersion: string
      EnvelopeTags: Map<string, string[]>
      // The task.
      ARN: string
      ResourceVersion: string
      LaunchType: string
      DesiredStatus: string
      KnownStatus: string
      Family: string
      Version: string
      AvailabilityZone: string
      ServiceName: string
      VpcID: string
      ContainerInstanceARN: string
      DaemonName: string
      TaskHostName: string
      Limits: Map<string, float>
      EphemeralStorageMetrics: Map<string, int64>
      PullStartedAt: DateTime option
      PullStoppedAt: DateTime option
      ExecutionStoppedAt: DateTime option
      /// JSON: a container nests ports, networks, volumes, health and log
      /// options, too deep for parallel arrays.
      Containers: string
      ContainerCount: uint32
      Tags: Map<string, string[]>
      ECSTags: Map<string, string[]>
      /// These belong to the machine the task runs on, not to the workload.
      ContainerInstanceTags: Map<string, string[]>
      /// The frame's message as JSON, without its task list.
      Envelope: string
      /// The task's Host message as JSON; TaskHostName is only its name.
      TaskHost: string }

module ECSTasks =
    let table: Table<ECSTaskRow> =
        { Table.create
              "storage_ecs_tasks"
              "ecs_tasks"
              [ "tenant_id"; "collected_at"; "org_id"; "subscription_id"; "header_timestamp"; "encoding"
                "aws_account_id"; "cluster_id"; "cluster_name"; "region"; "group_id"; "group_size"
                "host_name"; "agent_version"; "envelope_tags"
                "arn"; "resource_version"; "launch_type"; "desired_status"; "known_status"
                "family"; "version"; "availability_zone"; "service_name"; "vpc_id"
                "container_instance_arn"; "daemon_name"; "task_host_name"
                "limits"; "ephemeral_storage_metrics"
                "pull_started_at"; "pull_stopped_at"; "execution_stopped_at"
                "containers"; "container_count"
                "tags"; "ecs_tags"; "container_instance_tags"
                "envelope"; "task_host" ]
              (fun (r: ECSTaskRow) ->
                  [| r.TenantID; r.CollectedAt; r.OrgID; r.SubscriptionID; Col.opt r.HeaderTimestamp; r.Encoding
                     r.AWSAccountID; r.ClusterID; r.ClusterName; r.Region; r.GroupID; r.GroupSize
                     r.HostName; r.AgentVersion; r.EnvelopeTags
                     r.ARN; r.ResourceVersion; r.LaunchType; r.DesiredStatus; r.KnownStatus
                     r.Family; r.Version; r.AvailabilityZone; r.ServiceName; r.VpcID
                     r.ContainerInstanceARN; r.DaemonName; r.TaskHostName
                     r.Limits; r.EphemeralStorageMetrics
                     Col.opt r.PullStartedAt; Col.opt r.PullStoppedAt; Col.opt r.ExecutionStoppedAt
                     r.Containers; r.ContainerCount
                     r.Tags; r.ECSTags; r.ContainerInstanceTags
                     r.Envelope; r.TaskHost |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 5_000
              MaxInFlight = 2 }
