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

// The 24 kinds share a shape but no type: every collector message has the
// same envelope and one list of objects, and every object has metadata and
// tags. What differs is written out per kind below, plainly, so the compiler
// checks every field name.

/// One condition of an object. Each kind declares a condition message of its
/// own; a time a kind does not have is 0.
type private Condition =
    { Type: string
      Status: string
      Reason: string
      Message: string
      LastTransition: int64
      LastUpdate: int64
      LastProbe: int64 }

/// What a row takes from an object besides the columns of its kind.
type private ObjectParts =
    { Object: IMessage
      Metadata: Metadata
      Tags: string seq
      Conditions: Condition list
      /// The agent's own one-line summary of what is wrong, where it sends one.
      ConditionMessage: string
      ResourceRequirements: ResourceRequirements seq
      /// Null for the kinds that carry no computed metrics.
      Metrics: ResourceMetrics }

/// An object with metadata and tags and nothing else in common.
let private plainParts (object: IMessage) (metadata: Metadata) (tags: string seq) : ObjectParts =
    { Object = object
      Metadata = metadata
      Tags = tags
      Conditions = []
      ConditionMessage = ""
      ResourceRequirements = Seq.empty
      Metrics = null }

let private condition (kind: string) (status: string) (reason: string) (message: string) (lastTransition: int64) : Condition =
    { Type = kind
      Status = status
      Reason = reason
      Message = message
      LastTransition = lastTransition
      LastUpdate = 0L
      LastProbe = 0L }

let private podParts (pod: Pod) : ObjectParts =
    { plainParts pod pod.Metadata pod.Tags with
        Conditions =
            [ for c in pod.Conditions ->
                  { condition c.Type c.Status c.Reason c.Message c.LastTransitionTime with LastProbe = c.LastProbeTime } ]
        ConditionMessage = pod.ConditionMessage
        ResourceRequirements = pod.ResourceRequirements
        Metrics = pod.Metrics }

let private podDisruptionBudgetParts (budget: PodDisruptionBudget) : ObjectParts =
    { plainParts budget budget.Metadata budget.Tags with
        Conditions =
            [ if not (isNull budget.Status) then
                  for c in budget.Status.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ] }

let private replicaSetParts (replicaSet: ReplicaSet) : ObjectParts =
    { plainParts replicaSet replicaSet.Metadata replicaSet.Tags with
        Conditions = [ for c in replicaSet.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ]
        ResourceRequirements = replicaSet.ResourceRequirements
        Metrics = replicaSet.Metrics }

let private deploymentParts (deployment: Deployment) : ObjectParts =
    { plainParts deployment deployment.Metadata deployment.Tags with
        Conditions =
            [ for c in deployment.Conditions ->
                  { condition c.Type c.Status c.Reason c.Message c.LastTransitionTime with LastUpdate = c.LastUpdateTime } ]
        ConditionMessage = deployment.ConditionMessage
        ResourceRequirements = deployment.ResourceRequirements
        Metrics = deployment.Metrics }

let private serviceParts (service: Service) : ObjectParts =
    { plainParts service service.Metadata service.Tags with Metrics = service.Metrics }

let private nodeParts (node: Node) : ObjectParts =
    { plainParts node node.Metadata node.Tags with
        Conditions =
            [ if not (isNull node.Status) then
                  for c in node.Status.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ]
        Metrics = node.Metrics }

let private namespaceParts (ns: Namespace) : ObjectParts =
    { plainParts ns ns.Metadata ns.Tags with
        Conditions = [ for c in ns.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ]
        ConditionMessage = ns.ConditionMessage }

let private jobParts (job: Job) : ObjectParts =
    { plainParts job job.Metadata job.Tags with
        Conditions =
            [ for c in job.Conditions ->
                  { condition c.Type c.Status c.Reason c.Message c.LastTransitionTime with LastProbe = c.LastProbeTime } ]
        ConditionMessage = (if isNull job.Status then "" else job.Status.ConditionMessage)
        ResourceRequirements = (if isNull job.Spec then Seq.empty else job.Spec.ResourceRequirements) }

let private cronJobParts (cronJob: CronJob) : ObjectParts =
    { plainParts cronJob cronJob.Metadata cronJob.Tags with
        ResourceRequirements = (if isNull cronJob.Spec then Seq.empty else cronJob.Spec.ResourceRequirements) }

let private daemonSetParts (daemonSet: DaemonSet) : ObjectParts =
    { plainParts daemonSet daemonSet.Metadata daemonSet.Tags with
        Conditions = [ for c in daemonSet.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ]
        ResourceRequirements = (if isNull daemonSet.Spec then Seq.empty else daemonSet.Spec.ResourceRequirements)
        Metrics = daemonSet.Metrics }

let private statefulSetParts (statefulSet: StatefulSet) : ObjectParts =
    { plainParts statefulSet statefulSet.Metadata statefulSet.Tags with
        Conditions = [ for c in statefulSet.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime ]
        ResourceRequirements = (if isNull statefulSet.Spec then Seq.empty else statefulSet.Spec.ResourceRequirements)
        Metrics = statefulSet.Metrics }

let private persistentVolumeClaimParts (claim: PersistentVolumeClaim) : ObjectParts =
    { plainParts claim claim.Metadata claim.Tags with
        Conditions =
            [ if not (isNull claim.Status) then
                  for c in claim.Status.Conditions ->
                      { condition c.Type c.Status c.Reason c.Message c.LastTransitionTime with LastProbe = c.LastProbeTime } ] }

let private clusterRoleParts (role: ClusterRole) : ObjectParts =
    { plainParts role role.Metadata role.Tags with Metrics = role.Metrics }

/// A VerticalPodAutoscaler has two condition lists, on the object and in its
/// status, and they are different messages; the nested one is what a real
/// autoscaler fills. Both are read, the object's first.
let private verticalPodAutoscalerParts (autoscaler: VerticalPodAutoscaler) : ObjectParts =
    { plainParts autoscaler autoscaler.Metadata autoscaler.Tags with
        Conditions =
            [ for c in autoscaler.Conditions -> condition c.Type c.Status c.Reason c.Message c.LastTransitionTime
              if not (isNull autoscaler.Status) then
                  for c in autoscaler.Status.Conditions ->
                      condition c.ConditionType c.ConditionStatus c.Reason c.Message c.LastTransitionTime ] }

let private horizontalPodAutoscalerParts (autoscaler: HorizontalPodAutoscaler) : ObjectParts =
    { plainParts autoscaler autoscaler.Metadata autoscaler.Tags with
        Conditions =
            [ for c in autoscaler.Conditions ->
                  condition c.ConditionType c.ConditionStatus c.Reason c.Message c.LastTransitionTime ] }

/// The columns every kind can fill, from the parts read above.
let private withCommonFields (parts: ObjectParts) (row: K8sResourceRow) : K8sResourceRow =
    let conditions = Array.ofList parts.Conditions

    { row with
        ConditionTypes = conditions |> Array.map _.Type
        ConditionStatuses = conditions |> Array.map _.Status
        ConditionReasons = conditions |> Array.map _.Reason
        ConditionMessages = conditions |> Array.map _.Message
        ConditionLastTransition = conditions |> Array.map (fun c -> seconds c.LastTransition)
        ConditionLastUpdate = conditions |> Array.map (fun c -> seconds c.LastUpdate)
        ConditionLastProbe = conditions |> Array.map (fun c -> seconds c.LastProbe)
        ConditionMessage = parts.ConditionMessage
        ResourceRequirements = ProtoJson.messages parts.ResourceRequirements
        Metrics = (if isNull parts.Metrics then Map.empty else toMap parts.Metrics.MetricValues) }

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
            Taints = ProtoJson.messages node.Taints
            Capacity = toMap status.Capacity
            Allocatable = toMap status.Allocatable
            NodeAddresses =
                (if status.NodeAddresses.Count > 0 then
                     JsonSerializer.Serialize(status.NodeAddresses :> Generic.IDictionary<string, string>)
                 else
                     "")
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
            ServicePorts = ProtoJson.messages spec.Ports
            Counts = Map [ "ports", int64 spec.Ports.Count ] }
    | :? Role as role ->
        { row with
            RBACRules = ProtoJson.messages role.Rules
            Counts = Map [ "rules", int64 role.Rules.Count ] }
    | :? ClusterRole as role ->
        { row with
            RBACRules = ProtoJson.messages role.Rules
            Counts = Map [ "rules", int64 role.Rules.Count; "aggregation_rules", int64 role.AggregationRules.Count ] }
    | :? RoleBinding as binding ->
        { row with
            RBACSubjects = ProtoJson.messages binding.Subjects
            RBACRoleRef = ProtoJson.message binding.RoleRef
            Counts = Map [ "subjects", int64 binding.Subjects.Count ] }
    | :? ClusterRoleBinding as binding ->
        { row with
            RBACSubjects = ProtoJson.messages binding.Subjects
            RBACRoleRef = ProtoJson.message binding.RoleRef
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

/// The rows of one collector message: the envelope's columns on every row,
/// then each object's own.
///
/// An object without Metadata is skipped: with no namespace, name or uid the
/// row could never be queried or joined.
let private rowsOf
    (tenant: string)
    (now: DateTime)
    (frame: FrameColumns)
    (kind: string)
    (clusterName: string)
    (clusterId: string)
    (groupId: int32)
    (groupSize: int32)
    (hostName: string)
    (envelopeTags: string seq)
    (agentVersion: AgentVersion)
    (envelopeWithoutObjects: IMessage)
    (objects: ObjectParts seq)
    : K8sResourceRow[] =
    let envelopeTags = List.ofSeq envelopeTags

    // The same on every row of the frame.
    let envelope: K8sResourceRow =
        { K8sResources.empty with
            TenantID = tenant
            // The sender leaves the frame's timestamp unset, so arrival is
            // the collection time there is.
            CollectedAt = now
            ClusterID = clusterId
            ClusterName = clusterName
            Kind = kind
            AgentVersion = agentVersionText agentVersion
            GroupID = groupId
            GroupSize = groupSize
            OrgID = frame.OrgID
            SubscriptionID = frame.SubscriptionID
            HeaderTimestamp = frame.Timestamp
            Encoding = frame.Encoding
            HostName = hostName
            // Without the object list: the list is the rows.
            Envelope = ProtoJson.message envelopeWithoutObjects }

    let toRow (parts: ObjectParts) : K8sResourceRow =
        let metadata = parts.Metadata
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
            // Labels and annotations travel as "key:value" like tags, but
            // Kubernetes forbids duplicate keys.
            Labels = Tags.toMap metadata.Labels
            Annotations = Tags.toMap metadata.Annotations
            // The frame's tags apply to every object, the object's own on top.
            Tags = Tags.toMultiMap (envelopeTags @ List.ofSeq parts.Tags)
            CreationTimestamp = seconds metadata.CreationTimestamp
            DeletionTimestamp = seconds metadata.DeletionTimestamp
            // Only meaningful while the object is being deleted, and 0 is
            // then a real value (force delete), not absence.
            DeletionGracePeriodSeconds =
                (if metadata.DeletionTimestamp <> 0L then Some metadata.DeletionGracePeriodSeconds else None)
            Finalizers = Array.ofSeq metadata.Finalizers
            Object = ProtoJson.message parts.Object }
        |> withCommonFields parts
        |> withKindFields parts.Object

    objects
    |> Seq.filter (fun parts -> not (isNull parts.Metadata))
    |> Seq.map toRow
    |> Array.ofSeq

/// One row per object of a collector message; none for a message that is not
/// a collection of Kubernetes objects.
let resourceRows (tenant: string) (now: DateTime) (frame: FrameColumns) (body: IMessage) : K8sResourceRow[] =
    match body with
    | :? CollectorPod as c ->
        let bare = c.Clone()
        bare.Pods.Clear()
        rowsOf tenant now frame "Pod" c.ClusterName c.ClusterId c.GroupId c.GroupSize c.HostName c.Tags c.AgentVersion bare (Seq.map podParts c.Pods)
    | :? CollectorPodDisruptionBudget as c ->
        let bare = c.Clone()
        bare.PodDisruptionBudgets.Clear()
        rowsOf tenant now frame "PodDisruptionBudget" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map podDisruptionBudgetParts c.PodDisruptionBudgets)
    | :? CollectorReplicaSet as c ->
        let bare = c.Clone()
        bare.ReplicaSets.Clear()
        rowsOf tenant now frame "ReplicaSet" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map replicaSetParts c.ReplicaSets)
    | :? CollectorDeployment as c ->
        let bare = c.Clone()
        bare.Deployments.Clear()
        rowsOf tenant now frame "Deployment" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map deploymentParts c.Deployments)
    | :? CollectorService as c ->
        let bare = c.Clone()
        bare.Services.Clear()
        rowsOf tenant now frame "Service" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map serviceParts c.Services)
    | :? CollectorNode as c ->
        let bare = c.Clone()
        bare.Nodes.Clear()
        rowsOf tenant now frame "Node" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map nodeParts c.Nodes)
    | :? CollectorNamespace as c ->
        let bare = c.Clone()
        bare.Namespaces.Clear()
        rowsOf tenant now frame "Namespace" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map namespaceParts c.Namespaces)
    | :? CollectorJob as c ->
        let bare = c.Clone()
        bare.Jobs.Clear()
        rowsOf tenant now frame "Job" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map jobParts c.Jobs)
    | :? CollectorCronJob as c ->
        let bare = c.Clone()
        bare.CronJobs.Clear()
        rowsOf tenant now frame "CronJob" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map cronJobParts c.CronJobs)
    | :? CollectorDaemonSet as c ->
        let bare = c.Clone()
        bare.DaemonSets.Clear()
        rowsOf tenant now frame "DaemonSet" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map daemonSetParts c.DaemonSets)
    | :? CollectorStatefulSet as c ->
        let bare = c.Clone()
        bare.StatefulSets.Clear()
        rowsOf tenant now frame "StatefulSet" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map statefulSetParts c.StatefulSets)
    | :? CollectorPersistentVolume as c ->
        let bare = c.Clone()
        bare.PersistentVolumes.Clear()
        rowsOf tenant now frame "PersistentVolume" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.PersistentVolumes |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorPersistentVolumeClaim as c ->
        let bare = c.Clone()
        bare.PersistentVolumeClaims.Clear()
        rowsOf tenant now frame "PersistentVolumeClaim" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map persistentVolumeClaimParts c.PersistentVolumeClaims)
    | :? CollectorRole as c ->
        let bare = c.Clone()
        bare.Roles.Clear()
        rowsOf tenant now frame "Role" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.Roles |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorRoleBinding as c ->
        let bare = c.Clone()
        bare.RoleBindings.Clear()
        rowsOf tenant now frame "RoleBinding" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.RoleBindings |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorClusterRole as c ->
        let bare = c.Clone()
        bare.ClusterRoles.Clear()
        rowsOf tenant now frame "ClusterRole" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map clusterRoleParts c.ClusterRoles)
    | :? CollectorClusterRoleBinding as c ->
        let bare = c.Clone()
        bare.ClusterRoleBindings.Clear()
        rowsOf tenant now frame "ClusterRoleBinding" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.ClusterRoleBindings |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorServiceAccount as c ->
        let bare = c.Clone()
        bare.ServiceAccounts.Clear()
        rowsOf tenant now frame "ServiceAccount" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.ServiceAccounts |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorIngress as c ->
        let bare = c.Clone()
        bare.Ingresses.Clear()
        rowsOf tenant now frame "Ingress" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.Ingresses |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorVerticalPodAutoscaler as c ->
        let bare = c.Clone()
        bare.VerticalPodAutoscalers.Clear()
        rowsOf tenant now frame "VerticalPodAutoscaler" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map verticalPodAutoscalerParts c.VerticalPodAutoscalers)
    | :? CollectorHorizontalPodAutoscaler as c ->
        let bare = c.Clone()
        bare.HorizontalPodAutoscalers.Clear()
        rowsOf tenant now frame "HorizontalPodAutoscaler" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (Seq.map horizontalPodAutoscalerParts c.HorizontalPodAutoscalers)
    | :? CollectorNetworkPolicy as c ->
        let bare = c.Clone()
        bare.NetworkPolicies.Clear()
        rowsOf tenant now frame "NetworkPolicy" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.NetworkPolicies |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorLimitRange as c ->
        let bare = c.Clone()
        bare.LimitRanges.Clear()
        rowsOf tenant now frame "LimitRange" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.LimitRanges |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | :? CollectorStorageClass as c ->
        let bare = c.Clone()
        bare.StorageClasses.Clear()
        rowsOf tenant now frame "StorageClass" c.ClusterName c.ClusterId c.GroupId c.GroupSize "" c.Tags c.AgentVersion bare (c.StorageClasses |> Seq.map (fun o -> plainParts o o.Metadata o.Tags))
    | _ -> [||]

// ---- manifests → k8s_manifests ----

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
        // Without the manifest list: the list is the rows.
        let bare = collector.Clone()
        bare.Manifests.Clear()
        let envelope = ProtoJson.message bare

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
              ManifestHost = ProtoJson.message manifest.Host }

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

/// One row per task. The ARN is the identity: a task has no Kubernetes
/// metadata.
let ecsTaskRows (tenant: string) (now: DateTime) (frame: FrameColumns) (collector: CollectorECSTask) : ECSTaskRow[] =
    let envelopeTags = Tags.toMultiMap collector.Tags
    // The envelope has a Host and an Info of its own (the machine and agent
    // that reported the pass) and no column holds either.
    let bare = collector.Clone()
    bare.Tasks.Clear()
    let envelope = ProtoJson.message bare

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
          Containers = ProtoJson.messages task.Containers
          ContainerCount = uint32 task.Containers.Count
          Tags = Tags.toMultiMap task.Tags
          ECSTags = Tags.toMultiMap task.EcsTags
          ContainerInstanceTags = Tags.toMultiMap task.ContainerInstanceTags
          Envelope = envelope
          TaskHost = ProtoJson.message task.Host }

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
