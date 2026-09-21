package intake

import (
	"fmt"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/contimage"
	"github.com/DataDog/agent-payload/v5/contlcycle"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// contlcycle-intake.<site>, contimage-intake.<site> — containers.
//
//	config: none. Event platform pipelines have no dd_url, only
//	        container_lifecycle.enabled / container_image.enabled and
//	        <prefix>.additional_endpoints, which ADDS a recipient rather than
//	        replacing one. DD_SITE is the only way to point them here.
//
// Both carry protobuf, and both schemas ship in agent-payload — the same
// module we already use for metrics and processes.
//
// Decoded but not stored yet.
func (a *Server) routeContainers(g *gin.RouterGroup) {
	g.POST("/api/v2/contlcycle", a.HandleContainerLifecycle)
	g.POST("/api/v2/contimage", a.HandleContainerImages)
}

// containersLogLimit bounds the per-item lines under one payload header. The
// agent batches events and images; a full cluster inventory is not a log.
const containersLogLimit = 20

// HandleContainerLifecycle accepts container and pod start/stop/OOM events.
//
// One payload covers one object kind — containers OR pods OR tasks — named by
// ObjectKind, with the per-kind detail in a oneof on each event.
func (a *Server) HandleContainerLifecycle(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[contlcycle] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload contlcycle.EventsPayload
	if err := proto.Unmarshal(body, &payload); err != nil {
		log.Printf("[contlcycle] protobuf: %v (%d bytes)", err, len(body))
		return
	}

	if len(payload.GetEvents()) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a
		// payload meant for another endpoint decodes "successfully" into an
		// empty struct. Protobuf carries no type marker — the path is the
		// only contract — so an empty decode is the one signal we get.
		log.Printf("[contlcycle] decoded to zero events (%d bytes) — wrong payload type?", len(body))
		return
	}

	lcLogPayload(&payload)

	// The log above stops at containersLogLimit; the payload does not. Every
	// event the agent sent is still here, in the Datadog type, nothing
	// flattened or reformatted. The oneof is walked once more so each variant
	// stands on its own as a typed value: this is what storage will receive.
	for _, e := range payload.GetEvents() {
		switch {
		case e.GetContainer() != nil:
			container := e.GetContainer()
			_ = container // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case e.GetPod() != nil:
			pod := e.GetPod()
			_ = pod // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case e.GetTask() != nil:
			task := e.GetTask()
			_ = task // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		default:
			// No typed detail (nil oneof, or a variant newer than this
			// schema). The event itself, with its EventType, stays in
			// payload.Events; only the detail is missing.
		}
	}
	_ = &payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// lcLogPayload reports one lifecycle batch: the envelope, then one line per
// event up to containersLogLimit.
//
// Every nested message is reached through its getter. The agent fills these
// from several handlers (creation, termination, state transitions) and each
// leaves out what it does not know; a nil in the middle of the payload is
// normal, not an error.
func lcLogPayload(p *contlcycle.EventsPayload) {
	events := p.GetEvents()
	log.Printf("[contlcycle] version=%s host=%s cluster=%s kind=%s — %d events",
		p.GetVersion(), p.GetHost(), p.GetClusterId(), p.GetObjectKind(), len(events))

	for i, e := range events {
		if i == containersLogLimit {
			log.Printf("   ... and %d more", len(events)-containersLogLimit)
			break
		}
		switch {
		case e.GetContainer() != nil:
			lcLogContainer(e.GetEventType(), e.GetContainer())
		case e.GetPod() != nil:
			lcLogPod(e.GetEventType(), e.GetPod())
		case e.GetTask() != nil:
			lcLogTask(e.GetEventType(), e.GetTask())
		default:
			log.Printf("   %-10s %T", e.GetEventType(), e.GetTypedEvent())
		}
	}
}

// lcLogContainer reports one container event.
//
// The exit code is the point of this intake: OOM kill is 137 (128+SIGKILL),
// and Delete without a code means the runtime never reported one. Both are
// distinct from exit 0, so the oneof is read for presence, not value.
func lcLogContainer(kind contlcycle.Event_EventType, ev *contlcycle.ContainerEvent) {
	line := fmt.Sprintf("   %-10s container=%s name=%s source=%s exit=%s created=%s exited=%s",
		kind, ev.GetContainerID(), lcOrDash(ev.GetContainerName()), ev.GetSource(),
		lcExitCodeString(ev),
		lcOptionalTime(ev.GetOptionalCreationTimestamp() != nil, ev.GetCreationTimestamp()),
		lcOptionalTime(ev.GetOptionalExitTimestamp() != nil, ev.GetExitTimestamp()))
	if owner := ev.GetOwner(); owner != nil {
		line += fmt.Sprintf(" owner=%s/%s", owner.GetOwnerType(), owner.GetOwnerUID())
	}
	log.Print(line)

	if tr := ev.GetTransition(); tr != nil {
		log.Printf("              transition %s: %s -> %s at=%s precision=%s missed=%s",
			tr.GetContainerKind(),
			lcContainerState(tr.GetLastObservedState()),
			lcContainerState(tr.GetNewState()),
			lcOptionalTime(true, tr.GetTransitionTimestamp()),
			tr.GetPrecision(), tr.GetMissedIntermediate())
	}
}

// lcExitCode reads the exit code oneof. The second result is false when the
// sender left it out — which the getter alone cannot say, since GetExitCode
// returns 0 for both "exited cleanly" and "no code reported".
func lcExitCode(ev *contlcycle.ContainerEvent) (int32, bool) {
	if ev == nil || ev.GetOptionalExitCode() == nil {
		return 0, false
	}
	return ev.GetExitCode(), true
}

// lcExitCodeString formats the exit code for the log: "-" when absent, the
// code otherwise, with the signal spelled out for the 128+N convention.
func lcExitCodeString(ev *contlcycle.ContainerEvent) string {
	code, ok := lcExitCode(ev)
	if !ok {
		return "-"
	}
	s := strconv.Itoa(int(code))
	if code > 128 && code < 256 {
		s += "(signal " + strconv.Itoa(int(code-128)) + ")"
	}
	return s
}

// lcLogPod reports one pod event.
func lcLogPod(kind contlcycle.Event_EventType, ev *contlcycle.PodEvent) {
	log.Printf("   %-10s pod=%s source=%s created=%s exited=%s",
		kind, ev.GetPodUID(), ev.GetSource(),
		lcOptionalTime(ev.CreationTimestamp != nil, ev.GetCreationTimestamp()),
		lcOptionalTime(ev.ExitTimestamp != nil, ev.GetExitTimestamp()))

	if tr := ev.GetTransition(); tr != nil {
		log.Printf("              transition %s: %s -> %s at=%s precision=%s missed=%s",
			tr.GetField(),
			lcPodStatus(tr.GetLastObservedState()),
			lcPodStatus(tr.GetNewState()),
			lcOptionalTime(true, tr.GetTransitionTimestamp()),
			tr.GetPrecision(), tr.GetMissedIntermediate())
	}
}

// lcLogTask reports one ECS task event. The schema carries only the ARN,
// the source and an optional exit timestamp — no exit code at this level.
func lcLogTask(kind contlcycle.Event_EventType, ev *contlcycle.TaskEvent) {
	log.Printf("   %-10s task=%s source=%s exited=%s",
		kind, ev.GetTaskARN(), ev.GetSource(),
		lcOptionalTime(ev.ExitTimestamp != nil, ev.GetExitTimestamp()))
}

// lcContainerState formats a Kubernetes container state for a transition
// line: the kind, then reason/exit code/signal when the value carries them.
func lcContainerState(st *contlcycle.ContainerStateValue) string {
	if st == nil {
		return "-"
	}
	s := st.GetKind().String()
	if st.Reason != nil {
		s += "/" + st.GetReason()
	}
	if st.ExitCode != nil {
		s += " exit=" + strconv.Itoa(int(st.GetExitCode()))
	}
	if st.Signal != nil {
		s += " signal=" + strconv.Itoa(int(st.GetSignal()))
	}
	return s
}

// lcPodStatus formats a pod-level status value: either a phase string or a
// condition (type=status, with reason when given).
func lcPodStatus(st *contlcycle.PodStatusValue) string {
	if st == nil {
		return "-"
	}
	if cond := st.GetCondition(); cond != nil {
		s := cond.GetType() + "=" + cond.GetStatus()
		if cond.Reason != nil {
			s += "/" + cond.GetReason()
		}
		return s
	}
	return lcOrDash(st.GetPhase())
}

// lcOptionalTime formats a Unix-seconds timestamp for the log.
//
// Same rule as wireTime in payloads.go: absent or zero means "not supplied",
// never 1970. Here that is rendered as "-" rather than the receive time,
// because a log line is for reading, not for retention.
func lcOptionalTime(present bool, seconds int64) string {
	if !present || seconds <= 0 {
		return "-"
	}
	return time.Unix(seconds, 0).UTC().Format(time.RFC3339)
}

// HandleContainerImages accepts the container image inventory.
func (a *Server) HandleContainerImages(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[contimage] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload contimage.ContainerImagePayload
	if err := proto.Unmarshal(body, &payload); err != nil {
		log.Printf("[contimage] protobuf: %v (%d bytes)", err, len(body))
		return
	}

	if len(payload.GetImages()) == 0 {
		// See the note in HandleContainerLifecycle: an empty decode is the
		// only hint protobuf gives that the wrong message arrived.
		log.Printf("[contimage] decoded to zero images (%d bytes) — wrong payload type?", len(body))
		return
	}

	ciLogPayload(&payload)

	// Same as the lifecycle handler: the log is capped, the payload is not.
	// One variant only here — every image is a *contimage.ContainerImage
	// under payload.Images, layers and history included.
	_ = &payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ciLogPayload reports one image inventory: the envelope, then two lines per
// image up to containersLogLimit.
//
// The digest is the identity worth keeping. Tags move between builds; the
// digest is what the SBOM arriving on sbom-intake will reference, so it is
// the join key between the two tracks.
func ciLogPayload(p *contimage.ContainerImagePayload) {
	images := p.GetImages()
	log.Printf("[contimage] version=%s host=%s source=%s — %d images",
		p.GetVersion(), p.GetHost(), lcOrDash(p.GetSource()), len(images))

	for i, img := range images {
		if i == containersLogLimit {
			log.Printf("   ... and %d more", len(images)-containersLogLimit)
			break
		}
		ciLogImage(img)
	}
}

// ciLogImage reports one image: identity first, then platform and provenance.
func ciLogImage(img *contimage.ContainerImage) {
	log.Printf("   %s id=%s digest=%s registry=%s short=%s tags=%s repo_digests=%s",
		img.GetName(), lcOrDash(img.GetId()), lcOrDash(img.GetDigest()),
		lcOrDash(img.GetRegistry()), lcOrDash(img.GetShortName()),
		ciJoin(img.GetRepoTags()), ciJoin(img.GetRepoDigests()))

	layers := img.GetLayers()
	var layerBytes int64
	for _, l := range layers {
		layerBytes += l.GetSize()
	}
	log.Printf("      %d B os=%s layers=%d (%d B) built=%s published=%s dd_tags=%d",
		img.GetSize(), ciPlatform(img.GetOs()), len(layers), layerBytes,
		ciTime(img.GetBuiltAt()), ciTime(img.GetPublishedAt()), len(img.GetDdTags()))
}

// ciPlatform formats os/version/arch, skipping what the agent did not fill.
func ciPlatform(os *contimage.ContainerImage_OperatingSystem) string {
	if os == nil {
		return "-"
	}
	parts := make([]string, 0, 3)
	for _, s := range []string{os.GetName(), os.GetVersion(), os.GetArchitecture()} {
		if s != "" {
			parts = append(parts, s)
		}
	}
	if len(parts) == 0 {
		return "-"
	}
	return strings.Join(parts, "/")
}

// ciTime formats a protobuf Timestamp for the log.
//
// nil is "not supplied" — the agent sets BuiltAt from the newest layer
// history and often has no PublishedAt at all. Same rule as wireTime in
// payloads.go: a missing value never becomes 1970.
func ciTime(ts *timestamppb.Timestamp) string {
	if ts == nil || !ts.IsValid() || ts.GetSeconds() <= 0 {
		return "-"
	}
	return ts.AsTime().UTC().Format(time.RFC3339)
}

func ciJoin(s []string) string {
	if len(s) == 0 {
		return "-"
	}
	return strings.Join(s, ",")
}

func lcOrDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}
