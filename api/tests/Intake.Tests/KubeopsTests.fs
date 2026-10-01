/// Tests of Routers/Kubeops.fs beyond the golden fixtures.
///
/// One converter serves some 24 kinds of object, so these pin what it must
/// keep for all of them: the kind comes from the object's message name,
/// identity from Metadata, and the envelope lands on every row.
module NinjaCat.Api.Intake.Tests.KubeopsTests

open System
open System.Text
open System.Text.Json
open Xunit
open Google.Protobuf
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Datadog.ProcessAgent
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private now = DateTime(2026, 9, 21, 12, 0, 0, DateTimeKind.Utc)

let private at (seconds: int64) : DateTime = DateTime.UnixEpoch.AddSeconds(float seconds)

/// A header with every field set, so a test can tell "the converter copied
/// the header" from "the header happened to be zero".
let private frame: Kubeops.FrameColumns =
    { OrgID = 77
      SubscriptionID = 3uy
      Timestamp = Some 1700000000L
      Encoding = "protobuf" }

let private metadata (ns: string) (name: string) (uid: string) : Metadata =
    Metadata(Namespace = ns, Name = name, Uid = uid)

let private podRows (pods: Pod list) : K8sResourceRow[] =
    let collector = CollectorPod()
    collector.Pods.AddRange pods
    Kubeops.resourceRows "t" now frame collector

let private podRow (pod: Pod) : K8sResourceRow = podRows [ pod ] |> Assert.Single

let private json (text: string) : JsonElement =
    use document = JsonDocument.Parse text
    document.RootElement.Clone()

let private decoded (text: string) : Kubeops.ActionEvent =
    match Kubeops.decodeAction (json text) with
    | Ok event -> event
    | Error e -> failwith e

let private refused (text: string) : bool = Result.isError (Kubeops.decodeAction (json text))

/// Sends a body through the kubeops intake as the cluster agent would.
let private post (path: string) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey
    Replay.byGoNames deps [ "routeKubeops" ] http body, sink

// ---- object collections ----

[<Fact>]
let ``a pod row has its kind, identity and first owner, and the envelope of its frame`` () =
    let pod = Pod(Metadata = Metadata(Namespace = "default", Name = "web-abc12", Uid = "uid-pod", ResourceVersion = "42"))
    pod.Metadata.OwnerReferences.Add(OwnerReference(Kind = "ReplicaSet", Name = "web"))
    let collector = CollectorPod(ClusterName = "prod", ClusterId = "cid-1", GroupId = 3, GroupSize = 7)
    collector.Pods.Add pod

    let row = Kubeops.resourceRows "tenant-1" now frame collector |> Assert.Single

    Assert.Equal("Pod", row.Kind)
    Assert.Equal(("default", "web-abc12", "uid-pod"), (row.Namespace, row.Name, row.UID))
    Assert.Equal(("ReplicaSet", "web"), (row.OwnerKind, row.OwnerName))
    Assert.Equal("42", row.ResourceVersion)
    Assert.Equal(("tenant-1", "prod", "cid-1"), (row.TenantID, row.ClusterName, row.ClusterID))
    Assert.Equal((3, 7), (row.GroupID, row.GroupSize))
    Assert.Equal(now, row.CollectedAt)

[<Fact>]
let ``a deployment row has its kind and identity, and no owner when it has none`` () =
    let collector = CollectorDeployment(ClusterName = "prod", ClusterId = "cid-1", GroupId = 3, GroupSize = 7)
    collector.Deployments.Add(Deployment(Metadata = metadata "shop" "checkout" "uid-dep"))

    let row = Kubeops.resourceRows "tenant-1" now frame collector |> Assert.Single

    Assert.Equal("Deployment", row.Kind)
    Assert.Equal(("shop", "checkout", "uid-dep"), (row.Namespace, row.Name, row.UID))
    Assert.Equal(("", ""), (row.OwnerKind, row.OwnerName))
    Assert.Empty row.OwnerKinds
    Assert.Equal(("prod", "cid-1", 3, 7), (row.ClusterName, row.ClusterID, row.GroupID, row.GroupSize))

[<Fact>]
let ``pod numbers are sums over its container statuses`` () =
    let pod = Pod(Metadata = metadata "ns" "p" "u", NodeName = "node-1", Phase = "Running", Status = "CrashLoopBackOff")

    pod.ContainerStatuses.AddRange
        [ ContainerStatus(Name = "app", Ready = true, RestartCount = 4)
          ContainerStatus(Name = "sidecar", Ready = true, RestartCount = 1)
          ContainerStatus(Name = "init-ish", Ready = false, RestartCount = 0) ]

    let row = podRow pod

    Assert.Equal(("node-1", "Running", "CrashLoopBackOff"), (row.NodeName, row.Phase, row.Status))
    Assert.Equal((2, 3), (row.Ready, row.Desired))

    Assert.Equal<Map<string, int64>>(
        Map
            [ "restarts", 5L
              "ready_containers", 2L
              "total_containers", 3L
              "init_restarts", 0L
              "ready_init_containers", 0L
              "total_init_containers", 0L
              "pod_restart_count", 0L ],
        row.Counts
    )

[<Fact>]
let ``deployment numbers come from its own fields, under stable keys`` () =
    let collector = CollectorDeployment()

    collector.Deployments.Add(
        Deployment(
            Metadata = metadata "ns" "d" "u",
            ReplicasDesired = 5,
            ReadyReplicas = 3,
            UpdatedReplicas = 4,
            AvailableReplicas = 3,
            UnavailableReplicas = 2
        )
    )

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal((3, 5, 3), (row.Ready, row.Desired, row.Available))

    Assert.Equal<Map<string, int64>>(
        Map [ "ready", 3L; "desired", 5L; "updated", 4L; "available", 3L; "unavailable", 2L; "replicas", 0L ],
        row.Counts
    )

[<Fact>]
let ``an object without metadata has no identity and gets no row`` () =
    let rows = podRows [ Pod(NodeName = "metadata omitted on the wire"); Pod(Metadata = metadata "ns" "ok" "u") ]

    Assert.Equal("ok", (Assert.Single rows).Name)

[<Fact>]
let ``every row carries the frame header, and an unset header timestamp is None`` () =
    let row = podRow (Pod(Metadata = metadata "" "p" "u"))

    Assert.Equal((77, 3uy, "protobuf"), (row.OrgID, row.SubscriptionID, row.Encoding))
    // The raw integer off the wire: the frame format gives it no unit.
    Assert.Equal(Some 1700000000L, row.HeaderTimestamp)

    let unset: FrameHeader =
        { Version = 3uy
          Encoding = 0uy
          Type = 41uy
          SubscriptionID = 0uy
          OrgID = 0
          Timestamp = 0L }

    Assert.Equal(None, (Kubeops.frameColumns unset).Timestamp)

[<Fact>]
let ``a frame encoding is stored by name, an unknown one by number`` () =
    Assert.Equal<string list>(
        [ "protobuf"; "json"; "zstd_protobuf"; "unknown_3"; "zstd1x_protobuf"; "zstd_protobuf_nocgo" ],
        [ 0uy .. 5uy ] |> List.map Kubeops.encodingName
    )

[<Fact>]
let ``metadata reaches the row in full: dates, finalizers and every owner`` () =
    let meta =
        Metadata(
            Name = "web-abc",
            Uid = "uid-1",
            CreationTimestamp = 1600000000L,
            DeletionTimestamp = 1600003600L,
            DeletionGracePeriodSeconds = 30L
        )

    meta.Finalizers.Add "kubernetes.io/pvc-protection"

    meta.OwnerReferences.AddRange
        [ OwnerReference(Kind = "ReplicaSet", Name = "web", Uid = "uid-rs")
          OwnerReference(Kind = "Foo", Name = "bar", Uid = "uid-foo") ]

    let row = podRow (Pod(Metadata = meta))

    Assert.Equal(Some(at 1600000000L), row.CreationTimestamp)
    Assert.Equal(Some(at 1600003600L), row.DeletionTimestamp)
    Assert.Equal(Some 30L, row.DeletionGracePeriodSeconds)
    Assert.Equal<string[]>([| "kubernetes.io/pvc-protection" |], row.Finalizers)
    Assert.Equal(("ReplicaSet", "web"), (row.OwnerKind, row.OwnerName))
    Assert.Equal<string[]>([| "ReplicaSet"; "Foo" |], row.OwnerKinds)
    Assert.Equal<string[]>([| "web"; "bar" |], row.OwnerNames)
    // The uid is the only thing a join can use.
    Assert.Equal<string[]>([| "uid-rs"; "uid-foo" |], row.OwnerUIDs)

[<Fact>]
let ``a grace period exists only while an object is being deleted`` () =
    let live = podRow (Pod(Metadata = metadata "" "p" "u"))

    Assert.Equal(None, live.DeletionGracePeriodSeconds)
    Assert.Equal(None, live.DeletionTimestamp)
    Assert.Equal(None, live.CreationTimestamp)

    // For an object that IS being deleted, 0 is a force delete: a real value.
    let deleting = podRow (Pod(Metadata = Metadata(Name = "p", Uid = "u", DeletionTimestamp = 1600003600L)))
    Assert.Equal(Some 0L, deleting.DeletionGracePeriodSeconds)

[<Fact>]
let ``labels are single-valued, tags are a multiset of the frame's and the object's`` () =
    let pod = Pod(Metadata = metadata "" "p" "u")
    pod.Metadata.Labels.AddRange [ "app:web"; "app:api"; "tier" ]
    pod.Metadata.Annotations.Add "note:a:b"
    pod.Tags.Add "env:stage"
    let collector = CollectorPod()
    collector.Tags.AddRange [ "env:prod"; "team" ]
    collector.Pods.Add pod

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal<Map<string, string>>(Map [ "app", "api"; "tier", "" ], row.Labels)
    Assert.Equal<Map<string, string>>(Map [ "note", "a:b" ], row.Annotations)
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod"; "stage" |]; "team", [| "" |] ], row.Tags)

[<Fact>]
let ``a pod keeps each container's status, regular containers first, then init containers`` () =
    let pod =
        Pod(
            Metadata = metadata "shop" "checkout-1" "u",
            IP = "10.4.2.9",
            NominatedNodeName = "node-9",
            QOSClass = "Burstable",
            PriorityClass = "high",
            StartTime = 1600000100L,
            ScheduledTime = 1600000050L,
            Host = Host(Name = "cluster-agent-1"),
            Metrics = ResourceMetrics()
        )

    pod.ContainerStatuses.AddRange
        [ ContainerStatus(
              Name = "app",
              ContainerID = "containerd://aaa",
              Ready = true,
              State = "running",
              Image = "shop/app:1.2",
              ImageID = "sha256:aaa"
          )
          ContainerStatus(
              Name = "sidecar",
              ContainerID = "containerd://bbb",
              RestartCount = 7,
              State = "waiting",
              Message = "back-off 5m0s restarting failed container",
              Image = "envoy:1.29",
              ImageID = "sha256:bbb"
          ) ]

    pod.InitContainerStatuses.Add(
        ContainerStatus(Name = "wait-for-db", ContainerID = "containerd://ccc", Ready = true, State = "terminated")
    )

    pod.Conditions.Add(
        PodCondition(
            Type = "Ready",
            Status = "False",
            Reason = "ContainersNotReady",
            Message = "containers with unready status: [sidecar]",
            LastTransitionTime = 1600000200L,
            LastProbeTime = 1600000300L
        )
    )

    let requirements = ResourceRequirements(Name = "app")
    requirements.Requests["cpu"] <- 100L
    requirements.Limits["memory"] <- 536870912L
    pod.ResourceRequirements.Add requirements
    pod.Metrics.MetricValues["cpu.usage"] <- 0.42

    let row = podRow pod

    Assert.Equal(("10.4.2.9", "node-9", "Burstable", "high"), (row.PodIP, row.NominatedNodeName, row.QOSClass, row.PriorityClass))
    Assert.Equal(Some(at 1600000100L), row.StartTime)
    Assert.Equal(Some(at 1600000050L), row.ScheduledTime)
    // The reporting host, which the pod names itself.
    Assert.Equal("cluster-agent-1", row.HostName)

    Assert.Equal<string[]>([| "app"; "sidecar"; "wait-for-db" |], row.ContainerNames)
    Assert.Equal<string[]>([| "containerd://aaa"; "containerd://bbb"; "containerd://ccc" |], row.ContainerIDs)
    Assert.Equal<uint8[]>([| 0uy; 0uy; 1uy |], row.ContainerIsInit)
    Assert.Equal<uint8[]>([| 1uy; 0uy; 1uy |], row.ContainerReady)
    Assert.Equal<int32[]>([| 0; 7; 0 |], row.ContainerRestarts)
    Assert.Equal<string[]>([| "running"; "waiting"; "terminated" |], row.ContainerStates)
    // The line an operator is after.
    Assert.Equal("back-off 5m0s restarting failed container", row.ContainerMessages[1])
    Assert.Equal(("envoy:1.29", "sha256:bbb"), (row.ContainerImages[1], row.ContainerImageIDs[1]))
    Assert.Equal(1L, row.Counts["ready_init_containers"])

    Assert.Equal<string[]>([| "Ready" |], row.ConditionTypes)
    Assert.Equal<string[]>([| "False" |], row.ConditionStatuses)
    Assert.Equal<string[]>([| "ContainersNotReady" |], row.ConditionReasons)
    Assert.Equal<string[]>([| "containers with unready status: [sidecar]" |], row.ConditionMessages)
    Assert.Equal<DateTime option[]>([| Some(at 1600000200L) |], row.ConditionLastTransition)
    Assert.Equal<DateTime option[]>([| Some(at 1600000300L) |], row.ConditionLastProbe)
    // A pod condition has no lastUpdateTime: the slot is None, not a made-up date.
    Assert.Equal<DateTime option[]>([| None |], row.ConditionLastUpdate)

    Assert.Equal("""[{"limits":{"memory":536870912},"requests":{"cpu":100},"name":"app"}]""", row.ResourceRequirements)
    Assert.Equal<Map<string, float>>(Map [ "cpu.usage", 0.42 ], row.Metrics)

[<Fact>]
let ``a node keeps its cordon flag, taints, capacity, versions and the conditions nested in its status`` () =
    let status =
        NodeStatus(
            Status = "Ready,SchedulingDisabled",
            KubeletVersion = "v1.29.4",
            KubeProxyVersion = "v1.29.4",
            OperatingSystem = "linux",
            Architecture = "amd64",
            KernelVersion = "6.1.0",
            OsImage = "Amazon Linux 2",
            ContainerRuntimeVersion = "containerd://1.7.13"
        )

    status.Capacity["cpu"] <- 8000L
    status.Capacity["memory"] <- 33554432000L
    status.Allocatable["cpu"] <- 7800L
    status.NodeAddresses["InternalIP"] <- "10.0.1.7"

    status.Conditions.Add(
        NodeCondition(Type = "MemoryPressure", Status = "False", Reason = "KubeletHasSufficientMemory", LastTransitionTime = 1600000500L)
    )

    let node =
        Node(
            Metadata = metadata "" "node-1" "u",
            PodCIDR = "10.244.1.0/24",
            Unschedulable = true,
            ProviderID = "aws:///eu-west-1a/i-0abc",
            Status = status
        )

    node.PodCIDRs.Add "10.244.1.0/24"
    node.Roles.Add "worker"
    node.Taints.Add(Taint(Key = "node.kubernetes.io/unreachable", Effect = "NoExecute", TimeAdded = 1600000000L))
    let collector = CollectorNode()
    collector.Nodes.Add node

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal("Node", row.Kind)
    Assert.Equal("Ready,SchedulingDisabled", row.Status)
    Assert.Equal(1uy, row.Unschedulable)
    Assert.Equal("10.244.1.0/24", row.PodCIDR)
    Assert.Equal<string[]>([| "10.244.1.0/24" |], row.PodCIDRs)
    Assert.Equal("aws:///eu-west-1a/i-0abc", row.ProviderID)
    Assert.Equal<string[]>([| "worker" |], row.NodeRoles)
    Assert.Equal("""[{"key":"node.kubernetes.io/unreachable","effect":"NoExecute","timeAdded":1600000000}]""", row.Taints)
    Assert.Equal<Map<string, int64>>(Map [ "cpu", 8000L; "memory", 33554432000L ], row.Capacity)
    Assert.Equal<Map<string, int64>>(Map [ "cpu", 7800L ], row.Allocatable)
    Assert.Equal("""{"InternalIP":"10.0.1.7"}""", row.NodeAddresses)
    Assert.Equal(("v1.29.4", "v1.29.4"), (row.KubeletVersion, row.KubeProxyVersion))
    Assert.Equal(("linux", "amd64", "6.1.0"), (row.OperatingSystem, row.Architecture, row.KernelVersion))
    Assert.Equal(("Amazon Linux 2", "containerd://1.7.13"), (row.OSImage, row.ContainerRuntimeVersion))
    Assert.Equal<string[]>([| "MemoryPressure" |], row.ConditionTypes)
    Assert.Equal<DateTime option[]>([| Some(at 1600000500L) |], row.ConditionLastTransition)
    Assert.Equal<Map<string, int64>>(Map [ "taints", 1L; "images", 0L ], row.Counts)

[<Fact>]
let ``an autoscaler keeps its replica numbers, and its conditions under their other field names`` () =
    let autoscaler =
        HorizontalPodAutoscaler(
            Metadata = metadata "shop" "checkout" "u",
            Spec = HorizontalPodAutoscalerSpec(MinReplicas = 2, MaxReplicas = 20),
            Status = HorizontalPodAutoscalerStatus(CurrentReplicas = 6, DesiredReplicas = 11)
        )

    autoscaler.Conditions.Add(
        HorizontalPodAutoscalerCondition(ConditionType = "ScalingLimited", ConditionStatus = "True", Reason = "TooManyReplicas")
    )

    let collector = CollectorHorizontalPodAutoscaler()
    collector.HorizontalPodAutoscalers.Add autoscaler

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal("HorizontalPodAutoscaler", row.Kind)
    Assert.Equal((6, 11), (row.Ready, row.Desired))

    Assert.Equal<Map<string, int64>>(
        Map [ "min_replicas", 2L; "max_replicas", 20L; "current_replicas", 6L; "desired_replicas", 11L ],
        row.Counts
    )

    Assert.Equal<string[]>([| "ScalingLimited" |], row.ConditionTypes)
    Assert.Equal<string[]>([| "True" |], row.ConditionStatuses)
    Assert.Equal<string[]>([| "TooManyReplicas" |], row.ConditionReasons)

[<Fact>]
let ``each workload kind stores its own numbers under stable keys`` () =
    let rowOf (collector: IMessage) : K8sResourceRow = Kubeops.resourceRows "t" now frame collector |> Assert.Single
    let meta = metadata "ns" "x" "u"

    let replicaSets = CollectorReplicaSet()

    replicaSets.ReplicaSets.Add(
        ReplicaSet(Metadata = meta, ReplicasDesired = 5, Replicas = 4, FullyLabeledReplicas = 4, ReadyReplicas = 3, AvailableReplicas = 2)
    )

    let replicaSet = rowOf replicaSets
    Assert.Equal((3, 5, 2), (replicaSet.Ready, replicaSet.Desired, replicaSet.Available))

    Assert.Equal<Map<string, int64>>(
        Map [ "ready", 3L; "desired", 5L; "available", 2L; "replicas", 4L; "fully_labeled", 4L ],
        replicaSet.Counts
    )

    let daemonSets = CollectorDaemonSet()

    daemonSets.DaemonSets.Add(
        DaemonSet(
            Metadata = meta,
            Status =
                DaemonSetStatus(
                    CurrentNumberScheduled = 6,
                    NumberMisscheduled = 1,
                    DesiredNumberScheduled = 7,
                    NumberReady = 5,
                    UpdatedNumberScheduled = 4,
                    NumberAvailable = 3,
                    NumberUnavailable = 2
                )
        )
    )

    let daemonSet = rowOf daemonSets
    Assert.Equal((5, 7, 3), (daemonSet.Ready, daemonSet.Desired, daemonSet.Available))

    Assert.Equal<Map<string, int64>>(
        Map [ "ready", 5L; "desired", 7L; "current", 6L; "updated", 4L; "available", 3L; "misscheduled", 1L; "unavailable", 2L ],
        daemonSet.Counts
    )

    // A stateful set's desired count is in its spec, not its status.
    let statefulSets = CollectorStatefulSet()

    statefulSets.StatefulSets.Add(
        StatefulSet(
            Metadata = meta,
            Spec = StatefulSetSpec(DesiredReplicas = 3, Partition = 1),
            Status = StatefulSetStatus(Replicas = 3, ReadyReplicas = 2, CurrentReplicas = 2, UpdatedReplicas = 1)
        )
    )

    let statefulSet = rowOf statefulSets
    Assert.Equal((2, 3, 0), (statefulSet.Ready, statefulSet.Desired, statefulSet.Available))

    Assert.Equal<Map<string, int64>>(
        Map [ "ready", 2L; "desired", 3L; "current", 2L; "updated", 1L; "replicas", 3L; "partition", 1L ],
        statefulSet.Counts
    )

    let jobs = CollectorJob()

    jobs.Jobs.Add(
        Job(
            Metadata = meta,
            Spec = JobSpec(Parallelism = 2, Completions = 4, BackoffLimit = 6, ActiveDeadlineSeconds = 600L),
            Status = JobStatus(Active = 1, Succeeded = 3, Failed = 1, StartTime = 1600000000L)
        )
    )

    let job = rowOf jobs
    Assert.Equal(Some(at 1600000000L), job.StartTime)

    Assert.Equal<Map<string, int64>>(
        Map
            [ "active", 1L
              "succeeded", 3L
              "failed", 1L
              "parallelism", 2L
              "completions", 4L
              "backoff_limit", 6L
              "active_deadline_seconds", 600L ],
        job.Counts
    )

    let cronJob = CronJob(Metadata = meta, Spec = CronJobSpec(Suspend = true, SuccessfulJobsHistoryLimit = 3, FailedJobsHistoryLimit = 1), Status = CronJobStatus())
    cronJob.Status.Active.Add(ObjectReference(Name = "run-1"))
    let cronJobs = CollectorCronJob()
    cronJobs.CronJobs.Add cronJob

    Assert.Equal<Map<string, int64>>(
        Map
            [ "active_jobs", 1L
              "suspend", 1L
              "successful_jobs_history_limit", 3L
              "failed_jobs_history_limit", 1L
              "starting_deadline_seconds", 0L ],
        (rowOf cronJobs).Counts
    )

    let budget =
        PodDisruptionBudget(
            Metadata = meta,
            Status = PodDisruptionBudgetStatus(DisruptionsAllowed = 1, CurrentHealthy = 3, DesiredHealthy = 2, ExpectedPods = 3)
        )

    budget.Status.DisruptedPods["pod-1"] <- 1600000000L
    let budgets = CollectorPodDisruptionBudget()
    budgets.PodDisruptionBudgets.Add budget

    Assert.Equal<Map<string, int64>>(
        Map [ "disruptions_allowed", 1L; "current_healthy", 3L; "desired_healthy", 2L; "expected_pods", 3L; "disrupted_pods", 1L ],
        (rowOf budgets).Counts
    )

    let service = Service(Metadata = meta, Spec = ServiceSpec(Type = "ClusterIP", ClusterIP = "10.96.0.10"))
    service.Spec.Ports.Add(ServicePort(Name = "http", Protocol = "TCP", Port = 80, TargetPort = "8080"))
    let services = CollectorService()
    services.Services.Add service
    let serviceRow = rowOf services
    Assert.Equal(("ClusterIP", "10.96.0.10"), (serviceRow.ServiceType, serviceRow.ClusterIP))
    Assert.Equal("""[{"name":"http","protocol":"TCP","port":80,"targetPort":"8080"}]""", serviceRow.ServicePorts)
    Assert.Equal<Map<string, int64>>(Map [ "ports", 1L ], serviceRow.Counts)

    let namespaces = CollectorNamespace()
    namespaces.Namespaces.Add(Namespace(Metadata = meta, Status = "Terminating"))
    Assert.Equal("Terminating", (rowOf namespaces).Status)

    // A kind whose status or spec is missing reads as zeros, not a failure.
    let bare = CollectorDaemonSet()
    bare.DaemonSets.Add(DaemonSet(Metadata = meta))
    Assert.Equal(0L, (rowOf bare).Counts["ready"])

[<Fact>]
let ``a role keeps the permissions it grants`` () =
    let rule = PolicyRule()
    rule.ApiGroups.Add ""
    rule.Resources.Add "secrets"
    rule.Verbs.AddRange [ "get"; "list" ]
    let role = Role(Metadata = metadata "kube-system" "secret-reader" "u")
    role.Rules.Add rule
    let collector = CollectorRole()
    collector.Roles.AddRange [ role; Role(Metadata = metadata "kube-system" "empty" "u2") ]

    let rows = Kubeops.resourceRows "t" now frame collector

    Assert.Equal(1L, rows[0].Counts["rules"])
    Assert.Equal("""[{"verbs":["get","list"],"apiGroups":[""],"resources":["secrets"]}]""", rows[0].RBACRules)
    // What the Go server stored for a role with no rules.
    Assert.Equal("null", rows[1].RBACRules)

[<Fact>]
let ``a role binding keeps its subjects and the role it refers to`` () =
    let binding =
        RoleBinding(
            Metadata = metadata "shop" "readers" "u",
            RoleRef = TypedLocalObjectReference(Kind = "Role", Name = "secret-reader")
        )

    binding.Subjects.Add(Subject(Kind = "ServiceAccount", Name = "ci", Namespace = "shop"))
    let collector = CollectorRoleBinding()
    collector.RoleBindings.AddRange [ binding; RoleBinding(Metadata = metadata "shop" "empty" "u2") ]

    let rows = Kubeops.resourceRows "t" now frame collector

    Assert.Equal("""[{"kind":"ServiceAccount","name":"ci","namespace":"shop"}]""", rows[0].RBACSubjects)
    Assert.Equal("""{"kind":"Role","name":"secret-reader"}""", rows[0].RBACRoleRef)
    Assert.Equal<Map<string, int64>>(Map [ "subjects", 1L ], rows[0].Counts)
    Assert.Equal(("null", "null"), (rows[1].RBACSubjects, rows[1].RBACRoleRef))

[<Fact>]
let ``a kind with no column of its own is kept whole in the object column`` () =
    let storageClass =
        StorageClass(
            Metadata = metadata "" "gp3" "u",
            Provisioner = "ebs.csi.aws.com",
            ReclaimPolicy = "Delete",
            VolumeBindingMode = "WaitForFirstConsumer"
        )

    storageClass.Parameters["type"] <- "gp3"
    storageClass.Parameters["encrypted"] <- "true"
    let collector = CollectorStorageClass(ClusterName = "prod")
    collector.StorageClasses.Add storageClass

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal("StorageClass", row.Kind)

    for expected in [ "ebs.csi.aws.com"; "gp3"; "WaitForFirstConsumer"; "Delete"; "encrypted" ] do
        Assert.Contains(expected, row.Object)

    // The envelope must not repeat the object list: that would square the payload.
    Assert.Equal("""{"clusterName":"prod"}""", row.Envelope)

[<Fact>]
let ``the JSON columns hold what Go's encoding/json wrote, not protobuf's JSON`` () =
    // The expected text is the Go server's own output for this frame: .proto
    // field names, zero values left out, 64-bit integers and enums as
    // numbers, bytes as base64, an empty message still present.
    let requirements = ResourceRequirements(Name = "app", Type = enum<ResourceRequirementsType> 1)
    requirements.Requests["memory"] <- 9007199254740993L
    requirements.Requests["cpu"] <- 100L

    let pod =
        Pod(
            Metadata = metadata "shop" "web-1" "u-1",
            IP = "10.0.0.1",
            QOSClass = "Burstable",
            Metrics = ResourceMetrics(),
            Host = Host(Id = 9007199254740993L, Name = "node-1"),
            NodeAffinity = NodeAffinity()
        )

    // The yaml field (10, bytes) is deprecated and its setter with it, so the
    // two bytes are merged in off the wire.
    pod.MergeFrom [| 0x52uy; 0x02uy; 0xffuy; 0x00uy |]
    pod.Metadata.Labels.Add "app:web"
    pod.ContainerStatuses.Add(ContainerStatus(Name = "app", Ready = true, RestartCount = 2))
    pod.ResourceRequirements.Add requirements
    pod.Metrics.MetricValues["cpu.usage"] <- 0.42

    let collector =
        CollectorPod(
            HostName = "cluster-agent-1",
            ClusterName = "prod",
            ClusterId = "cid",
            GroupId = 1,
            GroupSize = 2,
            IsTerminated = true,
            Info = SystemInfo(Uuid = "sys-uuid", TotalMemory = 9007199254740993L),
            AgentVersion = AgentVersion(Major = 7L, Minor = 55L, Patch = 1L)
        )

    collector.Tags.Add "env:prod"
    collector.Pods.Add pod

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal(
        """{"metadata":{"name":"web-1","namespace":"shop","uid":"u-1","labels":["app:web"]},"IP":"10.0.0.1","containerStatuses":[{"name":"app","ready":true,"restartCount":2}],"yaml":"/wA=","host":{"id":9007199254740993,"name":"node-1"},"resourceRequirements":[{"requests":{"cpu":100,"memory":9007199254740993},"name":"app","type":1}],"QOSClass":"Burstable","metrics":{"metricValues":{"cpu.usage":0.42}},"nodeAffinity":{}}""",
        row.Object
    )

    // The envelope keeps the extras no column holds: the cluster agent's own
    // system info, and the flag that says this is the terminated-pods batch.
    Assert.Equal(
        """{"hostName":"cluster-agent-1","clusterName":"prod","clusterId":"cid","groupId":1,"groupSize":2,"tags":["env:prod"],"info":{"uuid":"sys-uuid","totalMemory":9007199254740993},"isTerminated":true,"agentVersion":{"Major":7,"Minor":55,"Patch":1}}""",
        row.Envelope
    )

    Assert.Equal("""[{"requests":{"cpu":100,"memory":9007199254740993},"name":"app","type":1}]""", row.ResourceRequirements)
    Assert.Equal("7.55.1", row.AgentVersion)
    // The pod names its own host, which wins over the envelope's.
    Assert.Equal("node-1", row.HostName)

[<Fact>]
let ``a number with no JSON form empties the object column and keeps the row`` () =
    let pod = Pod(Metadata = metadata "" "p" "u", Metrics = ResourceMetrics())
    pod.Metrics.MetricValues["nan"] <- nan

    let row = podRow pod

    Assert.Equal("", row.Object)
    Assert.True(Double.IsNaN row.Metrics["nan"])

[<Fact>]
let ``a vertical autoscaler's conditions are read from its status`` () =
    // The list nested in Status is the one a real autoscaler fills, and its
    // fields are named ConditionType/ConditionStatus.
    let status = VerticalPodAutoscalerStatus()

    status.Conditions.Add(
        VPACondition(
            ConditionType = "RecommendationProvided",
            ConditionStatus = "True",
            Message = "recommendation computed",
            LastTransitionTime = 1700000000L
        )
    )

    let collector = CollectorVerticalPodAutoscaler(ClusterId = "cid")
    collector.VerticalPodAutoscalers.Add(VerticalPodAutoscaler(Metadata = metadata "shop" "vpa-1" "u-1", Status = status))

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal<string[]>([| "RecommendationProvided" |], row.ConditionTypes)
    Assert.Equal<string[]>([| "True" |], row.ConditionStatuses)
    Assert.Equal<string[]>([| "recommendation computed" |], row.ConditionMessages)
    Assert.Equal<DateTime option[]>([| Some(at 1700000000L) |], row.ConditionLastTransition)

[<Fact>]
let ``a vertical autoscaler with conditions in both places keeps both, the object's first`` () =
    let status = VerticalPodAutoscalerStatus()
    status.Conditions.Add(VPACondition(ConditionType = "Nested", ConditionStatus = "False"))
    let autoscaler = VerticalPodAutoscaler(Metadata = metadata "" "vpa-1" "u-1", Status = status)
    autoscaler.Conditions.Add(VerticalPodAutoscalerCondition(Type = "TopLevel", Status = "True"))
    let collector = CollectorVerticalPodAutoscaler()
    collector.VerticalPodAutoscalers.Add autoscaler

    let row = Kubeops.resourceRows "t" now frame collector |> Assert.Single

    Assert.Equal<string[]>([| "TopLevel"; "Nested" |], row.ConditionTypes)
    Assert.Equal<string[]>([| "True"; "False" |], row.ConditionStatuses)

// ---- the cluster summary ----

[<Fact>]
let ``the cluster row keeps both tag sets apart and the per-node breakdown`` () =
    let cluster =
        Cluster(NodeCount = 2, ResourceVersion = "9912", CreationTimestamp = 1600000000L, Metrics = ResourceMetrics())

    cluster.Tags.Add "kube_cluster_name:prod"
    cluster.KubeletVersions["v1.29.4"] <- 1
    cluster.KubeletVersions["v1.28.9"] <- -1
    cluster.Metrics.MetricValues["nodes.ready"] <- 2.0
    cluster.ExtendedResourcesCapacity["nvidia.com/gpu"] <- 4L
    let first = ClusterNodeInfo(Name = "node-1", Region = "eu-west-1", InstanceType = "m6i.large", KubeletVersion = "v1.29.4")
    first.ResourceCapacity["cpu"] <- "2"
    let second = ClusterNodeInfo(Name = "node-2", Region = "eu-west-1", InstanceType = "m6i.xlarge", KubeletVersion = "v1.28.9")
    cluster.NodesInfo.AddRange [ first; second ]

    let collector =
        CollectorCluster(
            ClusterName = "prod",
            ClusterId = "cid",
            GroupId = 1,
            GroupSize = 2,
            AgentVersion = AgentVersion(Major = 7L, Minor = 55L, Patch = 1L, Pre = "rc.1"),
            Cluster = cluster
        )

    collector.Tags.AddRange [ "env:prod"; "team:infra" ]

    let row = Kubeops.clusterRows "t" now frame collector |> Assert.Single

    Assert.Equal((77, "protobuf"), (row.OrgID, row.Encoding))
    Assert.Equal((1, 2, "7.55.1-rc.1"), (row.GroupID, row.GroupSize, row.AgentVersion))
    Assert.Equal(2u, row.NodeCount)
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |]; "team", [| "infra" |] ], row.Tags)
    Assert.Equal<Map<string, string[]>>(Map [ "kube_cluster_name", [| "prod" |] ], row.ClusterTags)
    Assert.Equal("9912", row.ResourceVersion)
    Assert.Equal(Some(at 1600000000L), row.CreationTimestamp)
    Assert.Equal<Map<string, float>>(Map [ "nodes.ready", 2.0 ], row.Metrics)
    Assert.Equal<Map<string, int64>>(Map [ "nvidia.com/gpu", 4L ], row.ExtendedResourcesCapacity)
    // A negative node count cannot be real, and must not wrap to billions.
    Assert.Equal<Map<string, uint32>>(Map [ "v1.29.4", 1u; "v1.28.9", 0u ], row.KubeletVersions)
    Assert.Equal<string[]>([| "node-1"; "node-2" |], row.NodesName)
    Assert.Equal<string[]>([| "v1.29.4"; "v1.28.9" |], row.NodesKubeletVersion)
    Assert.Equal<Map<string, string>[]>([| Map [ "cpu", "2" ]; Map.empty |], row.NodesCapacity)
    Assert.Equal<Map<string, string>[]>([| Map.empty; Map.empty |], row.NodesAllocatable)

[<Fact>]
let ``a cluster frame with no cluster in it gives no row`` () =
    Assert.Empty(Kubeops.clusterRows "t" now frame (CollectorCluster(ClusterName = "prod")))

// ---- manifests ----

[<Fact>]
let ``a CRD or CR frame whose inner envelope is missing gives no rows`` () =
    Assert.Empty(Kubeops.manifestRows "t" now frame null [])
    Assert.Empty(Kubeops.manifestRows "t" now frame (CollectorManifestCRD().Manifest) [])

[<Fact>]
let ``a manifest row holds the document as it was collected`` () =
    let collector = CollectorManifest(ClusterName = "prod", ClusterId = "cid")

    collector.Manifests.Add(
        Manifest(
            Uid = "u1",
            Kind = "ConfigMap",
            ApiVersion = "v1",
            ResourceVersion = "7",
            Content = ByteString.CopyFromUtf8 "kind: ConfigMap",
            ContentType = "yaml",
            IsTerminated = true
        )
    )

    let row = Kubeops.manifestRows "t" now frame collector [] |> Assert.Single

    Assert.Equal(("u1", "ConfigMap", "v1", "7"), (row.UID, row.Kind, row.APIVersion, row.ResourceVersion))
    Assert.Equal("kind: ConfigMap", Encoding.UTF8.GetString row.Content)
    Assert.Equal(("yaml", 1uy, 1uy), (row.ContentType, row.IsTerminated, row.ContentIsUTF8))
    Assert.Equal(("prod", "cid"), (row.ClusterName, row.ClusterID))
    // A manifest with no Host leaves the column empty, not "null".
    Assert.Equal("", row.ManifestHost)

[<Fact>]
let ``manifest tags merge the frame's, the wrapper's and the manifest's, and remember which was which`` () =
    let manifest =
        Manifest(
            Uid = "u1",
            Kind = "DatadogAgent",
            ApiVersion = "datadoghq.com/v2alpha1",
            Type = 81,
            Version = "v1",
            NodeName = "node-3",
            Content = ByteString.CopyFromUtf8 "apiVersion: datadoghq.com/v2alpha1",
            ContentType = "json"
        )

    manifest.Tags.Add "team:infra"
    manifest.ExtraAttributes["crd_group"] <- "datadoghq.com"

    let collector =
        CollectorManifest(
            ClusterName = "prod",
            ClusterId = "cid",
            GroupId = 4,
            GroupSize = 8,
            HostName = "cluster-agent-1",
            AgentVersion = AgentVersion(Major = 7L, Minor = 55L, Patch = 1L),
            OriginCollector = OriginCollector.DatadogAgent
        )

    collector.Tags.Add "env:prod"
    collector.Manifests.Add manifest

    let row = Kubeops.manifestRows "t" now frame collector [ "source:crd" ] |> Assert.Single

    Assert.Equal((4, 8, "cluster-agent-1", "7.55.1"), (row.GroupID, row.GroupSize, row.HostName, row.AgentVersion))
    Assert.Equal(("datadogAgent", 77), (row.OriginCollector, row.OrgID))
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |]; "source", [| "crd" |]; "team", [| "infra" |] ], row.Tags)

    Assert.Equal<Map<string, string[]>>(
        Map [ "envelope", [| "env:prod" |]; "wrapper", [| "source:crd" |]; "manifest", [| "team:infra" |] ],
        row.TagSources
    )

    Assert.Equal(("node-3", 81, "v1"), (row.NodeName, row.Type, row.Version))
    Assert.Equal<Map<string, string>>(Map [ "crd_group", "datadoghq.com" ], row.ExtraAttributes)
    Assert.Equal(1uy, row.ContentIsUTF8)

[<Fact>]
let ``content that is not UTF-8 is kept byte for byte and flagged`` () =
    let collector = CollectorManifest()
    collector.Manifests.Add(Manifest(Uid = "u", Content = ByteString.CopyFrom [| 0xffuy; 0xfeuy; 0x00uy |]))

    let row = Kubeops.manifestRows "t" now frame collector [] |> Assert.Single

    Assert.Equal(0uy, row.ContentIsUTF8)
    Assert.Equal<byte[]>([| 0xffuy; 0xfeuy; 0x00uy |], row.Content)
    // A source with no tags is left out rather than listed empty.
    Assert.Empty row.TagSources

[<Fact>]
let ``a manifest row keeps the sender's system info and the host the object was seen on`` () =
    let collector =
        CollectorManifest(
            ClusterId = "cid",
            SystemInfo = SystemInfo(Uuid = "agent-uuid-1", Os = OSInfo(Name = "linux", Platform = "ubuntu"), TotalMemory = 8589934592L)
        )

    collector.Manifests.Add(
        Manifest(Uid = "u-1", Kind = "Pod", Content = ByteString.CopyFromUtf8 "{}", Host = Host(Id = 42L, OrgId = 7, Name = "node-1", NumCpus = 8))
    )

    let row = Kubeops.manifestRows "t" now frame collector [] |> Assert.Single

    // Without the manifest list: the list is the rows.
    Assert.Equal(
        """{"clusterId":"cid","systemInfo":{"uuid":"agent-uuid-1","os":{"name":"linux","platform":"ubuntu"},"totalMemory":8589934592}}""",
        row.Envelope
    )

    Assert.Equal("""{"id":42,"orgId":7,"name":"node-1","numCpus":8}""", row.ManifestHost)

// ---- ECS tasks ----

[<Fact>]
let ``an ECS task row is keyed on its ARN and keeps its four tag sets apart`` () =
    let task =
        ECSTask(
            Arn = "arn:aws:ecs:eu-west-1:123:task/abc",
            ResourceVersion = "rv-1",
            LaunchType = "FARGATE",
            DesiredStatus = "RUNNING",
            KnownStatus = "RUNNING",
            Family = "checkout",
            Version = "3",
            AvailabilityZone = "eu-west-1a",
            ServiceName = "checkout-svc",
            VpcId = "vpc-1",
            ContainerInstanceArn = "arn:aws:ecs:...:container-instance/xyz",
            DaemonName = "datadog-agent",
            Host = Host(Name = "ip-10-0-1-7"),
            PullStartedAt = 1600000000L,
            PullStoppedAt = 1600000030L
        )

    task.Limits["CPU"] <- 1024.0
    task.Limits["Memory"] <- 2048.0
    task.EphemeralStorageMetrics["utilized"] <- 100L
    task.Containers.Add(ECSContainer(Name = "app", DockerID = "d1"))
    task.Tags.Add "service:checkout"
    task.EcsTags.Add "ecs:tag"
    task.ContainerInstanceTags.Add "instance:tag"

    let collector =
        CollectorECSTask(
            AwsAccountID = 123456789012L,
            ClusterName = "ecs-prod",
            ClusterId = "ecs-cid",
            Region = "eu-west-1",
            GroupId = 2,
            GroupSize = 5,
            HostName = "ecs-agent-1",
            AgentVersion = AgentVersion(Major = 7L, Minor = 55L, Patch = 1L)
        )

    collector.Tags.Add "env:prod"
    collector.Tasks.Add task

    let row = Kubeops.ecsTaskRows "t" now frame collector |> Assert.Single

    Assert.Equal(("arn:aws:ecs:eu-west-1:123:task/abc", "checkout", "FARGATE"), (row.ARN, row.Family, row.LaunchType))
    Assert.Equal(123456789012L, row.AWSAccountID)
    Assert.Equal(("eu-west-1", "ecs-agent-1", "ip-10-0-1-7"), (row.Region, row.HostName, row.TaskHostName))
    Assert.Equal(("ecs-prod", "ecs-cid", 2, 5, "7.55.1"), (row.ClusterName, row.ClusterID, row.GroupID, row.GroupSize, row.AgentVersion))
    Assert.Equal((77, 3uy, Some 1700000000L, "protobuf"), (row.OrgID, row.SubscriptionID, row.HeaderTimestamp, row.Encoding))
    Assert.Equal<Map<string, float>>(Map [ "CPU", 1024.0; "Memory", 2048.0 ], row.Limits)
    Assert.Equal<Map<string, int64>>(Map [ "utilized", 100L ], row.EphemeralStorageMetrics)
    // A task still running has no stop time, and unset must not become 1970.
    Assert.Equal((Some(at 1600000000L), Some(at 1600000030L), None), (row.PullStartedAt, row.PullStoppedAt, row.ExecutionStoppedAt))
    Assert.Equal(1u, row.ContainerCount)
    Assert.Equal("""[{"dockerID":"d1","name":"app"}]""", row.Containers)
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |] ], row.EnvelopeTags)
    Assert.Equal<Map<string, string[]>>(Map [ "service", [| "checkout" |] ], row.Tags)
    Assert.Equal<Map<string, string[]>>(Map [ "ecs", [| "tag" |] ], row.ECSTags)
    Assert.Equal<Map<string, string[]>>(Map [ "instance", [| "tag" |] ], row.ContainerInstanceTags)

[<Fact>]
let ``an ECS task row keeps the envelope's own host and info, and the task's host in full`` () =
    let collector =
        CollectorECSTask(
            ClusterId = "ecs-cid",
            Host = Host(Id = 11L, Name = "ecs-collector"),
            Info = SystemInfo(Uuid = "ecs-agent-uuid", TotalMemory = 4294967296L)
        )

    collector.Host.AllTags.Add "role:collector"

    collector.Tasks.Add(
        ECSTask(Arn = "arn:aws:ecs:eu-west-1:123:task/abc", Host = Host(Id = 99L, OrgId = 7, Name = "ip-10-0-1-7", NumCpus = 4))
    )

    let row = Kubeops.ecsTaskRows "t" now frame collector |> Assert.Single

    Assert.Equal(
        """{"clusterId":"ecs-cid","host":{"id":11,"name":"ecs-collector","allTags":["role:collector"]},"info":{"uuid":"ecs-agent-uuid","totalMemory":4294967296}}""",
        row.Envelope
    )

    Assert.Equal("""{"id":99,"orgId":7,"name":"ip-10-0-1-7","numCpus":4}""", row.TaskHost)
    Assert.Equal("ip-10-0-1-7", row.TaskHostName)
    // What the Go server stored for a task with no containers.
    Assert.Equal("null", row.Containers)

// ---- kubeactions ----

[<Fact>]
let ``an empty or unparseable action timestamp becomes the arrival time, never 1970`` () =
    let timestampOf (text: string) : DateTime =
        (Kubeops.actionRow "t" now (decoded $"""{{"timestamp":"{text}"}}""")).Timestamp

    Assert.Equal(now, timestampOf "")
    Assert.Equal(now, timestampOf "yesterday-ish")
    Assert.Equal(DateTime(2026, 9, 20, 8, 30, 0, DateTimeKind.Utc), timestampOf "2026-09-20T08:30:00Z")
    Assert.Equal(DateTime(2026, 9, 20, 8, 30, 0, DateTimeKind.Utc), timestampOf "2026-09-20T10:30:00+02:00")

[<Fact>]
let ``keys an action event does not declare surface in the row, sorted`` () =
    let event =
        decoded
            """{
                "action_id": "a-1",
                "event_type": "action_executed",
                "status": "success",
                "zeta_field": 1,
                "alpha_field": {"nested": true}
            }"""

    let row = Kubeops.actionRow "t" now event

    Assert.Equal(("a-1", "action_executed", "success"), (row.ActionID, row.EventType, row.Status))
    Assert.Equal<string[]>([| "alpha_field"; "zeta_field" |], row.ExtraKeys)
    Assert.Equal("""{"alpha_field":{"nested":true},"zeta_field":1}""", row.Extra)

[<Fact>]
let ``an action row keeps its attachments, its undeclared values and the timestamp as it arrived`` () =
    let event =
        decoded
            """{
                "action_id": "a-1",
                "org_id": 9007199254740993,
                "status": "failed",
                "timestamp": "not-a-date",
                "payloads": {"stderr": "YmFk", "binary": "/wD+", "nothing": null},
                "new_field": {"nested": 7}
            }"""

    let row = Kubeops.actionRow "t" now event

    Assert.Equal(9007199254740993L, row.OrgID)
    Assert.Equal<byte[]>("bad"B, row.Payloads["stderr"])
    Assert.Equal<byte[]>([| 0xffuy; 0x00uy; 0xfeuy |], row.Payloads["binary"])
    Assert.Equal<byte[]>([||], row.Payloads["nothing"])
    Assert.Equal<string[]>([| "new_field" |], row.ExtraKeys)
    Assert.Equal("""{"new_field":{"nested":7}}""", row.Extra)
    Assert.Equal("not-a-date", row.TimestampRaw)
    Assert.Equal(now, row.Timestamp)

[<Fact>]
let ``an action row with nothing extra has empty extra columns`` () =
    let row = Kubeops.actionRow "t" now (decoded """{"action_id":"a-1"}""")

    Assert.Empty row.ExtraKeys
    Assert.Equal("", row.Extra)
    Assert.Empty row.Payloads

[<Fact>]
let ``an action event is decoded as the Go server decoded it`` () =
    // A key matches whatever its letter case; the last one wins.
    Assert.Equal("y", (decoded """{"action_id":"x","ACTION_ID":"y"}""").ActionID)
    Assert.Equal("x", (decoded """{"ACTION_ID":"y","action_id":"x"}""").ActionID)
    // ...and a key that is not exactly a known one is also kept as extra.
    Assert.Equal<string list>([ "ACTION_ID" ], (decoded """{"ACTION_ID":"y"}""").Extra |> Map.toList |> List.map fst)
    Assert.Equal("", (decoded """{"actionid":"x","action-id":"x"}""").ActionID)

    // null leaves a field as it was.
    Assert.Equal("x", (decoded """{"action_id":"x","action_id":null}""").ActionID)
    Assert.Equal(0L, (decoded """{"org_id":null,"payloads":null}""").OrgID)
    Assert.Equal(-5L, (decoded """{"org_id":-5}""").OrgID)

    // null and {} are events with nothing in them.
    Assert.Equal("", (decoded "null").ActionID)
    Assert.Equal("", (decoded "{}").ActionID)

    // A value of the wrong type refuses the whole event.
    Assert.True(refused """{"action_id":5}""")
    Assert.True(refused """{"timestamp":123}""")
    Assert.True(refused """{"org_id":"5"}""")
    Assert.True(refused """{"org_id":5.0}""")
    Assert.True(refused """{"org_id":1e3}""")
    Assert.True(refused """{"org_id":9223372036854775808}""")
    Assert.True(refused """{"payloads":"x"}""")
    Assert.True(refused """{"action_id":"ok","org_id":"bad"}""")
    Assert.True(refused "5")
    Assert.True(refused "[]")
    Assert.True(refused "\"text\"")

[<Fact>]
let ``attachments are padded standard base64, line breaks allowed`` () =
    let attachment (text: string) : byte[] =
        (decoded $"""{{"payloads":{{"a":"{text}"}}}}""").Payloads["a"]

    Assert.Equal<byte[]>("ba"B, attachment "YmE=")
    Assert.Equal<byte[]>("bad"B, attachment "Ym\\nFk")
    Assert.Equal<byte[]>([||], attachment "")
    Assert.True(refused """{"payloads":{"a":"YmE"}}""")
    Assert.True(refused """{"payloads":{"a":"Y mFk"}}""")
    Assert.True(refused """{"payloads":{"a":"-_-_"}}""")
    Assert.True(refused """{"payloads":{"a":5}}""")

    // A repeated "payloads" key adds to what the first one attached.
    let merged = decoded """{"payloads":{"a":"YQ=="},"payloads":{"b":"Yg=="}}"""
    Assert.Equal<string list>([ "a"; "b" ], merged.Payloads |> Map.toList |> List.map fst)

// ---- through the intake ----

[<Fact>]
let ``POST /api/v2/orchmanif stores a CRD frame's manifests with the wrapper's tags`` () =
    let inner = CollectorManifest(ClusterName = "prod")
    inner.Manifests.Add(Manifest(Uid = "u1", Kind = "CustomResourceDefinition", Content = ByteString.CopyFromUtf8 "kind: CRD"))
    let wrapper = CollectorManifestCRD(Manifest = inner)
    wrapper.Tags.Add "source:crd"

    let response, sink = post "/api/v2/orchmanif" (ProcessFrame.encode 81uy 0L wrapper)

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    Assert.Equal<string list>([ "storage_k8s_manifests" ], sink.Writes |> List.map _.Writer)
    let row = sink.Rows<K8sManifestRow>() |> Assert.Single
    Assert.Equal((Replay.testTenant, "prod", "u1"), (row.TenantID, row.ClusterName, row.UID))
    Assert.Equal<Map<string, string[]>>(Map [ "wrapper", [| "source:crd" |] ], row.TagSources)
    Assert.Equal(None, row.HeaderTimestamp)

[<Fact>]
let ``a CR frame with nothing inside it is accepted and stores nothing`` () =
    let response, sink = post "/api/v2/orchmanif" (ProcessFrame.encode 82uy 0L (CollectorManifestCR()))

    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``POST /api/v1/orchestrator, the legacy path, stores a cluster frame`` () =
    let collector = CollectorCluster(ClusterName = "prod", ClusterId = "cid", Cluster = Cluster(NodeCount = 3))

    let response, sink = post "/api/v1/orchestrator" (ProcessFrame.encode 46uy 1700000123L collector)

    Assert.Equal(202, response.Status)
    Assert.Equal<string list>([ "storage_k8s_cluster" ], sink.Writes |> List.map _.Writer)
    let row = sink.Rows<K8sClusterRow>() |> Assert.Single
    Assert.Equal((Replay.testTenant, "cid", 3u), (row.TenantID, row.ClusterID, row.NodeCount))
    Assert.Equal(Some 1700000123L, row.HeaderTimestamp)

[<Fact>]
let ``one bad action in a batch is kept raw while the rest become rows`` () =
    let body = """[{"action_id":"a-1"}, {"action_id": 5}, {"action_id":"a-3"}]"""

    let response, sink = post "/api/v2/kubeactions" (Encoding.UTF8.GetBytes body)

    Assert.Equal(202, response.Status)
    Assert.Equal<string list>([ "storage_raw_payloads"; "storage_k8s_actions" ], sink.Writes |> List.map _.Writer)
    let raw = sink.Rows<RawPayloadRow>() |> Assert.Single
    Assert.Equal(("kubeactions", "decode_error"), (raw.Intake, raw.Reason))
    Assert.StartsWith("batch element 1: ", raw.Note)
    // The element's own JSON, as it stood in the batch.
    Assert.Equal("""{"action_id": 5}""", Encoding.UTF8.GetString raw.Body)
    Assert.Equal<string list>([ "a-1"; "a-3" ], sink.Rows<K8sActionRow>() |> List.map _.ActionID)

[<Fact>]
let ``a kubeactions body of null or an empty array stores nothing`` () =
    for body in [ "null"; "[]" ] do
        let response, sink = post "/api/v2/kubeactions" (Encoding.UTF8.GetBytes body)
        Assert.Equal(202, response.Status)
        Assert.Empty sink.Writes
