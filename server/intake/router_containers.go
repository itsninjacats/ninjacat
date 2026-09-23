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

	"github.com/itsninjacats/server/apps/storage"
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
// Lifecycle events land in ninjacat.container_events, the image inventory in
// ninjacat.container_images — both in full: every field of both payloads has
// a column, including the structured state transitions and the per-layer
// build history. A body that does not decode, or that decodes to nothing,
// goes to raw_payloads instead of disappearing.
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

	var payload contlcycle.EventsPayload
	if err := proto.Unmarshal(body, &payload); err != nil {
		log.Printf("[contlcycle] protobuf: %v (%d bytes)", err, len(body))
		a.storeRaw(c, "contlcycle", "decode_error", err.Error(), body)
		return
	}

	if len(payload.GetEvents()) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a
		// payload meant for another endpoint decodes "successfully" into an
		// empty struct. Protobuf carries no type marker — the path is the
		// only contract — so an empty decode is the one signal we get, and
		// the bytes are exactly what a future decoder needs.
		log.Printf("[contlcycle] decoded to zero events (%d bytes) — wrong payload type?", len(body))
		a.storeRaw(c, "contlcycle", "unexpected_shape",
			"EventsPayload decoded with no events", body)
		return
	}

	lcLogPayload(&payload)

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// The log above stops at containersLogLimit; the rows do not. Storage is
	// fire-and-forget — the 202 is already deferred and never waits on it.
	rows := lcRows(&payload, tenant, time.Now().UTC())
	a.store(storage.ContainerEventsWriter, storage.WriteContainerEvents{Events: rows}, len(rows))
}

// lcRows converts one lifecycle payload into container_events rows.
//
// The envelope (host, cluster, object kind) repeats on every row so a row is
// self-sufficient at query time; the per-kind detail comes from the oneof, and
// each variant fills only its own identity column — a pod row has an empty
// ContainerID, not a borrowed one.
//
// now is the row timestamp: the payload carries no "when was this batch sent",
// only the per-event creation/exit/transition times, which keep their own
// Nullable columns. Passed in rather than read here so tests can pin it.
func lcRows(p *contlcycle.EventsPayload, tenant string, now time.Time) []storage.ContainerEventRow {
	events := p.GetEvents()
	rows := make([]storage.ContainerEventRow, 0, len(events))
	for _, e := range events {
		row := storage.ContainerEventRow{
			TenantID:   tenant,
			Timestamp:  now,
			Host:       p.GetHost(),
			ClusterID:  p.GetClusterId(),
			ObjectKind: p.GetObjectKind().String(),
			EventType:  e.GetEventType().String(),
			// The payload's own schema version, and which arm of the oneof
			// this row came from. Without the variant an unknown arm looks
			// exactly like a typed event whose fields happened to be empty.
			PayloadVersion: p.GetVersion(),
			EventVariant:   "unknown",
		}
		switch {
		case e.GetContainer() != nil:
			ev := e.GetContainer()
			row.EventVariant = "container"
			row.ContainerID = ev.GetContainerID()
			row.ContainerName = ev.GetContainerName()
			// ContainerName is a proto3 optional: "never set" and "set to
			// empty" are different wire states that the getter collapses.
			// The column cannot become Nullable in place, so presence
			// travels beside it. See 0015_k8s_fidelity.sql.
			if ev.ContainerName != nil {
				row.ContainerNamePresent = 1
			}
			row.Source = ev.GetSource()
			// The oneof is read for presence, not value: nil means the
			// runtime never reported a code, and that must reach ClickHouse
			// as NULL, distinct from an honest exit 0. See lcExitCode.
			if code, ok := lcExitCode(ev); ok {
				row.ExitCode = &code
			}
			row.CreatedAt = lcTimePtr(ev.GetOptionalCreationTimestamp() != nil, ev.GetCreationTimestamp())
			row.ExitedAt = lcTimePtr(ev.GetOptionalExitTimestamp() != nil, ev.GetExitTimestamp())
			if owner := ev.GetOwner(); owner != nil {
				row.OwnerType = owner.GetOwnerType().String()
				row.OwnerUID = owner.GetOwnerUID()
			}
			if tr := ev.GetTransition(); tr != nil {
				// Same formatter as the log line, so the stored state and
				// the logged state cannot drift apart. The structured
				// columns beside them are what a query needs — "every OOM
				// kill with signal 9" cannot be asked of a formatted string.
				row.OldState = lcContainerState(tr.GetLastObservedState())
				row.NewState = lcContainerState(tr.GetNewState())
				row.TransitionAt = lcTimePtr(true, tr.GetTransitionTimestamp())
				row.ContainerKind = tr.GetContainerKind().String()
				row.Precision = tr.GetPrecision().String()
				row.MissedIntermediate = tr.GetMissedIntermediate().String()
				row.OldStateKind, row.OldReason, row.OldExitCode, row.OldSignal =
					lcContainerStateFields(tr.GetLastObservedState())
				row.NewStateKind, row.NewReason, row.NewExitCode, row.NewSignal =
					lcContainerStateFields(tr.GetNewState())
			}
		case e.GetPod() != nil:
			ev := e.GetPod()
			row.EventVariant = "pod"
			row.PodUID = ev.GetPodUID()
			row.Source = ev.GetSource()
			row.CreatedAt = lcTimePtr(ev.CreationTimestamp != nil, ev.GetCreationTimestamp())
			row.ExitedAt = lcTimePtr(ev.ExitTimestamp != nil, ev.GetExitTimestamp())
			if tr := ev.GetTransition(); tr != nil {
				row.OldState = lcPodStatus(tr.GetLastObservedState())
				row.NewState = lcPodStatus(tr.GetNewState())
				row.TransitionAt = lcTimePtr(true, tr.GetTransitionTimestamp())
				row.PodStatusField = tr.GetField().String()
				row.Precision = tr.GetPrecision().String()
				row.MissedIntermediate = tr.GetMissedIntermediate().String()
				was := lcPodStatusFields(tr.GetLastObservedState())
				became := lcPodStatusFields(tr.GetNewState())
				row.OldStateVariant, row.OldPhase = was.variant, was.phase
				row.OldConditionType, row.OldConditionStatus = was.condType, was.condStatus
				row.OldConditionReason, row.OldConditionMessage = was.condReason, was.condMessage
				row.NewStateVariant, row.NewPhase = became.variant, became.phase
				row.NewConditionType, row.NewConditionStatus = became.condType, became.condStatus
				row.NewConditionReason, row.NewConditionMessage = became.condReason, became.condMessage
			}
		case e.GetTask() != nil:
			ev := e.GetTask()
			row.EventVariant = "task"
			row.TaskARN = ev.GetTaskARN()
			row.Source = ev.GetSource()
			row.ExitedAt = lcTimePtr(ev.ExitTimestamp != nil, ev.GetExitTimestamp())
		default:
			// No typed detail (nil oneof, or a variant newer than this
			// schema). The EventType is still real information — a Delete
			// with no detail is still a Delete — so the row stands with
			// the envelope and event type alone.
		}
		rows = append(rows, row)
	}
	return rows
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

// lcContainerStateFields reads a container state value into the four columns
// that hold it structurally: the kind (an enum, so always present when there
// is a state at all) and the three proto3 optionals beside it.
//
// The pointers are returned as they arrive. Reason, ExitCode and Signal are
// optional on purpose upstream — TERMINATED carries a code and a signal,
// RUNNING carries neither — and collapsing "not reported" into "" or 0 is
// exactly the distinction that made the old flattened string unqueryable.
func lcContainerStateFields(st *contlcycle.ContainerStateValue) (kind string, reason *string, exitCode, signal *int32) {
	if st == nil {
		return "", nil, nil, nil
	}
	return st.GetKind().String(), st.Reason, st.ExitCode, st.Signal
}

// lcPodStatusValue is one arm of the PodStatusValue oneof, unpacked.
//
// variant says which arm was populated ("phase", "condition", or "" for a nil
// value), because the two arms use different columns and a reader has to know
// which are meaningful. Everything else is a pointer: a phase row has no
// condition and vice versa, and NULL says so without inventing an empty
// string that would sort next to a real one.
type lcPodStatusValue struct {
	variant                                       string
	phase                                         *string
	condType, condStatus, condReason, condMessage *string
}

// lcPodStatusFields unpacks a pod status value.
//
// Message is the field this used to lose entirely: lcPodStatus never read it,
// so "PodScheduled=False" survived and "0/3 nodes are available: insufficient
// memory" did not — which is the half that says what to do about it.
func lcPodStatusFields(st *contlcycle.PodStatusValue) lcPodStatusValue {
	if st == nil {
		return lcPodStatusValue{}
	}
	if cond := st.GetCondition(); cond != nil {
		t, s := cond.GetType(), cond.GetStatus()
		return lcPodStatusValue{
			variant:    "condition",
			condType:   &t,
			condStatus: &s,
			// Reason and Message are proto3 optionals; pass the pointers
			// through rather than the getters so absent stays absent.
			condReason:  cond.Reason,
			condMessage: cond.Message,
		}
	}
	// The other arm. A PodStatusValue whose oneof is unset reaches here too,
	// which is why the phase arm is only claimed when the wrapper is really
	// the phase one.
	if _, ok := st.GetValue().(*contlcycle.PodStatusValue_Phase); !ok {
		return lcPodStatusValue{}
	}
	phase := st.GetPhase()
	return lcPodStatusValue{variant: "phase", phase: &phase}
}

// lcTimePtr converts a Unix-seconds timestamp off the wire into a value for a
// Nullable(DateTime) column.
//
// Same rule as wireTime in payloads.go: absent or zero means "not supplied",
// never 1970. Unlike wireTime there is no fall-back to the receive time
// either — the row's own Timestamp already records arrival, and inventing an
// exit time would turn "we do not know when it exited" into a lie.
func lcTimePtr(present bool, seconds int64) *time.Time {
	if !present || seconds <= 0 {
		return nil
	}
	t := time.Unix(seconds, 0).UTC()
	return &t
}

// lcOptionalTime formats a Unix-seconds timestamp for the log: "-" when
// lcTimePtr says absent, so the log and the stored value share one presence
// rule and cannot drift apart.
func lcOptionalTime(present bool, seconds int64) string {
	t := lcTimePtr(present, seconds)
	if t == nil {
		return "-"
	}
	return t.Format(time.RFC3339)
}

// HandleContainerImages accepts the container image inventory.
func (a *Server) HandleContainerImages(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[contimage] cannot read body: %v", err)
		return
	}
	var payload contimage.ContainerImagePayload
	if err := proto.Unmarshal(body, &payload); err != nil {
		log.Printf("[contimage] protobuf: %v (%d bytes)", err, len(body))
		a.storeRaw(c, "contimage", "decode_error", err.Error(), body)
		return
	}

	if len(payload.GetImages()) == 0 {
		// See the note in HandleContainerLifecycle: an empty decode is the
		// only hint protobuf gives that the wrong message arrived.
		log.Printf("[contimage] decoded to zero images (%d bytes) — wrong payload type?", len(body))
		a.storeRaw(c, "contimage", "unexpected_shape",
			"ContainerImagePayload decoded with no images", body)
		return
	}

	ciLogPayload(&payload)

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// Same as the lifecycle handler: the log is capped, the rows are not, and
	// storage is fire-and-forget behind the already-deferred 202.
	rows, skipped := ciRows(&payload, tenant, time.Now().UTC())
	if skipped > 0 {
		log.Printf("[contimage] %d of %d images skipped: neither digest nor image id, so nothing to key on", skipped, len(payload.GetImages()))
		// A log line records this once and is read never. The metric is what
		// makes an incomplete inventory noticeable without going looking.
		a.countSelf(tenant, payload.GetHost(), SelfImagesSkipped, float64(skipped),
			map[string][]string{"reason": {"no_identity"}})
	}
	a.store(storage.ContainerImagesWriter, storage.WriteContainerImages{Images: rows}, len(rows))
}

// ciRows converts one image inventory into container_images rows, returning
// the rows and how many images had no usable identity at all.
//
// The table replaces on (tenant, digest, host): the digest IS the identity —
// it is what the SBOM track will join on — so an image without one has
// nowhere to land and is skipped rather than stored under an empty key,
// where every digestless image on a host would collapse into one row.
//
// now is the sighting time; the payload does not date itself. The image's own
// provenance times (BuiltAt, PublishedAt) keep their Nullable columns.
func ciRows(p *contimage.ContainerImagePayload, tenant string, now time.Time) (rows []storage.ContainerImageRow, skipped int) {
	images := p.GetImages()
	rows = make([]storage.ContainerImageRow, 0, len(images))
	for _, img := range images {
		// A missing digest is a normal state, not a defect — an image built
		// locally and never pushed, loaded from a tarball, or pulled moments
		// ago has none. Dropping those would make the inventory quietly
		// incomplete, so the image id stands in: it is the config digest,
		// content-addressed the same way, just local to the runtime rather
		// than assigned by a registry.
		//
		// Only an image with neither is genuinely unidentifiable. Keying such
		// rows on the empty string would collapse them all into one, which
		// loses more than skipping does.
		key, source := img.GetDigest(), "digest"
		if key == "" {
			key, source = img.GetId(), "image_id"
		}
		if key == "" {
			skipped++
			continue
		}
		// Same sum as ciLogImage, so the stored total and the logged total
		// cannot disagree.
		layers := img.GetLayers()
		var layerBytes int64
		for _, l := range layers {
			layerBytes += l.GetSize()
		}

		// The layer stack, as parallel arrays in wire order. An image IS its
		// ordered layer list, so nothing here is sorted or deduplicated, and
		// the inner URL slice is filled even when empty — the driver rejects
		// a nil inside an Array(Array(String)).
		var (
			mediaTypes  = make([]string, 0, len(layers))
			digests     = make([]string, 0, len(layers))
			sizes       = make([]int64, 0, len(layers))
			urls        = make([][]string, 0, len(layers))
			hCreated    = make([]*time.Time, 0, len(layers))
			hCreatedBy  = make([]string, 0, len(layers))
			hAuthor     = make([]string, 0, len(layers))
			hComment    = make([]string, 0, len(layers))
			hEmptyLayer = make([]uint8, 0, len(layers))
		)
		for _, l := range layers {
			mediaTypes = append(mediaTypes, l.GetMediaType())
			digests = append(digests, l.GetDigest())
			sizes = append(sizes, l.GetSize())
			u := l.GetUrls()
			if u == nil {
				u = []string{}
			}
			urls = append(urls, u)
			h := l.GetHistory()
			hCreated = append(hCreated, ciTimePtr(h.GetCreated()))
			hCreatedBy = append(hCreatedBy, h.GetCreatedBy())
			hAuthor = append(hAuthor, h.GetAuthor())
			hComment = append(hComment, h.GetComment())
			hEmptyLayer = append(hEmptyLayer, boolToUint8(h.GetEmptyLayer()))
		}

		// size_bytes is unsigned and the wire field is signed. A negative
		// size cannot happen on a healthy agent and wraps to ~18 exabytes if
		// cast blindly, poisoning every SUM over the table — so it is
		// clamped and the clamp is recorded. Same guard orchVersionSpread
		// already applies to the cluster version counts.
		size, negative := img.GetSize(), uint8(0)
		if size < 0 {
			size, negative = 0, 1
		}

		rows = append(rows, storage.ContainerImageRow{
			TenantID:    tenant,
			CollectedAt: now,
			Host:        p.GetHost(),

			ImageKey:       key,
			IdentitySource: source,

			ImageID:      img.GetId(),
			Digest:       img.GetDigest(),
			Name:         img.GetName(),
			ShortName:    img.GetShortName(),
			Registry:     img.GetRegistry(),
			RepoTags:     img.GetRepoTags(),
			RepoDigests:  img.GetRepoDigests(),
			SizeBytes:    uint64(size),
			OSName:       img.GetOs().GetName(),
			OSVersion:    img.GetOs().GetVersion(),
			Architecture: img.GetOs().GetArchitecture(),
			LayerCount:   uint32(len(layers)),
			LayerBytes:   uint64(layerBytes),
			BuiltAt:      ciTimePtr(img.GetBuiltAt()),
			PublishedAt:  ciTimePtr(img.GetPublishedAt()),
			DDTags:       tagsToMultiMap(img.GetDdTags()),

			// Which pipeline produced this inventory, kept as the pointer it
			// arrives as: "the sender did not say" is a real answer.
			PayloadVersion: p.GetVersion(),
			Source:         p.Source,

			LayerMediaTypes:        mediaTypes,
			LayerDigests:           digests,
			LayerSizes:             sizes,
			LayerURLs:              urls,
			LayerHistoryCreated:    hCreated,
			LayerHistoryCreatedBy:  hCreatedBy,
			LayerHistoryAuthor:     hAuthor,
			LayerHistoryComment:    hComment,
			LayerHistoryEmptyLayer: hEmptyLayer,

			SizeNegative: negative,
		})
	}
	return rows, skipped
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

// ciTimePtr converts a protobuf Timestamp into a value for a
// Nullable(DateTime) column.
//
// nil is "not supplied" — the agent sets BuiltAt from the newest layer
// history and often has no PublishedAt at all. Same rule as wireTime in
// payloads.go: a missing value never becomes 1970, and no receive-time
// fall-back either — CollectedAt records arrival, and a fabricated build
// date would poison the provenance record.
func ciTimePtr(ts *timestamppb.Timestamp) *time.Time {
	if ts == nil || !ts.IsValid() || ts.GetSeconds() <= 0 {
		return nil
	}
	t := ts.AsTime().UTC()
	return &t
}

// ciTime formats a protobuf Timestamp for the log: "-" when ciTimePtr says
// absent, so the log and the stored value share one presence rule.
func ciTime(ts *timestamppb.Timestamp) string {
	t := ciTimePtr(ts)
	if t == nil {
		return "-"
	}
	return t.Format(time.RFC3339)
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

// boolToUint8 renders a wire bool for a UInt8 column. ClickHouse has Bool,
// but every flag column in this schema is UInt8 and consistency is worth more
// than the byte.
func boolToUint8(b bool) uint8 {
	if b {
		return 1
	}
	return 0
}
