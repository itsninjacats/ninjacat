package intake

import (
	"encoding/json"
	"log"
	"net/http"
	"strconv"
	"time"

	"github.com/DataDog/datadog-api-client-go/v2/api/datadogV2"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// http-intake.logs.<site> — the logs intake.
//
//	config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL
//
// Needs logs_config.use_http=true; logs otherwise use their own TCP+TLS
// transport and never reach an HTTP endpoint.
//
// STORED: one `logs` row per item — the declared fields, plus status, plus a
// timestamp resolved from four possible wire forms (timestamp_source records
// which one), plus every remaining attribute in the `attributes` JSON column.
// An item the model could not parse goes to raw_payloads as
// ("logs", "unexpected_shape"), and a body that is not JSON at all as
// ("logs", "decode_error"). Nothing reaches only a log line.

func (a *Server) routeLogs(g *gin.RouterGroup) {
	g.POST("/api/v2/logs", a.HandleLogs)      // current format
	g.POST("/v1/input", a.HandleLogs)         // older format
	g.POST("/v1/input/:apikey", a.HandleLogs) // key in the path
}

// logAttrKeys are the attribute names this handler lifts into columns. They
// are removed from `attributes` only when they were actually USED — a
// timestamp string we could not parse stays in the JSON column rather than
// disappearing between a column that ignored it and a map that dropped it.
const (
	logAttrStatus    = "status"
	logAttrTimestamp = "timestamp"
	logAttrDate      = "date"
	logAttrHost      = "host"
)

// HandleLogs accepts logs. Datadog's intake replies with an empty object and
// a 202 here, not with the error structure it uses for metrics.
//
// Three framings arrive on this path and all three are accepted: a JSON array
// (the agent), a single bare object (small clients), and NDJSON — one object
// per line, which is what the browser RUM/logs SDK sends. parseLogItems tells
// them apart from the first significant byte.
//
// Four of the fields may also arrive as QUERY PARAMETERS (ddsource, ddtags,
// hostname/host, service): Datadog's intake accepts them there so a sender
// that cannot shape its body can still say what it is. The body wins for the
// scalars; ddtags is the exception and MERGES, because the query form is how a
// sender tags a whole batch while each item tags itself.
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
	if len(body) == 0 {
		return
	}

	raws, err := parseLogItems(body)
	if err != nil {
		log.Printf("[logs] %v", err)
		a.storeRaw(c, "logs", "decode_error", err.Error(), body)
		return
	}

	tenant := TenantFromContext(c)

	// Query-parameter defaults, read once for the batch.
	qSource := c.Query("ddsource")
	qService := c.Query("service")
	qHost := firstNonEmpty(c.Query("hostname"), c.Query("host"))
	qTags := splitDDTags(c.Query("ddtags"))

	arrival := time.Now().UTC()
	rows := make([]storage.LogRow, 0, len(raws))

	for i, raw := range raws {
		var e datadogV2.HTTPLogItem
		if err := json.Unmarshal(raw, &e); err != nil {
			// The generated unmarshaller swallows most odd shapes into
			// UnparsedObject, so reaching here means the bytes are not even an
			// object. Keep them: this is the only copy.
			log.Printf("[logs] item #%d: %v", i, err)
			a.storeRaw(c, "logs", "decode_error", "item #"+strconv.Itoa(i)+": "+err.Error(), raw)
			continue
		}
		if e.UnparsedObject != nil {
			// The item decoded as JSON but not as a log: none of the declared
			// fields were populated, so a row built from it would be a row of
			// empty strings pretending to be a log line.
			log.Printf("[logs] item #%d did not fit HTTPLogItem, kept raw, keys: %s",
				i, apiKeys(e.UnparsedObject))
			a.storeRaw(c, "logs", "unexpected_shape", "item #"+strconv.Itoa(i)+" did not fit datadogV2.HTTPLogItem", raw)
			continue
		}
		if tenant == "" {
			continue
		}

		row, err := logRow(tenant, e, arrival, qSource, qService, qHost, qTags)
		if err != nil {
			// attributes is the only thing that can fail here, and it fails
			// only if a decoded value cannot be re-encoded — which would mean
			// the attribute is unrepresentable, not that the log is bad.
			log.Printf("[logs] item #%d attributes: %v", i, err)
			a.storeRaw(c, "logs", "unexpected_shape", "item #"+strconv.Itoa(i)+" attributes: "+err.Error(), raw)
			continue
		}
		rows = append(rows, row)
	}

	a.store(storage.LogsWriter, storage.WriteLogs{Entries: rows}, len(rows))
}

// logRow turns one decoded item into the row, applying the batch-level query
// defaults. Pure, so the resolution rules below get a table-driven test that
// needs no HTTP.
func logRow(tenant string, e datadogV2.HTTPLogItem, arrival time.Time,
	qSource, qService, qHost string, qTags []string) (storage.LogRow, error) {

	props := e.AdditionalProperties

	// `host` is an alternate spelling of `hostname` that Datadog accepts; it
	// is not a declared field, so it arrives as an ordinary attribute.
	host := e.GetHostname()
	usedHostAttr := false
	if host == "" {
		if v, ok := attrStringOk(props, logAttrHost); ok && v != "" {
			host, usedHostAttr = v, true
		}
	}
	if host == "" {
		host = qHost
	}

	ts, source, usedTimeKey := logTimestamp(props, arrival)

	status, usedStatus := attrStringOk(props, logAttrStatus)

	// Query tags first, then the item's own: a batch-level tag is the weaker
	// statement and reads better at the front of the list.
	tags := tagsToMultiMap(append(append([]string{}, qTags...), splitDDTags(e.GetDdtags())...))

	attrs, err := logAttributes(props, usedTimeKey, usedStatus, usedHostAttr)
	if err != nil {
		return storage.LogRow{}, err
	}

	return storage.LogRow{
		TenantID:        tenant,
		Timestamp:       ts,
		Host:            host,
		Service:         firstNonEmpty(e.GetService(), qService),
		Source:          firstNonEmpty(e.GetDdsource(), qSource),
		Status:          status,
		Message:         e.Message,
		Tags:            tags,
		Attributes:      attrs,
		TimestampSource: source,
	}, nil
}

// logTimestamp resolves the row's timestamp and says where it came from.
//
// Four wire forms are in the wild for the same idea, and the handler used to
// read exactly one of them: `timestamp` as a millisecond number. A sender
// using any of the other three got ARRIVAL TIME silently — which is not a
// missing value but a WRONG one, and one that survives every sanity check
// because it always looks plausible.
//
// The order is Datadog's own precedence: `timestamp` before `date`, number
// before string. A value that does not parse falls through to the next
// candidate rather than ending the search, and the key it came from is
// reported so the caller can leave it in `attributes` instead of dropping it.
func logTimestamp(props map[string]interface{}, arrival time.Time) (ts time.Time, source, usedKey string) {
	if ms, ok := attrInt64Ok(props, logAttrTimestamp); ok && ms > 0 {
		return time.UnixMilli(ms).UTC(), "timestamp_ms", logAttrTimestamp
	}
	if s, ok := attrStringOk(props, logAttrTimestamp); ok {
		if t, err := time.Parse(time.RFC3339, s); err == nil {
			return t.UTC(), "timestamp_string", logAttrTimestamp
		}
	}
	if ms, ok := attrInt64Ok(props, logAttrDate); ok && ms > 0 {
		return time.UnixMilli(ms).UTC(), "date_ms", logAttrDate
	}
	if s, ok := attrStringOk(props, logAttrDate); ok {
		if t, err := time.Parse(time.RFC3339, s); err == nil {
			return t.UTC(), "date_string", logAttrDate
		}
	}
	// Arrival, the same fallback wireTimeMillis applies elsewhere and for the
	// same reason: HTTPLogItem has no timestamp field at all, so an official
	// client cannot always send one. The difference is that the row now says
	// so instead of looking like the sender's own clock.
	return arrival, "arrival", ""
}

// logAttributes renders everything that did not become a column as the JSON
// text of the `attributes` column. Only the keys actually LIFTED are removed:
// a `timestamp` we could not parse stays here, because a value dropped by both
// the column and the map is a value nobody can find again.
func logAttributes(props map[string]interface{}, usedTimeKey string, usedStatus, usedHost bool) (string, error) {
	if len(props) == 0 {
		return "", nil
	}
	rest := make(map[string]interface{}, len(props))
	for k, v := range props {
		switch {
		case k == usedTimeKey:
		case k == logAttrStatus && usedStatus:
		case k == logAttrHost && usedHost:
		default:
			rest[k] = v
		}
	}
	if len(rest) == 0 {
		return "", nil
	}
	// json.Number values survive Marshal as their original digits, so a 64-bit
	// dd.trace_id is written out exactly as it arrived.
	out, err := json.Marshal(rest)
	if err != nil {
		return "", err
	}
	return string(out), nil
}

// attrStringOk reads a string out of a model's AdditionalProperties and says
// whether it was there as a string at all.
func attrStringOk(props map[string]interface{}, key string) (string, bool) {
	v, ok := props[key]
	if !ok {
		return "", false
	}
	s, ok := v.(string)
	return s, ok
}

// attrString reads a string out of a model's AdditionalProperties.
func attrString(props map[string]interface{}, key string) string {
	s, _ := attrStringOk(props, key)
	return s
}

// attrInt64Ok reads an integer out of AdditionalProperties.
//
// The generated unmarshaller uses json.Number, not float64 — deliberately, so
// 64-bit trace IDs keep every digit. Handle both anyway: a hand-rolled client
// may produce plain floats.
func attrInt64Ok(props map[string]interface{}, key string) (int64, bool) {
	switch v := props[key].(type) {
	case json.Number:
		if n, err := v.Int64(); err == nil {
			return n, true
		}
	case float64:
		return int64(v), true
	case int64:
		return v, true
	}
	return 0, false
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if v != "" {
			return v
		}
	}
	return ""
}
