# Profiling tables

Fed by `intake/router_profiling.go`'s three hosts:

| Host | Route(s) | Table |
|---|---|---|
| `intake.profile.<site>` | `POST /api/v2/profile`, `POST /v1/input` | `profiles` |
| `debugger-intake.<site>` | `POST /api/v2/debugger` (json variant) | `debugger_logs` |
| `debugger-intake.<site>` | `POST /api/v2/debugger` (diagnostics variant) | `debugger_diagnostics` |
| `debugger-intake.<site>` | `POST /api/v2/debugger` (symdb variant) | `symdb_uploads` |
| `sourcemap-intake.<site>` | `POST /api/v2/srcmap` | `symbol_uploads` |

Every handler answers `202 {}` regardless of outcome (`isDiagnose` does not
apply to this router — these endpoints do not receive the diagnose probe). A
body that fails to decode, or a debugger multipart that is neither the symdb
nor the diagnostics shape, is kept whole in `raw_payloads` via `storeRaw`
instead (reasons: `decode_error` for a body that could not be parsed at all,
`unexpected_shape` for the debugger multipart with neither a `file` nor an
`event` part). A tenant-less request (no API key resolved) still gets its 202
but stores nothing, same as every other intake.

All five tables use `received_at` — arrival time, always known — as their
`PARTITION BY`/`ORDER BY` time column, never a wire-supplied timestamp. Wire
timestamps (a profile's `start`/`end`, a diagnostic's `timestamp`) are
informational only and live in their own Nullable columns, so an absent or
unparseable one simply stays NULL — it never falls back to `received_at` and
never becomes 1970-01-01.

## `profiles`

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, service, received_at)`, `TTL` 7 days (bulky-document
class, same reasoning as `k8s_manifests`: rows carry megabyte-scale pprof
attachments). Writer tuning copies `k8s_manifests`' numbers for the same
reason (`MaxRows: 200, FlushInterval: 10s, BufferLimit: 5_000, MaxInFlight: 1`).

One row per multipart submission to `/api/v2/profile` (proxied from the
tracer's own multipart) or `/v1/input` (dd-trace-go's internal profiling).

**Samples are not exploded into rows.** A single CPU profile can carry tens
of thousands of them, and `github.com/google/pprof/profile` is a perfectly
good reader for the bytes kept in `attach_bytes` — exploding would multiply
write volume by the sample count to reproduce a shape an existing tool
already parses for free. What is pulled out per attachment is its pprof
*envelope* (sample types/units, period, counts), as parallel arrays, so a
query can filter by shape without re-parsing every attachment.

| Column | Source |
|---|---|
| `variant` | The router's own label: `"profile"` for `/api/v2/profile`, `"profile-v1"` for `/v1/input`. Left as a plain `String`, not `Enum8`, so a future RUM profiler reuse (`"browser"` — see `rum-spec.md`'s browser-profiler note) needs no migration. |
| `dd_evp_origin`, `dd_evp_origin_version` | `DD-EVP-ORIGIN`/`DD-EVP-ORIGIN-VERSION` request headers. |
| `event` | The `"event"` multipart part, byte for byte — the lossless copy. Every column below to `tags_profiler` is a convenience extract of it. |
| `start_raw`/`start_parsed`, `end_raw`/`end_parsed` | `event.start`/`event.end`. `*_raw` is the wire value's own text (a `json.Number`'s `.String()`, never rounded through `float64`); `*_parsed` is `profParseTimestamp`'s best-effort `Nullable(DateTime64)` — RFC3339 string, or an epoch number whose magnitude picks seconds/millis/micros/nanos. `nil`/`""` when absent or unparseable — never a guessed time. |
| `family`, `version`, `runtime`, `language` | `event.family`/`.version`/`.runtime`/`.language`, `""` when absent or not a string. |
| `tags_profiler` | `event.tags_profiler`, a **third** Datadog tag convention (a single comma-joined string, unlike the flat array or `map[string]string` elsewhere in the intake) — split with `splitDDTags`, then `tagsToMultiMap` for the usual multiset shape. |
| `service` | `MATERIALIZED tags_profiler['service'][1]` — the hot-tag column every other tagged table gets. |
| `attach_name`, `attach_bytes`, `attach_size` | One entry per multipart part other than `"event"`, in wire order. `attach_bytes` is the part's raw bytes exactly as received — **still gzip if the part was gzip'd**, since `profile.ParseData` undoes that internally and this column is the undecoded original. |
| `attach_parsed` | `1` when `profile.ParseData` accepted the part as pprof, `0` otherwise. Every other `attach_*` field at that index is zero-valued (an empty array, not NULL — there is no per-attachment Nullable here) when this is `0`. |
| `attach_sample_types`, `attach_sample_units` | Per attachment, the pprof `SampleType[].Type`/`.Unit` lists, e.g. `["cpu"]`/`["nanoseconds"]`. |
| `attach_sample_count` | `len(Sample)`. |
| `attach_time_nanos`, `attach_duration_nanos` | pprof `Profile.TimeNanos`/`.DurationNanos`. |
| `attach_period_type`, `attach_period` | `"Type/Unit"` of `Profile.PeriodType`, and `Profile.Period`. |
| `attach_mapping_count`, `attach_location_count`, `attach_function_count` | `len(Mapping)`/`len(Location)`/`len(Function)` — legitimately `0` when the pprof profile carried none (e.g. no `Mapping` at all is normal for some runtimes), not a decode failure. |

## `debugger_logs`

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, service, received_at)`, `TTL` 14 days (trickle-in
class, same as `check_runs`: DI log entries only exist while a probe is
attached to a running service). Writer: `MaxRows: 500, FlushInterval: 5s`.

The `"json"` variant of `/api/v2/debugger` — a DI logs/snapshots batch, one
row per array element. `dbgDecodeLogs` sniffs the first non-whitespace byte
to tell a JSON array (`[`, the dyninst logSender/tracer shape) from NDJSON
(`{`, the browser SDK's debugger track — see `rum-spec.md`'s framing-gap
note) — either way, every entry becomes one row.

| Column | Source |
|---|---|
| `service`, `ddsource` | The entry's own `service`/`ddsource` keys, `""` when absent or not a string. |
| `ddtags` | The `?ddtags=` query string, split and parsed as a multiset — it travels alongside every entry in a batch, not per entry. |
| `entry` | The array element (or NDJSON line), byte for byte. |
| `extra_keys` | The entry's top-level keys other than `service`/`ddsource`, sorted — there is no fixed schema here to decode against (the upstream dyninst logSender type is not published), so "extra" is everything else, same pattern as `k8s_actions.extra_keys`. |

## `debugger_diagnostics`

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, service, received_at)`, `TTL` 90 days (human-scale
class, same as `events`/`k8s_actions`: a probe's status changes a handful of
times per install). Writer: `MaxRows: 200, FlushInterval: 2s`.

The `"diagnostics"` variant of `/api/v2/debugger` (multipart with an `event`
part only, no `file`) — one row per message. The wire shape is
`uploader.DiagnosticMessage`, decoded generically since the upstream type is
unexported: top-level `service`/`ddsource`/`timestamp`, and
`debugger.diagnostics.{runtimeId, probeId, status, probeVersion, exception}`.

| Column | Source |
|---|---|
| `timestamp` | `Nullable(DateTime64)`, `profParseTimestamp` of the message's own `timestamp` — NULL when absent or unparseable, never `received_at` or the epoch. |
| `service`, `ddsource`, `runtime_id`, `probe_id`, `status`, `probe_version` | The named fields, `""` when absent. |
| `exception_type`, `exception_message` | `Nullable(String)`, both together: `debugger.diagnostics.exception` is optional on the wire, so "no exception" (both NULL) and "an exception with an empty message" (both non-NULL, one `""`) stay distinguishable. |
| `message` | The array element, byte for byte — a per-entry decode failure (an element that is not a JSON object) still lands here with every extracted column empty/NULL, rather than dropping the whole batch. |

## `symdb_uploads`

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, service, received_at)`, `TTL` 7 days (bulky-document
class, same as `k8s_manifests`: `file` carries an entire gzip'd
symbol-database batch). Writer: `MaxRows: 200, FlushInterval: 10s,
BufferLimit: 5_000, MaxInFlight: 1`.

The `"symdb"` variant of `/api/v2/debugger` (multipart with both `file` and
`event` parts) — one row per upload.

**The event part and the envelope inside the gzip'd file describe the same
upload but are two different producers/serializers.** The event spells its
keys camelCase (`uploadId`, `batchNum`); the file's envelope spells them
snake_case (`upload_id`, `batch_num`). Both are kept — the event's under
their own names, the envelope's under `env_`-prefixed names — rather than
merged into one guess, because the drift itself is worth being able to see.

| Column | Source |
|---|---|
| `service`, `version`, `language`, `runtime_id` | From the **event** part (`ev.service`/`.version`/`.language`/`.runtimeId`). |
| `upload_id`, `batch_num` | From the event (`uploadId`/`batchNum`), kept as the wire's **literal text**, not parsed to an integer — they are identifiers, not quantities, and nothing guarantees they stay numeric across producers. |
| `final`, `attachment_size` | `Nullable(UInt8)`/`Nullable(UInt64)` from the event's `final`/`attachmentSize` — NULL when the key was absent, which is a different state from "false"/"0 bytes". |
| `file` | The gzip'd file part exactly as received — the lossless copy. `inflated_size`/`scope_count`/`scopes_ok` and every `env_*` column below are convenience extracts of what is already fully present here (gunzip it to get everything back, including nested child scopes this table does not otherwise count). |
| `inflated_size` | `len()` of the gunzipped file, `0` when the part was not valid gzip. |
| `scope_count` | `len(envelope.scopes)` — **top-level scopes only**; nested child scopes are not counted (see "Dropped by decision" below). |
| `scopes_ok` | `1` when the envelope's `"scopes"` key was present and decoded; `0` when it was missing or malformed. The envelope itself may still have decoded fine when this is `0` — `env_*` stay populated independently (envelope-vs-scopes nil-ness is intentionally independent, matching `dbgDecodeSymdbFile`'s own contract). |
| `env_service`, `env_version`, `env_language`, `env_upload_id`, `env_batch_num`, `env_final` | The same six concepts, read from the **file's envelope** (`json.RawMessage` values rendered as plain text by `rawJSONText`: a JSON string unquotes, anything else keeps its literal JSON form). |

## `symbol_uploads`

`MergeTree`, `PARTITION BY toDate(received_at)`,
`ORDER BY (tenant_id, symbol_source, received_at)` — there is no `service`
field on a symbol upload, and a build id is exactly the wrong shape for a
sort prefix (unique per build, not a grouping key), so `symbol_source` (the
uploader identifying itself) is the closest bounded "which producer" column
this payload has. `TTL` 7 days (bulky-document class, same as
`k8s_manifests`: `elf` carries a whole ELF symbol file). Writer:
`MaxRows: 200, FlushInterval: 10s, BufferLimit: 5_000, MaxInFlight: 1`.

`sourcemap-intake.<site>`'s `POST /api/v2/srcmap` — one row per upload.

| Column | Source |
|---|---|
| `type`, `arch`, `gnu_build_id`, `go_build_id`, `file_hash`, `symbol_source`, `origin`, `origin_version`, `filename` | The `"event"` part's named keys (`symbolUploadRequestMetadata` from `comp/host-profiler/symboluploader`, unexported upstream, hence a generic decode). `""` when absent. |
| `meta` | The `"event"` part, byte for byte — the lossless copy of which the nine columns above are convenience extracts. |
| `has_elf`, `elf_class`, `elf_endianness` | Whether an `elf_symbol_file` part was sent, and `elfInfo`'s sniff of its ELF ident bytes (magic + class byte + data byte) — `"ELF32"`/`"ELF64"`/`"ELF?"` and `"LE"`/`"BE"`/`"?"`, or both `""` together with `has_elf = 0` when no such part arrived. No ELF section/symbol table parsing is attempted anywhere in this router. |
| `elf`, `elf_size` | The `elf_symbol_file` part, raw bytes, unparsed. |
| `other_parts` | Every multipart part besides `"event"` and `"elf_symbol_file"`, verbatim (`Map(String, String)`, not a Datadog tags map, so no bloom index) — nothing this handler names is thrown away either. |

## Sensitive columns

None of these five tables carry anything from the credential-bearing header
set (`Dd-Api-Key`, `Authorization`) — those never reach a row anywhere in the
intake. `profiles.event`/`symbol_uploads.meta`/`debugger_logs.entry`/
`debugger_diagnostics.message` are raw, attacker-controlled JSON text kept
for fidelity; nothing about that JSON is redacted, matching `raw_payloads`'
own treatment of arbitrary body content.

## Dropped by decision

- **pprof samples are not exploded into rows** (`profiles`). A profile's
  bytes stay fully queryable via `attach_bytes` + `google/pprof/profile`;
  exploding would multiply write volume by the sample count (often tens of
  thousands per attachment) for a shape that tool already parses for free.
  The pprof *envelope* (sample types/units, period, counts) is kept as
  columns so a query can filter without re-parsing.
- **symdb nested child scopes are not counted or flattened**
  (`symdb_uploads.scope_count` is top-level only). The full scope tree,
  arbitrarily deep, is fully recoverable by gunzipping `file` — the raw
  column is the lossless copy, `scope_count`/`scopes_ok` are cheap summary
  columns, not an attempt at a complete index.
- **ELF internals are not parsed** (`symbol_uploads.elf`). Only the ident
  bytes (class/endianness) are sniffed, same as the pre-existing `elfHeader`
  log line; a build-id note, section table or symbol table would need a real
  ELF parser, which this router has never included. The whole file is kept
  raw, so nothing is lost — only unindexed.
- **`upload_id`/`batch_num` (and their `env_*` counterparts) are kept as
  text, not parsed to integers.** They are identifiers on the wire, and nothing
  guarantees they stay numeric across producers; a String column round-trips
  every producer's spelling exactly, an Int64 column would silently reject or
  mis-parse a future non-numeric batch id.
- **Insertion order of MetaStruct-like undeclared fields is not separately
  tracked beyond the sorted `extra_keys` list** (`debugger_logs`). The
  original order is fully recoverable from `entry`, the raw copy; `extra_keys`
  is a cheap "what's here" index, not a second source of truth.
