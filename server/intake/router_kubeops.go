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
	"unicode/utf8"

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
// Stored: object collections in k8s_resources (with k8s_resources_current
// derived from it by a materialized view), raw manifests in k8s_manifests, the
// cluster summary in k8s_cluster, ECS tasks in ecs_tasks, action results in
// k8s_actions. Every one of them carries the frame header as well. A frame
// that does not decode, and any message type that is not an orchestrator
// message, goes to raw_payloads rather than only to the log.
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
		a.storeRaw(c, "orchestrator", "decode_error", label+": "+err.Error(), body)
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
	// orchestrator sender leaves it unset, so 0 means "not provided". OrgID and
	// SubscriptionID are unused by the agent today — which is why they are
	// stored now rather than when a sender starts filling them.
	frame := orchFrameOf(msg.Header)

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
		a.storeOrchResources(c, frame, payload)

	case *process.CollectorCluster: // 46 — Cluster *process.Cluster, one per frame
		a.storeOrchCluster(c, frame, payload)

	// Manifests carry the object's own YAML/JSON in Content. The CRD and CR
	// wrappers hold an inner *process.CollectorManifest that MAY BE NIL —
	// GetManifest absorbs that, and the converter treats nil as empty.
	//
	// The CRD and CR wrappers carry Tags of their OWN, beside the inner
	// manifest's, and they used to be read by nobody — so they are passed
	// down rather than discarded with the wrapper.
	case *process.CollectorManifest: // 80 — Manifests []*process.Manifest
		a.storeOrchManifests(c, frame, payload, nil)
	case *process.CollectorManifestCRD: // 81 — Manifest *process.CollectorManifest (may be nil), CRDs
		a.storeOrchManifests(c, frame, payload.GetManifest(), payload.GetTags())
	case *process.CollectorManifestCR: // 82 — Manifest *process.CollectorManifest (may be nil), custom resources
		a.storeOrchManifests(c, frame, payload.GetManifest(), payload.GetTags())

	case *process.CollectorECSTask: // 200 — Tasks []*process.ECSTask; no Metadata, has AwsAccountID (int64), Region
		// ECS, not Kubernetes: a task has no Metadata, so nothing here maps
		// onto k8s_resources' identity columns (namespace/name/uid). It has a
		// table of its own instead — forcing it into k8s_resources would have
		// meant an empty sort key on every task row.
		a.storeOrchECSTasks(c, frame, payload)

	default:
		// Every other type process.DecodeMessage can produce belongs to the
		// process agent's /api/v1/collector family and has its own router:
		// CollectorProc, CollectorRealTime, CollectorContainer,
		// CollectorContainerRealTime, CollectorProcDiscovery,
		// CollectorConnections, CollectorProcEvent, ResCollector. Landing here
		// means a misconfigured agent — the frame is whole and decoded, but
		// this router has nowhere to put it, so the bytes go to raw_payloads
		// where the process router's owner can find them.
		log.Printf("[%s] %T is not an orchestrator message; kept as is", label, payload)
		a.storeRaw(c, "orchestrator", "unexpected_shape",
			fmt.Sprintf("%s: %T on an orchestrator intake", label, payload), body)
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

// orchFrame is the 16-byte frame header, unpacked into the four columns every
// orchestrator row carries.
//
// It is passed down rather than read again inside the converters so that the
// converters stay pure functions of their input — which is what lets the
// tests build a payload and a header by hand and assert on the row.
type orchFrame struct {
	OrgID          int32
	SubscriptionID uint8
	// Timestamp is the header's raw int64. The frame format documents no unit
	// for it and no sender we have seen sets it, so it is stored as the
	// integer it is; nil when the wire said 0, which means "not provided".
	Timestamp *int64
	Encoding  string
}

func orchFrameOf(h process.MessageHeader) orchFrame {
	f := orchFrame{
		OrgID:          h.OrgID,
		SubscriptionID: h.SubscriptionID,
		Encoding:       orchEncodingName(h.Encoding),
	}
	if h.Timestamp != 0 {
		ts := h.Timestamp
		f.Timestamp = &ts
	}
	return f
}

// orchEncodingName names the body encoding the frame declared. The wire value
// is a byte and the type has no String method, so the mapping lives here —
// and an unknown value keeps its number rather than becoming "".
func orchEncodingName(e process.MessageEncoding) string {
	switch e {
	case process.MessageEncodingProtobuf:
		return "protobuf"
	case process.MessageEncodingJSON:
		return "json"
	case process.MessageEncodingZstdPB:
		return "zstd_protobuf"
	case process.MessageEncodingZstd1xPB:
		return "zstd1x_protobuf"
	case process.MessageEncodingZstdPBxNoCgo:
		return "zstd_protobuf_nocgo"
	default:
		return fmt.Sprintf("unknown_%d", uint8(e))
	}
}

// storeOrchResources turns one Collector* object list into rows for
// ninjacat.k8s_resources. Fire-and-forget: the 202 was already deferred, and
// a missing tenant (a request that skipped the API key middleware) stores
// nothing rather than writing rows nobody can query.
func (a *Server) storeOrchResources(c *gin.Context, frame orchFrame, body process.MessageBody) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	// The orchestrator sender leaves the frame timestamp unset (see the header
	// comment above), so arrival time is the collection time we have. The
	// object's OWN creation timestamp now has a column of its own, so this is
	// no longer the only date on the row.
	rows := orchResourceRows(tenant, time.Now().UTC(), frame, body)
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
// THREE THINGS make this lossless rather than a projection, which is what it
// used to be:
//
//   - the fields every kind shares (metadata dates, finalizers, all owner
//     references, conditions, resource requirements, metrics) are read
//     generically by name, so a kind nobody wrote a case for still gets them;
//   - the numbers and typed extras that differ per kind go through the switch
//     in orchResourceNumbers;
//   - Object holds the WHOLE element as JSON and Envelope the frame around it,
//     so a StorageClass's provisioner or a NetworkPolicy's rules survive with
//     no column of their own.
//
// An element that is nil, or has no Metadata, is skipped: without
// namespace/name/uid the row has no identity and could never be queried or
// joined — dropping it loses less than storing it as noise.
func orchResourceRows(tenant string, now time.Time, frame orchFrame, body process.MessageBody) []storage.K8sResourceRow {
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
	itemsField := -1
	for i := 0; i < v.NumField(); i++ {
		f := v.Field(i)
		if f.Kind() == reflect.Slice && f.Type().Elem().Kind() == reflect.Ptr && f.Type().Elem().Elem().Kind() == reflect.Struct {
			items, itemsField = f, i
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
	hostName := str("HostName")
	var agentVersion string
	if f := v.FieldByName("AgentVersion"); f.IsValid() && f.CanInterface() {
		av, _ := f.Interface().(*process.AgentVersion)
		agentVersion = agentVersionString(av)
	}
	envelope := orchEnvelopeJSON(v, itemsField)

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

			OrgID:           frame.OrgID,
			SubscriptionID:  frame.SubscriptionID,
			HeaderTimestamp: frame.Timestamp,
			Encoding:        frame.Encoding,

			// The object's own dates. Without creationTimestamp "how old is
			// this pod" had no answer for any kind, because collected_at only
			// says when we saw it.
			CreationTimestamp: orchTimePtr(md.GetCreationTimestamp()),
			DeletionTimestamp: orchTimePtr(md.GetDeletionTimestamp()),
			Finalizers:        md.GetFinalizers(),

			HostName: hostName,
			Object:   orchJSON(e.Interface()),
			Envelope: envelope,
		}
		// A grace period is only meaningful while an object is being deleted,
		// and 0 is then a real value (force delete), not absence — so the
		// pointer is set exactly when the deletion timestamp is.
		if md.GetDeletionTimestamp() != 0 {
			g := md.GetDeletionGracePeriodSeconds()
			row.DeletionGracePeriodSeconds = &g
		}
		// The first owner reference, like orchOwner in the log: Kubernetes
		// allows several but in practice writes exactly one controller. The
		// rest, and the uid a join needs, go into the parallel arrays.
		if refs := md.GetOwnerReferences(); len(refs) > 0 {
			row.OwnerKind, row.OwnerName = refs[0].GetKind(), refs[0].GetName()
			row.OwnerKinds = make([]string, 0, len(refs))
			row.OwnerNames = make([]string, 0, len(refs))
			row.OwnerUIDs = make([]string, 0, len(refs))
			for _, ref := range refs {
				row.OwnerKinds = append(row.OwnerKinds, ref.GetKind())
				row.OwnerNames = append(row.OwnerNames, ref.GetName())
				row.OwnerUIDs = append(row.OwnerUIDs, ref.GetUid())
			}
		}
		// The frame's tags apply to every object, the object's own on top.
		objTags := orchStringSlice(e.Elem(), "Tags")
		if n := len(envTags) + len(objTags); n > 0 {
			tags := make([]string, 0, n)
			tags = append(tags, envTags...)
			tags = append(tags, objTags...)
			row.Tags = tagsToMultiMap(tags)
		}
		orchCommonFields(e.Elem(), &row)
		orchResourceNumbers(e.Interface(), &row)
		rows = append(rows, row)
	}
	return rows
}

// orchCommonFields fills the parts every kind can carry but only some do —
// conditions, the agent's condition summary, resource requirements and the
// agent's computed metrics — by looking them up by name.
//
// By name rather than by type because each kind declares its OWN condition
// message (PodCondition, NodeCondition, DeploymentCondition, ...) with the
// same field names and slightly different sets: NodeCondition has no
// lastUpdateTime, PodCondition has lastProbeTime and DeploymentCondition has
// lastUpdateTime. A switch over thirteen near-identical types would be
// thirteen places to forget a new one; reading Type/Status/Reason/Message and
// the three optional times by name covers every present and future condition
// shape at once.
//
// Conditions and resource requirements live either on the object or inside
// its Status/Spec depending on the kind, so both places are tried.
func orchCommonFields(e reflect.Value, row *storage.K8sResourceRow) {
	if lists := orchConditionSlices(e); len(lists) > 0 {
		n := 0
		for _, l := range lists {
			n += l.Len()
		}
		row.ConditionTypes = make([]string, 0, n)
		row.ConditionStatuses = make([]string, 0, n)
		row.ConditionReasons = make([]string, 0, n)
		row.ConditionMessages = make([]string, 0, n)
		row.ConditionLastTransition = make([]*time.Time, 0, n)
		row.ConditionLastUpdate = make([]*time.Time, 0, n)
		row.ConditionLastProbe = make([]*time.Time, 0, n)
		for _, conds := range lists {
			for i := 0; i < conds.Len(); i++ {
				cv := reflect.Indirect(conds.Index(i))
				if !cv.IsValid() || cv.Kind() != reflect.Struct {
					continue
				}
				// Two naming conventions in one payload family: most
				// conditions call the fields Type/Status, while
				// HorizontalPodAutoscaler's and VPACondition's call them
				// ConditionType/ConditionStatus. Reading both is the
				// difference between an autoscaler's conditions arriving and
				// arriving empty.
				row.ConditionTypes = append(row.ConditionTypes, orchFirstString(cv, "Type", "ConditionType"))
				row.ConditionStatuses = append(row.ConditionStatuses, orchFirstString(cv, "Status", "ConditionStatus"))
				row.ConditionReasons = append(row.ConditionReasons, orchStringOf(cv, "Reason"))
				row.ConditionMessages = append(row.ConditionMessages, orchStringOf(cv, "Message"))
				row.ConditionLastTransition = append(row.ConditionLastTransition, orchTimePtr(orchInt64Of(cv, "LastTransitionTime")))
				row.ConditionLastUpdate = append(row.ConditionLastUpdate, orchTimePtr(orchInt64Of(cv, "LastUpdateTime")))
				row.ConditionLastProbe = append(row.ConditionLastProbe, orchTimePtr(orchInt64Of(cv, "LastProbeTime")))
			}
		}
	}

	row.ConditionMessage = orchStringOf(e, "ConditionMessage")
	if row.ConditionMessage == "" {
		if st := reflect.Indirect(orchValue(e, "Status")); st.IsValid() && st.Kind() == reflect.Struct {
			row.ConditionMessage = orchStringOf(st, "ConditionMessage")
		}
	}

	if rr := orchFindSlice(e, "ResourceRequirements", "Spec"); rr.IsValid() && rr.Len() > 0 {
		row.ResourceRequirements = orchJSON(rr.Interface())
	}

	if m := orchValue(e, "Metrics"); m.IsValid() && m.CanInterface() {
		if rm, ok := m.Interface().(*process.ResourceMetrics); ok && rm != nil {
			row.Metrics = rm.GetMetricValues()
		}
	}
}

// orchResourceNumbers fills the per-kind numbers and typed extras — the part
// reflection cannot see, because every kind names its counters differently.
// The arithmetic mirrors the orchPod/orchLogBody lines above: what the log
// prints for a kind is exactly what its Counts map stores.
//
// The dedicated Ready/Desired/Available columns carry the same values where
// the kind has them, so the common "how healthy" query needs no map lookup.
// A kind with no case here is NOT a gap any more: its identity, metadata,
// conditions and metrics are filled generically and its whole message is in
// the object column. What a case adds is a number somebody charts.
func orchResourceNumbers(obj any, row *storage.K8sResourceRow) {
	switch o := obj.(type) {
	case *process.Pod:
		// Same sums as orchPod: ready and restarts come from the container
		// statuses, not from a field of their own. The per-container detail
		// now survives beside the sums — "which container of the three is
		// crashlooping, and with what message" used to have no answer.
		ready, restarts := 0, int32(0)
		for _, cs := range o.GetContainerStatuses() {
			if cs.GetReady() {
				ready++
			}
			restarts += cs.GetRestartCount()
		}
		total := len(o.GetContainerStatuses())
		initReady, initRestarts := 0, int32(0)
		for _, cs := range o.GetInitContainerStatuses() {
			if cs.GetReady() {
				initReady++
			}
			initRestarts += cs.GetRestartCount()
		}
		row.NodeName, row.Phase, row.Status = o.GetNodeName(), o.GetPhase(), o.GetStatus()
		// For a pod "ready/desired" is containers ready / containers total —
		// the same fraction the log prints as "containers=2/3 ready".
		row.Ready, row.Desired = int32(ready), int32(total)
		row.Counts = map[string]int64{
			"restarts":              int64(restarts),
			"ready_containers":      int64(ready),
			"total_containers":      int64(total),
			"init_restarts":         int64(initRestarts),
			"ready_init_containers": int64(initReady),
			"total_init_containers": int64(len(o.GetInitContainerStatuses())),
			// The agent's own aggregate, kept beside the recomputed one: if
			// they ever disagree, that is worth seeing rather than hiding.
			"pod_restart_count": int64(o.GetRestartCount()),
		}
		row.PodIP = o.GetIP()
		row.NominatedNodeName = o.GetNominatedNodeName()
		row.QOSClass = o.GetQOSClass()
		row.PriorityClass = o.GetPriorityClass()
		row.StartTime = orchTimePtr(o.GetStartTime())
		row.ScheduledTime = orchTimePtr(o.GetScheduledTime())
		if h := o.GetHost(); h != nil && h.GetName() != "" {
			row.HostName = h.GetName()
		}
		// Regular containers first, then init containers, told apart by
		// ContainerIsInit — one set of arrays rather than two, because every
		// question about a container ("is it ready", "what is its image") is
		// the same question for both.
		statuses := make([]*process.ContainerStatus, 0, total+len(o.GetInitContainerStatuses()))
		isInit := make([]uint8, 0, cap(statuses))
		for _, cs := range o.GetContainerStatuses() {
			statuses, isInit = append(statuses, cs), append(isInit, 0)
		}
		for _, cs := range o.GetInitContainerStatuses() {
			statuses, isInit = append(statuses, cs), append(isInit, 1)
		}
		row.ContainerNames = make([]string, 0, len(statuses))
		row.ContainerIDs = make([]string, 0, len(statuses))
		row.ContainerReady = make([]uint8, 0, len(statuses))
		row.ContainerRestarts = make([]int32, 0, len(statuses))
		row.ContainerStates = make([]string, 0, len(statuses))
		row.ContainerMessages = make([]string, 0, len(statuses))
		row.ContainerImages = make([]string, 0, len(statuses))
		row.ContainerImageIDs = make([]string, 0, len(statuses))
		row.ContainerIsInit = isInit
		for _, cs := range statuses {
			row.ContainerNames = append(row.ContainerNames, cs.GetName())
			row.ContainerIDs = append(row.ContainerIDs, cs.GetContainerID())
			row.ContainerReady = append(row.ContainerReady, boolToUint8(cs.GetReady()))
			row.ContainerRestarts = append(row.ContainerRestarts, cs.GetRestartCount())
			row.ContainerStates = append(row.ContainerStates, cs.GetState())
			row.ContainerMessages = append(row.ContainerMessages, cs.GetMessage())
			row.ContainerImages = append(row.ContainerImages, cs.GetImage())
			row.ContainerImageIDs = append(row.ContainerImageIDs, cs.GetImageID())
		}
	case *process.Deployment:
		row.Ready, row.Desired, row.Available = o.GetReadyReplicas(), o.GetReplicasDesired(), o.GetAvailableReplicas()
		row.Counts = map[string]int64{
			"ready":       int64(o.GetReadyReplicas()),
			"desired":     int64(o.GetReplicasDesired()),
			"updated":     int64(o.GetUpdatedReplicas()),
			"available":   int64(o.GetAvailableReplicas()),
			"unavailable": int64(o.GetUnavailableReplicas()),
			// The observed total, distinct from desired: during a rollout
			// they differ, and that difference is the rollout.
			"replicas": int64(o.GetReplicas()),
		}
	case *process.ReplicaSet:
		row.Ready, row.Desired, row.Available = o.GetReadyReplicas(), o.GetReplicasDesired(), o.GetAvailableReplicas()
		row.Counts = map[string]int64{
			"ready":         int64(o.GetReadyReplicas()),
			"desired":       int64(o.GetReplicasDesired()),
			"available":     int64(o.GetAvailableReplicas()),
			"replicas":      int64(o.GetReplicas()),
			"fully_labeled": int64(o.GetFullyLabeledReplicas()),
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
			// The seventh number of DaemonSetStatus, and the one that says a
			// rollout is stuck.
			"unavailable": int64(st.GetNumberUnavailable()),
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
			// Status.Replicas, the observed total — a different field from
			// current, and it used to be the one that went missing.
			"replicas":  int64(st.GetReplicas()),
			"partition": int64(sp.GetPartition()),
		}
	case *process.Node:
		// No counts, but the condition summary ("Ready", "NotReady") is the
		// one status a node has — the same string orchNode logs.
		st := o.GetStatus()
		row.Status = st.GetStatus()
		row.PodCIDR = o.GetPodCIDR()
		row.PodCIDRs = o.GetPodCIDRs()
		row.Unschedulable = boolToUint8(o.GetUnschedulable())
		row.ProviderID = o.GetProviderID()
		row.NodeRoles = o.GetRoles()
		if len(o.GetTaints()) > 0 {
			row.Taints = orchJSON(o.GetTaints())
		}
		row.Capacity = st.GetCapacity()
		row.Allocatable = st.GetAllocatable()
		if len(st.GetNodeAddresses()) > 0 {
			row.NodeAddresses = orchJSON(st.GetNodeAddresses())
		}
		row.KubeletVersion = st.GetKubeletVersion()
		row.KubeProxyVersion = st.GetKubeProxyVersion()
		row.OperatingSystem = st.GetOperatingSystem()
		row.Architecture = st.GetArchitecture()
		row.KernelVersion = st.GetKernelVersion()
		row.OSImage = st.GetOsImage()
		row.ContainerRuntimeVersion = st.GetContainerRuntimeVersion()
		if h := o.GetHost(); h != nil && h.GetName() != "" {
			row.HostName = h.GetName()
		}
		row.Counts = map[string]int64{
			"taints": int64(len(o.GetTaints())),
			// The node-local image cache, counted; the images themselves are
			// the contimage intake's job and are in object anyway.
			"images": int64(len(st.GetImages())),
		}
	case *process.Namespace:
		row.Status = o.GetStatus() // Active / Terminating
	case *process.Service:
		sp := o.GetSpec()
		row.ServiceType = sp.GetType()
		row.ClusterIP = sp.GetClusterIP()
		if len(sp.GetPorts()) > 0 {
			row.ServicePorts = orchJSON(sp.GetPorts())
		}
		row.Counts = map[string]int64{"ports": int64(len(sp.GetPorts()))}
	case *process.Role:
		row.RBACRules = orchJSON(o.GetRules())
		row.Counts = map[string]int64{"rules": int64(len(o.GetRules()))}
	case *process.ClusterRole:
		row.RBACRules = orchJSON(o.GetRules())
		row.Counts = map[string]int64{
			"rules":             int64(len(o.GetRules())),
			"aggregation_rules": int64(len(o.GetAggregationRules())),
		}
	case *process.RoleBinding:
		row.RBACSubjects = orchJSON(o.GetSubjects())
		row.RBACRoleRef = orchJSON(o.GetRoleRef())
		row.Counts = map[string]int64{"subjects": int64(len(o.GetSubjects()))}
	case *process.ClusterRoleBinding:
		row.RBACSubjects = orchJSON(o.GetSubjects())
		row.RBACRoleRef = orchJSON(o.GetRoleRef())
		row.Counts = map[string]int64{"subjects": int64(len(o.GetSubjects()))}
	case *process.Job:
		st, sp := o.GetStatus(), o.GetSpec()
		row.Counts = map[string]int64{
			"active":                  int64(st.GetActive()),
			"succeeded":               int64(st.GetSucceeded()),
			"failed":                  int64(st.GetFailed()),
			"parallelism":             int64(sp.GetParallelism()),
			"completions":             int64(sp.GetCompletions()),
			"backoff_limit":           int64(sp.GetBackoffLimit()),
			"active_deadline_seconds": sp.GetActiveDeadlineSeconds(),
		}
		row.StartTime = orchTimePtr(st.GetStartTime())
	case *process.CronJob:
		st, sp := o.GetStatus(), o.GetSpec()
		row.Counts = map[string]int64{
			"active_jobs":                   int64(len(st.GetActive())),
			"suspend":                       int64(boolToUint8(sp.GetSuspend())),
			"successful_jobs_history_limit": int64(sp.GetSuccessfulJobsHistoryLimit()),
			"failed_jobs_history_limit":     int64(sp.GetFailedJobsHistoryLimit()),
			"starting_deadline_seconds":     sp.GetStartingDeadlineSeconds(),
		}
	case *process.HorizontalPodAutoscaler:
		st, sp := o.GetStatus(), o.GetSpec()
		row.Ready, row.Desired = st.GetCurrentReplicas(), st.GetDesiredReplicas()
		row.Counts = map[string]int64{
			"min_replicas":     int64(sp.GetMinReplicas()),
			"max_replicas":     int64(sp.GetMaxReplicas()),
			"current_replicas": int64(st.GetCurrentReplicas()),
			"desired_replicas": int64(st.GetDesiredReplicas()),
		}
	case *process.PodDisruptionBudget:
		st := o.GetStatus()
		row.Counts = map[string]int64{
			// The number a PDB exists to produce: how many pods may be
			// evicted right now.
			"disruptions_allowed": int64(st.GetDisruptionsAllowed()),
			"current_healthy":     int64(st.GetCurrentHealthy()),
			"desired_healthy":     int64(st.GetDesiredHealthy()),
			"expected_pods":       int64(st.GetExpectedPods()),
			"disrupted_pods":      int64(len(st.GetDisruptedPods())),
		}
	}
}

// orchValue reads a field by name off a struct value, or an invalid Value
// when the message has no such field.
func orchValue(v reflect.Value, name string) reflect.Value {
	if !v.IsValid() || v.Kind() != reflect.Struct {
		return reflect.Value{}
	}
	f := v.FieldByName(name)
	if !f.IsValid() || !f.CanInterface() {
		return reflect.Value{}
	}
	return f
}

// orchFirstString reads the first of several candidate field names that is
// present and non-empty.
func orchFirstString(v reflect.Value, names ...string) string {
	for _, n := range names {
		if s := orchStringOf(v, n); s != "" {
			return s
		}
	}
	return ""
}

func orchStringOf(v reflect.Value, name string) string {
	f := orchValue(v, name)
	if f.IsValid() && f.Kind() == reflect.String {
		return f.String()
	}
	return ""
}

func orchInt64Of(v reflect.Value, name string) int64 {
	f := orchValue(v, name)
	if f.IsValid() && (f.Kind() == reflect.Int64 || f.Kind() == reflect.Int32) {
		return f.Int()
	}
	return 0
}

// orchFindSlice looks for a slice field by name on the object, then in the
// named sub-message. Deployment keeps its conditions on the object while a
// PodDisruptionBudget keeps them in Status, and a Job keeps its resource
// requirements in Spec while a Pod keeps them on the object — one lookup
// covers both layouts.
//
// PRESENCE IS NOT ENOUGH: a kind can declare the field in both places and
// fill only the inner one, and a declared-but-nil slice satisfies every check
// a reflect.Value can make. Returning the first field that merely exists then
// reads an empty list and never looks further, which is exactly how
// VerticalPodAutoscaler's real conditions went missing. So a populated slice
// wins over an empty one, whichever layer it sits in.
func orchFindSlice(e reflect.Value, name, nested string) reflect.Value {
	var direct reflect.Value
	if f := orchValue(e, name); f.IsValid() && f.Kind() == reflect.Slice {
		if f.Len() > 0 {
			return f
		}
		direct = f
	}
	if sub := reflect.Indirect(orchValue(e, nested)); sub.IsValid() && sub.Kind() == reflect.Struct {
		if f := orchValue(sub, name); f.IsValid() && f.Kind() == reflect.Slice && f.Len() > 0 {
			return f
		}
	}
	return direct
}

// orchConditionSlices returns every condition list the object carries.
//
// Most kinds keep exactly one, on the object or inside Status. A
// VerticalPodAutoscaler declares BOTH and they are different messages:
// VerticalPodAutoscaler.Conditions holds VerticalPodAutoscalerCondition
// (Type/Status), while Status.Conditions holds VPACondition
// (ConditionType/ConditionStatus) — and the nested one is the list the
// upstream Kubernetes VPA API actually defines, so it is the one a real
// autoscaler fills. Reading either alone drops the other, so both are read
// and concatenated, the object's first; empty lists contribute nothing.
func orchConditionSlices(e reflect.Value) []reflect.Value {
	var out []reflect.Value
	if f := orchValue(e, "Conditions"); f.IsValid() && f.Kind() == reflect.Slice && f.Len() > 0 {
		out = append(out, f)
	}
	if sub := reflect.Indirect(orchValue(e, "Status")); sub.IsValid() && sub.Kind() == reflect.Struct {
		if f := orchValue(sub, "Conditions"); f.IsValid() && f.Kind() == reflect.Slice && f.Len() > 0 {
			out = append(out, f)
		}
	}
	return out
}

// orchEnvelopeJSON renders the frame envelope without its object list.
//
// The list is the rows, so repeating it per row would square the payload; the
// rest of the envelope is small, identical for every row of the pass, and
// holds the per-kind extras nothing else reads — CollectorPod's Host, Info and
// IsTerminated, CollectorNode's HostAliasMapping. ZSTD collapses the repeats
// inside the part.
//
// The copy is made through reflection rather than by mutating the caller's
// message, which is still being read by the log path and by the converters.
func orchEnvelopeJSON(v reflect.Value, itemsField int) string {
	if itemsField < 0 || !v.IsValid() || v.Kind() != reflect.Struct {
		return ""
	}
	cp := reflect.New(v.Type()).Elem()
	for i := 0; i < v.NumField(); i++ {
		if i == itemsField || !cp.Field(i).CanSet() {
			continue
		}
		cp.Field(i).Set(v.Field(i))
	}
	return orchJSON(cp.Addr().Interface())
}

// orchEnvelopeOf is orchEnvelopeJSON for a message held as a typed pointer
// rather than reached by reflection: it locates the repeated field by name
// and drops it, so the caller does not have to carry a field index.
//
// The manifest and ECS collectors need this because they are decoded into
// their concrete types (they have per-kind logic the generic object path does
// not), and their envelopes carry fields no column holds — SystemInfo on
// both, Host on CollectorECSTask.
func orchEnvelopeOf(msg any, itemsField string) string {
	v := reflect.Indirect(reflect.ValueOf(msg))
	if !v.IsValid() || v.Kind() != reflect.Struct {
		return ""
	}
	f, ok := v.Type().FieldByName(itemsField)
	if !ok || len(f.Index) != 1 {
		return ""
	}
	return orchEnvelopeJSON(v, f.Index[0])
}

// orchJSONOf renders a sub-message only when it is there. A nil pointer would
// marshal to the four bytes "null", which is a value pretending to be a fact;
// an absent sub-message leaves its column empty instead.
func orchJSONOf[T any](p *T) string {
	if p == nil {
		return ""
	}
	return orchJSON(p)
}

// orchJSON renders a decoded sub-message for a String column.
//
// encoding/json rather than protojson: agent-payload's process package is
// generated by protoc-gen-gogo, so its messages are not protoreflect messages
// and protojson cannot see them. encoding/json reads the same generated
// struct tags, keeps int64 ids as exact digits rather than floats, and needs
// no new dependency.
//
// A marshalling failure returns "" rather than propagating: the column is a
// completeness guarantee, not the row's identity, and losing the whole row
// over it would be the worse trade. In practice generated structs always
// marshal.
func orchJSON(v any) string {
	b, err := json.Marshal(v)
	if err != nil {
		log.Printf("[orch] cannot render %T as JSON: %v", v, err)
		return ""
	}
	return string(b)
}

// orchTimePtr converts a Unix-seconds timestamp off the orchestrator wire
// into a value for a Nullable(DateTime) column.
//
// Same rule as lcTimePtr and wireTime: 0 means the sender did not supply the
// field, and it must never become 1970-01-01. Every date in this payload
// family is an int64 of seconds, including the ones inside conditions.
func orchTimePtr(seconds int64) *time.Time {
	if seconds <= 0 {
		return nil
	}
	t := time.Unix(seconds, 0).UTC()
	return &t
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
// nil — orchManifestRows treats that as an empty collection — plus the
// wrapper's own tags, which nothing used to read.
func (a *Server) storeOrchManifests(c *gin.Context, frame orchFrame, m *process.CollectorManifest, wrapperTags []string) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := orchManifestRows(tenant, time.Now().UTC(), frame, m, wrapperTags)
	a.store(storage.K8sManifestsWriter, storage.WriteK8sManifests{Manifests: rows}, len(rows))
}

// orchManifestRows converts one CollectorManifest into rows, one per object.
// Content is stored whole — the entire YAML or JSON document — which is why
// the manifests writer runs with much smaller batches than everything else.
//
// TAGS COME FROM THREE PLACES and used to come from none: the frame envelope,
// the CRD/CR wrapper around it, and the manifest itself. They are merged into
// one multiset, because a query asking for env:prod does not care who
// attached the tag, and TagSources keeps the provenance beside it so the
// question "who said so" is still answerable.
func orchManifestRows(tenant string, now time.Time, frame orchFrame, m *process.CollectorManifest, wrapperTags []string) []storage.K8sManifestRow {
	if m == nil {
		return nil // a CRD/CR wrapper around a nil envelope — orchManifests logs it
	}
	envTags := m.GetTags()
	// The envelope minus its manifest list, read once for the whole frame:
	// CollectorManifest.SystemInfo (the reporting agent's uuid, OS, CPUs and
	// memory) has no column, and neither will whatever the envelope grows
	// next.
	envelope := orchEnvelopeOf(m, "Manifests")
	rows := make([]storage.K8sManifestRow, 0, len(m.GetManifests()))
	for _, man := range m.GetManifests() {
		if man == nil {
			continue
		}
		content := man.GetContent()
		manTags := man.GetTags()

		merged := make([]string, 0, len(envTags)+len(wrapperTags)+len(manTags))
		merged = append(merged, envTags...)
		merged = append(merged, wrapperTags...)
		merged = append(merged, manTags...)

		sources := make(map[string][]string, 3)
		for name, tags := range map[string][]string{
			"envelope": envTags,
			"wrapper":  wrapperTags,
			"manifest": manTags,
		} {
			if len(tags) > 0 {
				sources[name] = tags
			}
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
			Content:         string(content),
			ContentType:     man.GetContentType(),
			IsTerminated:    boolToUint8(man.GetIsTerminated()),

			OrgID:           frame.OrgID,
			SubscriptionID:  frame.SubscriptionID,
			HeaderTimestamp: frame.Timestamp,
			Encoding:        frame.Encoding,

			GroupID:         m.GetGroupId(),
			GroupSize:       m.GetGroupSize(),
			HostName:        m.GetHostName(),
			AgentVersion:    agentVersionString(m.GetAgentVersion()),
			OriginCollector: m.GetOriginCollector().String(),

			Tags:       tagsToMultiMap(merged),
			TagSources: sources,

			NodeName:        man.GetNodeName(),
			Type:            man.GetType(),
			Version:         man.GetVersion(),
			ExtraAttributes: man.GetExtraAttributes(),

			// A ClickHouse String is a byte string, so the content survives
			// either way — but whoever reads the column deserves to know
			// whether it can be parsed as text before trying.
			ContentIsUTF8: boolToUint8(utf8.Valid(content)),

			Envelope: envelope,
			// Manifest.Host: which machine this one object was observed on,
			// in full. Its shape belongs to the host inventory rather than to
			// a manifest, so it stays JSON instead of spreading five columns
			// nobody queries yet across every manifest row.
			ManifestHost: orchJSONOf(man.GetHost()),
		})
	}
	return rows
}

// storeOrchCluster turns the CollectorCluster summary into its single row for
// ninjacat.k8s_cluster.
func (a *Server) storeOrchCluster(c *gin.Context, frame orchFrame, m *process.CollectorCluster) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := orchClusterRows(tenant, time.Now().UTC(), frame, m)
	a.store(storage.K8sClusterWriter, storage.WriteK8sCluster{Clusters: rows}, len(rows))
}

// orchClusterRows converts a CollectorCluster into its one row — or none,
// when the inner Cluster message is missing.
//
// NodesInfo is the per-node breakdown behind NodeCount, kept as parallel
// arrays in wire order. Only the total used to survive, so "which of my nodes
// is still on the old kubelet" was a histogram with no names on it.
func orchClusterRows(tenant string, now time.Time, frame orchFrame, m *process.CollectorCluster) []storage.K8sClusterRow {
	cl := m.GetCluster()
	if cl == nil {
		return nil
	}
	nodeCount := cl.GetNodeCount()
	if nodeCount < 0 {
		nodeCount = 0 // a count cannot be negative; don't let int32->uint32 invent 4 billion nodes
	}

	info := cl.GetNodesInfo()
	row := storage.K8sClusterRow{
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

		OrgID:           frame.OrgID,
		SubscriptionID:  frame.SubscriptionID,
		HeaderTimestamp: frame.Timestamp,
		Encoding:        frame.Encoding,

		GroupID:      m.GetGroupId(),
		GroupSize:    m.GetGroupSize(),
		AgentVersion: agentVersionString(m.GetAgentVersion()),

		// Two tag sets, not merged: the frame's and the Cluster object's own
		// are different fields on the wire, and which producer attached a tag
		// is worth keeping.
		Tags:        tagsToMultiMap(m.GetTags()),
		ClusterTags: tagsToMultiMap(cl.GetTags()),

		ResourceVersion:              cl.GetResourceVersion(),
		CreationTimestamp:            orchTimePtr(cl.GetCreationTimestamp()),
		Metrics:                      cl.GetMetrics().GetMetricValues(),
		ExtendedResourcesCapacity:    cl.GetExtendedResourcesCapacity(),
		ExtendedResourcesAllocatable: cl.GetExtendedResourcesAllocatable(),

		NodesName:                    make([]string, 0, len(info)),
		NodesRegion:                  make([]string, 0, len(info)),
		NodesInstanceType:            make([]string, 0, len(info)),
		NodesOS:                      make([]string, 0, len(info)),
		NodesOSImage:                 make([]string, 0, len(info)),
		NodesArchitecture:            make([]string, 0, len(info)),
		NodesKernelVersion:           make([]string, 0, len(info)),
		NodesContainerRuntimeVersion: make([]string, 0, len(info)),
		NodesKubeletVersion:          make([]string, 0, len(info)),
		NodesAllocatable:             make([]map[string]string, 0, len(info)),
		NodesCapacity:                make([]map[string]string, 0, len(info)),
	}
	for _, n := range info {
		row.NodesName = append(row.NodesName, n.GetName())
		row.NodesRegion = append(row.NodesRegion, n.GetRegion())
		row.NodesInstanceType = append(row.NodesInstanceType, n.GetInstanceType())
		row.NodesOS = append(row.NodesOS, n.GetOperatingSystem())
		row.NodesOSImage = append(row.NodesOSImage, n.GetOperatingSystemImage())
		row.NodesArchitecture = append(row.NodesArchitecture, n.GetArchitecture())
		row.NodesKernelVersion = append(row.NodesKernelVersion, n.GetKernelVersion())
		row.NodesContainerRuntimeVersion = append(row.NodesContainerRuntimeVersion, n.GetContainerRuntimeVersion())
		row.NodesKubeletVersion = append(row.NodesKubeletVersion, n.GetKubeletVersion())
		// An Array(Map(...)) rejects a nil element, and a node that reported
		// no quantities is a normal occurrence — so the hole is filled here,
		// not left for the driver to reject the whole batch over.
		row.NodesAllocatable = append(row.NodesAllocatable, orEmptyStringMap(n.GetResourceAllocatable()))
		row.NodesCapacity = append(row.NodesCapacity, orEmptyStringMap(n.GetResourceCapacity()))
	}
	return []storage.K8sClusterRow{row}
}

// orEmptyStringMap is the per-element guard for Array(Map(String, String)):
// storage's orEmpty fixes a nil map passed as a whole column, but a nil
// INSIDE an array never reaches it.
func orEmptyStringMap(m map[string]string) map[string]string {
	if m == nil {
		return map[string]string{}
	}
	return m
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

// storeOrchECSTasks turns one CollectorECSTask frame into rows for
// ninjacat.ecs_tasks.
func (a *Server) storeOrchECSTasks(c *gin.Context, frame orchFrame, m *process.CollectorECSTask) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := orchECSTaskRows(tenant, time.Now().UTC(), frame, m)
	a.store(storage.ECSTasksWriter, storage.WriteECSTasks{Tasks: rows}, len(rows))
}

// orchECSTaskRows converts a CollectorECSTask into one row per task.
//
// ECS, not Kubernetes: a task has no Metadata, so it has no namespace, name or
// uid — the columns k8s_resources sorts by. The ARN is the identity instead,
// which is why this has a table of its own rather than an emptied-out row in
// somebody else's.
//
// FOUR tag sets stay apart: the frame envelope's, and the three ECS itself
// keeps separate (Datadog tags, the task's own ECS tags, and the tags of the
// container instance it happens to run on — those belong to the machine, not
// the workload).
func orchECSTaskRows(tenant string, now time.Time, frame orchFrame, m *process.CollectorECSTask) []storage.ECSTaskRow {
	tasks := m.GetTasks()
	envelopeTags := tagsToMultiMap(m.GetTags())
	// CollectorECSTask has a Host and an Info of its OWN — the machine and
	// the agent that reported the pass, distinct from each task's Host below
	// — and no column holds either, so the envelope is kept whole.
	envelope := orchEnvelopeOf(m, "Tasks")
	rows := make([]storage.ECSTaskRow, 0, len(tasks))
	for _, t := range tasks {
		if t == nil {
			continue
		}
		rows = append(rows, storage.ECSTaskRow{
			TenantID:    tenant,
			CollectedAt: now,

			OrgID:           frame.OrgID,
			SubscriptionID:  frame.SubscriptionID,
			HeaderTimestamp: frame.Timestamp,
			Encoding:        frame.Encoding,

			AWSAccountID: m.GetAwsAccountID(),
			ClusterID:    m.GetClusterId(),
			ClusterName:  m.GetClusterName(),
			Region:       m.GetRegion(),
			GroupID:      m.GetGroupId(),
			GroupSize:    m.GetGroupSize(),
			HostName:     m.GetHostName(),
			AgentVersion: agentVersionString(m.GetAgentVersion()),
			EnvelopeTags: envelopeTags,

			ARN:                  t.GetArn(),
			ResourceVersion:      t.GetResourceVersion(),
			LaunchType:           t.GetLaunchType(),
			DesiredStatus:        t.GetDesiredStatus(),
			KnownStatus:          t.GetKnownStatus(),
			Family:               t.GetFamily(),
			Version:              t.GetVersion(),
			AvailabilityZone:     t.GetAvailabilityZone(),
			ServiceName:          t.GetServiceName(),
			VpcID:                t.GetVpcId(),
			ContainerInstanceARN: t.GetContainerInstanceArn(),
			DaemonName:           t.GetDaemonName(),
			TaskHostName:         t.GetHost().GetName(),

			Limits:                  t.GetLimits(),
			EphemeralStorageMetrics: t.GetEphemeralStorageMetrics(),

			PullStartedAt:      orchTimePtr(t.GetPullStartedAt()),
			PullStoppedAt:      orchTimePtr(t.GetPullStoppedAt()),
			ExecutionStoppedAt: orchTimePtr(t.GetExecutionStoppedAt()),

			// The containers as JSON. An ECSContainer nests ports, networks,
			// volumes, health and log options, each a repeated sub-message —
			// parallel arrays would have to be arrays of arrays of structs.
			Containers:     orchJSON(t.GetContainers()),
			ContainerCount: uint32(len(t.GetContainers())),

			Tags:                  tagsToMultiMap(t.GetTags()),
			ECSTags:               tagsToMultiMap(t.GetEcsTags()),
			ContainerInstanceTags: tagsToMultiMap(t.GetContainerInstanceTags()),

			Envelope: envelope,
			// The task's Host in full. TaskHostName is only its Name, and the
			// rest (id, orgId, allTags, numCpus, totalMemory) is what tells
			// two identically named container instances apart.
			TaskHost: orchJSONOf(t.GetHost()),
		})
	}
	return rows
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
		// describe() prints the shape without guessing a schema; the bytes
		// themselves go to raw_payloads, because a shape we cannot parse is
		// exactly the payload somebody will want to read later.
		a.storeRaw(c, "kubeactions", "decode_error", err.Error(), body)
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
			// One bad element must not cost the batch, and must not cost
			// itself either: the element's own JSON goes to raw_payloads
			// while the rest of the batch becomes rows.
			a.storeRaw(c, "kubeactions", "decode_error",
				fmt.Sprintf("batch element %d: %v", i, err), raw)
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
	// the cluster agent did with a remote action. Every declared field, the
	// attachments, and the undeclared keys with their values, which used to
	// survive as names alone.
	if tenant := TenantFromContext(c); tenant != "" && len(events) > 0 {
		now := time.Now().UTC()
		rows := make([]storage.K8sActionRow, 0, len(events))
		for _, ev := range events {
			rows = append(rows, kubeActionRow(tenant, now, ev))
		}
		a.store(storage.K8sActionsWriter, storage.WriteK8sActions{Actions: rows}, len(rows))
	}
}

// kubeActionRow converts one decoded event into its row. Nothing is left
// behind: the attachments and the undeclared keys' values, both previously
// dropped by design, have columns now.
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
	var extraJSON string
	if len(ev.Extra) > 0 {
		extra = make([]string, 0, len(ev.Extra))
		for k := range ev.Extra {
			extra = append(extra, k)
		}
		sort.Strings(extra) // deterministic: map order must not leak into rows
		// The names stay a column of their own because filtering on them
		// then needs no JSON parsing; the values come too, because a newer
		// agent's addition is worth keeping and not just counting.
		extraJSON = orchJSON(ev.Extra)
	}
	// Attachments, name -> bytes. ClickHouse Strings are byte strings, so a
	// non-text attachment survives the trip unchanged.
	var payloads map[string]string
	if len(ev.Payloads) > 0 {
		payloads = make(map[string]string, len(ev.Payloads))
		for k, v := range ev.Payloads {
			payloads[k] = string(v)
		}
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
		Payloads:          payloads,
		Extra:             extraJSON,
		// The string exactly as it arrived. Timestamp above falls back to
		// arrival time when this is empty or unparseable, and that fallback
		// used to erase its own evidence.
		TimestampRaw: ev.Timestamp,
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
