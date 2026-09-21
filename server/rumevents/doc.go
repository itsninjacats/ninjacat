// Package rumevents holds Go types for every event accepted on the Datadog
// RUM intake track (/api/v2/rum): RUM events, SDK telemetry events and
// timeseries events, as published by DataDog/rum-events-format.
//
// The types are generated from the JSON Schemas in that repository, the same
// schemas Datadog's own SDKs generate their TypeScript, Kotlin and Swift models
// from; the header of each *_gen.go file names the exact commit. Nothing here
// is hand-modelled: to change a type, change the schema checkout and
// regenerate.
//
// Guarantees that matter for an intake:
//
//   - Nothing is lost. Every object keeps keys the schema does not declare in
//     AdditionalProperties, and MarshalJSON writes them back. RUM objects are
//     open by design, so undeclared keys are the norm.
//   - No number goes through float64. `integer` fields are int64 and `number`
//     fields are json.Number; unknown values are decoded with UseNumber.
//   - Variants are selected by the discriminator fields the schema declares
//     (`type`, `vital.type`, `telemetry.type`, `telemetry.status`,
//     `timeseries.name`), never by shape.
//
// Decode takes one JSON object (one NDJSON line of a RUM batch) and returns
// the concrete variant behind the Event interface:
//
//	ev, err := rumevents.Decode(line)
//	switch e := ev.(type) {
//	case *rumevents.RumViewEvent:
//	case *rumevents.TelemetryErrorEvent:
//	}
//
// Session Replay segments (/api/v2/replay) live in the subpackage replay,
// generated from the same checkout by the same generator.
//
// Regeneration (requires a checkout of DataDog/rum-events-format):
//
//	RUM_EVENTS_FORMAT=/path/to/rum-events-format go generate ./rumevents/...
package rumevents

//go:generate go run ./internal/gen -preset rum -schemas $RUM_EVENTS_FORMAT/schemas -out .
