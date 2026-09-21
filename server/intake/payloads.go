package intake

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/gogen"
	"github.com/DataDog/datadog-api-client-go/v2/api/datadogV1"
	"github.com/DataDog/datadog-api-client-go/v2/api/datadogV2"
	"github.com/gin-gonic/gin"
)

// Wire formats.
//
// Every payload here is decoded into a type DATADOG PUBLISHES, never one we
// wrote ourselves:
//
//	/api/v1/series               datadogV1.MetricsPayload
//	/api/v2/series               gogen.MetricPayload          (also the protobuf)
//	/api/beta/sketches           gogen.SketchPayload          (also the protobuf)
//	/api/v1/check_run            []datadogV1.ServiceCheck
//	/api/v1/events               datadogV1.EventCreateRequest
//	/api/v1/distribution_points  datadogV1.DistributionPointsPayload
//	/api/v2/logs                 []datadogV2.HTTPLogItem
//
// This is not only about saving work. Every datadog-api-client model carries
// AdditionalProperties, filled by a generated UnmarshalJSON that decodes the
// body twice — once into the declared fields, once into a map — and removes
// the declared keys from the map. Whatever Datadog adds next lands there
// without a line of change here.
//
// A hand-written struct has a ceiling. Ours had seven fields and silently
// dropped dd.trace_id, which is the one key that joins a log to its trace.

// bodyIsJSON works out the body format: first from Content-Type, and when that
// lies or is missing, from the first significant byte.
func bodyIsJSON(c *gin.Context, body []byte) bool {
	ct := c.GetHeader("Content-Type")
	switch {
	case strings.Contains(ct, "json"):
		return true
	case strings.Contains(ct, "protobuf"), strings.Contains(ct, "octet-stream"):
		return false
	}
	for _, b := range body {
		switch b {
		case ' ', '\t', '\r', '\n':
			continue
		case '{', '[':
			return true
		}
		return false
	}
	return false
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

// parseSeriesProtobuf and parseSeriesJSONv2 differ only in how the bytes reach
// gogen.MetricPayload: the protobuf-generated struct carries json tags matching
// the public API's v2 shape, so one type serves both wires.
func parseSeriesProtobuf(body []byte) (gogen.MetricPayload, error) {
	var payload gogen.MetricPayload
	if err := payload.Unmarshal(body); err != nil {
		return payload, fmt.Errorf("protobuf MetricPayload: %w", err)
	}
	return payload, nil
}

func parseSeriesJSONv2(body []byte) (gogen.MetricPayload, error) {
	var payload gogen.MetricPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		return payload, fmt.Errorf("JSON v2 series: %w", err)
	}
	return payload, nil
}

// parseSeriesJSONv1 decodes the v1 shape. It is NOT converted into v2 — the
// two wires stay separate all the way to the row, because v1 says things v2
// does not: host is a field, the type is a word, points are bare pairs.
func parseSeriesJSONv1(body []byte) (datadogV1.MetricsPayload, error) {
	var payload datadogV1.MetricsPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		return payload, fmt.Errorf("JSON v1 series: %w", err)
	}
	return payload, nil
}

// metricTypeName turns a metric type number into its name.
//
// Zero means UNSPECIFIED and it is NOT rare: Datadog's own documentation
// example submits MetricIntakeType.UNSPECIFIED, so anything copy-pasted from
// their docs arrives this way. We store it faithfully rather than guessing a
// type at ingest — resolving it is a query-time decision.
func metricTypeName(t int32) string {
	switch t {
	case 1:
		return "COUNT"
	case 2:
		return "RATE"
	case 3:
		return "GAUGE"
	default:
		return "UNSPECIFIED"
	}
}

// normalizeTypeWord uppercases the v1 wire's type word. v1 sends "gauge",
// v2 sends the enum 3; both end up as "GAUGE" in the row, and an unknown word
// stays UNSPECIFIED rather than being guessed at.
func normalizeTypeWord(s string) string {
	switch strings.ToLower(s) {
	case "count":
		return "COUNT"
	case "rate":
		return "RATE"
	case "gauge":
		return "GAUGE"
	default:
		return "UNSPECIFIED"
	}
}

// resourceNamed pulls one resource out of a v2 series. Host and device arrive
// this way in v2, as entries in a list rather than named fields.
func resourceNamed(res []*gogen.MetricPayload_Resource, kind string) string {
	for _, r := range res {
		if r.Type == kind {
			return r.Name
		}
	}
	return ""
}

// ---------------------------------------------------------------------------
// Sketches
// ---------------------------------------------------------------------------

func parseSketchesProtobuf(body []byte) (gogen.SketchPayload, error) {
	var payload gogen.SketchPayload
	if err := payload.Unmarshal(body); err != nil {
		return payload, fmt.Errorf("protobuf SketchPayload: %w", err)
	}
	return payload, nil
}

func parseSketchesJSON(body []byte) (gogen.SketchPayload, error) {
	var payload gogen.SketchPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		return payload, fmt.Errorf("JSON sketches: %w", err)
	}
	return payload, nil
}

// ---------------------------------------------------------------------------
// Logs, checks, events, distributions
// ---------------------------------------------------------------------------

// parseLogs reads a logs payload into Datadog's own log model.
//
// The agent sends an array; clients sometimes send a bare object, and
// Datadog's API accepts both.
func parseLogs(body []byte) ([]datadogV2.HTTPLogItem, error) {
	items, err := decodeJSONList[datadogV2.HTTPLogItem](body)
	if err != nil {
		return nil, fmt.Errorf("JSON logs: %w", err)
	}
	return items, nil
}

// parseCheckRuns decodes a check_run batch, per item.
//
// Two things make this more than a decodeJSONList call.
//
// First, datadogV1.ServiceCheck marks `tags` REQUIRED, because Datadog's
// OpenAPI spec says so. The agent disagrees: in a captured batch from a real
// cluster, SEVEN of twelve checks carried no tags at all — containerd.health
// and the kubelet checks among them. The spec describes what the API
// documents; an intake has to take what actually arrives.
//
// Second, decoding the batch as one unit means a single odd check discards
// every other check beside it. Per-item decoding keeps the good ones.
func parseCheckRuns(body []byte) ([]datadogV1.ServiceCheck, error) {
	var raw []json.RawMessage
	if err := json.Unmarshal(body, &raw); err != nil {
		// Not an array: fall back to the shared decoder, which also handles a
		// bare object as datadogpy sends it.
		runs, err := decodeJSONList[datadogV1.ServiceCheck](body)
		if err != nil {
			return nil, fmt.Errorf("JSON check_run: %w", err)
		}
		return runs, nil
	}

	runs := make([]datadogV1.ServiceCheck, 0, len(raw))
	var skipped int
	for _, item := range raw {
		var run datadogV1.ServiceCheck
		if err := json.Unmarshal(withTags(item), &run); err != nil {
			skipped++
			continue
		}
		runs = append(runs, run)
	}
	if skipped > 0 {
		log.Printf("[check_run] %d of %d checks could not be decoded and were skipped", skipped, len(raw))
	}
	return runs, nil
}

// withTags supplies an empty tag list when a check has none, so the generated
// client's required-field check passes. It adds nothing that was not already
// implied: a check with no tags has an empty tag set either way.
//
// Note the agent does not OMIT the key — it sends it explicitly null:
//
//	{"check":"containerd.health", ..., "tags": null}
//
// so testing for the key's presence is not enough, and a version of this that
// did exactly that looked correct and changed nothing. The value has to be
// inspected.
func withTags(item json.RawMessage) json.RawMessage {
	var probe map[string]json.RawMessage
	if err := json.Unmarshal(item, &probe); err != nil {
		return item
	}
	if v, ok := probe["tags"]; ok && !bytes.Equal(bytes.TrimSpace(v), []byte("null")) {
		return item
	}
	probe["tags"] = json.RawMessage(`[]`)
	patched, err := json.Marshal(probe)
	if err != nil {
		return item
	}
	return patched
}

func parseEvents(body []byte) ([]datadogV1.EventCreateRequest, error) {
	events, err := decodeJSONList[datadogV1.EventCreateRequest](body)
	if err != nil {
		return nil, fmt.Errorf("JSON events: %w", err)
	}
	return events, nil
}

func parseDistributionPoints(body []byte) (datadogV1.DistributionPointsPayload, error) {
	var payload datadogV1.DistributionPointsPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		return payload, fmt.Errorf("JSON distribution_points: %w", err)
	}
	return payload, nil
}

// distributionPoint splits one [timestamp, [values...]] pair.
//
// The pair is heterogeneous, so Datadog models each element as a oneof —
// DistributionPointItem is either a timestamp or the value list, never both.
func distributionPoint(pair []datadogV1.DistributionPointItem) (time.Time, []float64, bool) {
	var ts float64
	var values []float64
	for _, item := range pair {
		if item.DistributionPointTimestamp != nil {
			ts = *item.DistributionPointTimestamp
		}
		if item.DistributionPointData != nil {
			values = *item.DistributionPointData
		}
	}
	if len(values) == 0 {
		return time.Time{}, nil, false
	}
	return wireTime(int64(ts)), values, true
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

// splitDDTags turns Datadog's comma-separated ddtags string into a list.
// Logs carry tags this way; everything else sends an array.
func splitDDTags(s string) []string {
	if s == "" {
		return nil
	}
	return strings.Split(s, ",")
}

// wireTime turns a timestamp taken off the wire into a time.Time, falling back
// to the receive time when the sender left the field out.
//
// A zero value does NOT mean 1970 — it means "not supplied". Datadog's own
// intake stamps such payloads on arrival, and some official clients cannot do
// otherwise: HTTPLogItem has no timestamp field at all.
//
// Mapping it to the epoch is worse than wrong, it is invisible: retention
// discards the row on arrival while the sender still gets a 202.
func wireTime(seconds int64) time.Time {
	if seconds <= 0 {
		return time.Now().UTC()
	}
	return time.Unix(seconds, 0).UTC()
}

// wireTimeMillis is wireTime for sources that count in milliseconds.
func wireTimeMillis(millis int64) time.Time {
	if millis <= 0 {
		return time.Now().UTC()
	}
	return time.UnixMilli(millis).UTC()
}

// decodeJSONList unmarshals a body that may arrive either as a JSON array or
// as a single bare object.
//
// The agent always sends an array. Clients often do not: datadogpy posts a
// bare object to /api/v1/check_run. Datadog's public API accepts both.
//
// When BOTH attempts fail, the array error is the one worth reporting for a
// body that is visibly an array. Returning the single-object error instead
// produces the actively misleading
//
//	json: cannot unmarshal array into Go value of type map[string]interface {}
//
// which says the payload is an array — true, and exactly what we wanted — and
// hides the real reason the array decode failed. That message cost a while to
// see through once; it should not do so again.
func decodeJSONList[T any](body []byte) ([]T, error) {
	var many []T
	arrayErr := json.Unmarshal(body, &many)
	if arrayErr == nil {
		return many, nil
	}

	var one T
	if err := json.Unmarshal(body, &one); err != nil {
		if bytes.HasPrefix(bytes.TrimSpace(body), []byte("[")) {
			return nil, arrayErr
		}
		return nil, err
	}
	return []T{one}, nil
}
