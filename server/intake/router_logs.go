package intake

import (
	"encoding/json"
	"log"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// http-intake.logs.<site> — the logs intake.
//
//	config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL
//
// Needs logs_config.use_http=true; logs otherwise use their own TCP+TLS
// transport and never reach an HTTP endpoint.

func (a *Server) routeLogs(g *gin.RouterGroup) {
	g.POST("/api/v2/logs", a.HandleLogs)      // current format
	g.POST("/v1/input", a.HandleLogs)         // older format
	g.POST("/v1/input/:apikey", a.HandleLogs) // key in the path
}

// HandleLogs accepts logs. Datadog's intake replies with an empty object and
// a 202 here, not with the error structure it uses for metrics.
func (a *Server) HandleLogs(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[logs] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	if len(body) == 0 {
		return
	}

	// items is the whole payload in Datadog's own model,
	// []datadogV2.HTTPLogItem. The model declares five fields (ddsource,
	// ddtags, hostname, message, service); every other key the sender
	// included — status, timestamp, dd.trace_id, usr.*, http.*, error.*,
	// nested objects — is kept by the generated unmarshaller in
	// AdditionalProperties, decoded with UseNumber, so a 64-bit trace ID
	// arrives as json.Number with every digit intact. Nothing below mutates
	// it; the rows are a projection of it.
	items, err := parseLogs(body)
	if err != nil {
		log.Printf("[logs] %v", err)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// Storage: one LogRow per item. The row has columns for the declared
	// fields plus status and timestamp; everything else in
	// AdditionalProperties has no column and stays in items only. Counted so
	// the gap is visible until the schema decision is made.
	rows := make([]storage.LogRow, 0, len(items))
	unstored := 0

	for _, e := range items {
		// status and timestamp are RESERVED attributes per Datadog's docs but
		// are NOT declared in HTTPLogItem, so they arrive as additional
		// properties like any custom field. A timestamp sent as a string
		// (RFC 3339, which Datadog also accepts) is not parsed here and the
		// row falls back to arrival time; the original stays in items.
		rows = append(rows, storage.LogRow{
			TenantID:  tenant,
			Timestamp: wireTimeMillis(attrInt64(e.AdditionalProperties, "timestamp")),
			Host:      e.GetHostname(),
			Service:   e.GetService(),
			Source:    e.GetDdsource(),
			Status:    attrString(e.AdditionalProperties, "status"),
			Message:   e.Message,
			Tags:      tagsToMultiMap(splitDDTags(e.GetDdtags())),
		})

		if n := len(e.AdditionalProperties) - countPresent(e.AdditionalProperties, "status", "timestamp"); n > 0 {
			unstored += n
		}
	}

	if unstored > 0 {
		log.Printf("[logs] %d attributes decoded but not in LogRow (no column yet); complete in items — see docs/zadania", unstored)
	}

	a.store(storage.LogsWriter, storage.WriteLogs{Entries: rows}, len(rows))

	_ = items // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// attrString reads a string out of a model's AdditionalProperties.
func attrString(props map[string]interface{}, key string) string {
	if v, ok := props[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return ""
}

// attrInt64 reads an integer out of AdditionalProperties.
//
// The generated unmarshaller uses json.Number, not float64 — deliberately, so
// 64-bit trace IDs keep every digit. Handle both anyway: a hand-rolled client
// may produce plain floats.
func attrInt64(props map[string]interface{}, key string) int64 {
	switch v := props[key].(type) {
	case json.Number:
		if n, err := v.Int64(); err == nil {
			return n
		}
	case float64:
		return int64(v)
	case int64:
		return v
	}
	return 0
}

func countPresent(props map[string]interface{}, keys ...string) int {
	n := 0
	for _, k := range keys {
		if _, ok := props[k]; ok {
			n++
		}
	}
	return n
}
