package intake

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"io"
	"log"
	"mime"
	"mime/multipart"
	"net/http"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/apikeys"
	"github.com/itsninjacats/server/apps/storage"
)

// Server receives traffic from Datadog agents.
//
// It does not run itself: Handler returns a handler that the meta-process in
// apps/httpapi starts. That keeps the blocking where it belongs and keeps the
// server supervised by the Ergo tree.
//
// Routes live in routes.go, one block per Datadog intake. Handlers live in a
// file per signal: metrics.go, checks.go, logs.go, events.go, hosts.go,
// processes.go, traces.go.
type Server struct {
	Node  gen.Node
	Store *apikeys.Store
}

// isDiagnose recognises the agent's connectivity sweep at startup. Those
// requests carry an empty body or "{}", so there is nothing to decode.
func isDiagnose(c *gin.Context) bool {
	return c.GetHeader("X-Requested-With") == "datadog-agent-diagnose"
}

// splitTag splits one Datadog "key:value" tag. The ONLY place that knows how
// a tag string comes apart — every map builder below goes through it.
//
// Only the FIRST colon splits, because values legitimately contain colons
// ("url:http://x"). A tag without a colon keeps its key with an empty value —
// Datadog allows bare tags and dropping them silently would lose information.
func splitTag(t string) (key, value string) {
	key, value, _ = strings.Cut(t, ":")
	return key, value
}

// tagsToMultiMap turns Datadog's flat "key:value" tag list into a map of
// key -> every value seen, in arrival order.
//
// The value is a SLICE because tags are a multiset, not properties: nothing
// forbids two tags sharing a key, and real clusters emit them constantly
// (kube_service:a plus kube_service:b on a pod behind two services). The old
// map[string]string here kept whichever came last and silently dropped the
// rest — see docs/decisions/0001-tags-are-a-multiset.md for the damage that
// did. A bare tag appends an empty value, keeping its old meaning of
// "key present".
func tagsToMultiMap(tags []string) map[string][]string {
	if len(tags) == 0 {
		return nil
	}
	out := make(map[string][]string, len(tags))
	for _, t := range tags {
		k, v := splitTag(t)
		out[k] = append(out[k], v)
	}
	return out
}

// kvToMap is the single-valued cousin for maps that are NOT tags — Kubernetes
// labels and annotations, which travel in the same flat "key:value" form but
// whose keys the Kubernetes API guarantees unique. They stay plain maps in
// the schema, so the multiset shape would cost lookup speed for a collision
// that cannot happen.
func kvToMap(pairs []string) map[string]string {
	if len(pairs) == 0 {
		return nil
	}
	out := make(map[string]string, len(pairs))
	for _, p := range pairs {
		k, v := splitTag(p)
		out[k] = v
	}
	return out
}

// store hands a batch to the writer that owns it.
//
// Send is asynchronous: the agent gets its 202 immediately, and what happens
// to the batch afterwards belongs to apps/storage.
func (a *Server) store(writer gen.Atom, msg any, n int) {
	if a.Node == nil || n == 0 {
		return
	}
	if err := a.Node.Send(writer, msg); err != nil {
		log.Printf("[storage] cannot reach writer %s: %s", writer, err)
	}
}

// ---------------------------------------------------------------------------
// Trivial endpoints
// ---------------------------------------------------------------------------

func (a *Server) HandlePing(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"message": "pong"})
}

func (a *Server) HandleHealth(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"status": "ok"})
}

// flareIntake is the storeRaw label for /support/flare — one host serving
// every intake (engineAuth registers it on all of them), so its own label
// keeps its raw_payloads rows apart from whichever product actually owns the
// Host header a given upload arrived on.
const flareIntake = "flare"

// flareResponse mirrors the agent's own (unexported) flareResponse struct —
// $(go env GOMODCACHE)/github.com/!data!dog/datadog-agent@.../comp/core/flare/helpers/send_flare.go,
// analyzeResponse: `json.Unmarshal(b, &res)` against exactly these three
// keys, and it requires Content-Type: application/json on a 200 or reports
// "could not deserialize response body" regardless of the JSON underneath.
// omitempty is deliberately NOT used here (unlike the agent's own struct):
// Error is meaningful as an explicit empty string on the success path, not
// an absent key.
type flareResponse struct {
	CaseID      int64  `json:"case_id"`
	Error       string `json:"error"`
	RequestUUID string `json:"request_uuid"`
}

// flareKnownFields are the multipart field names getFlareReader in the
// agent's uploader writes that get their own column. Everything else —
// rc_task_uuid today, whatever a future agent version adds — lands in
// AgentFlareRow.Fields instead of being dropped.
var flareKnownFields = map[string]bool{
	"case_id": true, "email": true, "source": true,
	"agent_version": true, "hostname": true,
}

// HandleFlare answers HEAD and POST /support/flare, and the same two verbs on
// /support/flare/:case_id — registered once, on every engine (routes.go's
// engineAuth), because the agent's flare uploader talks to whichever host its
// dd_url (or the versioned "-flare.agent." rewrite, see the routes.go
// comment) resolves to, not to a dedicated one. mkURL in the agent's
// send_flare.go appends "/" + caseID to the URL whenever a flare is attached
// to an already-known case, so both path shapes reach this one handler.
//
// HEAD is resolveFlarePOSTURL's redirect probe in the agent's send_flare.go:
// it accepts either 200 or 404 as "reachable", so 200 with no body is enough
// and nothing is stored.
//
// POST must answer 200 with a JSON flareResponse: analyzeResponse in the
// agent parses it into the case id shown to the operator and stops retrying
// only once it gets one. A body that fails to parse still gets that
// contract — an empty case_id and a non-empty Error — because refusing the
// request would just make the agent retry the same unreadable upload.
func (a *Server) HandleFlare(c *gin.Context) {
	if c.Request.Method == http.MethodHead {
		c.Status(http.StatusOK)
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[flare] cannot read body: %v", err)
		c.JSON(http.StatusOK, flareResponse{Error: "cannot read request body", RequestUUID: uuid.NewString()})
		return
	}

	_, params, err := mime.ParseMediaType(c.GetHeader("Content-Type"))
	if err != nil || params["boundary"] == "" {
		log.Printf("[flare] not multipart: content-type=%q err=%v", c.GetHeader("Content-Type"), err)
		a.storeRaw(c, flareIntake, "decode_error", "content-type is not multipart/form-data", body)
		c.JSON(http.StatusOK, flareResponse{Error: "content-type is not multipart/form-data", RequestUUID: uuid.NewString()})
		return
	}

	// case_id, email, source, agent_version, hostname get their own column;
	// everything else (rc_task_uuid today) goes in fields. agent_version and
	// hostname are written AFTER the archive by the agent's own uploader, so
	// nothing here may assume field order.
	fields := map[string]string{}
	named := map[string]string{}
	var filename string
	var archive []byte

	mr := multipart.NewReader(bytes.NewReader(body), params["boundary"])
	for {
		part, err := mr.NextPart()
		if err == io.EOF {
			break
		}
		if err != nil {
			log.Printf("[flare] multipart: %v", err)
			break
		}
		name := part.FormName()
		data, err := io.ReadAll(part)
		part.Close()
		if err != nil {
			log.Printf("[flare] cannot read part %q: %v", name, err)
			continue
		}

		switch {
		case name == "flare_file":
			filename = part.FileName()
			archive = data
		case flareKnownFields[name]:
			named[name] = string(data)
		case name != "":
			fields[name] = string(data)
		}
	}

	if archive == nil {
		a.storeRaw(c, flareIntake, "decode_error", "no flare_file part in the multipart body", body)
		c.JSON(http.StatusOK, flareResponse{Error: "no flare archive in the request", RequestUUID: uuid.NewString()})
		return
	}

	// mkURL (send_flare.go) appends the case id to the PATH whenever a flare
	// is attached to an already-known support case; the multipart "case_id"
	// field carries the same value independently (getFlareReader writes both
	// when SendTo was called with one). The form field wins on the rare
	// chance the two ever disagree — it is the value the agent's own
	// analyzeResponse would echo back to the operator.
	caseIDStr := named["case_id"]
	if caseIDStr == "" {
		caseIDStr = c.Param("case_id")
	}

	reqUUID := uuid.NewString()
	caseID := flareCaseIDNumber(caseIDStr)

	if tenant := TenantFromContext(c); tenant != "" {
		a.store(storage.AgentFlaresWriter, storage.WriteAgentFlares{Flares: []storage.AgentFlareRow{{
			TenantID:     tenant,
			ReceivedAt:   time.Now().UTC(),
			Hostname:     named["hostname"],
			CaseID:       caseIDStr,
			Email:        named["email"],
			Source:       named["source"],
			AgentVersion: named["agent_version"],
			Filename:     filename,
			SizeBytes:    uint64(len(archive)),
			Fields:       fields,
			Archive:      string(archive),
		}}}, 1)
	}

	c.JSON(http.StatusOK, flareResponse{CaseID: caseID, Error: "", RequestUUID: reqUUID})
}

// flareCaseIDNumber turns the agent's case_id form field into the integer
// flareResponse.CaseID carries. A brand-new upload sends no case_id at all —
// getFlareReader only writes the field when non-empty — so a number is
// minted the same way HandleEvents (router_api.go) mints an event id: the
// low 63 bits of a fresh UUID, which stays inside int64 and carries no
// meaning beyond "unique".
func flareCaseIDNumber(caseID string) int64 {
	if n, err := strconv.ParseInt(caseID, 10, 64); err == nil {
		return n
	}
	id := uuid.New()
	return int64(binary.BigEndian.Uint64(id[:8]) >> 1)
}

// HandleUnknown catches endpoints we do not know yet and logs them loudly —
// this is how the whole route list was discovered. Set NINJACAT_ACK_UNKNOWN=true
// to answer 202 instead of 404, which keeps a chatty agent quiet while you look.
func (a *Server) HandleUnknown(c *gin.Context) {
	log.Printf("UNHANDLED ENDPOINT: %s %s (Content-Type: %s, User-Agent: %s)",
		c.Request.Method, c.Request.URL.Path,
		c.GetHeader("Content-Type"), c.GetHeader("User-Agent"))

	if os.Getenv("NINJACAT_ACK_UNKNOWN") == "true" {
		c.JSON(http.StatusAccepted, gin.H{})
		return
	}
	c.JSON(http.StatusNotFound, gin.H{"errors": []string{"unknown endpoint"}})
}

// describe logs what an unparsed payload looks like.
//
// Used by intakes we receive but do not store yet: enough to confirm the wire
// format and see the shape, without guessing a schema. For JSON it prints the
// top-level keys; for anything else, the size and first bytes.
func describe(label, contentType string, body []byte) {
	if len(body) == 0 {
		log.Printf("[%s] empty body", label)
		return
	}

	if strings.Contains(contentType, "json") || body[0] == '{' || body[0] == '[' {
		var top map[string]json.RawMessage
		if json.Unmarshal(body, &top) == nil {
			keys := make([]string, 0, len(top))
			for k := range top {
				keys = append(keys, k)
			}
			sort.Strings(keys)
			log.Printf("[%s] JSON %d B, keys: %s", label, len(body), strings.Join(keys, ", "))
			return
		}
		var arr []json.RawMessage
		if json.Unmarshal(body, &arr) == nil {
			log.Printf("[%s] JSON array %d B, %d entries", label, len(body), len(arr))
			return
		}
	}

	n := min(len(body), 24)
	log.Printf("[%s] %s %d B, first bytes: %x", label, orUnknown(contentType), len(body), body[:n])
}

func orUnknown(s string) string {
	if s == "" {
		return "(no content-type)"
	}
	return s
}
