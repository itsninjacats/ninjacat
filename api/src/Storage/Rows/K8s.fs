namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

// Every orchestrator row carries the frame header (OrgID, SubscriptionID,
// HeaderTimestamp, Encoding). Today's agent leaves the first three at zero,
// which is the reason to keep them: the day a sender fills them, they are
// already stored. HeaderTimestamp is the raw integer because the frame
// format documents no unit for it; None when the wire said 0.

/// One Kubernetes object in one collection pass, whatever its kind.
///
/// Three layers keep 27 kinds in one table: what every kind has is a column;
/// numbers that differ per kind travel in Counts; and Object holds the whole
/// message as JSON, so a kind with no typed column of its own loses nothing.
///
/// The parallel arrays (Owner*, Condition*, Container*) are index-aligned and
/// in wire order.
///
/// k8s_resources_current is fed from this table by a materialized view that
/// lists every column by name: a field added here needs the column on both
/// tables and in the view.
type K8sResourceRow =
    { TenantID: string
      CollectedAt: DateTime
      ClusterID: string
      ClusterName: string
      Kind: string
      Namespace: string
      Name: string
      UID: string
      ResourceVersion: string
      /// The first owner reference; all of them are in Owner* below.
      OwnerKind: string
      OwnerName: string
      NodeName: string
      Phase: string
      Status: string
      Ready: int32
      Desired: int32
      Available: int32
      Counts: Map<string, int64>
      Labels: Map<string, string>
      Annotations: Map<string, string>
      Tags: Map<string, string[]>
      AgentVersion: string
      GroupID: int32
      GroupSize: int32
      OrgID: int32
      SubscriptionID: uint8
      HeaderTimestamp: int64 option
      Encoding: string
      CreationTimestamp: DateTime option
      DeletionTimestamp: DateTime option
      DeletionGracePeriodSeconds: int64 option
      Finalizers: string[]
      OwnerKinds: string[]
      OwnerNames: string[]
      OwnerUIDs: string[]
      ConditionTypes: string[]
      ConditionStatuses: string[]
      ConditionReasons: string[]
      ConditionMessages: string[]
      ConditionLastTransition: DateTime option[]
      ConditionLastUpdate: DateTime option[]
      ConditionLastProbe: DateTime option[]
      /// The agent's own one-line summary, a separate wire field from the
      /// condition list.
      ConditionMessage: string
      /// JSON: one object carries several (one per container), each with two
      /// maps, so a flat map would lose which container the numbers belong to.
      ResourceRequirements: string
      Metrics: Map<string, float>
      /// The whole object as JSON.
      Object: string
      // Pod.
      PodIP: string
      NominatedNodeName: string
      QOSClass: string
      PriorityClass: string
      StartTime: DateTime option
      ScheduledTime: DateTime option
      /// The reporting agent's host, which is not NodeName for cluster-scoped kinds.
      HostName: string
      // Container statuses: regular containers first, then init containers,
      // told apart by ContainerIsInit.
      ContainerNames: string[]
      ContainerIDs: string[]
      ContainerReady: uint8[]
      ContainerRestarts: int32[]
      ContainerStates: string[]
      ContainerMessages: string[]
      ContainerImages: string[]
      ContainerImageIDs: string[]
      ContainerIsInit: uint8[]
      // Node.
      PodCIDR: string
      PodCIDRs: string[]
      Unschedulable: uint8
      /// JSON.
      Taints: string
      ProviderID: string
      NodeRoles: string[]
      Capacity: Map<string, int64>
      Allocatable: Map<string, int64>
      /// JSON: address type → address.
      NodeAddresses: string
      KubeletVersion: string
      KubeProxyVersion: string
      OperatingSystem: string
      Architecture: string
      KernelVersion: string
      OSImage: string
      ContainerRuntimeVersion: string
      // Service.
      ServiceType: string
      ClusterIP: string
      /// JSON.
      ServicePorts: string
      // RBAC, all three JSON. The rules are the permissions actually granted.
      RBACRules: string
      RBACSubjects: string
      RBACRoleRef: string
      /// The frame's message as JSON without its object list: the per-kind
      /// extras no column holds. The same on every row of a pass.
      Envelope: string }

/// One object's manifest, its YAML or JSON exactly as the cluster agent
/// collected it.
type K8sManifestRow =
    { TenantID: string
      CollectedAt: DateTime
      ClusterID: string
      ClusterName: string
      UID: string
      Kind: string
      APIVersion: string
      ResourceVersion: string
      Content: byte[]
      ContentType: string
      IsTerminated: uint8
      OrgID: int32
      SubscriptionID: uint8
      HeaderTimestamp: int64 option
      Encoding: string
      GroupID: int32
      GroupSize: int32
      HostName: string
      AgentVersion: string
      OriginCollector: string
      /// Every tag that applies, from all three places the wire attaches
      /// them, merged. TagSources keeps who attached what.
      Tags: Map<string, string[]>
      TagSources: Map<string, string[]>
      NodeName: string
      Type: int32
      Version: string
      ExtraAttributes: Map<string, string>
      /// 1 when Content is valid UTF-8, so a reader knows before parsing.
      ContentIsUTF8: uint8
      /// The frame's message as JSON, without its manifest list.
      Envelope: string
      /// The machine this one object was seen on, as JSON.
      ManifestHost: string }

/// The cluster as the agent sums it up: one row per cluster per pass.
type K8sClusterRow =
    { TenantID: string
      CollectedAt: DateTime
      ClusterID: string
      ClusterName: string
      NodeCount: uint32
      PodCapacity: uint32
      PodAllocatable: uint32
      CPUCapacity: uint64
      CPUAllocatable: uint64
      MemoryCapacity: uint64
      MemoryAllocatable: uint64
      /// Version → nodes running it.
      KubeletVersions: Map<string, uint32>
      APIServerVersions: Map<string, uint32>
      OrgID: int32
      SubscriptionID: uint8
      HeaderTimestamp: int64 option
      Encoding: string
      GroupID: int32
      GroupSize: int32
      AgentVersion: string
      /// The frame's tags; ClusterTags are the cluster object's own.
      Tags: Map<string, string[]>
      ClusterTags: Map<string, string[]>
      ResourceVersion: string
      CreationTimestamp: DateTime option
      Metrics: Map<string, float>
      /// GPUs and whatever else a node advertises beyond cpu, memory, pods.
      ExtendedResourcesCapacity: Map<string, int64>
      ExtendedResourcesAllocatable: Map<string, int64>
      // The per-node breakdown behind NodeCount, parallel and in wire order.
      NodesName: string[]
      NodesRegion: string[]
      NodesInstanceType: string[]
      NodesOS: string[]
      NodesOSImage: string[]
      NodesArchitecture: string[]
      NodesKernelVersion: string[]
      NodesContainerRuntimeVersion: string[]
      NodesKubeletVersion: string[]
      /// Kubernetes quantity strings ("16Gi", "4"), kept as sent.
      NodesAllocatable: Map<string, string>[]
      NodesCapacity: Map<string, string>[] }

/// What the cluster agent did with a remote action: the audit record.
type K8sActionRow =
    { TenantID: string
      Timestamp: DateTime
      ActionID: string
      OrgID: int64
      EventType: string
      Status: string
      ActionType: string
      ClusterID: string
      ClusterName: string
      ResourceID: string
      ResourceKind: string
      ResourceName: string
      ResourceNamespace: string
      RequestedBy: string
      Message: string
      /// The names of the keys the event carried beyond the known ones,
      /// sorted; Extra has them with their values, as a JSON object.
      ExtraKeys: string[]
      /// What the executor attached, name → bytes.
      Payloads: Map<string, byte[]>
      Extra: string
      /// The timestamp exactly as it arrived. Timestamp falls back to the
      /// arrival time when this is empty or does not parse.
      TimestampRaw: string }

module K8sResources =
    let table: Table<K8sResourceRow> =
        { Table.create
              "storage_k8s_resources"
              "k8s_resources"
              [ "tenant_id"; "collected_at"; "cluster_id"; "cluster_name"; "kind"; "namespace"; "name"
                "uid"; "resource_version"; "owner_kind"; "owner_name"; "node_name"; "phase"; "status"
                "ready"; "desired"; "available"; "counts"; "labels"; "annotations"; "tags"
                "agent_version"; "group_id"; "group_size"
                "org_id"; "subscription_id"; "header_timestamp"; "encoding"
                "creation_timestamp"; "deletion_timestamp"; "deletion_grace_period_seconds"
                "finalizers"; "owner_kinds"; "owner_names"; "owner_uids"
                "condition_types"; "condition_statuses"; "condition_reasons"; "condition_messages"
                "condition_last_transition"; "condition_last_update"; "condition_last_probe"
                "condition_message"; "resource_requirements"; "metrics"; "object"
                "pod_ip"; "nominated_node_name"; "qos_class"; "priority_class"
                "start_time"; "scheduled_time"; "host_name"
                "container_names"; "container_ids"; "container_ready"; "container_restarts"
                "container_states"; "container_messages"; "container_images"
                "container_image_ids"; "container_is_init"
                "pod_cidr"; "pod_cidrs"; "unschedulable"; "taints"; "provider_id"; "node_roles"
                "capacity"; "allocatable"; "node_addresses"
                "kubelet_version"; "kube_proxy_version"; "operating_system"; "architecture"
                "kernel_version"; "os_image"; "container_runtime_version"
                "service_type"; "cluster_ip"; "service_ports"
                "rbac_rules"; "rbac_subjects"; "rbac_role_ref"; "envelope" ]
              (fun (r: K8sResourceRow) ->
                  [| r.TenantID; r.CollectedAt; r.ClusterID; r.ClusterName; r.Kind; r.Namespace; r.Name
                     r.UID; r.ResourceVersion; r.OwnerKind; r.OwnerName; r.NodeName; r.Phase; r.Status
                     r.Ready; r.Desired; r.Available; r.Counts; r.Labels; r.Annotations; r.Tags
                     r.AgentVersion; r.GroupID; r.GroupSize
                     r.OrgID; r.SubscriptionID; Col.opt r.HeaderTimestamp; r.Encoding
                     Col.opt r.CreationTimestamp; Col.opt r.DeletionTimestamp; Col.opt r.DeletionGracePeriodSeconds
                     r.Finalizers; r.OwnerKinds; r.OwnerNames; r.OwnerUIDs
                     r.ConditionTypes; r.ConditionStatuses; r.ConditionReasons; r.ConditionMessages
                     Array.map Option.toNullable r.ConditionLastTransition
                     Array.map Option.toNullable r.ConditionLastUpdate
                     Array.map Option.toNullable r.ConditionLastProbe
                     r.ConditionMessage; r.ResourceRequirements; r.Metrics; r.Object
                     r.PodIP; r.NominatedNodeName; r.QOSClass; r.PriorityClass
                     Col.opt r.StartTime; Col.opt r.ScheduledTime; r.HostName
                     r.ContainerNames; r.ContainerIDs; r.ContainerReady; r.ContainerRestarts
                     r.ContainerStates; r.ContainerMessages; r.ContainerImages
                     r.ContainerImageIDs; r.ContainerIsInit
                     r.PodCIDR; r.PodCIDRs; r.Unschedulable; r.Taints; r.ProviderID; r.NodeRoles
                     r.Capacity; r.Allocatable; r.NodeAddresses
                     r.KubeletVersion; r.KubeProxyVersion; r.OperatingSystem; r.Architecture
                     r.KernelVersion; r.OSImage; r.ContainerRuntimeVersion
                     r.ServiceType; r.ClusterIP; r.ServicePorts
                     r.RBACRules; r.RBACSubjects; r.RBACRoleRef; r.Envelope |])
          with
              // The cluster agent sends a whole collection pass at once:
              // thousands of objects, then silence. Rows carry the object's
              // JSON, so the buffer ceiling is real memory.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

    /// A row with nothing in it: what a kind does not have stays empty.
    let empty: K8sResourceRow =
        { TenantID = ""
          CollectedAt = DateTime.UnixEpoch
          ClusterID = ""
          ClusterName = ""
          Kind = ""
          Namespace = ""
          Name = ""
          UID = ""
          ResourceVersion = ""
          OwnerKind = ""
          OwnerName = ""
          NodeName = ""
          Phase = ""
          Status = ""
          Ready = 0
          Desired = 0
          Available = 0
          Counts = Map.empty
          Labels = Map.empty
          Annotations = Map.empty
          Tags = Map.empty
          AgentVersion = ""
          GroupID = 0
          GroupSize = 0
          OrgID = 0
          SubscriptionID = 0uy
          HeaderTimestamp = None
          Encoding = ""
          CreationTimestamp = None
          DeletionTimestamp = None
          DeletionGracePeriodSeconds = None
          Finalizers = [||]
          OwnerKinds = [||]
          OwnerNames = [||]
          OwnerUIDs = [||]
          ConditionTypes = [||]
          ConditionStatuses = [||]
          ConditionReasons = [||]
          ConditionMessages = [||]
          ConditionLastTransition = [||]
          ConditionLastUpdate = [||]
          ConditionLastProbe = [||]
          ConditionMessage = ""
          ResourceRequirements = ""
          Metrics = Map.empty
          Object = ""
          PodIP = ""
          NominatedNodeName = ""
          QOSClass = ""
          PriorityClass = ""
          StartTime = None
          ScheduledTime = None
          HostName = ""
          ContainerNames = [||]
          ContainerIDs = [||]
          ContainerReady = [||]
          ContainerRestarts = [||]
          ContainerStates = [||]
          ContainerMessages = [||]
          ContainerImages = [||]
          ContainerImageIDs = [||]
          ContainerIsInit = [||]
          PodCIDR = ""
          PodCIDRs = [||]
          Unschedulable = 0uy
          Taints = ""
          ProviderID = ""
          NodeRoles = [||]
          Capacity = Map.empty
          Allocatable = Map.empty
          NodeAddresses = ""
          KubeletVersion = ""
          KubeProxyVersion = ""
          OperatingSystem = ""
          Architecture = ""
          KernelVersion = ""
          OSImage = ""
          ContainerRuntimeVersion = ""
          ServiceType = ""
          ClusterIP = ""
          ServicePorts = ""
          RBACRules = ""
          RBACSubjects = ""
          RBACRoleRef = ""
          Envelope = "" }

module K8sManifests =
    let table: Table<K8sManifestRow> =
        { Table.create
              "storage_k8s_manifests"
              "k8s_manifests"
              [ "tenant_id"; "collected_at"; "cluster_id"; "cluster_name"; "uid"; "kind"; "api_version"
                "resource_version"; "content"; "content_type"; "is_terminated"
                "org_id"; "subscription_id"; "header_timestamp"; "encoding"
                "group_id"; "group_size"; "host_name"; "agent_version"; "origin_collector"
                "tags"; "tag_sources"; "node_name"; "type"; "version"; "extra_attributes"
                "content_is_utf8"; "envelope"; "manifest_host" ]
              (fun (r: K8sManifestRow) ->
                  [| r.TenantID; r.CollectedAt; r.ClusterID; r.ClusterName; r.UID; r.Kind; r.APIVersion
                     r.ResourceVersion; r.Content; r.ContentType; r.IsTerminated
                     r.OrgID; r.SubscriptionID; Col.opt r.HeaderTimestamp; r.Encoding
                     r.GroupID; r.GroupSize; r.HostName; r.AgentVersion; r.OriginCollector
                     r.Tags; r.TagSources; r.NodeName; r.Type; r.Version; r.ExtraAttributes
                     r.ContentIsUTF8; r.Envelope; r.ManifestHost |])
          with
              // Every row is a whole document, so these counts stand for
              // megabytes.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

module K8sCluster =
    let table: Table<K8sClusterRow> =
        { Table.create
              "storage_k8s_cluster"
              "k8s_cluster"
              [ "tenant_id"; "collected_at"; "cluster_id"; "cluster_name"; "node_count"
                "pod_capacity"; "pod_allocatable"; "cpu_capacity"; "cpu_allocatable"
                "memory_capacity"; "memory_allocatable"; "kubelet_versions"; "apiserver_versions"
                "org_id"; "subscription_id"; "header_timestamp"; "encoding"
                "group_id"; "group_size"; "agent_version"; "tags"; "cluster_tags"
                "resource_version"; "creation_timestamp"; "metrics"
                "extended_resources_capacity"; "extended_resources_allocatable"
                "nodes_name"; "nodes_region"; "nodes_instance_type"; "nodes_os"; "nodes_os_image"
                "nodes_architecture"; "nodes_kernel_version"; "nodes_container_runtime_version"
                "nodes_kubelet_version"; "nodes_allocatable"; "nodes_capacity" ]
              (fun (r: K8sClusterRow) ->
                  [| r.TenantID; r.CollectedAt; r.ClusterID; r.ClusterName; r.NodeCount
                     r.PodCapacity; r.PodAllocatable; r.CPUCapacity; r.CPUAllocatable
                     r.MemoryCapacity; r.MemoryAllocatable; r.KubeletVersions; r.APIServerVersions
                     r.OrgID; r.SubscriptionID; Col.opt r.HeaderTimestamp; r.Encoding
                     r.GroupID; r.GroupSize; r.AgentVersion; r.Tags; r.ClusterTags
                     r.ResourceVersion; Col.opt r.CreationTimestamp; r.Metrics
                     r.ExtendedResourcesCapacity; r.ExtendedResourcesAllocatable
                     r.NodesName; r.NodesRegion; r.NodesInstanceType; r.NodesOS; r.NodesOSImage
                     r.NodesArchitecture; r.NodesKernelVersion; r.NodesContainerRuntimeVersion
                     r.NodesKubeletVersion; r.NodesAllocatable; r.NodesCapacity |])
          with
              MaxRows = 50
              FlushInterval = TimeSpan.FromSeconds 15.0
              BufferLimit = 1_000
              MaxInFlight = 1 }

module K8sActions =
    let table: Table<K8sActionRow> =
        { Table.create
              "storage_k8s_actions"
              "k8s_actions"
              [ "tenant_id"; "timestamp"; "action_id"; "org_id"; "event_type"; "status"; "action_type"
                "cluster_id"; "cluster_name"; "resource_id"; "resource_kind"; "resource_name"
                "resource_namespace"; "requested_by"; "message"; "extra_keys"
                "payloads"; "extra"; "timestamp_raw" ]
              (fun (r: K8sActionRow) ->
                  [| r.TenantID; r.Timestamp; r.ActionID; r.OrgID; r.EventType; r.Status; r.ActionType
                     r.ClusterID; r.ClusterName; r.ResourceID; r.ResourceKind; r.ResourceName
                     r.ResourceNamespace; r.RequestedBy; r.Message; r.ExtraKeys
                     r.Payloads; r.Extra; r.TimestampRaw |])
          with
              // Human scale: an operator triggers an action and waits to see
              // its result.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 10_000
              MaxInFlight = 1 }
