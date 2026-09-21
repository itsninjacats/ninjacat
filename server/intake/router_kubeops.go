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

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/gin-gonic/gin"
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
// Decoded but not stored yet — the schema decision waits.
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
	// Kinds the log above knows how to summarise.
	case *process.CollectorPod: // 41 — Pods []*process.Pod
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorNode: // 45 — Nodes []*process.Node
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorDeployment: // 43 — Deployments []*process.Deployment
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorReplicaSet: // 42 — ReplicaSets []*process.ReplicaSet
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorDaemonSet: // 49 — DaemonSets []*process.DaemonSet
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorStatefulSet: // 50 — StatefulSets []*process.StatefulSet
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorService: // 44 — Services []*process.Service
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorNamespace: // 61 — Namespaces []*process.Namespace
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorCluster: // 46 — Cluster *process.Cluster, one per frame
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorManifest: // 80 — Manifests []*process.Manifest, Content is the object's own YAML/JSON
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorManifestCRD: // 81 — Manifest *process.CollectorManifest (may be nil), CRDs
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorManifestCR: // 82 — Manifest *process.CollectorManifest (may be nil), custom resources
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.

	// Kinds the log above reports through orchFallback: envelope and identity
	// only. The payload is still the whole message.
	case *process.CollectorJob: // 47 — Jobs []*process.Job
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorCronJob: // 48 — CronJobs []*process.CronJob
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorPersistentVolume: // 51 — PersistentVolumes []*process.PersistentVolume
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorPersistentVolumeClaim: // 52 — PersistentVolumeClaims []*process.PersistentVolumeClaim
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorRole: // 54 — Roles []*process.Role
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorRoleBinding: // 55 — RoleBindings []*process.RoleBinding
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorClusterRole: // 56 — ClusterRoles []*process.ClusterRole
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorClusterRoleBinding: // 57 — ClusterRoleBindings []*process.ClusterRoleBinding
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorServiceAccount: // 58 — ServiceAccounts []*process.ServiceAccount
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorIngress: // 59 — Ingresses []*process.Ingress
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorVerticalPodAutoscaler: // 83 — VerticalPodAutoscalers []*process.VerticalPodAutoscaler
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorHorizontalPodAutoscaler: // 84 — HorizontalPodAutoscalers []*process.HorizontalPodAutoscaler
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorNetworkPolicy: // 85 — NetworkPolicies []*process.NetworkPolicy
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorLimitRange: // 86 — LimitRanges []*process.LimitRange
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorStorageClass: // 87 — StorageClasses []*process.StorageClass
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorPodDisruptionBudget: // 88 — PodDisruptionBudgets []*process.PodDisruptionBudget
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorECSTask: // 200 — Tasks []*process.ECSTask; ECS, not Kubernetes: no Metadata, has AwsAccountID (int64), Region
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.

	default:
		// Every other type process.DecodeMessage can produce belongs to the
		// process agent's /api/v1/collector family and has its own router:
		// CollectorProc, CollectorRealTime, CollectorContainer,
		// CollectorContainerRealTime, CollectorProcDiscovery,
		// CollectorConnections, CollectorProcEvent, ResCollector. Landing here
		// means a misconfigured agent, and the frame is still whole.
		log.Printf("[%s] %T is not an orchestrator message; kept as is", label, payload)
		_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
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
	_ = events // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = batch  // TODO(ninjacat): tables. Complete, unconverted, ready to take.
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
