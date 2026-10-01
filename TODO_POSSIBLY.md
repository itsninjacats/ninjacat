# Possibly

Ideas worth a look, not commitments. Nothing here is scheduled; `TODO.md` is the list of
work that is. Each entry says what it would buy and what is not known yet.

## F# records straight from `.proto` (FsGrpc)

Today `protoc` generates C# classes (`api/src/Proto`) and the F# code reads them: mutable,
null where a sub-message is absent, `RepeatedField` and `MapField` instead of lists and maps.
[FsGrpc](https://github.com/dmgtech/fsgrpc) generates F# records, with options for optional
fields and unions for `oneof`.

- Would buy: no `isNull` checks on sub-messages, `match` on a `oneof`, records all the way.
- Not known: whether it works well. Checked on 2026-10-01: a small project (2 stars), last
  release v1.0.6 in April 2024, last commit December 2025 targeting .NET 8. Nothing about
  proto3 `optional`, groups, the well-known types, or the size of our schemas (the
  process-agent's `agent.proto` is 2 000 lines). Try it on one small schema (`sds_result.proto`)
  before anything else.
- Cost if it fails later: a generator nobody else maintains, in the path of every protobuf
  payload. The official `protoc` output is ugly and will always work.

## Types from JSON Schema and OpenAPI

Where Datadog publishes a schema, the types could be generated instead of read by hand.

- **RUM events**: `rum-events-format` is JSON Schema. The Go server generated its types from
  it; the F# port reads the events by path instead (`Routers/Rum.fs`). Generated records
  would make a new schema version a regeneration, not a code review.
- **Session Replay records**: the same repository, `session-replay-browser` and
  `session-replay-mobile`. Needed anyway the day a player is written.
- **Datadog's public API** (`/api/v1/series`, `/api/v1/events`, `/api/v2/logs`, the query
  API): one OpenAPI document. `ApiPayloads.fs` re-implements what the generated Go models
  did, field by field.
- **Tracer telemetry**: Datadog publishes its request schemas (to be confirmed which
  repository).
- Not known: which generator. Candidates to compare: NJsonSchema and NSwag (C# output, like
  our protobuf classes), a type provider (FSharp.Data's JsonProvider infers from samples,
  not from a schema), Hawaii (F# from OpenAPI). What matters for us and is easy to lose:
  unknown keys must be kept, a field of the wrong type must not cost the whole event, and
  64-bit integers must keep their digits.

## Plain deserialisation for JSON without a schema

Most of what the agent sends as JSON has no published schema, only Go structs in its source.
`System.Text.Json` into F# records would be the least code, and has two costs: one field of
the wrong type refuses the whole document, and keys the record does not declare are dropped.

What we do know makes this safer than it sounds: the shapes come from the agent's own source
and from real payloads recorded per agent version (`Fixtures/real/`). A possible middle:
records for the fields a table has columns for, `JsonExtensionData` for the rest, and the
whole body to `raw_payloads` when deserialisation fails — with the real fixtures telling us
when a new agent version changes a shape.

## Other

- **Profiles in one shape**: convert JFR to the sample form pprof decodes into, so one table
  and one flame graph serve every language (see `TODO.md`, Decode deeper).
- **Replace hand-written Go arithmetic**: `TraceSketch.GoMath` exists to match the Go
  server's sums to the last bit. `Math.*` would do, at the price of last-bit differences
  from rows already stored.
- **A Sentry-compatible intake** is in `TODO.md` (Error Tracking); listed here only because
  it is the cheapest way to get application errors in before any tracer work.
