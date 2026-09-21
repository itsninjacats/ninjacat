package intake

import (
	"encoding/json"
	"log"
	"net/http"
	"os"
	"sort"
	"strings"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
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

func (a *Server) HandleFlare(c *gin.Context) {
	c.Status(http.StatusOK)
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
