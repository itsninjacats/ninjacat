# dbm_events

Feeds: `dbm-metrics-intake.<site>`, all six Database Monitoring tracks
(`intake/router_dbm.go`, `schema/migrations/0006_dbm.sql`):

| Route                              | `track` value          |
|-------------------------------------|-------------------------|
| `POST /api/v2/dbmmetrics`           | `dbmmetrics`            |
| `POST /api/v2/dbmactivity`          | `dbmactivity`           |
| `POST /api/v2/databasequery`        | `databasequery`         |
| `POST /api/v2/dbmmetadata`          | `dbmmetadata`           |
| `POST /api/v2/dbmhealth`            | `dbmhealth`             |
| `POST /api/v2/dbmcolumnstatistics`  | `dbmcolumnstatistics`   |

Every route answers `202 {}` unconditionally (via a `defer` set before any
decoding runs) — that contract is unchanged by this work.

## Why one table for six tracks and every database engine

Each track carries an engine-specific row array under its own key
(`oracle_rows`, `oracle_activity`, `postgres_rows`, `mysql_activity`, ...),
and none of those arrays has a Go type anywhere in this module: the only
in-repo producer is the oracle corecheck
(`pkg/collector/corechecks/oracle/*.go`), which is behind `//go:build oracle`
and excluded from the build; every other engine is a Python integration this
workspace never sees. A table per engine-variant would mean inventing a
schema for data ninjacat genuinely cannot decode, which is the same
antipattern `0001_initial.sql`'s header warns against for metrics, aimed at
engines instead.

So the design is: decode the envelope fields every track agrees on
(`intake/router_dbm.go`'s `dbmEnvelope`) into real columns, and keep the
**complete raw event** in the `event` column — the only lossless
representation of whatever engine-specific rows it carries, and the thing
that makes decoding a new engine a future SELECT + Go type instead of a
permanent gap.

## Engine

`MergeTree` — append-only facts, same class as `metrics`/`logs`/`events`.

## ORDER BY

`(tenant_id, track, host, database_instance, received_at)`

Point-query shape: "what did this database instance report on this track" —
the same reasoning as `metrics`' `(tenant_id, metric, host, timestamp)`, with
`track` standing in for `metric` (it is the same kind of "which shape of row
is this" filter) and `database_instance` added because `host` alone is not a
full database identity — one host can run several instances.

## PARTITION BY / TTL

`PARTITION BY toDate(received_at)`, `TTL toDateTime(received_at) + INTERVAL 30 DAY`.

30 days, not the 90-day human-scale class: query samples and activity
snapshots are bulky — a row can hold a full raw event with dozens of
`oracle_activity`/`oracle_rows` entries plus complete SQL text — so this
table sits with the high-cardinality tables (`metrics`, `sketches`,
`k8s_resources`) rather than the audit-scale ones (`events`, `check_runs`).

## Columns

| Column | Source | Notes |
|---|---|---|
| `tenant_id` | `TenantFromContext(c)` | First ORDER BY column, per CLAUDE.md |
| `received_at` | `time.Now().UTC()` | Arrival time — always set |
| `track` | the route (see table above) | `dbmcolumnstatistics` now matches its route; the previous internal label `dbmcolstat` is gone |
| `timestamp` | envelope `"timestamp"` (unix ms) | `Nullable`; `NULL` when the wire never sent one (`dbmTimestamp`) |
| `host` | envelope `"host"` | the **database** host, not the agent's |
| `database_instance` | envelope `"database_instance"` | |
| `agent_hostname` | envelope `"ddagenthostname"` | metrics track only |
| `agent_version` | envelope `"ddagentversion"` / `"agent_version"` | merged; `dbmmetadata` is the one track that says `agent_version` |
| `source` | envelope `"ddsource"` | samples/activity only |
| `dbm_type` | envelope `"dbm_type"` | `fqt` \| `plan` \| `activity` |
| `kind` | envelope `"kind"` | `lock_metrics`, `database_instance`, ... |
| `dbms` | envelope `"dbms"` | metadata track |
| `dbms_version` | envelope `"dbms_version"` | metadata track |
| `collection_interval` | envelope `"collection_interval"` / `"min_collection_interval"` | `Nullable(Float64)` — a real `0s` and "neither key was sent" are different facts |
| `tags` | `tagsToMultiMap` over the envelope's merged `ddtags`+`tags` | the standard multiset shape, with the bloom-filter index pair on keys/values |
| `extra_keys` | sorted keys of `dbmEnvelope.Extra` | names the engine-specific arrays (`oracle_rows`, `postgres_activity`, ...) and any other undeclared key, without needing a Go type for their contents |
| `undecoded_keys` | `dbmEnvelope.Undecoded` | envelope keys whose *value* did not fit the declared field — a smaller, usually-empty list, distinct from `extra_keys` |
| `event` | the exact request bytes for this event (`json.RawMessage`, never re-marshalled) | the lossless fallback — everything not covered by a column above lives here |
| `db_instance` | `db.instance` (databasequery's `"db"` object) | `Nullable`; only present on `databasequery` events whose `db` object decodes |
| `query_signature` | `db.query_signature` | `Nullable` |
| `statement` | `db.statement` | `Nullable`, `CODEC(ZSTD(3))` — full SQL text, can be large |
| `plan_signature` | `db.plan.signature` | `Nullable`; only on `dbm_type=plan` events |
| `plan_definition_steps` | `len(db.plan.definition)` | `Nullable(UInt32)`; `NULL` when no plan was sent, `0` when a plan was sent with zero steps — different facts, kept distinct. A third case, a plan sent with a `definition` that is not a JSON array, also stores `NULL` (the column cannot represent "malformed") but is not silently folded into "no plan sent": it is recorded in `undecoded_keys` as `db.plan.definition` instead |

## Semantics worth knowing

- **No upsert key.** This is an append-only event log like `logs`/`events`,
  not a current-state table — no `ReplacingMergeTree`.
- **`event` is the source of truth for anything engine-specific.** The
  typed columns above are convenience only; a query-sample's full
  `oracle_rows`/`postgres_activity`/`db.metadata` contents are only in
  `event`, by design (see "Why one table" above).
- **Nothing here is sensitive** in the credential sense, but `event` and
  `statement` can contain full SQL text, which may itself embed literal
  values from application queries — treat this table with the same care as
  `logs.message`.
- **Absent vs. zero, twice over:** `collection_interval` and
  `plan_definition_steps` are both `Nullable` specifically so a genuine `0`
  reported by an integration is never confused with "the field was not
  sent". `timestamp` follows the same rule already established for
  `container_events`/`container_images` in `0001_initial.sql`.

## Decode-error handling

- A request body that does not split into a JSON array or a bare object
  (`dbmEvents` fails) → `storeRaw(c, "dbm", "decode_error", ..., body)`
  (`raw_payloads`), not dropped.
- A batch element that is not a JSON object → `storeRaw` with that one
  element's raw bytes, reason `decode_error`. The rest of the batch is
  still decoded and stored normally. This includes a literal JSON `null`
  element: `encoding/json` special-cases `null` for a map target (no error,
  unlike a string or a number), so `dbmEnvelope.UnmarshalJSON` checks for it
  explicitly — otherwise it would silently become a zero-value row instead
  of reaching `storeRaw`.
- `dbmEnvelope`'s own `UnmarshalJSON` never fails an event over one bad
  field — a field that does not fit its declared type is recorded in
  `undecoded_keys` and its raw value kept in `extra_keys`/`event` instead.
  `storeRaw` is only reached for input `dbmEnvelope` cannot even attempt to
  read.
- With no tenant in context (`TenantFromContext(c) == ""`), both the
  `dbm_events` path and any `storeRaw` calls store nothing — the response
  is unaffected (`202 {}`).

## dropped_by_decision

- **Engine-specific row arrays as typed columns** (`oracle_rows`,
  `postgres_activity`, `mysql_rows`, the schema-collection `metadata`
  object, ...): not decoded into columns. Reason: no Go type exists for any
  of them in this module (Oracle's are `//go:build oracle`-excluded, every
  other engine is Python-only), so typing them would mean guessing a schema
  from captured traffic rather than a published contract. They are fully
  preserved, losslessly, in `event` (and named in `extra_keys`), so nothing
  is actually lost — this is a decode-later decision, not a data-loss one.
- **`db.metadata`'s two shapes** (`dd_tables`/`dd_commands` on `fqt`,
  `tables`/`commands` on `plan`) as columns: not promoted, for the same
  reason — the two shapes genuinely differ and only `event` needs to agree
  with both.
- **A separate `dbm_events_current`/upsert view**: not built. Nothing in
  this data is a "current state" object the way a host or a Kubernetes
  resource is; every event is its own fact, so `ReplacingMergeTree` would be
  the wrong tool.
