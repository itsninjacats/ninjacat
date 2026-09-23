package intake

import (
	"encoding/json"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/itsninjacats/server/apps/storage"
)

// webhook-intake.<site> and intake.synthetics.<site> — two hosts that have
// nothing to do with each other beyond arriving in the same change.
//
//	webhook-intake.<site>     POST /api/v2/webhook, /api/v2/webhook/
//	intake.synthetics.<site>  GET  /api/unstable/synthetics/agents/tests
//
// ---------------------------------------------------------------------------
// webhook-intake.<site> — CI provider pipeline events
//
// Datadog's one generic "CI provider webhook" intake. The Jenkins plugin
// posts here (DatadogApiClient.java: webhook-intake.<site>/api/v2/webhook/
// with ?service=<ci instance name>, headers DD-API-KEY and
// DD-CI-PROVIDER-NAME: jenkins, Content-Encoding: gzip, Content-Type:
// application/json), and Datadog's own GitLab documentation gives the
// identical URL for GitLab's legacy webhook integration — same intake, a
// different provider name.
//
// The body is a JSON ARRAY of payload objects batched to 5 MB uncompressed
// (BatchSender.java:48-88). One batch MIXES levels: every element carries
// "level": "pipeline" | "stage" | "job", and that field — not the URL, not a
// header — is what tells them apart. Gzip is unwrapped by the Decompress
// middleware before this handler runs, like everywhere else in this package.
//
// Both path spellings are registered rather than relying on Gin's trailing-
// slash redirect: the plugin's URL ends in a slash, a 301 on a POST drops the
// body, and a dropped body here is a CI run that never appears.
//
// UNVERIFIED — GitLab. Datadog's docs give the URL and say the secret token
// is left empty, but GitLab's own emitter lives in gitlab-org/gitlab and was
// not read, so whether GitLab sends Datadog's level-shaped schema or its own
// native webhook JSON (object_kind, project, builds…) is not confirmed. The
// decoder below reads both: it looks for "level" and falls back to
// "object_kind", keeps whatever hot fields match, and stores the element
// verbatim regardless. That is the same posture router_evp.go takes for
// tracks with no published type.
// ---------------------------------------------------------------------------

func (a *Server) routeCIWebhook(g *gin.RouterGroup) {
	g.POST("/api/v2/webhook", a.HandleCIWebhook)
	g.POST("/api/v2/webhook/", a.HandleCIWebhook)
}

// civWebhookHeaders is the allowlist of request headers kept on a webhook
// row. Same rule as intake/raw.go's: an allowlist, never a blocklist, so a
// table with a 90-day TTL cannot become a place a credential is durable.
// DD-API-KEY and Authorization are deliberately absent.
var civWebhookHeaders = []string{
	"User-Agent",
	"Dd-Ci-Provider-Name",
	"Dd-Evp-Origin",
	"Dd-Evp-Origin-Version",
	"X-Gitlab-Event",
	"X-Gitlab-Event-Uuid",
	"X-Gitlab-Instance",
	"X-Github-Event",
	"X-Github-Delivery",
	"X-Github-Hook-Id",
	"X-Jenkins",
	"Via",
}

// civWebhookKnownKeys are the top-level keys of a Datadog CI webhook element
// this decoder names; anything else lands in the row's extra map.
var civWebhookKnownKeys = map[string]bool{
	"payload_version": true, "level": true, "url": true, "start": true, "end": true,
	"partial_retry": true, "queue_time": true, "status": true, "is_manual": true,
	"trace_id": true, "span_id": true, "parent_span_id": true, "pipeline_id": true,
	"unique_id": true, "name": true, "id": true, "user": true, "parameters": true,
	"tags": true, "node": true, "git": true, "parent_pipeline": true,
	"pipeline_unique_id": true, "pipeline_name": true, "stage_id": true,
	"stage_name": true, "parent_stage_id": true, "error": true,
}

// HandleCIWebhook accepts a batch of CI pipeline, stage and job events.
//
// The Jenkins plugin has no documented success body — its HTTP client only
// distinguishes 2xx from everything else for its circuit breaker — so 202
// with an empty object, the same answer every other intake in this package
// gives, is the whole contract.
func (a *Server) HandleCIWebhook(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[ciwebhook] cannot read body: %v", err)
		return
	}
	if len(body) == 0 {
		log.Printf("[ciwebhook] empty body — probe?")
		return
	}

	// A batch is an array; a provider that skipped the batcher sends one
	// object. decodeJSONList accepts both, the same way every event-platform
	// track in router_evp.go does.
	items, err := decodeJSONList[json.RawMessage](body)
	if err != nil {
		log.Printf("[ciwebhook] json: %v (%d bytes)", err, len(body))
		describe("ciwebhook", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "webhook", "decode_error", "batch: "+err.Error(), body)
		return
	}

	// DD-CI-PROVIDER-NAME is what Jenkins sets. It is only ever a default
	// here: an element that names its own provider wins, and neither is
	// guessed into a real provider name — "unknown" stays "unknown".
	headerProvider := strings.ToLower(strings.TrimSpace(c.GetHeader("Dd-Ci-Provider-Name")))
	// The ?service= the Jenkins plugin appends is its CI instance name.
	service := c.Query("service")
	delivery := civFirstNonEmpty(
		c.GetHeader("X-Gitlab-Event-Uuid"),
		c.GetHeader("X-Github-Delivery"),
		c.GetHeader("Dd-Request-Id"),
	)

	headers := make(map[string]string, len(civWebhookHeaders))
	for _, h := range civWebhookHeaders {
		if v := c.GetHeader(h); v != "" {
			headers[h] = v
		}
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()

	log.Printf("[ciwebhook] provider=%s service=%s %d elements (%d bytes)",
		orDash(headerProvider), orDash(service), len(items), len(body))

	rows := make([]storage.CIWebhookEventRow, 0, len(items))
	levels := map[string]int{}
	for i, raw := range items {
		v, err := civDecodeJSON(raw)
		if err != nil {
			a.storeRaw(c, "webhook", "decode_error", "element is not JSON: "+err.Error(), raw)
			continue
		}
		element := civMap(v)
		if element == nil {
			a.storeRaw(c, "webhook", "unexpected_shape", "element is not a JSON object", raw)
			continue
		}

		row := civWebhookRow(element, headerProvider)
		row.Service = service
		row.DeliveryID = delivery
		row.Headers = headers
		row.Body = string(raw)
		levels[orDash(row.Level)]++

		if i < civCycleLogLimit {
			log.Printf("   %s/%s %s status=%s pipeline=%s name=%q %s..%s",
				orDash(row.Provider), orDash(row.Level), orDash(row.UniqueID),
				orDash(row.Status), orDash(row.PipelineUniqueID), row.Name,
				orDash(row.StartRaw), orDash(row.EndRaw))
		} else if i == civCycleLogLimit {
			log.Printf("   ... %d more elements", len(items)-i)
		}

		if tenant == "" {
			continue
		}
		row.TenantID = tenant
		row.ReceivedAt = now
		rows = append(rows, row)
	}
	log.Printf("[ciwebhook] by level: %s", civCounts(levels))

	if tenant == "" {
		return
	}
	a.store(storage.CIWebhookEventsWriter, storage.WriteCIWebhookEvents{Events: rows}, len(rows))
}

// civWebhookRow converts one element of a webhook batch.
//
// Every id is read as a STRING even where Jenkins sends a decimal number:
// the same fields carry UUIDs from other providers, and a UInt64 column
// would turn those into 0 without anybody noticing. civString renders a
// json.Number by its exact literal, so a numeric id keeps every digit.
func civWebhookRow(element map[string]any, headerProvider string) storage.CIWebhookEventRow {
	node := civMap(element["node"])
	git := civMap(element["git"])
	user := civMap(element["user"])
	errObj := civMap(element["error"])
	parentPipeline := civMap(element["parent_pipeline"])

	startRaw := civFieldString(element, "start")
	endRaw := civFieldString(element, "end")

	row := storage.CIWebhookEventRow{
		Provider: civWebhookProvider(element, headerProvider),
		Level:    civWebhookLevel(element),

		PayloadVersion: civInt64Ptr(civField(element, "payload_version")),
		PartialRetry:   civJSONBoolPtr(civField(element, "partial_retry")),
		IsManual:       civJSONBoolPtr(civField(element, "is_manual")),

		TraceID:          civFieldString(element, "trace_id"),
		SpanID:           civFieldString(element, "span_id"),
		ParentSpanID:     civFieldString(element, "parent_span_id"),
		ID:               civFieldString(element, "id"),
		UniqueID:         civFieldString(element, "unique_id"),
		PipelineID:       civFieldString(element, "pipeline_id"),
		PipelineUniqueID: civFieldString(element, "pipeline_unique_id"),
		PipelineName:     civFieldString(element, "pipeline_name"),
		StageID:          civFieldString(element, "stage_id"),
		StageName:        civFieldString(element, "stage_name"),
		ParentStageID:    civFieldString(element, "parent_stage_id"),

		Name:   civFieldString(element, "name"),
		URL:    civFieldString(element, "url"),
		Status: civFieldString(element, "status"),

		// The raw string is kept next to the parse: a format we do not
		// recognise loses the index, never the value, and an absent
		// timestamp stays NULL rather than becoming 1970.
		StartRaw:    startRaw,
		StartParsed: evpParseTime(startRaw),
		EndRaw:      endRaw,
		EndParsed:   evpParseTime(endRaw),
		QueueTimeMs: civInt64Ptr(civField(element, "queue_time")),

		NodeName:      civFieldString(node, "name"),
		NodeHostname:  civFieldString(node, "hostname"),
		NodeWorkspace: civFieldString(node, "workspace"),
		NodeLabels:    civStrings(civField(node, "labels")),

		GitRepositoryURL:  civFieldString(git, "repository_url"),
		GitDefaultBranch:  civFieldString(git, "default_branch"),
		GitBranch:         civFieldString(git, "branch"),
		GitSHA:            civFieldString(git, "sha"),
		GitTag:            civFieldString(git, "tag"),
		GitMessage:        civFieldString(git, "message"),
		GitAuthorName:     civFieldString(git, "author_name"),
		GitAuthorEmail:    civFieldString(git, "author_email"),
		GitAuthorTime:     civFieldString(git, "author_time"),
		GitCommitterName:  civFieldString(git, "committer_name"),
		GitCommitterEmail: civFieldString(git, "committer_email"),
		GitCommitTime:     civFieldString(git, "commit_time"),

		UserName:  civFieldString(user, "name"),
		UserEmail: civFieldString(user, "email"),

		ErrorMessage: civFieldString(errObj, "message"),
		ErrorType:    civFieldString(errObj, "type"),
		ErrorDomain:  civFieldString(errObj, "domain"),
		ErrorStack:   civFieldString(errObj, "stack"),

		ParentPipelineTraceID: civFieldString(parentPipeline, "trace_id"),
		ParentPipelineURL:     civFieldString(parentPipeline, "url"),

		Parameters: civStringMap(element["parameters"]),

		// tags is an array of "key:value" strings and is a genuine Datadog
		// tag multiset — the Jenkins plugin repeats keys for per-configuration
		// axes, and a plain map would keep the last value and drop the rest.
		// See docs/decisions/0001-tags-are-a-multiset.md.
		Tags: tagsToMultiMap(civStrings(element["tags"])),

		Extra: civUnknown(element, civWebhookKnownKeys),
	}
	return row
}

// civWebhookProvider decides which CI system sent an element.
//
// Order: what the element says about itself, then the DD-CI-PROVIDER-NAME
// header, then a sniff for GitLab's native webhook shape, then "unknown".
// Never a guess that produces a real provider name — a row tagged "jenkins"
// that came from somewhere else is worse than one tagged "unknown".
func civWebhookProvider(element map[string]any, headerProvider string) string {
	if p := civFieldString(element, "provider"); p != "" {
		return strings.ToLower(p)
	}
	if headerProvider != "" {
		return headerProvider
	}
	// GitLab's own webhooks carry object_kind and, for pipeline hooks, a
	// "project" object. Neither appears in Datadog's level-shaped schema.
	if _, ok := element["object_kind"]; ok {
		return "gitlab"
	}
	return "unknown"
}

// civWebhookLevel reads the discriminator: Datadog's "level" first, then
// GitLab's native "object_kind".
func civWebhookLevel(element map[string]any) string {
	if l := civFieldString(element, "level"); l != "" {
		return l
	}
	return civFieldString(element, "object_kind")
}

// civJSONBoolPtr reads a JSON bool as *uint8 for a Nullable(UInt8) column:
// nil when the key is absent or not a bool, so "the provider never said"
// stays distinct from "false".
func civJSONBoolPtr(v any) *uint8 {
	b, ok := v.(bool)
	if !ok {
		return nil
	}
	u := boolToUint8(b)
	return &u
}

// ---------------------------------------------------------------------------
// intake.synthetics.<site> — the agent's synthetics test poller
//
//	config: synthetics.collector.enabled
//
// This is the one GET in the whole intake whose answer DRIVES an agent loop
// rather than acknowledging something it sent. datadog-agent's
// comp/syntheticstestscheduler/impl/testpoller.go builds the URL itself —
//
//	"https://intake.synthetics." + site + "/api/unstable/synthetics/agents/tests"
//
// — appends ?agent_hostname=<hostname>&agent_version=<version>, sets
// DD-API-KEY, and polls it every two seconds. Anything other than 200 counts
// as a failure, and five consecutive failures flip the poller unhealthy and
// hand scheduling to an in-memory fallback.
//
// THE RESPONSE SHAPE IS UNFORGIVING. The body is unmarshalled into
// `struct{ Tests []common.SyntheticsTestConfig }` and
// SyntheticsTestConfig has a custom UnmarshalJSON that switches on the
// top-level "subtype" to choose the type of config.request — an unrecognised
// subtype returns an error, and because it is one Decode call over the whole
// body, that error discards EVERY test in the response, not just the bad one.
// So the only safe way to add a test here is to be certain its subtype is
// one of "UDP", "TCP" or "ICMP".
//
// The results of whatever the agent runs come back to http-synthetics.<site>
// and land in synthetics_results (router_evp.go's HandleSynthetics), which
// already works. This is the other direction.
// ---------------------------------------------------------------------------

func (a *Server) routeSyntheticsAgent(g *gin.RouterGroup) {
	g.GET("/api/unstable/synthetics/agents/tests", a.HandleSyntheticsAgentTests)
}

// civSyntheticsTestsResponse mirrors testpoller.go's testPollerResponse.
//
// Tests is a slice of raw JSON rather than a typed list because the poller's
// decoder is all-or-nothing (see above): whoever fills this later must build
// each element from a synthetics_test_configs row and be able to drop one
// that fails validation without taking the rest of the response with it.
type civSyntheticsTestsResponse struct {
	Tests []json.RawMessage `json:"tests"`
}

// HandleSyntheticsAgentTests answers the poller with an empty test list.
//
// EMPTY, not 404 and not an error: an agent with synthetics.collector.enabled
// set polls this every two seconds whether or not any test exists, and five
// non-200s in a row mark the poller unhealthy. An empty list is the correct
// answer to "what should I run" when the answer is "nothing", and it keeps
// the agent healthy and ready for the moment there is something.
//
// TODO(ninjacat): serve real tests from synthetics_test_configs
// (schema/migrations/0017_ci_visibility.sql, apps/storage/rows_ci.go). The
// table exists and is empty; nothing writes to it yet. Doing this properly
// means a READ, and reads belong to the query app, not to the intake — so
// the shape of the work is: a panel UI writes test definitions through
// panelapi, the query app gains a lookup by tenant, and this handler asks it
// rather than answering from a constant. Until then the constant below is
// the honest answer, and it is the only one that keeps the poller healthy.
//
// Nothing is stored for this request. It is a GET that carries no telemetry —
// only the two query parameters the poller appends, which are logged.
func (a *Server) HandleSyntheticsAgentTests(c *gin.Context) {
	log.Printf("[synthetics-agent] poll from agent_hostname=%s agent_version=%s — 0 tests (catalogue empty)",
		orDash(c.Query("agent_hostname")), orDash(c.Query("agent_version")))

	// json.RawMessage{} rather than nil: a nil slice marshals to "null", and
	// the poller's `for _, test := range response.Tests` would survive that,
	// but "tests": [] is what the field means and what a capture should show.
	c.JSON(http.StatusOK, civSyntheticsTestsResponse{Tests: []json.RawMessage{}})
}
