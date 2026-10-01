/// kubeops-intake.<site>, orchestrator.<site> — Kubernetes.
///
///   agent config: orchestrator_explorer.orchestrator_dd_url
///                 / DD_ORCHESTRATOR_EXPLORER_ORCHESTRATOR_DD_URL
///
/// Sent by the CLUSTER agent, not the node agent. orch and orchmanif carry
/// the same frame as the process agent's /api/v1/collector, with other
/// message types (41..88, 200). kubeactions is different: event platform,
/// JSON.
///
/// Object collections land in k8s_resources, raw manifests in k8s_manifests,
/// the cluster summary in k8s_cluster, ECS tasks in ecs_tasks, action results
/// in k8s_actions. A frame that does not decode, and a message that is not an
/// orchestrator one, goes to raw_payloads.
module NinjaCat.Api.Intake.Routers.Kubeops

open System
open System.Collections
open System.Globalization
open System.IO
open System.Linq
open System.Reflection
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.Unicode
open Google.Protobuf
open Google.Protobuf.Collections
open Google.Protobuf.Reflection
open Microsoft.Extensions.Logging
open Datadog.ProcessAgent
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// JSON the way Go's encoding/json wrote the agent's generated structs, which
/// is what the JSON columns of these tables have held from the start. It is
/// not protobuf's own JSON: a field at its zero value is left out, 64-bit
/// integers and enums are numbers, bytes are base64, and an empty list on its
/// own is `null`.
module GoJson =
    let private isZero (value: obj) : bool =
        match value with
        | null -> true
        | :? string as text -> text = ""
        | :? bool as flag -> not flag
        | :? int32 as n -> n = 0
        | :? int64 as n -> n = 0L
        | :? uint32 as n -> n = 0u
        | :? uint64 as n -> n = 0UL
        | :? float32 as n -> n = 0.0f
        | :? float as n -> n = 0.0
        | :? Enum as number -> Convert.ToInt32 number = 0
        | :? ByteString as bytes -> bytes.IsEmpty
        | :? ICollection as items -> items.Count = 0
        | _ -> false

    let private mapKey (key: obj) : string =
        match key with
        | :? string as text -> text
        | :? bool as flag -> if flag then "true" else "false"
        | other -> Convert.ToString(other, CultureInfo.InvariantCulture)

    let rec private writeValue (writer: Utf8JsonWriter) (value: obj) : unit =
        match value with
        | null -> writer.WriteNullValue()
        | :? string as text -> writer.WriteStringValue text
        | :? bool as flag -> writer.WriteBooleanValue flag
        | :? int32 as n -> writer.WriteNumberValue n
        | :? int64 as n -> writer.WriteNumberValue n
        | :? uint32 as n -> writer.WriteNumberValue n
        | :? uint64 as n -> writer.WriteNumberValue n
        | :? float32 as n -> writer.WriteNumberValue n
        | :? float as n -> writer.WriteNumberValue n
        | :? Enum as number -> writer.WriteNumberValue(Convert.ToInt32 number)
        | :? ByteString as bytes -> writer.WriteBase64StringValue bytes.Span
        | :? IMessage as message -> writeMessage writer message None
        | :? IDictionary as map ->
            let keys = map.Keys |> Seq.cast<obj> |> Seq.sortWith (fun a b -> String.CompareOrdinal(mapKey a, mapKey b))
            writer.WriteStartObject()

            for key in keys do
                writer.WritePropertyName(mapKey key)
                writeValue writer map[key]

            writer.WriteEndObject()
        | :? IList as items ->
            writer.WriteStartArray()

            for item in items do
                writeValue writer item

            writer.WriteEndArray()
        | other -> invalidArg "value" $"no JSON form for {other.GetType().FullName}"

    and private writeMessage (writer: Utf8JsonWriter) (message: IMessage) (leftOut: FieldDescriptor option) : unit =
        writer.WriteStartObject()

        for field in message.Descriptor.Fields.InDeclarationOrder() do
            let value = field.Accessor.GetValue message

            if Some field <> leftOut && not (isZero value) then
                writer.WritePropertyName field.Name
                writeValue writer value

        writer.WriteEndObject()

    /// "" when a value has no JSON form (NaN, infinity): the column is extra
    /// detail, and losing the row over it would be the worse trade.
    let private text (write: Utf8JsonWriter -> unit) : string =
        try
            use buffer = new MemoryStream()

            do
                use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
                write writer

            Encoding.UTF8.GetString(buffer.ToArray())
        with :? ArgumentException ->
            ""

    /// A message, a list of them, or a map. A missing message and an empty
    /// list are both `null`.
    let render (value: obj) : string =
        match value with
        | :? ICollection as items when items.Count = 0 -> "null"
        | _ -> text (fun writer -> writeValue writer value)

    /// A message without one of its fields.
    let renderWithout (leftOut: FieldDescriptor) (message: IMessage) : string =
        text (fun writer -> writeMessage writer message (Some leftOut))

/// An enum value under its name in the .proto, which is what the Go server
/// stored; the number when the schema has no name for it.
let private protoName (value: 'enum when 'enum: enum<int32>) : string =
    let name = string value

    match typeof<'enum>.GetField name with
    | null -> name
    | field -> field.GetCustomAttribute<OriginalNameAttribute>().Name

/// The frame header as the four columns every orchestrator row carries.
type FrameColumns =
    { OrgID: int32
      SubscriptionID: uint8
      /// The header's raw integer; None when the wire said 0, "not provided".
      Timestamp: int64 option
      Encoding: string }

/// An unknown encoding keeps its number rather than becoming "".
let encodingName (encoding: uint8) : string =
    match encoding with
    | 0uy -> "protobuf"
    | 1uy -> "json"
    | 2uy -> "zstd_protobuf"
    | 4uy -> "zstd1x_protobuf"
    | 5uy -> "zstd_protobuf_nocgo"
    | other -> $"unknown_{other}"

let frameColumns (header: FrameHeader) : FrameColumns =
    { OrgID = header.OrgID
      SubscriptionID = header.SubscriptionID
      Timestamp = (if header.Timestamp <> 0L then Some header.Timestamp else None)
      Encoding = encodingName header.Encoding }

/// Unix seconds for a nullable column: 0 means the sender did not supply the
/// time, and it must never become 1970.
let private seconds (value: int64) : DateTime option = Time.optionalSeconds true value

/// major.minor.patch, with the prerelease tag when set.
let private agentVersionText (version: AgentVersion) : string =
    if isNull version then
        ""
    else
        let numbers = $"{version.Major}.{version.Minor}.{version.Patch}"
        if version.Pre = "" then numbers else numbers + "-" + version.Pre

let private toMap (field: MapField<'key, 'value>) : Map<'key, 'value> =
    field |> Seq.map (fun pair -> pair.Key, pair.Value) |> Map.ofSeq

/// A sub-message the sender left out reads as an empty one.
let private orEmpty (message: 'message) : 'message when 'message: null and 'message: (new: unit -> 'message) =
    if isNull message then new 'message() else message

// ---- object collections → k8s_resources ----

// The 24 collectors share a shape but no type: the same envelope fields and
// one list of objects, each with a Metadata. So one converter reads them by
// field name instead of 24 near-identical branches. The names are the Go
// server's: the .proto name with its first letter in upper case.

let private isNamed (name: string) (field: FieldDescriptor) : bool =
    field.Name.Length = name.Length
    && Char.ToUpperInvariant field.Name[0] = name[0]
    && String.CompareOrdinal(field.Name, 1, name, 1, name.Length - 1) = 0

let private fieldValue (message: IMessage) (name: string) : obj =
    match message.Descriptor.Fields.InDeclarationOrder() |> Seq.tryFind (isNamed name) with
    | Some field -> field.Accessor.GetValue message
    | None -> null

let private stringField (message: IMessage) (name: string) : string =
    match fieldValue message name with
    | :? string as text -> text
    | _ -> ""

let private int32Field (message: IMessage) (name: string) : int32 =
    match fieldValue message name with
    | :? int32 as n -> n
    | _ -> 0

let private int64Field (message: IMessage) (name: string) : int64 =
    match fieldValue message name with
    | :? int64 as n -> n
    | :? int32 as n -> int64 n
    | _ -> 0L

let private messageField (message: IMessage) (name: string) : IMessage option =
    match fieldValue message name with
    | :? IMessage as inner -> Some inner
    | _ -> None

let private stringList (message: IMessage) (name: string) : string list =
    match fieldValue message name with
    | :? RepeatedField<string> as items -> List.ofSeq items
    | _ -> []

/// The named list when the message has one and it is not empty.
let private filledList (message: IMessage) (name: string) : IList option =
    match fieldValue message name with
    | :? IList as items when items.Count > 0 -> Some items
    | _ -> None

let private firstNonEmptyString (message: IMessage) (names: string list) : string =
    names |> List.map (stringField message) |> Text.firstNonEmpty

/// What every kind can carry but only some do: conditions, the agent's
/// condition summary, resource requirements and computed metrics.
///
/// Read by name because each kind declares its own condition message
/// (PodCondition, NodeCondition, DeploymentCondition, ...) with the same
/// field names and slightly different sets, and because these live on the
/// object for some kinds and inside Status or Spec for others.
let private withCommonFields (object: IMessage) (row: K8sResourceRow) : K8sResourceRow =
    let status = messageField object "Status"

    let conditionsOf (owner: IMessage) : IMessage list =
        match fieldValue owner "Conditions" with
        | :? IList as items -> List.ofSeq (Enumerable.OfType<IMessage> items)
        | _ -> []

    // A VerticalPodAutoscaler has two condition lists, on the object and in
    // Status, and they are different messages; the nested one is what a real
    // autoscaler fills. Both are read, the object's first.
    let conditions =
        match status with
        | Some status -> conditionsOf object @ conditionsOf status
        | None -> conditionsOf object

    let conditionTimes (name: string) : DateTime option[] =
        conditions |> List.map (fun condition -> seconds (int64Field condition name)) |> Array.ofList

    let conditionMessage =
        match stringField object "ConditionMessage", status with
        | "", Some status -> stringField status "ConditionMessage"
        | summary, _ -> summary

    // A kind can declare the list in both places and fill only the inner
    // one, so a filled list wins wherever it sits.
    let resourceRequirements =
        match filledList object "ResourceRequirements", messageField object "Spec" with
        | Some items, _ -> Some items
        | None, Some spec -> filledList spec "ResourceRequirements"
        | None, None -> None

    { row with
        // Most conditions call their fields Type/Status; the autoscalers'
        // call them ConditionType/ConditionStatus.
        ConditionTypes = conditions |> List.map (fun c -> firstNonEmptyString c [ "Type"; "ConditionType" ]) |> Array.ofList
        ConditionStatuses = conditions |> List.map (fun c -> firstNonEmptyString c [ "Status"; "ConditionStatus" ]) |> Array.ofList
        ConditionReasons = conditions |> List.map (fun c -> stringField c "Reason") |> Array.ofList
        ConditionMessages = conditions |> List.map (fun c -> stringField c "Message") |> Array.ofList
        ConditionLastTransition = conditionTimes "LastTransitionTime"
        ConditionLastUpdate = conditionTimes "LastUpdateTime"
        ConditionLastProbe = conditionTimes "LastProbeTime"
        ConditionMessage = conditionMessage
        ResourceRequirements =
            (match resourceRequirements with
             | Some items -> GoJson.render items
             | None -> "")
        Metrics =
            (match fieldValue object "Metrics" with
             | :? ResourceMetrics as metrics -> toMap metrics.MetricValues
             | _ -> Map.empty) }

/// The numbers and typed extras of the kinds people ask about by name. Every
/// kind names its counters differently, so this is the part that cannot be
/// read generically. A kind with no case here loses nothing: its whole
/// message is in the object column.
let private withKindFields (object: IMessage) (row: K8sResourceRow) : K8sResourceRow =
    match object with
    | :? Pod as pod ->
        // Ready and restarts are sums over the container statuses; the pod
        // has no field for them. Regular containers first, then init
        // containers, in one set of arrays: every question about a container
        // is the same question for both.
        let regular = Array.ofSeq pod.ContainerStatuses
        let init = Array.ofSeq pod.InitContainerStatuses
        let all = Array.append regular init
        let readyCount (statuses: ContainerStatus[]) = statuses |> Array.filter _.Ready |> Array.length
        let restarts (statuses: ContainerStatus[]) = statuses |> Array.sumBy _.RestartCount

        { row with
            NodeName = pod.NodeName
            Phase = pod.Phase
            Status = pod.Status
            // For a pod, ready/desired is containers ready / containers total.
            Ready = readyCount regular
            Desired = regular.Length
            Counts =
                Map
                    [ "restarts", int64 (restarts regular)
                      "ready_containers", int64 (readyCount regular)
                      "total_containers", int64 regular.Length
                      "init_restarts", int64 (restarts init)
                      "ready_init_containers", int64 (readyCount init)
                      "total_init_containers", int64 init.Length
                      // The agent's own aggregate, beside the recomputed one:
                      // if they ever disagree, that is worth seeing.
                      "pod_restart_count", int64 pod.RestartCount ]
            PodIP = pod.IP
            NominatedNodeName = pod.NominatedNodeName
            QOSClass = pod.QOSClass
            PriorityClass = pod.PriorityClass
            StartTime = seconds pod.StartTime
            ScheduledTime = seconds pod.ScheduledTime
            HostName = (if not (isNull pod.Host) && pod.Host.Name <> "" then pod.Host.Name else row.HostName)
            ContainerNames = all |> Array.map _.Name
            ContainerIDs = all |> Array.map _.ContainerID
            ContainerReady = all |> Array.map (fun status -> Text.flag status.Ready)
            ContainerRestarts = all |> Array.map _.RestartCount
            ContainerStates = all |> Array.map _.State
            ContainerMessages = all |> Array.map _.Message
            ContainerImages = all |> Array.map _.Image
            ContainerImageIDs = all |> Array.map _.ImageID
            ContainerIsInit = Array.append (Array.replicate regular.Length 0uy) (Array.replicate init.Length 1uy) }
    | :? Deployment as deployment ->
        { row with
            Ready = deployment.ReadyReplicas
            Desired = deployment.ReplicasDesired
            Available = deployment.AvailableReplicas
            Counts =
                Map
                    [ "ready", int64 deployment.ReadyReplicas
                      "desired", int64 deployment.ReplicasDesired
                      "updated", int64 deployment.UpdatedReplicas
                      "available", int64 deployment.AvailableReplicas
                      "unavailable", int64 deployment.UnavailableReplicas
                      // The observed total: during a rollout it differs from
                      // desired, and that difference is the rollout.
                      "replicas", int64 deployment.Replicas ] }
    | :? ReplicaSet as replicaSet ->
        { row with
            Ready = replicaSet.ReadyReplicas
            Desired = replicaSet.ReplicasDesired
            Available = replicaSet.AvailableReplicas
            Counts =
                Map
                    [ "ready", int64 replicaSet.ReadyReplicas
                      "desired", int64 replicaSet.ReplicasDesired
                      "available", int64 replicaSet.AvailableReplicas
                      "replicas", int64 replicaSet.Replicas
                      "fully_labeled", int64 replicaSet.FullyLabeledReplicas ] }
    | :? DaemonSet as daemonSet ->
        let status = orEmpty daemonSet.Status

        { row with
            Ready = status.NumberReady
            Desired = status.DesiredNumberScheduled
            Available = status.NumberAvailable
            Counts =
                Map
                    [ "ready", int64 status.NumberReady
                      "desired", int64 status.DesiredNumberScheduled
                      "current", int64 status.CurrentNumberScheduled
                      "updated", int64 status.UpdatedNumberScheduled
                      "available", int64 status.NumberAvailable
                      "misscheduled", int64 status.NumberMisscheduled
                      // The one that says a rollout is stuck.
                      "unavailable", int64 status.NumberUnavailable ] }
    | :? StatefulSet as statefulSet ->
        // Desired lives in the spec here, unlike the other workloads.
        let status = orEmpty statefulSet.Status
        let spec = orEmpty statefulSet.Spec

        { row with
            Ready = status.ReadyReplicas
            Desired = spec.DesiredReplicas
            Counts =
                Map
                    [ "ready", int64 status.ReadyReplicas
                      "desired", int64 spec.DesiredReplicas
                      "current", int64 status.CurrentReplicas
                      "updated", int64 status.UpdatedReplicas
                      "replicas", int64 status.Replicas
                      "partition", int64 spec.Partition ] }
    | :? Node as node ->
        let status = orEmpty node.Status

        { row with
            // "Ready", "NotReady": the one status a node has.
            Status = status.Status
            PodCIDR = node.PodCIDR
            PodCIDRs = Array.ofSeq node.PodCIDRs
            Unschedulable = Text.flag node.Unschedulable
            ProviderID = node.ProviderID
            NodeRoles = Array.ofSeq node.Roles
            Taints = (if node.Taints.Count > 0 then GoJson.render node.Taints else "")
            Capacity = toMap status.Capacity
            Allocatable = toMap status.Allocatable
            NodeAddresses = (if status.NodeAddresses.Count > 0 then GoJson.render status.NodeAddresses else "")
            KubeletVersion = status.KubeletVersion
            KubeProxyVersion = status.KubeProxyVersion
            OperatingSystem = status.OperatingSystem
            Architecture = status.Architecture
            KernelVersion = status.KernelVersion
            OSImage = status.OsImage
            ContainerRuntimeVersion = status.ContainerRuntimeVersion
            HostName = (if not (isNull node.Host) && node.Host.Name <> "" then node.Host.Name else row.HostName)
            // The images themselves are the contimage intake's job.
            Counts = Map [ "taints", int64 node.Taints.Count; "images", int64 status.Images.Count ] }
    | :? Namespace as ns -> { row with Status = ns.Status }
    | :? Service as service ->
        let spec = orEmpty service.Spec

        { row with
            ServiceType = spec.Type
            ClusterIP = spec.ClusterIP
            ServicePorts = (if spec.Ports.Count > 0 then GoJson.render spec.Ports else "")
            Counts = Map [ "ports", int64 spec.Ports.Count ] }
    | :? Role as role ->
        { row with
            RBACRules = GoJson.render role.Rules
            Counts = Map [ "rules", int64 role.Rules.Count ] }
    | :? ClusterRole as role ->
        { row with
            RBACRules = GoJson.render role.Rules
            Counts = Map [ "rules", int64 role.Rules.Count; "aggregation_rules", int64 role.AggregationRules.Count ] }
    | :? RoleBinding as binding ->
        { row with
            RBACSubjects = GoJson.render binding.Subjects
            RBACRoleRef = GoJson.render binding.RoleRef
            Counts = Map [ "subjects", int64 binding.Subjects.Count ] }
    | :? ClusterRoleBinding as binding ->
        { row with
            RBACSubjects = GoJson.render binding.Subjects
            RBACRoleRef = GoJson.render binding.RoleRef
            Counts = Map [ "subjects", int64 binding.Subjects.Count ] }
    | :? Job as job ->
        let status = orEmpty job.Status
        let spec = orEmpty job.Spec

        { row with
            Counts =
                Map
                    [ "active", int64 status.Active
                      "succeeded", int64 status.Succeeded
                      "failed", int64 status.Failed
                      "parallelism", int64 spec.Parallelism
                      "completions", int64 spec.Completions
                      "backoff_limit", int64 spec.BackoffLimit
                      "active_deadline_seconds", spec.ActiveDeadlineSeconds ]
            StartTime = seconds status.StartTime }
    | :? CronJob as cronJob ->
        let status = orEmpty cronJob.Status
        let spec = orEmpty cronJob.Spec

        { row with
            Counts =
                Map
                    [ "active_jobs", int64 status.Active.Count
                      "suspend", int64 (Text.flag spec.Suspend)
                      "successful_jobs_history_limit", int64 spec.SuccessfulJobsHistoryLimit
                      "failed_jobs_history_limit", int64 spec.FailedJobsHistoryLimit
                      "starting_deadline_seconds", spec.StartingDeadlineSeconds ] }
    | :? HorizontalPodAutoscaler as autoscaler ->
        let status = orEmpty autoscaler.Status
        let spec = orEmpty autoscaler.Spec

        { row with
            Ready = status.CurrentReplicas
            Desired = status.DesiredReplicas
            Counts =
                Map
                    [ "min_replicas", int64 spec.MinReplicas
                      "max_replicas", int64 spec.MaxReplicas
                      "current_replicas", int64 status.CurrentReplicas
                      "desired_replicas", int64 status.DesiredReplicas ] }
    | :? PodDisruptionBudget as budget ->
        let status = orEmpty budget.Status

        { row with
            Counts =
                Map
                    [ // The number a budget exists to produce: how many pods
                      // may be evicted right now.
                      "disruptions_allowed", int64 status.DisruptionsAllowed
                      "current_healthy", int64 status.CurrentHealthy
                      "desired_healthy", int64 status.DesiredHealthy
                      "expected_pods", int64 status.ExpectedPods
                      "disrupted_pods", int64 status.DisruptedPods.Count ] }
    | _ -> row

/// One row per object of a collector message.
///
/// The kind is the object's message name ("Pod", "StorageClass"): the agent
/// names its messages after the objects they mirror. An object without
/// Metadata is skipped: with no namespace, name or uid the row could never
/// be queried or joined.
let resourceRows (tenant: string) (now: DateTime) (frame: FrameColumns) (body: IMessage) : K8sResourceRow[] =
    let objectList =
        body.Descriptor.Fields.InDeclarationOrder()
        |> Seq.tryFind (fun field -> field.IsRepeated && not field.IsMap && field.FieldType = FieldType.Message)

    match objectList with
    | None -> [||]
    | Some objectList ->
        let envelopeTags = stringList body "Tags"

        // The same on every row of the frame.
        let envelope: K8sResourceRow =
            { K8sResources.empty with
                TenantID = tenant
                // The sender leaves the frame's timestamp unset, so arrival
                // is the collection time there is.
                CollectedAt = now
                ClusterID = stringField body "ClusterId"
                ClusterName = stringField body "ClusterName"
                Kind = objectList.MessageType.Name
                AgentVersion =
                    (match fieldValue body "AgentVersion" with
                     | :? AgentVersion as version -> agentVersionText version
                     | _ -> "")
                GroupID = int32Field body "GroupId"
                GroupSize = int32Field body "GroupSize"
                OrgID = frame.OrgID
                SubscriptionID = frame.SubscriptionID
                HeaderTimestamp = frame.Timestamp
                Encoding = frame.Encoding
                HostName = stringField body "HostName"
                // Without the object list: the list is the rows.
                Envelope = GoJson.renderWithout objectList body }

        let toRow (object: IMessage) (metadata: Metadata) : K8sResourceRow =
            let owners = Array.ofSeq metadata.OwnerReferences

            { envelope with
                Namespace = metadata.Namespace
                Name = metadata.Name
                UID = metadata.Uid
                ResourceVersion = metadata.ResourceVersion
                // Kubernetes allows several owners but in practice writes one
                // controller; the first is the convenience column.
                OwnerKind = (if owners.Length > 0 then owners[0].Kind else "")
                OwnerName = (if owners.Length > 0 then owners[0].Name else "")
                OwnerKinds = owners |> Array.map _.Kind
                OwnerNames = owners |> Array.map _.Name
                OwnerUIDs = owners |> Array.map _.Uid
                // Labels and annotations travel as "key:value" like tags,
                // but Kubernetes forbids duplicate keys.
                Labels = Tags.toMap metadata.Labels
                Annotations = Tags.toMap metadata.Annotations
                // The frame's tags apply to every object, the object's own on top.
                Tags = Tags.toMultiMap (envelopeTags @ stringList object "Tags")
                CreationTimestamp = seconds metadata.CreationTimestamp
                DeletionTimestamp = seconds metadata.DeletionTimestamp
                // Only meaningful while the object is being deleted, and 0 is
                // then a real value (force delete), not absence.
                DeletionGracePeriodSeconds =
                    (if metadata.DeletionTimestamp <> 0L then Some metadata.DeletionGracePeriodSeconds else None)
                Finalizers = Array.ofSeq metadata.Finalizers
                Object = GoJson.render object }
            |> withCommonFields object
            |> withKindFields object

        objectList.Accessor.GetValue body :?> IList
        |> Seq.cast<IMessage>
        |> Seq.choose (fun object ->
            match fieldValue object "Metadata" with
            | :? Metadata as metadata -> Some(toRow object metadata)
            | _ -> None)
        |> Array.ofSeq

// ---- manifests → k8s_manifests ----

let private manifestList = CollectorManifest.Descriptor.FindFieldByNumber CollectorManifest.ManifestsFieldNumber

/// One row per manifest, its content stored whole. `collector` is null for a
/// CRD or CR frame whose inner envelope the agent left out.
///
/// Tags come from three places: the frame envelope, the CRD/CR wrapper
/// around it, and the manifest itself. They are merged, because a query for
/// env:prod does not care who attached the tag, and TagSources keeps who did.
let manifestRows
    (tenant: string)
    (now: DateTime)
    (frame: FrameColumns)
    (collector: CollectorManifest)
    (wrapperTags: string seq)
    : K8sManifestRow[] =
    if isNull collector then
        [||]
    else
        let envelopeTags = List.ofSeq collector.Tags
        let wrapperTags = List.ofSeq wrapperTags
        let envelope = GoJson.renderWithout manifestList collector

        let toRow (manifest: Manifest) : K8sManifestRow =
            let manifestTags = List.ofSeq manifest.Tags
            let content = manifest.Content.ToByteArray()

            { TenantID = tenant
              CollectedAt = now
              ClusterID = collector.ClusterId
              ClusterName = collector.ClusterName
              UID = manifest.Uid
              Kind = manifest.Kind
              APIVersion = manifest.ApiVersion
              ResourceVersion = manifest.ResourceVersion
              Content = content
              ContentType = manifest.ContentType
              IsTerminated = Text.flag manifest.IsTerminated
              OrgID = frame.OrgID
              SubscriptionID = frame.SubscriptionID
              HeaderTimestamp = frame.Timestamp
              Encoding = frame.Encoding
              GroupID = collector.GroupId
              GroupSize = collector.GroupSize
              HostName = collector.HostName
              AgentVersion = agentVersionText collector.AgentVersion
              OriginCollector = protoName collector.OriginCollector
              Tags = Tags.toMultiMap (envelopeTags @ wrapperTags @ manifestTags)
              TagSources =
                [ "envelope", envelopeTags; "wrapper", wrapperTags; "manifest", manifestTags ]
                |> List.filter (fun (_, tags) -> not tags.IsEmpty)
                |> List.map (fun (source, tags) -> source, Array.ofList tags)
                |> Map.ofList
              NodeName = manifest.NodeName
              Type = manifest.Type
              Version = manifest.Version
              ExtraAttributes = toMap manifest.ExtraAttributes
              ContentIsUTF8 = Text.flag (Utf8.IsValid(ReadOnlySpan content))
              Envelope = envelope
              ManifestHost = (if isNull manifest.Host then "" else GoJson.render manifest.Host) }

        collector.Manifests |> Seq.map toRow |> Array.ofSeq

// ---- the cluster summary → k8s_cluster ----

/// The one row of a CollectorCluster, or none when the cluster itself is
/// missing from it.
let clusterRows (tenant: string) (now: DateTime) (frame: FrameColumns) (collector: CollectorCluster) : K8sClusterRow[] =
    let cluster = collector.Cluster

    if isNull cluster then
        [||]
    else
        let nodes = Array.ofSeq cluster.NodesInfo

        [| { TenantID = tenant
             CollectedAt = now
             ClusterID = collector.ClusterId
             ClusterName = collector.ClusterName
             // A count cannot be negative, and cast as is it would be 4 billion nodes.
             NodeCount = uint32 (max cluster.NodeCount 0)
             PodCapacity = cluster.PodCapacity
             PodAllocatable = cluster.PodAllocatable
             CPUCapacity = cluster.CpuCapacity
             CPUAllocatable = cluster.CpuAllocatable
             MemoryCapacity = cluster.MemoryCapacity
             MemoryAllocatable = cluster.MemoryAllocatable
             KubeletVersions = Wire.versionSpread (cluster.KubeletVersions |> Seq.map (fun pair -> pair.Key, pair.Value))
             APIServerVersions = Wire.versionSpread (cluster.ApiServerVersions |> Seq.map (fun pair -> pair.Key, pair.Value))
             OrgID = frame.OrgID
             SubscriptionID = frame.SubscriptionID
             HeaderTimestamp = frame.Timestamp
             Encoding = frame.Encoding
             GroupID = collector.GroupId
             GroupSize = collector.GroupSize
             AgentVersion = agentVersionText collector.AgentVersion
             // Not merged: the frame's tags and the cluster object's own are
             // different fields, and who attached a tag is worth keeping.
             Tags = Tags.toMultiMap collector.Tags
             ClusterTags = Tags.toMultiMap cluster.Tags
             ResourceVersion = cluster.ResourceVersion
             CreationTimestamp = seconds cluster.CreationTimestamp
             Metrics = (if isNull cluster.Metrics then Map.empty else toMap cluster.Metrics.MetricValues)
             ExtendedResourcesCapacity = toMap cluster.ExtendedResourcesCapacity
             ExtendedResourcesAllocatable = toMap cluster.ExtendedResourcesAllocatable
             NodesName = nodes |> Array.map _.Name
             NodesRegion = nodes |> Array.map _.Region
             NodesInstanceType = nodes |> Array.map _.InstanceType
             NodesOS = nodes |> Array.map _.OperatingSystem
             NodesOSImage = nodes |> Array.map _.OperatingSystemImage
             NodesArchitecture = nodes |> Array.map _.Architecture
             NodesKernelVersion = nodes |> Array.map _.KernelVersion
             NodesContainerRuntimeVersion = nodes |> Array.map _.ContainerRuntimeVersion
             NodesKubeletVersion = nodes |> Array.map _.KubeletVersion
             NodesAllocatable = nodes |> Array.map (fun node -> toMap node.ResourceAllocatable)
             NodesCapacity = nodes |> Array.map (fun node -> toMap node.ResourceCapacity) } |]

// ---- ECS tasks → ecs_tasks ----

let private taskList = CollectorECSTask.Descriptor.FindFieldByNumber CollectorECSTask.TasksFieldNumber

/// One row per task. The ARN is the identity: a task has no Kubernetes
/// metadata.
let ecsTaskRows (tenant: string) (now: DateTime) (frame: FrameColumns) (collector: CollectorECSTask) : ECSTaskRow[] =
    let envelopeTags = Tags.toMultiMap collector.Tags
    // The envelope has a Host and an Info of its own (the machine and agent
    // that reported the pass) and no column holds either.
    let envelope = GoJson.renderWithout taskList collector

    let toRow (task: ECSTask) : ECSTaskRow =
        { TenantID = tenant
          CollectedAt = now
          OrgID = frame.OrgID
          SubscriptionID = frame.SubscriptionID
          HeaderTimestamp = frame.Timestamp
          Encoding = frame.Encoding
          AWSAccountID = collector.AwsAccountID
          ClusterID = collector.ClusterId
          ClusterName = collector.ClusterName
          Region = collector.Region
          GroupID = collector.GroupId
          GroupSize = collector.GroupSize
          HostName = collector.HostName
          AgentVersion = agentVersionText collector.AgentVersion
          EnvelopeTags = envelopeTags
          ARN = task.Arn
          ResourceVersion = task.ResourceVersion
          LaunchType = task.LaunchType
          DesiredStatus = task.DesiredStatus
          KnownStatus = task.KnownStatus
          Family = task.Family
          Version = task.Version
          AvailabilityZone = task.AvailabilityZone
          ServiceName = task.ServiceName
          VpcID = task.VpcId
          ContainerInstanceARN = task.ContainerInstanceArn
          DaemonName = task.DaemonName
          TaskHostName = (if isNull task.Host then "" else task.Host.Name)
          Limits = toMap task.Limits
          EphemeralStorageMetrics = toMap task.EphemeralStorageMetrics
          // A task still pulling has no PullStoppedAt.
          PullStartedAt = seconds task.PullStartedAt
          PullStoppedAt = seconds task.PullStoppedAt
          ExecutionStoppedAt = seconds task.ExecutionStoppedAt
          Containers = GoJson.render task.Containers
          ContainerCount = uint32 task.Containers.Count
          Tags = Tags.toMultiMap task.Tags
          ECSTags = Tags.toMultiMap task.EcsTags
          ContainerInstanceTags = Tags.toMultiMap task.ContainerInstanceTags
          Envelope = envelope
          TaskHost = (if isNull task.Host then "" else GoJson.render task.Host) }

    collector.Tasks |> Seq.map toRow |> Array.ofSeq

// ---- the orchestrator handlers ----

let private accepted = Response.json 202 "{}"

let private handleFrame (label: string) (r: Request) : Response =
    match ProcessFrame.decode r.Body with
    | Error e ->
        r.Log.LogWarning("[{Label}] process frame: {Error} ({Bytes} bytes)", label, e, r.Body.Length)
        Raw.store r "orchestrator" "decode_error" (label + ": " + e) r.Body
    | Ok decoded ->
        if Environment.GetEnvironmentVariable "NINJACAT_DUMP_K8S" = "true" then
            // The whole message: how to find out what a real cluster sends.
            r.Log.LogInformation("[{Label}] {Body}", label, decoded.Body)

        let frame = frameColumns decoded.Header
        let now = DateTime.UtcNow
        let hasTenant = r.Tenant <> ""

        match decoded.Body with
        | :? CollectorCluster as collector ->
            if hasTenant then
                Sink.write r.Sink K8sCluster.table (clusterRows r.Tenant now frame collector)
        | :? CollectorManifest as collector ->
            if hasTenant then
                Sink.write r.Sink K8sManifests.table (manifestRows r.Tenant now frame collector [])
        // The CRD and CR wrappers hold an inner CollectorManifest that may be
        // missing, and tags of their own beside the inner one's.
        | :? CollectorManifestCRD as wrapper ->
            if hasTenant then
                Sink.write r.Sink K8sManifests.table (manifestRows r.Tenant now frame wrapper.Manifest wrapper.Tags)
        | :? CollectorManifestCR as wrapper ->
            if hasTenant then
                Sink.write r.Sink K8sManifests.table (manifestRows r.Tenant now frame wrapper.Manifest wrapper.Tags)
        | :? CollectorECSTask as collector ->
            if hasTenant then
                Sink.write r.Sink ECSTasks.table (ecsTaskRows r.Tenant now frame collector)
        | :? CollectorPod
        | :? CollectorReplicaSet
        | :? CollectorDeployment
        | :? CollectorService
        | :? CollectorNode
        | :? CollectorJob
        | :? CollectorCronJob
        | :? CollectorDaemonSet
        | :? CollectorStatefulSet
        | :? CollectorPersistentVolume
        | :? CollectorPersistentVolumeClaim
        | :? CollectorRole
        | :? CollectorRoleBinding
        | :? CollectorClusterRole
        | :? CollectorClusterRoleBinding
        | :? CollectorServiceAccount
        | :? CollectorIngress
        | :? CollectorNamespace
        | :? CollectorVerticalPodAutoscaler
        | :? CollectorHorizontalPodAutoscaler
        | :? CollectorNetworkPolicy
        | :? CollectorLimitRange
        | :? CollectorStorageClass
        | :? CollectorPodDisruptionBudget ->
            if hasTenant then
                Sink.write r.Sink K8sResources.table (resourceRows r.Tenant now frame decoded.Body)
        | other ->
            // The process agent's own messages belong on /api/v1/collector.
            // Here they mean a misconfigured agent: the frame decoded, but
            // this router has nowhere to put it.
            let goType = "*process." + other.Descriptor.Name
            r.Log.LogWarning("[{Label}] {Type} is not an orchestrator message; kept as is", label, goType)
            Raw.store r "orchestrator" "unexpected_shape" $"{label}: {goType} on an orchestrator intake" r.Body

    accepted

/// Kubernetes resource collections; /api/v1/orchestrator is the same under
/// orchestrator_explorer.use_legacy_endpoint.
let handleOrchestrator (r: Request) : Response = handleFrame "orch" r

/// Raw manifests (types 80-82): the object's own YAML or JSON, "exactly what
/// kubectl would show". Same frame, same decoder.
let handleManifests (r: Request) : Response = handleFrame "orchmanif" r

// ---- kubeactions → k8s_actions ----

/// One report on a remote action, as the cluster agent's ActionResultEvent
/// (pkg/clusteragent/kubeactions/reporter.go) sends it.
///
/// EventType is action_received, action_progress or action_executed; Status
/// is success, failed, skipped, expired, claimed or in_progress; Timestamp is
/// RFC 3339, kept as the text it came as.
type ActionEvent =
    { ActionID: string
      OrgID: int64
      EventType: string
      Status: string
      ActionType: string
      ClusterID: string
      ResourceID: string
      RequestedBy: string
      Timestamp: string
      Message: string
      /// What the executor attached, base64 on the wire.
      Payloads: Map<string, byte[]>
      ClusterName: string
      ResourceKind: string
      ResourceName: string
      ResourceNamespace: string
      /// Keys the event carried that are not among the known ones, so a newer
      /// agent loses nothing here.
      Extra: Map<string, JsonElement> }

/// The JSON keys of an ActionEvent's own fields.
let actionKnownKeys: Set<string> =
    set
        [ "action_id"; "org_id"; "event_type"; "status"; "action_type"; "cluster_id"; "resource_id"
          "requested_by"; "timestamp"; "message"; "payloads"; "cluster_name"; "resource_kind"
          "resource_name"; "resource_namespace" ]

let private emptyAction: ActionEvent =
    { ActionID = ""
      OrgID = 0L
      EventType = ""
      Status = ""
      ActionType = ""
      ClusterID = ""
      ResourceID = ""
      RequestedBy = ""
      Timestamp = ""
      Message = ""
      Payloads = Map.empty
      ClusterName = ""
      ResourceKind = ""
      ResourceName = ""
      ResourceNamespace = ""
      Extra = Map.empty }

let private kindName (value: JsonElement) : string =
    match value.ValueKind with
    | JsonValueKind.Object -> "an object"
    | JsonValueKind.Array -> "an array"
    | JsonValueKind.String -> "a string"
    | JsonValueKind.Number -> "a number"
    | JsonValueKind.True
    | JsonValueKind.False -> "a boolean"
    | _ -> "null"

/// Base64 as Go's decoder took it: padded, standard alphabet, line breaks
/// allowed and nothing else.
let private tryBase64 (text: string) : byte[] option =
    let joined = text.Replace("\r", "").Replace("\n", "")

    if joined |> Seq.exists Char.IsWhiteSpace then
        None
    else
        try
            Some(Convert.FromBase64String joined)
        with :? FormatException ->
            None

/// Decodes one event the way the Go server did, so the same events become
/// rows and the same ones are refused:
///
///  - a key matches a field whatever its letter case, and null leaves the
///    field as it was;
///  - a value of the wrong type refuses the whole event;
///  - a key that is not exactly a known one is kept in Extra. So "ACTION_ID"
///    both sets the field and is kept.
let decodeAction (raw: JsonElement) : Result<ActionEvent, string> =
    if raw.ValueKind = JsonValueKind.Null then
        Ok emptyAction
    elif raw.ValueKind <> JsonValueKind.Object then
        Error $"an event must be a JSON object, this is {kindName raw}"
    else
        let problems = ResizeArray<string>()

        let text (property: JsonProperty) (current: string) : string =
            match property.Value.ValueKind with
            | JsonValueKind.Null -> current
            | JsonValueKind.String -> property.Value.GetString()
            | _ ->
                problems.Add $"{property.Name} must be a string, this is {kindName property.Value}"
                current

        // Digits only: a fraction or an exponent is not an integer, and a
        // 64-bit id must not pass through a float.
        let integer (property: JsonProperty) (current: int64) : int64 =
            match property.Value.ValueKind with
            | JsonValueKind.Null -> current
            | JsonValueKind.Number ->
                match Int64.TryParse(property.Value.GetRawText(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
                | true, n -> n
                | false, _ ->
                    problems.Add $"{property.Name} must be a 64-bit integer, this is {property.Value.GetRawText()}"
                    current
            | _ ->
                problems.Add $"{property.Name} must be a number, this is {kindName property.Value}"
                current

        let attachments (property: JsonProperty) (current: Map<string, byte[]>) : Map<string, byte[]> =
            match property.Value.ValueKind with
            | JsonValueKind.Null -> current
            | JsonValueKind.Object ->
                let mutable attached = current

                for attachment in property.Value.EnumerateObject() do
                    match attachment.Value.ValueKind with
                    | JsonValueKind.Null -> attached <- attached.Add(attachment.Name, [||])
                    | JsonValueKind.String ->
                        match tryBase64 (attachment.Value.GetString()) with
                        | Some bytes -> attached <- attached.Add(attachment.Name, bytes)
                        | None -> problems.Add $"{property.Name}.{attachment.Name} is not base64"
                    | _ -> problems.Add $"{property.Name}.{attachment.Name} must be a base64 string, this is {kindName attachment.Value}"

                attached
            | _ ->
                problems.Add $"{property.Name} must be an object, this is {kindName property.Value}"
                current

        let mutable event = emptyAction

        try
            for property in raw.EnumerateObject() do
                match property.Name.ToLowerInvariant() with
                | "action_id" -> event <- { event with ActionID = text property event.ActionID }
                | "org_id" -> event <- { event with OrgID = integer property event.OrgID }
                | "event_type" -> event <- { event with EventType = text property event.EventType }
                | "status" -> event <- { event with Status = text property event.Status }
                | "action_type" -> event <- { event with ActionType = text property event.ActionType }
                | "cluster_id" -> event <- { event with ClusterID = text property event.ClusterID }
                | "resource_id" -> event <- { event with ResourceID = text property event.ResourceID }
                | "requested_by" -> event <- { event with RequestedBy = text property event.RequestedBy }
                | "timestamp" -> event <- { event with Timestamp = text property event.Timestamp }
                | "message" -> event <- { event with Message = text property event.Message }
                | "payloads" -> event <- { event with Payloads = attachments property event.Payloads }
                | "cluster_name" -> event <- { event with ClusterName = text property event.ClusterName }
                | "resource_kind" -> event <- { event with ResourceKind = text property event.ResourceKind }
                | "resource_name" -> event <- { event with ResourceName = text property event.ResourceName }
                | "resource_namespace" -> event <- { event with ResourceNamespace = text property event.ResourceNamespace }
                | _ -> ()

                if not (actionKnownKeys.Contains property.Name) then
                    event <- { event with Extra = event.Extra.Add(property.Name, property.Value) }

            if problems.Count > 0 then Error problems[0] else Ok event
        // A string with half a surrogate pair parses as JSON but cannot be read.
        with :? InvalidOperationException as e ->
            Error e.Message

let private extraJson (extra: Map<string, JsonElement>) : string =
    use buffer = new MemoryStream()

    do
        use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
        writer.WriteStartObject()

        for pair in extra do
            writer.WritePropertyName pair.Key
            pair.Value.WriteTo writer

        writer.WriteEndObject()

    Encoding.UTF8.GetString(buffer.ToArray())

/// An empty or unparseable timestamp means "not provided": the row gets the
/// arrival time, never 1970, and TimestampRaw keeps what was sent.
let actionRow (tenant: string) (arrival: DateTime) (event: ActionEvent) : K8sActionRow =
    { TenantID = tenant
      Timestamp = Time.tryRfc3339 event.Timestamp |> Option.defaultValue arrival
      ActionID = event.ActionID
      OrgID = event.OrgID
      EventType = event.EventType
      Status = event.Status
      ActionType = event.ActionType
      ClusterID = event.ClusterID
      ClusterName = event.ClusterName
      ResourceID = event.ResourceID
      ResourceKind = event.ResourceKind
      ResourceName = event.ResourceName
      ResourceNamespace = event.ResourceNamespace
      RequestedBy = event.RequestedBy
      Message = event.Message
      // A column of their own, sorted, so filtering on a name needs no JSON
      // parsing.
      ExtraKeys = event.Extra |> Map.toArray |> Array.map fst
      Payloads = event.Payloads
      Extra = (if event.Extra.IsEmpty then "" else extraJson event.Extra)
      TimestampRaw = event.Timestamp }

/// The cluster agent's reports on the actions it ran: the other half of
/// remote configuration. One event when an action is received, one when it
/// is executed. Event platform track "kubeactions": a JSON array of events.
let handleActions (r: Request) : Response =
    match Json.tryParse r.Body with
    | Error e ->
        r.Log.LogWarning("[kubeactions] not JSON: {Error}", e)
        Raw.store r "kubeactions" "decode_error" e r.Body
    | Ok root when root.ValueKind = JsonValueKind.Null -> ()
    | Ok root when root.ValueKind <> JsonValueKind.Array ->
        let note = $"the body must be a JSON array of events, this is {kindName root}"
        r.Log.LogWarning("[kubeactions] {Problem}", note)
        Raw.store r "kubeactions" "decode_error" note r.Body
    | Ok root ->
        let events = ResizeArray<ActionEvent>()

        root.EnumerateArray()
        |> Seq.iteri (fun i raw ->
            match decodeAction raw with
            | Ok event -> events.Add event
            | Error e ->
                // One bad element must not cost the batch, nor itself: its
                // own JSON is kept while the rest become rows.
                r.Log.LogWarning("[kubeactions] event {Index}: {Error}", i, e)
                Raw.store r "kubeactions" "decode_error" $"batch element {i}: {e}" (Encoding.UTF8.GetBytes(raw.GetRawText())))

        if r.Tenant <> "" then
            let arrival = DateTime.UtcNow
            Sink.write r.Sink K8sActions.table (events |> Seq.map (actionRow r.Tenant arrival) |> Array.ofSeq)

    accepted
