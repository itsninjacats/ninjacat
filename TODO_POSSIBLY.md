# Possibly

Ideas worth a look, not commitments. Nothing here is scheduled; `TODO.md` is the list of
work that is. Each entry says what it would buy and what is not known yet.

## Types from JSON Schema and OpenAPI

Where Datadog publishes a schema, the types could be generated instead of read by hand.

- **RUM events**: `rum-events-format` is JSON Schema. The Go server generated its types from
  it; the Rust intake reads the events by path instead (`intake/src/routers/rum.rs`). Generated types
  would make a new schema version a regeneration, not a code review.
- **Session Replay records**: the same repository, `session-replay-browser` and
  `session-replay-mobile`. Needed anyway the day a player is written.
- **Datadog's public API** (`/api/v1/series`, `/api/v1/events`, `/api/v2/logs`, the query
  API): one OpenAPI document. `api_payloads.rs` re-implements what the generated Go models
  did, field by field.
- **Tracer telemetry**: Datadog publishes its request schemas (to be confirmed which
  repository).
- Not known: which generator. Candidates to compare: typify (Rust from JSON Schema),
  progenitor or openapi-generator (Rust from OpenAPI). What matters for us and is easy to lose:
  unknown keys must be kept, a field of the wrong type must not cost the whole event, and
  64-bit integers must keep their digits.

## Plain deserialisation for JSON without a schema

Most of what the agent sends as JSON has no published schema, only Go structs in its source.
serde into structs would be the least code, and has two costs: one field of the wrong type
refuses the whole document, and keys the struct does not declare are dropped.

What we do know makes this safer than it sounds: the shapes come from the agent's own source
and from real payloads recorded per agent version (`fixtures/real/`). A possible middle:
structs for the fields a table has columns for, `#[serde(flatten)]` for the rest, and the
whole body to `raw_payloads` when deserialisation fails — with the real fixtures telling us
when a new agent version changes a shape.

## Other

- **Profiles in one shape**: convert JFR to the sample form pprof decodes into, so one table
  and one flame graph serve every language (see `TODO.md`, Decode deeper).
- **A Sentry-compatible intake** is in `TODO.md` (Error Tracking); listed here only because
  it is the cheapest way to get application errors in before any tracer work.
