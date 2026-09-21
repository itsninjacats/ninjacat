package intake

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"reflect"
	"sort"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// kubeops-intake.<site> — Kubernetes.
//
//	config: orchestrator_explorer.orchestrator_dd_url
//	        / DD_ORCHESTRATOR_EXPLORER_ORCHESTRATOR_DD_URL
//
// Sent by the CLUSTER agent, not the node agent — these are cluster-scoped
// resources. /api/v1/orchestrator is the same thing under
// orchestrator_explorer.use_legacy_endpoint.
//
// orch and orchmanif carry the SAME 16-byte frame as /api/v1/collector, so
// process.DecodeMessage handles them; only the message types differ, 41..88
// instead of 12. kubeactions is different: event platform, JSON.
//
// Stored: object collections in k8s_resources, raw manifests in k8s_manifests,
// the cluster summary in k8s_cluster, action results in k8s_actions. ECS tasks
// are decoded but not stored — no Kubernetes identity to store them under.
func (a *Server) routeKubeops(g *gin.RouterGroup) {
	g.POST("/api/v2/orch", a.HandleOrchestrator)
	g.POST("/api/v2/orchmanif", a.HandleOrchestratorManifests)
	g.POST("/api/v1/orchestrator", a.HandleOrchestrator) // legacy
	g.POST("/api/v2/kubeactions", a.HandleKubeActions)
}

// HandleOrchestrator accepts Kubernetes resource collections.
//
// msg.Body is the complete decoded payload — a *process.CollectorPod carries
// every pod's YAML, container statuses, labels, owner references and resource
// requirements. The log shows the collection and each object's identity plus
// what its kind is about; NINJACAT_DUMP_K8S=true adds protobuf's rendering of
// the whole thing.
func (a *Server) HandleOrchestrator(c *gin.Context) {
	a.handleOrchestratorFrame(c, "orch")
}

// HandleOrchestratorManifests accepts raw Kubernetes manifests (types 80-82).
//
// Unlike the collectors, a manifest carries the object's own YAML or JSON in
// Content — the "exactly what kubectl would show" channel. Same frame, same
// decoder; only the expected types differ, and the switch covers both.
func (a *Server) HandleOrchestratorManifests(c *gin.Context) {
	a.handleOrchestratorFrame(c, "orchmanif")
}

func (a *Server) handleOrchestratorFrame(c *gin.Context, label string) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	msg, err := process.DecodeMessage(body)
	if err != nil {
		log.Printf("[%s] process frame: %v (%d bytes)", label, err, len(body))
		return
	}

	log.Printf("[%s] type=%d %T host=%s %d B",
		label, msg.Header.Type, msg.Body, c.GetHeader("X-Dd-Hostname"), len(body))
	orchLogBody(label, msg.Body)

	if os.Getenv("NINJACAT_DUMP_K8S") == "true" {
		// The whole object, protobuf's own rendering. Verbose on purpose:
		// this is how you find out what a real cluster actually sends.
		log.Printf("   %s", msg.Body)
	}

	// The frame header, as decoded. Type is the message type (the switch below
	// resolves it to a concrete Go type); Version and Encoding say how the body
	// was framed. Timestamp is only present in the V3 header, and the agent's
	// orchestrator sender leaves it unset, so 0 means "not provided" — never
	// 1970-01-01. OrgID and SubscriptionID are unused by the agent.
	header := msg.Header
	_ = header // TODO(ninjacat): tables. Complete, unconverted, ready to take.

	// The decoded body, one typed variant per case. Everything the logging
	// above read came from these same pointers; nothing was copied, flattened
	// or trimmed on the way here. Each `payload` is the *process.Collector*
	// exactly as process.DecodeMessage produced it — the full envelope
	// (ClusterName, ClusterId, GroupId, GroupSize, Tags, AgentVersion) and the
	// full object list, every object with its Metadata, Spec, Status, Yaml,
	// Conditions and Tags. Read nested pointers through the generated getters
	// (payload.GetPods(), pod.GetMetadata(), ...): they are nil-safe.
	//
	// The message type numbers are process.TypeCollector*; the field named in
	// each comment is the object list of that message.
	switch payload := msg.Body.(type) {
	// The object-list collectors all take the same road: one K8sResourceRow
	// per object. They differ only in the element type — the envelope and the
	// per-object Metadata are identical across all of them — so one
	// reflection-based converter serves the lot; see orchResourceRows. The
	// rows are a projection: identity, ownership, labels and the numbers
	// worth charting. Spec, Conditions, resource requirements and metrics
	// have no column and stay only in payload.
	case *process.CollectorPod, // 41 — Pods []*process.Pod
		*process.CollectorReplicaSet,              // 42 — ReplicaSets []*process.ReplicaSet
		*process.CollectorDeployment,              // 43 — Deployments []*process.Deployment
		*process.CollectorService,                 // 44 — Services []*process.Service
		*process.CollectorNode,                    // 45 — Nodes []*process.Node
		*process.CollectorJob,                     // 47 — Jobs []*process.Job
		*process.CollectorCronJob,                 // 48 — CronJobs []*process.CronJob
		*process.CollectorDaemonSet,               // 49 — DaemonSets []*process.DaemonSet
		*process.CollectorStatefulSet,             // 50 — StatefulSets []*process.StatefulSet
		*process.CollectorPersistentVolume,        // 51 — PersistentVolumes []*process.PersistentVolume
		*process.CollectorPersistentVolumeClaim,   // 52 — PersistentVolumeClaims []*process.PersistentVolumeClaim
		*process.CollectorRole,                    // 54 — Roles []*process.Role
		*process.CollectorRoleBinding,             // 55 — RoleBindings []*process.RoleBinding
		*process.CollectorClusterRole,             // 56 — ClusterRoles []*process.ClusterRole
		*process.CollectorClusterRoleBinding,      // 57 — ClusterRoleBindings []*process.ClusterRoleBinding
		*process.CollectorServiceAccount,          // 58 — ServiceAccounts []*process.ServiceAccount
		*process.CollectorIngress,                 // 59 — Ingresses []*process.Ingress
		*process.CollectorNamespace,               // 61 — Namespaces []*process.Namespace
		*process.CollectorVerticalPodAutoscaler,   // 83 — VerticalPodAutoscalers []*process.VerticalPodAutoscaler
		*process.CollectorHorizontalPodAutoscaler, // 84 — HorizontalPodAutoscalers []*process.HorizontalPodAutoscaler
		*process.CollectorNetworkPolicy,           // 85 — NetworkPolicies []*process.NetworkPolicy
		*process.CollectorLimitRange,              // 86 — LimitRanges []*process.LimitRange
		*process.CollectorStorageClass,            // 87 — StorageClasses []*process.StorageClass
		*process.CollectorPodDisruptionBudget:     // 88 — PodDisruptionBudgets []*process.PodDisruptionBudget
		a.storeOrchResources(c, payload)

	case *process.CollectorCluster: // 46 — Cluster *process.Cluster, one per frame
		a.storeOrchCluster(c, payload)

	// Manifests carry the object's own YAML/JSON in Content. The CRD and CR
	// wrappers hold an inner *process.CollectorManifest that MAY BE NIL —
	// GetManifest absorbs that, and the converter treats nil as empty.
	case *process.CollectorManifest: // 80 — Manifests []*process.Manifest
		a.storeOrchManifests(c, payload)
	case *process.CollectorManifestCRD: // 81 — Manifest *process.CollectorManifest (may be nil), CRDs
		a.storeOrchManifests(c, payload.GetManifest())
	case *process.CollectorManifestCR: // 82 — Manifest *process.CollectorManifest (may be nil), custom resources
		a.storeOrchManifests(c, payload.GetManifest())

	case *process.CollectorECSTask: // 200 — Tasks []*process.ECSTask; no Metadata, has AwsAccountID (int64), Region
		// ECS, not Kubernetes: a task has no Metadata, so nothing here maps
		// onto k8s_resources' identity columns (namespace/name/uid). Stays
		// unstored until ECS earns a table of its own.
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.

	default:
		// Every other type process.DecodeMessage can produce belongs to the
		// process agent's /api/v1/collector family and has its own router:
		// CollectorProc, CollectorRealTime, CollectorContainer,
		// CollectorContainerRealTime, CollectorProcDiscovery,
		// CollectorConnections, CollectorProcEvent, ResCollector. Landing here
		// means a misconfigured agent, and the frame is still whole.
		log.Printf("[%s] %T is not an orchestrator message; kept as is", label, payload)
	}
}

// orchShow caps the per-object lines of one collection.
const orchShow = 20

// orchLogBody reports one collection: a header line with the cluster and the
// group this frame belongs to, then one line per object. Read-only: every
// value goes through the generated nil-safe getters, so a nil element or a
// missing Spec/Status prints as zero instead of crashing the server.
func orchLogBody(label string, body process.MessageBody) {
	switch m := body.(type) {
	case *process.CollectorPod:
		orchHeader(label, "pods", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetPods()))
		orchEach(len(m.GetPods()), func(i int) string { return orchPod(m.GetPods()[i]) })
	case *process.CollectorNode:
		orchHeader(label, "nodes", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetNodes()))
		orchEach(len(m.GetNodes()), func(i int) string { return orchNode(m.GetNodes()[i]) })
	case *process.CollectorDeployment:
		orchHeader(label, "deployments", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetDeployments()))
		orchEach(len(m.GetDeployments()), func(i int) string {
			d := m.GetDeployments()[i]
			return fmt.Sprintf("%s ready=%d/%d updated=%d available=%d unavailable=%d strategy=%s paused=%v",
				orchIdent(d.GetMetadata()), d.GetReadyReplicas(), d.GetReplicasDesired(), d.GetUpdatedReplicas(),
				d.GetAvailableReplicas(), d.GetUnavailableReplicas(), d.GetDeploymentStrategy(), d.GetPaused())
		})
	case *process.CollectorReplicaSet:
		orchHeader(label, "replicasets", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetReplicaSets()))
		orchEach(len(m.GetReplicaSets()), func(i int) string {
			r := m.GetReplicaSets()[i]
			return fmt.Sprintf("%s ready=%d/%d available=%d owner=%s",
				orchIdent(r.GetMetadata()), r.GetReadyReplicas(), r.GetReplicasDesired(), r.GetAvailableReplicas(), orchOwner(r.GetMetadata()))
		})
	case *process.CollectorDaemonSet:
		orchHeader(label, "daemonsets", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetDaemonSets()))
		orchEach(len(m.GetDaemonSets()), func(i int) string {
			d := m.GetDaemonSets()[i]
			st := d.GetStatus()
			return fmt.Sprintf("%s ready=%d/%d scheduled=%d updated=%d available=%d misscheduled=%d",
				orchIdent(d.GetMetadata()), st.GetNumberReady(), st.GetDesiredNumberScheduled(), st.GetCurrentNumberScheduled(),
				st.GetUpdatedNumberScheduled(), st.GetNumberAvailable(), st.GetNumberMisscheduled())
		})
	case *process.CollectorStatefulSet:
		orchHeader(label, "statefulsets", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetStatefulSets()))
		orchEach(len(m.GetStatefulSets()), func(i int) string {
			s := m.GetStatefulSets()[i]
			st, sp := s.GetStatus(), s.GetSpec()
			return fmt.Sprintf("%s ready=%d/%d current=%d updated=%d service=%s",
				orchIdent(s.GetMetadata()), st.GetReadyReplicas(), sp.GetDesiredReplicas(), st.GetCurrentReplicas(),
				st.GetUpdatedReplicas(), sp.GetServiceName())
		})
	case *process.CollectorService:
		orchHeader(label, "services", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetServices()))
		orchEach(len(m.GetServices()), func(i int) string { return orchService(m.GetServices()[i]) })
	case *process.CollectorNamespace:
		orchHeader(label, "namespaces", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetNamespaces()))
		orchEach(len(m.GetNamespaces()), func(i int) string {
			n := m.GetNamespaces()[i]
			return fmt.Sprintf("%s status=%s", orchIdent(n.GetMetadata()), n.GetStatus())
		})
	case *process.CollectorCluster:
		// One object per frame: the cluster itself, as the agent sums it up.
		orchHeader(label, "cluster", m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), 1)
		if cl := m.GetCluster(); cl != nil {
			log.Printf("   nodes=%d pods=%d/%d cpu=%d/%d memory=%d/%d (allocatable/capacity) kubelets=%v apiservers=%v",
				cl.GetNodeCount(), cl.GetPodAllocatable(), cl.GetPodCapacity(), cl.GetCpuAllocatable(), cl.GetCpuCapacity(),
				cl.GetMemoryAllocatable(), cl.GetMemoryCapacity(), cl.GetKubeletVersions(), cl.GetApiServerVersions())
		}
	case *process.CollectorManifest:
		orchManifests(label, "manifests", m)
	case *process.CollectorManifestCRD:
		orchManifests(label, "crd manifests", m.GetManifest())
	case *process.CollectorManifestCR:
		orchManifests(label, "cr manifests", m.GetManifest())
	default:
		orchFallback(label, body)
	}
}

func orchHeader(label, kind, cluster, clusterID string, group, groupSize int32, n int) {
	log.Printf("[%s] %s cluster=%q (%s) group %d/%d — %d objects",
		label, kind, cluster, clusterID, group, groupSize, n)
}

// orchEach logs one line per object up to orchShow, then how many were left.
func orchEach(n int, line func(i int) string) {
	for i := 0; i < n; i++ {
		if i == orchShow {
			log.Printf("   ... and %d more", n-orchShow)
			return
		}
		log.Printf("   %s", line(i))
	}
}

// orchIdent is namespace/name plus uid, the identity every object carries.
func orchIdent(md *process.Metadata) string {
	if md == nil {
		return "(no metadata)"
	}
	if md.GetNamespace() == "" {
		return fmt.Sprintf("%s uid=%s", md.GetName(), md.GetUid())
	}
	return fmt.Sprintf("%s/%s uid=%s", md.GetNamespace(), md.GetName(), md.GetUid())
}

// orchOwner is the first owner reference as Kind/Name, or "-".
func orchOwner(md *process.Metadata) string {
	refs := md.GetOwnerReferences()
	if len(refs) == 0 {
		return "-"
	}
	return refs[0].GetKind() + "/" + refs[0].GetName()
}

func orchPod(p *process.Pod) string {
	ready, restarts := 0, int32(0)
	for _, cs := range p.GetContainerStatuses() {
		if cs.GetReady() {
			ready++
		}
		restarts += cs.GetRestartCount()
	}
	return fmt.Sprintf("%s phase=%s status=%s node=%s ip=%s containers=%d/%d ready restarts=%d qos=%s owner=%s",
		orchIdent(p.GetMetadata()), p.GetPhase(), p.GetStatus(), p.GetNodeName(), p.GetIP(), ready, len(p.GetContainerStatuses()),
		restarts, p.GetQOSClass(), orchOwner(p.GetMetadata()))
}

func orchNode(n *process.Node) string {
	st := n.GetStatus()
	capacity := st.GetCapacity() // map[string]int64; reading a nil map is fine
	return fmt.Sprintf("%s roles=%s status=%s kubelet=%s runtime=%s os=%s/%s unschedulable=%v taints=%d capacity cpu=%d memory=%d pods=%d",
		orchIdent(n.GetMetadata()), rcJoin(n.GetRoles()), st.GetStatus(), st.GetKubeletVersion(), st.GetContainerRuntimeVersion(),
		st.GetOperatingSystem(), st.GetArchitecture(), n.GetUnschedulable(), len(n.GetTaints()),
		capacity["cpu"], capacity["memory"], capacity["pods"])
}

func orchService(s *process.Service) string {
	sp := s.GetSpec()
	ports := make([]string, 0, len(sp.GetPorts()))
	for _, p := range sp.GetPorts() {
		ports = append(ports, fmt.Sprintf("%d/%s->%s", p.GetPort(), p.GetProtocol(), p.GetTargetPort()))
	}
	return fmt.Sprintf("%s type=%s clusterIP=%s ports=%s",
		orchIdent(s.GetMetadata()), sp.GetType(), sp.GetClusterIP(), rcJoin(ports))
}

func orchManifests(label, kind string, m *process.CollectorManifest) {
	if m == nil {
		log.Printf("[%s] %s: empty envelope", label, kind)
		return
	}
	orchHeader(label, kind, m.GetClusterName(), m.GetClusterId(), m.GetGroupId(), m.GetGroupSize(), len(m.GetManifests()))
	orchEach(len(m.GetManifests()), func(i int) string {
		man := m.GetManifests()[i]
		return fmt.Sprintf("%-18s %-12s uid=%s rv=%s %d B of %s terminated=%v",
			man.GetKind(), man.GetApiVersion(), man.GetUid(), man.GetResourceVersion(), len(man.GetContent()),
			man.GetContentType(), man.GetIsTerminated())
	})
}

// orchFallback covers the collector types without a case of their own. Every
// Collector* message shares one envelope — ClusterName, ClusterId, GroupId,
// GroupSize and a single repeated field holding the objects, each with a
// Metadata — so reflection reads the envelope, the count and each object's
// identity without knowing the kind. Objects without a Metadata field (ECS
// tasks, and anything from the process family) are reported by type name.
func orchFallback(label string, body process.MessageBody) {
	v := reflect.Indirect(reflect.ValueOf(body))
	if v.Kind() != reflect.Struct {
		log.Printf("[%s] %T: not a collector message", label, body)
		return
	}
	str := func(name string) string {
		f := v.FieldByName(name)
		if f.IsValid() && f.Kind() == reflect.String {
			return f.String()
		}
		return ""
	}
	num := func(name string) int32 {
		f := v.FieldByName(name)
		if f.IsValid() && f.Kind() == reflect.Int32 {
			return int32(f.Int())
		}
		return 0
	}

	var items reflect.Value
	kind := fmt.Sprintf("%T", body)
	for i := 0; i < v.NumField(); i++ {
		f := v.Field(i)
		if f.Kind() == reflect.Slice && f.Type().Elem().Kind() == reflect.Ptr && f.Type().Elem().Elem().Kind() == reflect.Struct {
			items, kind = f, strings.ToLower(v.Type().Field(i).Name)
			break
		}
	}
	if !items.IsValid() {
		log.Printf("[%s] %s cluster=%q (%s): no object list in this message",
			label, kind, str("ClusterName"), str("ClusterId"))
		return
	}

	orchHeader(label, kind, str("ClusterName"), str("ClusterId"), num("GroupId"), num("GroupSize"), items.Len())
	orchEach(items.Len(), func(i int) string {
		e := items.Index(i)
		if e.IsNil() {
			return "(nil)"
		}
		mdField := e.Elem().FieldByName("Metadata")
		if !mdField.IsValid() || !mdField.CanInterface() {
			return fmt.Sprintf("%T (no Metadata field)", e.Interface())
		}
		md, _ := mdField.Interface().(*process.Metadata)
		return orchIdent(md)
	})
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

// storeOrchResources turns one Collector* object list into rows for
// ninjacat.k8s_resources. Fire-and-forget: the 202 was already deferred, and
// a missing tenant (a request that skipped the API key middleware) stores
// nothing rather than writing rows nobody can query.
func (a *Server) storeOrchResources(c *gin.Context, body process.MessageBody) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	// The orchestrator sender leaves the frame timestamp unset (see the header
	// comment above), so arrival time is the collection time we have.
	rows := orchResourceRows(tenant, time.Now().UTC(), body)
	a.store(storage.K8sResourcesWriter, storage.WriteK8sResources{Resources: rows}, len(rows))
}

// orchResourceRows converts a Collector* message into one row per object.
//
// Reflection, for the same reason orchFallback uses it: every Collector*
// message shares one envelope (ClusterName, ClusterId, GroupId, GroupSize,
// Tags, AgentVersion) and a single repeated field of objects, each carrying a
// *process.Metadata — so one converter reads the shared shape instead of 27
// near-identical branches. The Kubernetes kind IS the element's Go type name
// (*process.Pod -> "Pod"): agent-payload names its messages after the objects
// they mirror, which is exactly the string the kind column wants.
//
// Only the numbers differ per kind, and those go through a typed switch in
// orchResourceNumbers. An element that is nil, or has no Metadata, is skipped:
// without namespace/name/uid the row has no identity and could never be
// queried or joined — dropping it loses less than storing it as noise.
func orchResourceRows(tenant string, now time.Time, body process.MessageBody) []storage.K8sResourceRow {
	v := reflect.Indirect(reflect.ValueOf(body))
	if v.Kind() != reflect.Struct {
		return nil
	}
	str := func(name string) string {
		f := v.FieldByName(name)
		if f.IsValid() && f.Kind() == reflect.String {
			return f.String()
		}
		return ""
	}
	num := func(name string) int32 {
		f := v.FieldByName(name)
		if f.IsValid() && f.Kind() == reflect.Int32 {
			return int32(f.Int())
		}
		return 0
	}

	var items reflect.Value
	for i := 0; i < v.NumField(); i++ {
		f := v.Field(i)
		if f.Kind() == reflect.Slice && f.Type().Elem().Kind() == reflect.Ptr && f.Type().Elem().Elem().Kind() == reflect.Struct {
			items = f
			break
		}
	}
	if !items.IsValid() {
		return nil
	}
	kind := items.Type().Elem().Elem().Name()

	// The envelope is shared by every row of the frame; read it once.
	clusterName, clusterID := str("ClusterName"), str("ClusterId")
	groupID, groupSize := num("GroupId"), num("GroupSize")
	envTags := orchStringSlice(v, "Tags")
	var agentVersion string
	if f := v.FieldByName("AgentVersion"); f.IsValid() && f.CanInterface() {
		av, _ := f.Interface().(*process.AgentVersion)
		agentVersion = agentVersionString(av)
	}

	rows := make([]storage.K8sResourceRow, 0, items.Len())
	for i := 0; i < items.Len(); i++ {
		e := items.Index(i)
		if e.IsNil() {
			continue
		}
		mdField := e.Elem().FieldByName("Metadata")
		if !mdField.IsValid() || !mdField.CanInterface() {
			continue // no Metadata field at all: not a Kubernetes object
		}
		md, _ := mdField.Interface().(*process.Metadata)
		if md == nil {
			continue // Metadata omitted on the wire: no identity, no row
		}

		row := storage.K8sResourceRow{
			TenantID:        tenant,
			CollectedAt:     now,
			ClusterID:       clusterID,
			ClusterName:     clusterName,
			Kind:            kind,
			Namespace:       md.GetNamespace(),
			Name:            md.GetName(),
			UID:             md.GetUid(),
			ResourceVersion: md.GetResourceVersion(),
			// Labels and annotations travel as "key:value" strings, the same
			// flat form as Datadog tags, so the same splitter applies — but
			// they stay single-valued maps: the Kubernetes API forbids
			// duplicate keys, unlike tags.
			Labels:       kvToMap(md.GetLabels()),
			Annotations:  kvToMap(md.GetAnnotations()),
			AgentVersion: agentVersion,
			GroupID:      groupID,
			GroupSize:    groupSize,
		}
		// The first owner reference, like orchOwner in the log: Kubernetes
		// allows several but in practice writes exactly one controller.
		if refs := md.GetOwnerReferences(); len(refs) > 0 {
			row.OwnerKind, row.OwnerName = refs[0].GetKind(), refs[0].GetName()
		}
		// The frame's tags apply to every object, the object's own on top.
		objTags := orchStringSlice(e.Elem(), "Tags")
		if n := len(envTags) + len(objTags); n > 0 {
			tags := make([]string, 0, n)
			tags = append(tags, envTags...)
			tags = append(tags, objTags...)
			row.Tags = tagsToMultiMap(tags)
		}
		orchResourceNumbers(e.Interface(), &row)
		rows = append(rows, row)
	}
	return rows
}

// orchResourceNumbers fills the per-kind numbers — the one part of the row
// reflection cannot see, because every kind names its counters differently.
// The arithmetic mirrors the orchPod/orchLogBody lines above: what the log
// prints for a kind is exactly what its Counts map stores.
//
// The dedicated Ready/Desired/Available columns carry the same values where
// the kind has them, so the common "how healthy" query needs no map lookup.
// Kinds without meaningful numbers (Service, Role, Ingress, ...) leave
// Counts empty — expected, not a gap.
func orchResourceNumbers(obj any, row *storage.K8sResourceRow) {
	switch o := obj.(type) {
	case *process.Pod:
		// Same sums as orchPod: ready and restarts come from the container
		// statuses, not from a field of their own.
		ready, restarts := 0, int32(0)
		for _, cs := range o.GetContainerStatuses() {
			if cs.GetReady() {
				ready++
			}
			restarts += cs.GetRestartCount()
		}
		total := len(o.GetContainerStatuses())
		row.NodeName, row.Phase, row.Status = o.GetNodeName(), o.GetPhase(), o.GetStatus()
		// For a pod "ready/desired" is containers ready / containers total —
		// the same fraction the log prints as "containers=2/3 ready".
		row.Ready, row.Desired = int32(ready), int32(total)
		row.Counts = map[string]int64{
			"restarts":         int64(restarts),
			"ready_containers": int64(ready),
			"total_containers": int64(total),
		}
	case *process.Deployment:
		row.Ready, row.Desired, row.Available = o.GetReadyReplicas(), o.GetReplicasDesired(), o.GetAvailableReplicas()
		row.Counts = map[string]int64{
			"ready":       int64(o.GetReadyReplicas()),
			"desired":     int64(o.GetReplicasDesired()),
			"updated":     int64(o.GetUpdatedReplicas()),
			"available":   int64(o.GetAvailableReplicas()),
			"unavailable": int64(o.GetUnavailableReplicas()),
		}
	case *process.ReplicaSet:
		row.Ready, row.Desired, row.Available = o.GetReadyReplicas(), o.GetReplicasDesired(), o.GetAvailableReplicas()
		row.Counts = map[string]int64{
			"ready":     int64(o.GetReadyReplicas()),
			"desired":   int64(o.GetReplicasDesired()),
			"available": int64(o.GetAvailableReplicas()),
		}
	case *process.DaemonSet:
		st := o.GetStatus()
		row.Ready, row.Desired, row.Available = st.GetNumberReady(), st.GetDesiredNumberScheduled(), st.GetNumberAvailable()
		row.Counts = map[string]int64{
			"ready":        int64(st.GetNumberReady()),
			"desired":      int64(st.GetDesiredNumberScheduled()),
			"current":      int64(st.GetCurrentNumberScheduled()),
			"updated":      int64(st.GetUpdatedNumberScheduled()),
			"available":    int64(st.GetNumberAvailable()),
			"misscheduled": int64(st.GetNumberMisscheduled()),
		}
	case *process.StatefulSet:
		// Desired lives in the SPEC here, unlike the other workloads — the
		// same split orchLogBody reads for its "ready=%d/%d" line.
		st, sp := o.GetStatus(), o.GetSpec()
		row.Ready, row.Desired = st.GetReadyReplicas(), sp.GetDesiredReplicas()
		row.Counts = map[string]int64{
			"ready":   int64(st.GetReadyReplicas()),
			"desired": int64(sp.GetDesiredReplicas()),
			"current": int64(st.GetCurrentReplicas()),
			"updated": int64(st.GetUpdatedReplicas()),
		}
	case *process.Node:
		// No counts, but the condition summary ("Ready", "NotReady") is the
		// one status a node has — the same string orchNode logs.
		row.Status = o.GetStatus().GetStatus()
	case *process.Namespace:
		row.Status = o.GetStatus() // Active / Terminating
	}
}

// orchStringSlice reads a []string field by name, or nil when the message
// has no such field — the manifest wrappers, for one, carry no Tags.
func orchStringSlice(v reflect.Value, name string) []string {
	f := v.FieldByName(name)
	if f.IsValid() && f.CanInterface() {
		if s, ok := f.Interface().([]string); ok {
			return s
		}
	}
	return nil
}

// agentVersionString renders the collector's AgentVersion message the way the
// agent prints itself: major.minor.patch, with the prerelease tag when set.
// Commit and Meta stay behind — a version column wants "7.55.1", not a hash.
func agentVersionString(v *process.AgentVersion) string {
	if v == nil {
		return ""
	}
	s := fmt.Sprintf("%d.%d.%d", v.GetMajor(), v.GetMinor(), v.GetPatch())
	if pre := v.GetPre(); pre != "" {
		s += "-" + pre
	}
	return s
}

// storeOrchManifests turns one manifest envelope into rows for
// ninjacat.k8s_manifests. The CRD/CR callers pass GetManifest(), which may be
// nil — orchManifestRows treats that as an empty collection.
func (a *Server) storeOrchManifests(c *gin.Context, m *process.CollectorManifest) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := orchManifestRows(tenant, time.Now().UTC(), m)
	a.store(storage.K8sManifestsWriter, storage.WriteK8sManifests{Manifests: rows}, len(rows))
}

// orchManifestRows converts one CollectorManifest into rows, one per object.
// Content is stored whole — the entire YAML or JSON document — which is why
// the manifests writer runs with much smaller batches than everything else.
func orchManifestRows(tenant string, now time.Time, m *process.CollectorManifest) []storage.K8sManifestRow {
	if m == nil {
		return nil // a CRD/CR wrapper around a nil envelope — orchManifests logs it
	}
	rows := make([]storage.K8sManifestRow, 0, len(m.GetManifests()))
	for _, man := range m.GetManifests() {
		if man == nil {
			continue
		}
		var terminated uint8
		if man.GetIsTerminated() {
			terminated = 1
		}
		rows = append(rows, storage.K8sManifestRow{
			TenantID:        tenant,
			CollectedAt:     now,
			ClusterID:       m.GetClusterId(),
			ClusterName:     m.GetClusterName(),
			UID:             man.GetUid(),
			Kind:            man.GetKind(),
			APIVersion:      man.GetApiVersion(),
			ResourceVersion: man.GetResourceVersion(),
			Content:         string(man.GetContent()),
			ContentType:     man.GetContentType(),
			IsTerminated:    terminated,
		})
	}
	return rows
}

// storeOrchCluster turns the CollectorCluster summary into its single row for
// ninjacat.k8s_cluster.
func (a *Server) storeOrchCluster(c *gin.Context, m *process.CollectorCluster) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := orchClusterRows(tenant, time.Now().UTC(), m)
	a.store(storage.K8sClusterWriter, storage.WriteK8sCluster{Clusters: rows}, len(rows))
}

// orchClusterRows converts a CollectorCluster into its one row — or none,
// when the inner Cluster message is missing.
func orchClusterRows(tenant string, now time.Time, m *process.CollectorCluster) []storage.K8sClusterRow {
	cl := m.GetCluster()
	if cl == nil {
		return nil
	}
	nodeCount := cl.GetNodeCount()
	if nodeCount < 0 {
		nodeCount = 0 // a count cannot be negative; don't let int32->uint32 invent 4 billion nodes
	}
	return []storage.K8sClusterRow{{
		TenantID:          tenant,
		CollectedAt:       now,
		ClusterID:         m.GetClusterId(),
		ClusterName:       m.GetClusterName(),
		NodeCount:         uint32(nodeCount),
		PodCapacity:       cl.GetPodCapacity(),
		PodAllocatable:    cl.GetPodAllocatable(),
		CPUCapacity:       cl.GetCpuCapacity(),
		CPUAllocatable:    cl.GetCpuAllocatable(),
		MemoryCapacity:    cl.GetMemoryCapacity(),
		MemoryAllocatable: cl.GetMemoryAllocatable(),
		KubeletVersions:   orchVersionSpread(cl.GetKubeletVersions()),
		APIServerVersions: orchVersionSpread(cl.GetApiServerVersions()),
	}}
}

// orchVersionSpread converts the wire's int32 node counts (version -> nodes
// running it) to the table's uint32, clamping the negatives a broken agent
// could send rather than wrapping them into huge counts.
func orchVersionSpread(m map[string]int32) map[string]uint32 {
	if len(m) == 0 {
		return nil
	}
	out := make(map[string]uint32, len(m))
	for k, v := range m {
		if v < 0 {
			v = 0
		}
		out[k] = uint32(v)
	}
	return out
}

// HandleKubeActions accepts the cluster agent's reports on actions it ran.
//
// These are the other half of remote configuration: the config channel tells
// the agent what to do, and this reports what it did — one event when the
// action is received, one when it is executed (and progress in between on the
// newer component). Event platform track "kubeactions", so the body is what
// the logs pipeline produces: a JSON array of events, one per message.
func (a *Server) HandleKubeActions(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[kubeactions] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var batch []json.RawMessage
	if err := json.Unmarshal(body, &batch); err != nil {
		log.Printf("[kubeactions] not a JSON array (%v), raw follows", err)
		describe("kubeactions", c.GetHeader("Content-Type"), body)
		return
	}

	// Every event in the batch is decoded — the log cap below only limits
	// what is printed, never what is kept. An event that fails to decode is
	// reported and skipped; its raw JSON is still in batch[i].
	events := make([]kubeActionResultEvent, 0, len(batch))
	log.Printf("[kubeactions] %d events, %d B", len(batch), len(body))
	for i, raw := range batch {
		ev, err := kubeActionDecode(raw)
		if err != nil {
			log.Printf("   event %d: %v (%d B)", i, err, len(raw))
			continue
		}
		events = append(events, ev)
		if i < orchShow {
			kubeActionLog(ev)
		} else if i == orchShow {
			log.Printf("   ... and %d more", len(batch)-orchShow)
		}
	}

	// events is the whole batch in ActionResultEvent form: every declared
	// field as the agent sent it (OrgID as int64, Timestamp as the RFC 3339
	// string it was, Payloads base64-decoded by encoding/json) plus, in Extra,
	// any key the struct does not declare. batch is the same thing untouched.
	//
	// Storage: one K8sActionRow per decoded event — the audit record of what
	// the cluster agent did with a remote action. The row keeps every declared
	// field except Payloads (whole attachments, no column; they stay only in
	// events) and records the NAMES of undeclared keys in ExtraKeys, so a
	// newer agent's additions are at least visible in queries.
	if tenant := TenantFromContext(c); tenant != "" && len(events) > 0 {
		now := time.Now().UTC()
		rows := make([]storage.K8sActionRow, 0, len(events))
		for _, ev := range events {
			rows = append(rows, kubeActionRow(tenant, now, ev))
		}
		a.store(storage.K8sActionsWriter, storage.WriteK8sActions{Actions: rows}, len(rows))
	}

	_ = batch // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// kubeActionRow converts one decoded event into its row.
//
// Timestamp on the wire is an RFC 3339 string, and empty means "not provided"
// — the same rule wireTime encodes for numeric timestamps: fall back to
// arrival time, never let an absent value become 1970-01-01. An unparseable
// string gets the same fallback; the original was already printed by
// kubeActionLog, so nothing is lost silently.
func kubeActionRow(tenant string, arrival time.Time, ev kubeActionResultEvent) storage.K8sActionRow {
	ts := arrival
	if ev.Timestamp != "" {
		if t, err := time.Parse(time.RFC3339, ev.Timestamp); err == nil {
			ts = t.UTC()
		}
	}
	var extra []string
	if len(ev.Extra) > 0 {
		extra = make([]string, 0, len(ev.Extra))
		for k := range ev.Extra {
			extra = append(extra, k)
		}
		sort.Strings(extra) // deterministic: map order must not leak into rows
	}
	return storage.K8sActionRow{
		TenantID:          tenant,
		Timestamp:         ts,
		ActionID:          ev.ActionID,
		OrgID:             ev.OrgID,
		EventType:         ev.EventType,
		Status:            ev.Status,
		ActionType:        ev.ActionType,
		ClusterID:         ev.ClusterID,
		ClusterName:       ev.ClusterName,
		ResourceID:        ev.ResourceID,
		ResourceKind:      ev.ResourceKind,
		ResourceName:      ev.ResourceName,
		ResourceNamespace: ev.ResourceNamespace,
		RequestedBy:       ev.RequestedBy,
		Message:           ev.Message,
		ExtraKeys:         extra,
	}
}

// kubeActionResultEvent mirrors ActionResultEvent from datadog-agent
// pkg/clusteragent/kubeactions/reporter.go; the same struct, field for field,
// also lives in comp/kubeactions/kubeactions/impl/reporter.go. Neither is
// importable: both packages sit in the root datadog-agent module, which has no
// v0/v1 tag, and both files build only with the kubeapiserver tag.
//
// EventType is action_received, action_progress or action_executed; Status is
// success, failed, skipped, expired, claimed or in_progress; Timestamp is
// RFC 3339, kept as the string it came as (empty = not provided). Payloads is
// whatever the executor attached, base64 on the wire.
//
// Extra is ours, not the agent's: keys present on the wire that the struct
// above does not declare, kept verbatim so a newer agent loses nothing here.
type kubeActionResultEvent struct {
	ActionID          string            `json:"action_id"`
	OrgID             int64             `json:"org_id"`
	EventType         string            `json:"event_type"`
	Status            string            `json:"status"`
	ActionType        string            `json:"action_type"`
	ClusterID         string            `json:"cluster_id"`
	ResourceID        string            `json:"resource_id"`
	RequestedBy       string            `json:"requested_by"`
	Timestamp         string            `json:"timestamp"`
	Message           string            `json:"message"`
	Payloads          map[string][]byte `json:"payloads,omitempty"`
	ClusterName       string            `json:"cluster_name"`
	ResourceKind      string            `json:"resource_kind,omitempty"`
	ResourceName      string            `json:"resource_name,omitempty"`
	ResourceNamespace string            `json:"resource_namespace,omitempty"`

	Extra map[string]json.RawMessage `json:"-"`
}

// kubeActionKnownKeys is the set of JSON keys the struct above declares,
// read from its tags so the two cannot drift apart.
var kubeActionKnownKeys = func() map[string]bool {
	t := reflect.TypeOf(kubeActionResultEvent{})
	keys := make(map[string]bool, t.NumField())
	for i := 0; i < t.NumField(); i++ {
		name, _, _ := strings.Cut(t.Field(i).Tag.Get("json"), ",")
		if name != "" && name != "-" {
			keys[name] = true
		}
	}
	return keys
}()

// kubeActionDecode decodes one event. The raw object is decoded a second time
// into a map so keys the struct does not know end up in Extra rather than
// being silently dropped — the agent side of this is young and still growing.
func kubeActionDecode(raw json.RawMessage) (kubeActionResultEvent, error) {
	var ev kubeActionResultEvent
	if err := json.Unmarshal(raw, &ev); err != nil {
		return ev, err
	}

	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return ev, err
	}
	for k, v := range fields {
		if !kubeActionKnownKeys[k] {
			if ev.Extra == nil {
				ev.Extra = make(map[string]json.RawMessage)
			}
			ev.Extra[k] = v
		}
	}
	return ev, nil
}

// kubeActionLog reports one decoded event.
func kubeActionLog(ev kubeActionResultEvent) {
	resource := ev.ResourceKind
	if ev.ResourceNamespace != "" || ev.ResourceName != "" {
		resource += " " + ev.ResourceNamespace + "/" + ev.ResourceName
	}
	log.Printf("   %s %s action=%s id=%s by=%s cluster=%q (%s) resource=%q id=%s at=%s",
		ev.EventType, ev.Status, ev.ActionType, ev.ActionID, ev.RequestedBy,
		ev.ClusterName, ev.ClusterID, strings.TrimSpace(resource), ev.ResourceID, ev.Timestamp)
	if ev.Message != "" {
		log.Printf("      message: %s", ev.Message)
	}
	for name, data := range ev.Payloads {
		log.Printf("      payload %s: %d B", name, len(data))
	}
	if len(ev.Extra) > 0 {
		unknown := make([]string, 0, len(ev.Extra))
		for k := range ev.Extra {
			unknown = append(unknown, k)
		}
		sort.Strings(unknown)
		log.Printf("      keys not in ActionResultEvent: %s", strings.Join(unknown, ", "))
	}
}
