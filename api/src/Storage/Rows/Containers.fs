namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One container, pod or task lifecycle event: the restart and OOM history.
///
/// An option wherever the wire has a proto3 optional: exit 0 and "no code
/// reported" are different facts, and a missing time must not become 1970.
///
/// A state transition is stored twice on purpose: OldState/NewState are the
/// label a dashboard shows, the structured fields beside them are what "every
/// OOM kill with signal 9" needs.
type ContainerEventRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      ClusterID: string
      ObjectKind: string
      EventType: string
      ContainerID: string
      ContainerName: string
      PodUID: string
      TaskARN: string
      Source: string
      ExitCode: int32 option
      CreatedAt: DateTime option
      ExitedAt: DateTime option
      OwnerType: string
      OwnerUID: string
      OldState: string
      NewState: string
      TransitionAt: DateTime option
      /// The schema version of the batch, not of an event in it.
      PayloadVersion: string
      /// "container", "pod", "task", or "unknown" for an event with no typed
      /// detail — which otherwise looks like a typed event with empty fields.
      EventVariant: string
      ContainerKind: string
      Precision: string
      MissedIntermediate: string
      OldStateKind: string
      OldReason: string option
      OldExitCode: int32 option
      OldSignal: int32 option
      NewStateKind: string
      NewReason: string option
      NewExitCode: int32 option
      NewSignal: int32 option
      /// Which pod status field moved. Its value is a phase or a condition;
      /// *StateVariant says which, and each has its own columns.
      PodStatusField: string
      OldStateVariant: string
      OldPhase: string option
      OldConditionType: string option
      OldConditionStatus: string option
      OldConditionReason: string option
      OldConditionMessage: string option
      NewStateVariant: string
      NewPhase: string option
      NewConditionType: string option
      NewConditionStatus: string option
      NewConditionReason: string option
      NewConditionMessage: string option
      /// 1 when the sender set ContainerName at all. The name's own column
      /// cannot become Nullable without rewriting every stored row.
      ContainerNamePresent: uint8 }

/// One image as one host holds it. The table replaces on (tenant, image key,
/// host), so Host is part of the row's identity.
///
/// The Layer* arrays are parallel and in wire order: an image is its ordered
/// layer stack, so nothing is sorted or deduplicated.
type ContainerImageRow =
    { TenantID: string
      CollectedAt: DateTime
      Host: string
      /// The registry digest when there is one, the image id otherwise.
      ImageKey: string
      /// "digest" or "image_id".
      IdentitySource: string
      ImageID: string
      Digest: string
      Name: string
      ShortName: string
      Registry: string
      RepoTags: string[]
      RepoDigests: string[]
      SizeBytes: uint64
      OSName: string
      OSVersion: string
      Architecture: string
      LayerCount: uint32
      LayerBytes: uint64
      BuiltAt: DateTime option
      PublishedAt: DateTime option
      DDTags: Map<string, string[]>
      PayloadVersion: string
      /// Which pipeline produced the inventory ("agent", "other"); None when
      /// the sender did not say.
      Source: string option
      LayerMediaTypes: string[]
      LayerDigests: string[]
      /// As sent, negative values included.
      LayerSizes: int64[]
      LayerURLs: string[][]
      LayerHistoryCreated: DateTime option[]
      LayerHistoryCreatedBy: string[]
      LayerHistoryAuthor: string[]
      LayerHistoryComment: string[]
      LayerHistoryEmptyLayer: uint8[]
      /// 1 when the image's size was negative and SizeBytes was clamped to 0.
      SizeNegative: uint8
      /// 1 when any layer's size was negative: LayerBytes cannot be trusted.
      LayerSizeNegative: uint8 }

module ContainerEvents =
    let table: Table<ContainerEventRow> =
        { Table.create
              "storage_container_events"
              "container_events"
              [ "tenant_id"; "timestamp"; "host"; "cluster_id"; "object_kind"; "event_type"
                "container_id"; "container_name"; "pod_uid"; "task_arn"; "source"
                "exit_code"; "created_at"; "exited_at"; "owner_type"; "owner_uid"
                "old_state"; "new_state"; "transition_at"
                "payload_version"; "event_variant"
                "container_kind"; "precision"; "missed_intermediate"
                "old_state_kind"; "old_reason"; "old_exit_code"; "old_signal"
                "new_state_kind"; "new_reason"; "new_exit_code"; "new_signal"
                "pod_status_field"
                "old_state_variant"; "old_phase"; "old_condition_type"; "old_condition_status"
                "old_condition_reason"; "old_condition_message"
                "new_state_variant"; "new_phase"; "new_condition_type"; "new_condition_status"
                "new_condition_reason"; "new_condition_message"
                "container_name_present" ]
              (fun (r: ContainerEventRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.ClusterID; r.ObjectKind; r.EventType
                     r.ContainerID; r.ContainerName; r.PodUID; r.TaskARN; r.Source
                     Col.opt r.ExitCode; Col.opt r.CreatedAt; Col.opt r.ExitedAt; r.OwnerType; r.OwnerUID
                     r.OldState; r.NewState; Col.opt r.TransitionAt
                     r.PayloadVersion; r.EventVariant
                     r.ContainerKind; r.Precision; r.MissedIntermediate
                     r.OldStateKind; Col.opt r.OldReason; Col.opt r.OldExitCode; Col.opt r.OldSignal
                     r.NewStateKind; Col.opt r.NewReason; Col.opt r.NewExitCode; Col.opt r.NewSignal
                     r.PodStatusField
                     r.OldStateVariant; Col.opt r.OldPhase; Col.opt r.OldConditionType; Col.opt r.OldConditionStatus
                     Col.opt r.OldConditionReason; Col.opt r.OldConditionMessage
                     r.NewStateVariant; Col.opt r.NewPhase; Col.opt r.NewConditionType; Col.opt r.NewConditionStatus
                     Col.opt r.NewConditionReason; Col.opt r.NewConditionMessage
                     r.ContainerNamePresent |])
          with
              // A bad rollout turns restarts into a storm, and each row is a
              // crash somebody will look for: a deep buffer, two flushes.
              MaxRows = 1000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module ContainerImages =
    let table: Table<ContainerImageRow> =
        { Table.create
              "storage_container_images"
              "container_images"
              [ "tenant_id"; "collected_at"; "host"; "image_key"; "identity_source"
                "image_id"; "digest"; "name"; "short_name"
                "registry"; "repo_tags"; "repo_digests"; "size_bytes"; "os_name"; "os_version"
                "architecture"; "layer_count"; "layer_bytes"; "built_at"; "published_at"; "dd_tags"
                "payload_version"; "source"
                "layer_media_types"; "layer_digests"; "layer_sizes"; "layer_urls"
                "layer_history_created"; "layer_history_created_by"; "layer_history_author"
                "layer_history_comment"; "layer_history_empty_layer"
                "size_negative"; "layer_size_negative" ]
              (fun (r: ContainerImageRow) ->
                  [| r.TenantID; r.CollectedAt; r.Host; r.ImageKey; r.IdentitySource
                     r.ImageID; r.Digest; r.Name; r.ShortName
                     r.Registry; r.RepoTags; r.RepoDigests; r.SizeBytes; r.OSName; r.OSVersion
                     r.Architecture; r.LayerCount; r.LayerBytes; Col.opt r.BuiltAt; Col.opt r.PublishedAt; r.DDTags
                     r.PayloadVersion; Col.opt r.Source
                     r.LayerMediaTypes; r.LayerDigests; r.LayerSizes; r.LayerURLs
                     Array.map Option.toNullable r.LayerHistoryCreated; r.LayerHistoryCreatedBy; r.LayerHistoryAuthor
                     r.LayerHistoryComment; r.LayerHistoryEmptyLayer
                     r.SizeNegative; r.LayerSizeNegative |])
          with
              // Every node re-announces every image it holds: bursty and
              // repetitive. The table's engine absorbs the repetition.
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 20_000
              MaxInFlight = 2 }
