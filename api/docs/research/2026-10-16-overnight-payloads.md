# What the agents sent overnight (2026-10-15/16)

Two Kubernetes clusters sent data to one NinjaCat 0.2.0 deployment: **ninjapaw** (one node,
`tiri-ninjapaw`, RKE2, which also runs NinjaCat itself) and **jose** (three nodes, k3s). Both run
Datadog Agent and Cluster Agent **7.84.1** under Datadog Operator v1.31.0. The `ninjacat`
database was created at 2026-10-15 20:41 UTC, and the first agent rows arrived at about 21:03, so
"overnight" means about 16 h, up to about 13:00 on the 16th. Database Monitoring (DBM) was
switched on, and ClickHouse restarted, at **07:29 UTC**. The DBM data therefore covers only
about 5.5 h. Everything is in tenant `default`.

All rates are averages over each table's own window. Sizes are a snapshot taken at about 13:00.

| table | rows | on disk | uncompressed | rows/h | fed by |
|---|---|---|---|---|---|
| `metrics` | 146.3 M | 792 MiB | 32.7 GiB | ~10.4 M | `/api/v2/series` (v2 or v3) |
| `metrics_1m` | 38.8 M | **885 MiB** | 30.9 GiB | ~2.6 M | materialized view on `metrics` |
| `logs` | 7.8 M | 171 MiB | 4.1 GiB | ~485 k | `/api/v2/logs` (HTTP, zstd) |
| `processes` | 5.8 M | 105 MiB | 4.0 GiB | ~360 k | `/api/v1/collector` |
| `containers` | 1.19 M | 71 MiB | 691 MiB | ~74 k | `/api/v1/collector` |
| `k8s_manifests` | 639 k | 73 MiB | 5.0 GiB | ~40 k | `/api/v2/orchmanif` |
| `k8s_resources` | 507 k | 10 MiB | 1.25 GiB | ~31 k | `/api/v2/orch` |
| `check_runs` | 279 k | 0.1 MiB | 26 MiB | ~17 k | `/api/v1/check_run` |
| `process_snapshots` | 70 k | 2.3 MiB | 52 MiB | ~4.4 k | `/api/v1/collector` (one row per frame) |
| `dbm_events` | 48 k | 12 MiB | 344 MiB | ~8.7 k (5.5 h) | `dbm-metrics-intake`, `dbquery-intake` and the other DBM hosts |
| `process_groups` | 16 k | 0.2 MiB | 0.7 MiB | ~970 | `/intake/` (gohai resources) |
| `k8s_cluster` | 11.6 k | 0.3 MiB | 17 MiB | ~765 | `/api/v2/orch` (collector type 46) |
| `events` | 11.1 k | 0.7 MiB | 6.4 MiB | ~680 | `/intake/` and `/api/v1/events` |
| `agent_checks` | 9.0 k | 32 KiB | 3.8 MiB | ~560 | `/intake/` |
| `apm_telemetry` | 7.2 k | 0.7 MiB | 23 MiB | ~450 | `/api/v2/apmtelemetry` |
| `raw_payloads` | 2.4 k | 0.1 MiB | 6.0 MiB | ~148 | fallback, mostly `/api/v1/metadata` |
| `k8s_resources_current` | 1.7 k | 0.6 MiB | 4.6 MiB | – | materialized view on `k8s_resources` |
| `container_events` | 876 | 0.1 MiB | 0.3 MiB | ~55 | `/api/v2/contlcycle` |
| `agent_metadata` | 388 | 0.3 MiB | 14 MiB | 24 | `/api/v1/metadata` |
| `synthetics_results` | 387 | 21 KiB | 62 KiB | 24 | `/api/v2/synthetics` |
| `container_images` | 361 live | 0.4 MiB | 2.0 MiB | ~3 k inserted | `/api/v2/contimage` |
| `hosts` | 8 | 40 KiB | 61 KiB | – | `/intake/` (gohai) |
| `sketches`, `process_stats`, `container_stats`, `process_discoveries` | 0 | – | – | – | – |

Two numbers stand out. First, **83% of `logs` is NinjaCat's own ClickHouse logging at Debug
level**, which the agent ships back to NinjaCat (see Logs). Second, `metrics_1m` takes **more disk
than the raw `metrics`**. Apart from the decode errors listed below, nothing went undecoded:
`raw_payloads` holds only the Datadog Operator's metadata and 4 failures from one memory spike.

## Databases (DBM and DB metrics)

### Window and scope

Six database instances are monitored:

| cluster | instance (`host`) | engine |
|---|---|---|
| ninjapaw | `ninjacat-postgres` | PostgreSQL 18.6 (CloudNativePG) |
| ninjapaw | `ninjacat-clickhouse` (`database_instance` = the ClickHouse service, port 8123, database `default`) | ClickHouse 26.8 |
| jose | two application CNPG clusters, each as `-rw` and `-ro` (called *pg-a* and *pg-b* below) | PostgreSQL 18.4 |

`dbm_events` holds 07:29:50 to 13:03 UTC: 48,005 rows when analysed, 325 MiB of raw `event`
JSON. On disk that is 11.2 MiB, because `event` compresses about 29x under ZSTD(3). That is
roughly 50 MiB/day compressed, or about 1.5 GiB at the 30-day TTL.

- No `raw_payloads` row came from a DBM host, and `undecoded_keys` is empty on every row.
- No `dbmcolumnstatistics` events arrived: column statistics are off on every instance.
- **The analysis contaminated the ClickHouse data.** The ClickHouse check watches the server the
  analysis queried. From about 12:50, those queries, and the agent's EXPLAINs of them, show up as
  FQT events, samples, completions and errors. Cardinalities below are measured before 12:50.

| track | rows | per hour | raw JSON | share of bytes | from |
|---|---|---|---|---|---|
| dbmactivity | 30,300 | ~5,450 | 49 MiB | 15% | all 6 instances; ClickHouse alone ~3,800/h |
| databasequery | 12,359 | ~2,220 | 89 MiB | 27% | all 6 |
| dbmmetrics | 3,990 | ~720 | 145 MiB | **45%** | only the two ninjapaw instances |
| dbmmetadata | 1,330 | ~240 | 42 MiB | 13% | all 6 |
| dbmhealth | 13 | ~2 | 45 KiB | 0% | both agents |

### The envelope: what NinjaCat makes columns of

| track | dbm_type | kind | collection_interval | envelope columns filled |
|---|---|---|---|---|
| databasequery | `plan` (samples), `fqt` | – | NULL | host, database_instance, source, agent_version, plus db_instance, query_signature, statement, plan_* |
| dbmactivity (PG) | `activity` | – | 10 | host, database_instance, source |
| dbmactivity (CH) | `activity`, `query_completion`, `query_error`, `storage_health` | – | 1 / 10 / 10 / 60 | as PG; `dbms` and `dbms_version` **only** on `storage_health` |
| dbmmetadata | – | `database_instance`, `pg_settings`, `pg_extension`, `pg_databases` | 300 / 600 | dbms, dbms_version, kind; `agent_hostname` only on `database_instance` |
| dbmmetrics | – | – | 10 (`min_collection_interval`) | host; `database_instance` only for ClickHouse |
| dbmhealth | – | – | NULL | `agent_hostname` and tags; **`host` and `database_instance` empty** |

`extra_keys` per track:

- databasequery: PG `cloud_metadata, db, duration, network, postgres, service` (plans sometimes
  without `duration`; PG FQT only `db, postgres, service`); CH `clickhouse, db`.
- dbmactivity: `<dbms>_activity, <dbms>_connections, service` (plus `cloud_metadata` on PG). CH
  completions and errors: `clickhouse_query_completions` or `clickhouse_query_errors`,
  `clickhouse_version`, `service`. CH storage health: `clickhouse`.
- dbmmetrics: `cloud_metadata, postgres_rows, postgres_version, service` or
  `clickhouse_rows, clickhouse_version`.
- dbmmetadata: `cloud_metadata, metadata`; `pg_databases` adds `collection_payloads_count,
  collection_started_at`; `database_instance` adds `database_hostname, integration_version, port`.
- dbmhealth: `category, check_id, data, name, status, version`.

What is wrong or inconsistent in the envelope:

1. **The engine is not in one place.** `dbms` is empty on samples, activity and metrics; `source`
   is empty on metrics, metadata and health. Today a reader infers the engine from the array name
   (`postgres_rows` or `clickhouse_rows`).
2. **Postgres `dbmmetrics` has an empty `database_instance`.** The event has no top-level key, but
   `tags` carries `database_instance:<name>`. ClickHouse sends the key.
3. **`dbmhealth` has an empty `host` and `database_instance`.** Both are in `data.database_instance`.
4. **`db_instance` is the logical database, not the instance.** It comes from `db.instance`: the
   current database for ClickHouse (empty, stored as NULL, on 321 of 1,395 FQT rows), `datname` for
   PG. The name invites confusion with `database_instance`.
5. The `dbm_type` comment at `api/schema/migrations/0001_initial.sql:1852` lists `fqt | plan |
   activity`; ClickHouse adds `query_completion`, `query_error` and `storage_health`.
6. `timestamp` is never NULL and usually 0–10 s before `received_at`. The exception is PG samples of
   idle backends, where it equals the backend's `state_change` and can be hours old. Bucket samples
   by `received_at`.
7. No exact duplicates: `uniqExact(cityHash64(event)) = count()` on every track. There is semantic
   duplication in `dbmmetadata` (see below).

### databasequery: samples and FQT

**FQT** (`dbm_type = 'fqt'`) is a dictionary entry: signature → obfuscated statement. The agent
sends one for each newly seen signature and refreshes it about hourly.

- PG: `db{instance, query_signature, statement, metadata{tables, commands, comments}}` and
  `postgres{datname, rolname}`. Only `ninjacat-postgres` sends PG FQT: 532 rows, 83 signatures.
  FQT comes from the pg_stat_statements job, which fails on jose (see dbmmetrics).
- CH: the same `db` (with `metadata.comments` always null) plus
  `clickhouse{normalized_query_hash, user}`. About 100–120/h.

**Samples** (`dbm_type = 'plan'`) are sent for every sample, whether a plan was obtained or not.

- PG keys:
  - `db{application, instance, metadata, plan{definition, signature, collection_errors},
    query_signature, query_truncated, resource_hash, statement, user}`.
  - `postgres{pid, datid, usesysid, backend_type, backend_start, query_start, state, state_change,
    now, xact_start, backend_xid, backend_xmin, wait_event, wait_event_type, blocking_pids,
    query_signature, statement, dd_tables, dd_commands, dd_comments}`.
  - `network.client{ip, port, hostname}`, `duration` (ns, int or float), and an all-null
    `cloud_metadata{aws, azure, gcp}`.
- 60–320 samples/h per instance. The agent rate-limits per signature: the busiest jose instance
  sent 1,718 samples for 86 signatures, at most about 20 per signature in 5.5 h.
- `resource_hash` has the same cardinality as `query_signature`.
- Median `duration` 0.1–0.3 ms, maximum 250 ms.
- **92% of jose samples are idle backends**: the last statement of an idle connection waiting on
  the client. About 6% are `active` and 1% `idle in transaction`.
- Statements are obfuscated with `?` (none contain `$n`). `query_truncated` is `truncated` or
  `not_truncated`; jose truncates at `track_activity_query_size = 1024`.
- **No Postgres plan was collected on any instance.** `db.plan.definition` and `signature` are null
  on all 3,300+ PG samples:

  | `collection_errors` code | rows | cause |
  |---|---|---|
  | `invalid_schema` | 2,839 | the `datadog` schema and its `explain_statement` function were never created |
  | `no_plans_possible` | 447 | – |
  | `query_truncated` | 50 | – |

  The agent's own `dd.postgres.run_explain.error` series agrees.

- CH samples carry `clickhouse{query_duration_ms, query_kind, user}` and
  `db.plan{definition, signature, collection_errors}`.
  - 5,877 samples have a plan. `definition` is a **JSON string**, ClickHouse's `EXPLAIN json = 1`
    (`Plan` → `Node Type`, `Node Id`, `Description`, `Expression.Inputs[]`…), 10.8 KB on average,
    34 KB at most.
  - 660 failed with `database_error`. The agent runs EXPLAIN as its own user in database `default`:
    NinjaCat's tables are unknown there (code 60), tables it has no grant on fail with code 497, and
    obfuscated text fails to parse (code 62).
  - **Plan signatures are unstable.** Three signatures (the agent's own `system.*` queries) produced
    a new `plan_signature` on almost every sample, about 300 each. Removing digits from `definition`
    collapses each to one shape: the plans keep numeric literals, and those feed the signature.
  - **84% of CH sample statements read `system.*` tables**, as the agent's monitoring user. Most
    ClickHouse samples are the agent watching itself.
- `plan_definition_steps` is NULL on every row. That is correct, because both engines send a
  string, not a step list (`PlanSteps.Document`, `api/src/Intake/Routers/Dbm.fs:160`). Nothing
  about a plan beyond its signature is decoded.

### dbmactivity: live sessions

**Postgres**, every ~11 s:

- `postgres_activity[]`: one object per non-idle backend, with `pid, datid, datname, usename,
  usesysid, application_name, client_addr, client_port, backend_type, backend_start, xact_start,
  query_start, state, state_change, now, wait_event_type, wait_event, backend_xid, backend_xmin,
  blocking_pids[], query_signature, query_truncated, statement, dd_tables, dd_commands,
  dd_comments`.
- The array is nearly always empty. On the jose primaries it averages one row (max 21), almost
  always the `walsender` replicating to the `-ro` standby. Client waits seen: `LWLock/WALWrite`
  (27), `IO/WalSync` (7), `Client/ClientRead`, `IO/DataFileRead`. `ninjacat-postgres` had 3 active
  rows in 1,784 snapshots.
- `blocking_pids` was never non-empty: no lock blocking in the window.
- `postgres_connections[]`: counts by `{application_name, datname, state, usename}`, 2–12 groups.

**ClickHouse** sends four kinds:

| kind | every | content |
|---|---|---|
| `activity` | 1 s | `clickhouse_activity[]` (**83% of 19,300 rows empty**): `query_id, initial_query_id, is_initial_query, user, initial_user, address, port, client_*, http_user_agent, current_database, query_kind, elapsed, memory_usage, peak_memory_usage, read/written rows and bytes, total_rows_approx, is_cancelled, thread_ids[], clickhouse_node, query_signature, statement, dd_commands`; plus `clickhouse_connections[]` `{user, current_database, query_kind, connections}` |
| `query_completion` | 10 s | `clickhouse_query_completions[].query_details`, 5.9 per event (max 48): `query_id, username, database_name, query_kind, query_type, duration_ms, cpu_us, cpu_wait_us, memory_usage, read/written/result rows and bytes, query_start_time_microseconds, event_time_microseconds, normalized_query_hash, query_signature, statement, metadata{tables, commands, comments}, clickhouse_node` |
| `query_error` | 10 s | the completion fields plus `exception_code, exception, stack_trace, client_ip` |
| `storage_health` | 60 s | `clickhouse{thresholds[], top_tables_by_parts[], active_merges[], pending_mutations[], replication_queue[], detached_parts[]}` |

- Completions have no duration threshold (minimum 0 ms): 7,010 completions against about 185 k
  executions, roughly a 4% sample. 52% are the agent's own queries.
- Before the analysis, errors were almost all the agent's failed EXPLAINs (code 60: 534, 497: 147,
  62: 7).
- In `storage_health`, **every array but `thresholds` was empty in all 329 events**, including
  `top_tables_by_parts` on a server with about 100 tables. `thresholds` holds
  `parts_to_delay_insert`, `parts_to_throw_insert`, `merge_with_ttl_timeout` and
  `min_age_to_force_merge_seconds`.

None of these arrays is decoded (TODO.md §4, "DBM, the rest").

### dbmmetrics: per-query statistics

**Only the ninjapaw instances send it.** The four jose instances each report
`dd.postgres.statement_metrics.error` with reason `pg_stat_statements_not_created` 1,930 times:
the extension does not exist in the database the agent connects to. Without it there is no PG FQT
from jose either.

**The rows are deltas, not running totals**, for both engines. The integration diffs two
collections and sends only queries whose call count changed. One signature shows `calls`
`[1, 1, 1, …]` or `[10, 9, 9, …]` every 10 s. This answers the open question in TODO.md §4.

- `postgres_rows[]`, about 37 per event (max 73):
  - identity: `query_signature, queryid, query` (obfuscated text, repeated every 10 s), `datname,
    rolname, dd_tables, dd_commands, dd_comments`;
  - counters: `calls, rows, total_exec_time` (ms, float), `total_plan_time`,
    `min/max/mean/stddev_plan_time`;
  - blocks: `shared_blks_hit/read/dirtied/written`, `shared_blk_read_time/write_time`,
    `local_blks_*`, `temp_blks_read/written`;
  - WAL: `wal_records, wal_fpi, wal_bytes`. There is no `total_time` key (PG 13+ naming).
- `clickhouse_rows[]`, about 39 per event (max 97):
  - identity: `query_signature, normalized_query_hash, query, query_type, user, databases,
    server_node, exception_code` (a string);
  - counters: `count, total_time, mean_time` (int in 73% of rows, float in the rest: read with a
    float extractor), `cpu_us, cpu_wait_us, memory_usage, peak_memory_usage`, read/written/result
    rows and bytes; `dd_tables, dd_commands` (`dd_comments` always null).
- Cardinality, 07:29–12:50: **83 PG signatures** (94 signature × role × database) and **171 CH
  signatures**.
- **Most per-query telemetry describes the monitoring itself.**
  - On `ninjacat-postgres` the agent's role accounts for 51 of the 83 signatures and 62% of calls;
    the CNPG metrics exporter for 35%. NinjaCat's own role ran 10 signatures and 3% of calls.
  - On ClickHouse the intake ran 45 signatures and 52% of calls, the agent 266 signatures (mostly
    `system.*`, failing ones included) and 39%, the monitor service 4 signatures.
  - A panel needs a way to exclude the agent's user.
- dbmmetrics is the largest track by bytes (45%) because every row repeats the full `query` text
  every 10 s.

### dbmmetadata: inventory, settings, extensions, schemas

- `database_instance`, every 300 s: `metadata{dbm, connection_host}`; ClickHouse adds
  `{hosting_type, single_endpoint_mode, cluster_name, connect_node, nodes[]}`. The envelope has
  `port, database_hostname, integration_version`. This is the instance's inventory record.
- `pg_settings`, about every 20 min: 398–404 rows of `{name, setting, source, sourcefile,
  pending_restart}`, about 51 KB per event. **Only 2 distinct payloads per instance in 5.5 h**:
  about 95% of these events repeat the previous one.
- `pg_extension`, about every 20 min: `{id, name, version, owner, schema_name, relocatable,
  logical_database}`, 1–2 rows.
- `pg_databases` (`collect_schemas`, every 600 s). **Schema collection works.** Nesting:
  `metadata[] → {id, name, owner, encoding, description, schemas[] → {id, name, owner, tables[] →
  {id, name, owner, relkind, toast_table, columns[], indexes[], foreign_keys[]}}}`.
  - `columns`: `{name, data_type, nullable, default, table_id}`.
  - `indexes`: `{name, definition, is_unique, is_valid, is_ready, is_live, is_partial,
    is_clustered, is_exclusion, is_immediate, is_replident, is_checkxmin, table_id}`.
  - `foreign_keys`: `{name, definition, table_id}`. No `partitions` key appeared; `relkind` is
    always `r`.
  - One collection is split across several events, each with **its own** `collection_started_at`
    and `collection_payloads_count = 1`. The same database repeats inside one event, each entry
    with a slice of its tables. A reader must merge by `(database, schema, table)` within one
    10-minute cycle.

  | instance | databases | schemas | tables | columns | indexes |
  |---|---|---|---|---|---|
  | ninjacat-postgres | 1 | 2 | 10 | 87 | 24 |
  | pg-b (jose) | 1 | 1 | 52 | 389 | 94 |
  | pg-a (jose) | 5 | 5 | 180 | 1,269 | 607 |

  Each pg-a host stored 14.6 MiB of schema metadata in 5.5 h.
- `-rw` and `-ro` are the same CNPG cluster, so **schemas, settings and extensions are stored
  twice**. In one window pg-a `-rw` even counted the same tables twice, which suggests overlapping
  cycles.
- ClickHouse sends **no** schema or settings metadata; `schema_metrics` is off in its health
  `features`.

### dbmhealth

13 events. `category` is `postgresql` or `clickhouse`, `name` is `initialization` or
`missed_collection`, `status` is `ok` or `warning`; each carries `check_id`, `version = 1` and the
agent's tags.

- `initialization`: `data{database_instance, ddagenthostname, initialized_at, errors[], warnings[],
  config{…}, instance{…}, features[{key, name, enabled, description}]}`. `config` has 67 keys for
  PG and 36 for CH. Errors and warnings were empty everywhere, and the password is masked in both
  `config` and `instance`.
  - `features` is the best record of what DBM collects on an instance. PG: relation metrics
    **off** (they need `relations`), data observability off, column statistics off. CH:
    schema metrics and single-endpoint mode off.
- `missed_collection`, one per instance: `data{job_name = query-samples, last_run_start,
  elapsed_time ≈ 1000–1155, expected_collection_interval = 1000, …}`. The samples job ran just over
  its 1 s budget.

### DB series in `metrics` and `check_runs`

Last hour of the window:

| family | metrics | series | `host` | notes |
|---|---|---|---|---|
| `postgresql.*` | 114 | 4,401 | the instance name (5 hosts) | tags `database_instance, db, role, replication_role, cnpg_cluster, postgresql_version, system_identifier, kube_cluster_name, …` |
| `clickhouse.*` | 1,349 | 1,396 | **empty** | server-wide `metrics.*`, `events.*`, `threads.*`, `asynchronous_metrics.*`; instance tags only |
| `dd.postgres.*`, `dd.clickhouse.*` | 64 / 17 | 1,048 / 17 | – | the integrations' own job timings and errors |
| `cnpg.*` (OpenMetrics) | 64–84 | 1,080 | node name | no `kube_cluster_name` |
| `clickhouse_prom.*` (jose) | 59 | 118 | empty | tags `endpoint, clickhouse_replica` |

- `postgresql.*` by subfamily: `io` 2,680 series (by `backend_type`, `object`, `context`);
  `activity.wait_event` 80 (by app, user, wait event); per-database sessions, conflicts, deadlocks,
  rows, commits, buffer hit, size; replication and `wal_receiver`; bgwriter, wal, archiver, slru,
  control, snapshot.
  - No `postgresql.queries.*`: in DBM mode query metrics go only to dbmmetrics.
  - No `postgresql.relation*` / `index*`: relation metrics are off.
  - `postgresql.table` has a `schema` tag but no `table` tag.
- `clickhouse.*`: `parts.*`, `merge.*`, `table.*`, with effectively no per-table split.
  `clickhouse.dd.*` (6 metrics, 45 series) is tagged only `operation`. Nothing named
  `datadog.dbm.*` arrived.
- Useful `dd.postgres.*`: `statement_metrics.error`, `run_explain.error`,
  `statement_samples.error` (each with an `error` reason), `schema.tables_count`,
  `schema.payloads_count`, `collect_*_snapshot.time.*`, `async_job.missed_collection`.
- `cnpg.pg_extensions*` carries `extname, installed_version`; it would show the missing
  pg_stat_statements on jose.
- **Join key.** `postgresql.*` has `host` = `dbm_events.host`. `clickhouse.*` has an empty host,
  so join on `tags['database_instance']` = `dbm_events.database_instance`, the only key both engines
  share. `metrics.resources` also carries `database_instance`, but the query engine does not read
  `resources` (see Metrics).

`check_runs` over the window are all OK: `postgres.can_connect` (5 instances, ~240/h each),
`clickhouse.can_connect` (empty host, tagged `database_instance`), `cnpg.openmetrics.health` (3
node hosts), `clickhouse_prom.openmetrics.health` (empty host).

### How to read the database telemetry

| question | source |
|---|---|
| Is the server up and healthy? | `check_runs` `*.can_connect` and `*.openmetrics.health`; then `postgresql.*` (connections, `percent_usage_connections`, buffer hit, deadlocks, replication lag, WAL, `before_xid_wraparound`), `clickhouse.*` (`parts.*`, `merge.*`, memory, query counters), `cnpg.*`. Scope by `database_instance`. |
| Which queries cost the most, and how did that change? | `dbmmetrics`: sum the deltas per `query_signature` (and role/user, database) over the range; rank by `total_exec_time`/`total_time`, `calls`, `rows`, `shared_blks_read`, `wal_bytes`, `cpu_us`. Join the signature to the latest FQT `statement` for the text. Exclude the agent's user. Only where pg_stat_statements works, and for ClickHouse. |
| What is running now, and who waits on what? | `dbmactivity` `activity`: the `*_activity` rows (state, wait event, `blocking_pids`, elapsed, memory) and `*_connections`. `postgresql.activity.wait_event` is the cheap time-series form. |
| Example executions and plans | `databasequery` `plan` events (PG with backend context, CH with plans); ClickHouse `query_completion` and `query_error` for single executions with CPU, memory, rows and exceptions. No PG plans until the `datadog` schema function exists. |
| What do schema, settings and extensions look like? | `dbmmetadata`: merge `pg_databases` per cycle; latest `pg_settings` and `pg_extension` per instance; `database_instance` for the inventory. |
| Is the monitoring itself working? | `dbmhealth` (features, init errors, missed collections) and the `dd.postgres.*` / `dd.clickhouse.*` error counters. |

What the panel can build from this today: an instance list with health and enabled features; top
queries per instance (ninjapaw only); a live-sessions view; a schema browser with columns and
indexes; a settings diff over time; a ClickHouse slow and failed query log.

## Metrics

Window 2026-10-15 20:40 to 2026-10-16 12:56 UTC.

- **Volume.** 94,109 distinct series, 2,531 names, 22 `host` values. A steady 15 s cadence (4
  points per minute per series); about 40 k active series per hour, with spikes to 53 k at 22:00
  and 51 k at 07:00. About 10.4 M points/h now, 8.8 M/h without the DB families.
- **Sources.** Checks on the node and cluster agents (origin product 10, category 11, source type
  `System`). The stored rows do not say whether a point came over v2 or the v3 columnar route.
  - jose: about 98 M points (72%), including 16.8 M host-less cluster-level rows.
  - ninjapaw: about 37 M points, including 7.35 M host-less rows and NinjaCat's self-monitoring.
  - DB families: 6% of points over the window, 15.6% since DBM started (7,007 series).

Namespaces over the window, DB families excluded:

| namespace | points | series | names |
|---|---|---|---|
| kubernetes_state | 40.6 M | 12,765 | 128 |
| container | 26.4 M | 22,301 | 32 |
| kubernetes (kubelet) | 23.3 M | 24,221 | 90 |
| containerd | 20.2 M | 13,218 | 19 |
| system | 13.9 M | 5,016 | 155 |
| datadog | 5.17 M | 3,077 | 246 |
| cri | 2.21 M | 1,807 | 3 |
| dd (DBM check internals) | 1.20 M | 1,083 | 81 |
| redis | 1.16 M | 2,426 | 154 |
| cert_manager | 0.83 M | 580 | 9 |
| ninjacat (self-monitoring) | 0.70 M | 447 | 22 |
| kube_apiserver | 0.25 M | 134 | 1 |

- Top names by points: `kubernetes_state.crd.condition` (5.5 M),
  `kubernetes.kubelet.runtime.operations.duration.count` (3.4 M, histogram buckets with
  `upper_bound`), `containerd.blkio.serviced_recursive` and `…service_recursive_bytes` (3.0 M
  each), `containerd.image.size` (2.8 M).
- **The same containers are measured four times**, by `container.*`, `containerd.*`, `cri.*` and
  `kubernetes.*`: together about 53% of non-DB points.
- `datadog.operator.*` alone has 205 names (Go runtime metrics), about 230 k points/h.

### How to read a row

One `metrics` row is one point `(metric, host, tags, timestamp, value)`. `series_id` hashes the
tenant, metric, host and sorted tags; resources, unit and origin are not part of it. One
`metrics_1m` row is one series in one minute, as aggregate states (`countMerge(points)`,
`avgMerge(value_avg)`, …, `argMaxMerge(value_last)`, `sumMerge(value_events)` for RATE).

- Tags: 12.9 keys per series on average (max 28), about 570 characters serialised.
- Highest cardinality in the last hour: `image` (563), `kube_replica_set` (339), `uid` (303),
  `container_id` (253), `device_name` (237), `pod_name` (201).
- **Host tags are not on the points.** `system.cpu.user` has no tags at all; its cluster is only in
  `hosts.tags`.

| type | interval | points | names | what |
|---|---|---|---|---|
| GAUGE | 0 | 129.9 M | 783 | every check gauge, including agent-computed rates and cumulative counters |
| COUNT | 0 | 5.2 M | 126 | the checks' monotonic counts |
| COUNT / GAUGE | 15 | 0.7 M | 22 | `ninjacat.*` self-monitoring |
| RATE | 0 | 0.23 M | 12 | only `dd.postgres.*.count` and `dd.clickhouse.*.count` |
| COUNT / GAUGE | 10 | 46 k | 4 | `datadog.agent.started`, `datadog.process.*.host_count` |

### Decoded, stored, surprising

- **Little goes unread.** `extra` is empty everywhere; `unit` only on `ninjacat.*`; no NaN or Inf;
  almost no duplicate `(series_id, timestamp)`. The rollup matches the raw points exactly
  (34,240,625 points in both for 01:00–05:00).
- **`resources` is stored but never read.** 27% of rows have it: `device` on system, container,
  containerd and kubelet IO, `database_instance` on `postgresql.*` and `dd.*`. The engine
  (`api/src/Engine/Query/Compile.fs`) resolves only `host` and `tags[...]`, so `by {device}` and
  `{database_instance:…}` cannot be asked, and `kubernetes.io.*` has no `device_name` tag to fall
  back on. `series_id` also leaves `resources` out of its hash; no collisions today, but nothing
  guarantees that.
- **Host tags are not joined.** `{kube_cluster_name:jose}` matches only series that carry the tag
  themselves (cluster agent and KSM, about 30% of points). Every `system.*`, `container.*` and
  kubelet series is missed.
- **A host renamed mid-window.** One jose node reported as `jose1` until 22:02, then as
  `jose1-jose`: two `hosts` rows and 8.8 k orphaned series (also in TODO.md).
- **Churn.** 36 k of 87 k non-DB series lived under an hour, mostly short-lived pods on jose.
  `redis.*` churns with a pod-address tag. Self-monitoring uses the pod name as `host`, so 9
  monitor "hosts" appeared in 16 h.
- **The rollup is larger than the raw table.** `metrics_1m` is fully merged and takes 885 MiB
  against 792 MiB. Its `tags` (a plain `Map(String, Array(String))` on every minute row) take
  503 MiB; `value_last` 118 MiB; each other state 50–60 MiB. At today's rate: raw about
  1.4 GB/day × 30 d ≈ 42 GB; rollup about 1.5 GB/day × 400 d ≈ 600 GB.
- **`metrics.pod_name` costs 194 MiB, 25% of the table.** It is a MATERIALIZED plain `String` that
  compresses 17x, against 97x for `tags`, which already holds the value.
- **A metric name the parser cannot read.** `kubernetes.nvidia.com/gpu.limits` and `.requests`
  are stored verbatim. Datadog turns such characters into `_`; the parser
  (`api/src/Engine/MetricQuery/Parser.fs:178`) stops a name at `/`.
- `n_o_i_n_d_e_x.datadog.agent.data_plane.preflight_mode.probe` uses Datadog's "do not index"
  prefix and is listed like any other metric.
- **RATE with interval 0 is real**: exactly the 12 `dd.*.count` names. `as_count()` multiplies them
  by 1 s and undercounts by the check interval (the WARNING at `Compile.fs:61`).
- **Lost writes.** `ninjacat.storage.rows.failed` reports 1,915 `metrics` rows lost at 21:06 and
  38 at 07:28 (the ClickHouse restart), after the agent had its 202. `datadog.agent.point.dropped`
  stayed 0.
- **`sketches` is empty, correctly.** Nothing on either cluster sends DogStatsD distributions, and
  OpenMetrics histograms arrive as `upper_bound` buckets because
  `histogram_buckets_as_distributions` is off.
- **The `query` process sends no self-monitoring.** Only `IntakeHost.fs:85` registers
  `SelfMonitor`.

Panel candidates: per-cluster and per-node infrastructure views; pod readiness and restarts from
`kubernetes_state.*`; container top-N; an agent-health page from `datadog.agent.*`; a NinjaCat
self-health page from `ninjacat.*`.

## Logs

Window 2026-10-15 21:05 to 2026-10-16 12:58 UTC. 7.69 M rows when analysed, about 23 B per row on
disk; `message` is 149 MiB of the 168 MiB. Every row is a container log tailed by the agent (all
containers are collected on both clusters), sent as zstd JSON arrays to `/api/v2/logs`. The TCP
transport is not used: `timestamp_source` is `timestamp_ms` on every row.

| cluster | rows | rate before 07:29 | rate after |
|---|---|---|---|
| ninjapaw | 6.47 M | 359 k/h | 500 k/h (+39%) |
| jose | 1.22 M | 34 k/h | 71 k/h (x2.1) |

| producer | rows | share | avg length |
|---|---|---|---|
| ninjapaw / NinjaCat's ClickHouse | 6.38 M | **82.9%** | 209 |
| jose / crowdsec agent | 207 k | 2.7% | 74 |
| jose / GitHub Actions runners | 115 k | 1.5% | 86 |
| jose / an application's embedder | 103 k | 1.3% | 58 |
| jose / Datadog agent | 67 k | 0.9% | 492 |
| jose / an application's worker | 60 k | 0.8% | **7,746** (largest by bytes on jose) |

NinjaCat's own containers besides ClickHouse add little: intake about 22 k rows, monitors about
500.

### How to read a row

One row is one line, or one aggregated multi-line event, from one container: `host`, `service`,
`source`, `status`, `message`, `timestamp`, `tags`, `attributes`.

- On every row: `container_id`, `container_name`, `display_container_name`,
  `kube_container_name`, `pod_name`, `pod_phase`, `kube_namespace`, `kube_qos`, `image_*`,
  `short_image`, `filename`, `dirname`. On most: `kube_ownerref_*`, `kube_app_*`,
  `kube_stateful_set`, `persistentvolumeclaim`. Only `kube_service` is multi-valued (up to 3).
- **No `kube_cluster_name` and no `env`.** The cluster can only be inferred from the host suffix.
- `service` and `source` equal `short_image` on 99.2% of rows; only pods with unified service
  tagging differ. So a Postgres pod has the image name as `source`, not `postgresql`.
- Mostly plain text. About 1.9% are whole JSON objects stored as text: CNPG (`level, logger, msg,
  ts, …`), controller-runtime and Flux, Traefik access logs (~30 keys), NinjaCat's .NET console
  logger (`Category, EventId, LogLevel, Message, State`).
- Length p50 170, p90 391, p99 1,589, max 33,084 characters; no truncation markers.
- Multi-line: the agent's auto multi-line detection tags the row `multiline:auto_multiline` and
  joins lines with a literal backslash-n, never a real newline.

### Decoded, stored, surprising

- **`attributes` is `{}` on every row.** Container logs carry only the declared fields.
- **A self-amplification loop.** NinjaCat's ClickHouse logs at Debug: 1.49 M Debug lines in 3 h
  against 7.7 k Information. Every insert produces log lines, the agent ships them back, and
  NinjaCat inserts them again. When DBM added more writes, the rate rose from 356 k/h to 496 k/h.
- **`status` is the stream, not the level.** stderr becomes `error`, stdout `info`, nothing else.
  91.5% of rows are `error`, including ClickHouse Debug lines and CNPG lines with `level:info`,
  while the agent's own ERROR lines and NinjaCat's warnings are `info`. Datadog fixes this
  server-side with JSON auto-parsing and status remapping; NinjaCat does neither.
- **`timestamp` is when the agent encoded the line.** ClickHouse's embedded times are 0.5–2 s
  earlier. At startup the agent read a backlog (218 k rows in minute 21:06, 151 k in 21:07), and
  lines about 21:05:56 events are stamped 21:11. With no `received_at` column, this lateness cannot
  be measured in general.
- **The host rename** (`jose1` → `jose1-jose` at 22:02) splits that node's log history too.
- **Three batches lost** at 21:05:56, kept in `raw_payloads` as `decode_error`: two 1.1–1.4 MB
  arrays hit `OutOfMemoryException` in `Logs.parseItems`, and one 15 KB body is still a zstd frame:
  `Body.decompress` failed and passed the body through, so the JSON parse failed.
- Duplicates are negligible: 262 rows in an hour share host, container, timestamp and message,
  and they are genuine repeats.
- **DBM side effects in the logs.**
  - jose's agent Postgres check: about 8 k warnings and 1.4 k errors per hour, about 98% about
    pg_stat_statements, all stored as `info`.
  - jose's Postgres servers went from about 120 to 4.4 k lines/h, about 94% errors caused by the
    agent's queries.
  - NinjaCat's ClickHouse: about 300 errors/h since 07:29 (codes 60 and 497 from the agent's
    EXPLAINs), each logged twice.
- **NinjaCat's intake noise.** 18.8 k unhandled-endpoint warnings, mostly internet scanners probing
  the root host, and 3.3 k "flushes in flight" deferrals, 2.9 k of them for `k8s_manifests`.

Panel candidates: a log explorer with facets on namespace, container, service and host (cluster
from the host suffix), with a level derived from the message rather than `status`; a "noisiest
containers" view by rows and bytes per hour, which would have caught the loop. Trace correlation
is not possible yet: no row carries a trace id.

## Processes and containers

Window 2026-10-15 21:03 to 2026-10-16 12:56 UTC. Five host names: `tiri-ninjapaw`, `jose1-jose`,
`jose2-jose`, `camilo-1-jose`, and `jose1` (the same node before 22:02).

| table | rows | on disk | per hour | fed by |
|---|---|---|---|---|
| `processes` | 5.72 M | 102 MiB (4.0 GiB raw) | ~360 k | `CollectorProc.processes[]` |
| `containers` | 1.18 M | 69 MiB | ~74 k | `CollectorProc.containers[]` |
| `process_snapshots` | 69,378 | 2.3 MiB | ~4.4 k | one row per frame (828 MiB of frames) |
| `process_groups` | 15,418 | 174 KiB | ~970 | `/intake/` resources |
| `container_events` | 867 | 69 KiB | ~55 | `/api/v2/contlcycle` |
| `container_images` | 257 live, 46 k inserted | 455 KiB | ~3 k inserted | `/api/v2/contimage`, whole inventory every 5 min |
| `process_stats`, `container_stats`, `process_discoveries` | 0 | – | – | see below |

Every host sends a snapshot every 10 s (median gap 10.0 s, worst 52 s), split into chunks of at
most 100 processes. A snapshot is the set of frames that share `(host, group_id)`. At the 7-day
TTL, `processes` levels off at about 1.1 GiB and `containers` at about 0.7 GiB.

### How to read it

- **`processes`**: one process in one snapshot, keyed `(host, pid, create_time)`; each process
  appears about 300 times an hour, 250–470 processes per host. Filled: `pid`, `ns_pid` (differs on
  72%, i.e. containerised), `ppid`, `comm`, `cmdline` (always `args` joined by spaces), memory,
  CPU percentages, threads, context switches, `state`, `container_id` (54%, always resolvable in
  the same frame), `language`, `injection_state`, `port_tcp` (18%), `generated_service_name`
  (19%), `process_context`, `service_resources`, OS and kernel.
- **`containers`**: one container in one snapshot: `id`, `type = containerd`, `state`, `health`,
  `created`/`started`, CPU/memory/IO/network rates, thread count and limit, limits and requests
  (31–47%), addresses and ports (68%), `repo_digest`. **`name` and `image` are empty on every
  row**; the identity is only in `tags` (about 19 keys: container name, namespace, pod, owner,
  image name/tag/id, QoS, git sha).
- **`process_snapshots`**: one frame. `host_id`, `host_num_cpus`, `host_total_memory`,
  `host_all_tags` and `header_timestamp` are always empty. `agent_container_count` is per chunk.
- **`process_groups`**: exactly **20 rows per 300 s snapshot**, a top-20 list rather than the whole
  host. `cpu_pct` is always 0.
- **`container_events`**: only `Delete` (796 Container, 71 Pod). Filled: `container_id`,
  `exit_code` (614 rows, 536 nonzero), `exited_at`, `owner_type = Pod` and `owner_uid`. Always
  empty: `container_name`, `created_at`, every state-transition column.
- **`container_images`**: one image per node; 181 digests, 102 names, 7 registries, median 81 MiB,
  largest 7.65 GiB, 17.5 layers on average with full history. `published_at` is never set.

### Decoded, stored, surprising

- **Nothing went undecoded.**
- **About 28 `processes` columns are always empty** from agent 7.84: `tags`, `open_fds`, every
  `io_*`, `cpu_core_*`, `mem_lib`/`mem_dirty`, `euid`/`egid`/`suid`/`sgid`, `net_*`, `byte_key`,
  `container_key`, `process_host`, `network_id`, `dd_service_name`, `tracer_*`, `zombie_*`,
  `root`, `on_disk`. `exe` and `cwd` are filled on 7%. The decoder maps them; the agent does not
  send them (IO and fds need system-probe or more privilege).
- **`user` is empty on 91% of rows with uid > 0**: container uids do not resolve on the node. Show
  `uid` when the name is missing.
- **The empty tables are by design.** The realtime stream (`process_stats`, `container_stats`)
  starts only when the reply announces active clients, and `replyFrame` sends 0
  (`api/src/Intake/Routers/Process.fs:861`). `process_discoveries` runs only when process
  collection is off.
- **Duplicates.** A container whose processes span two chunks appears twice in one snapshot
  (1–3%). Count containers with `uniqExact(id)` per `(host, group_id)`.
- **Storage.** `processes.request_id` and `snapshot_id` are about 20% of the table and already on
  `process_snapshots`. In `containers` the ten Float32 rate columns compress about 1:1 (~45 of
  69 MiB), because the sort key interleaves containers.
- **Redundancy.** The container numbers repeat the `container.*` metrics. What only this feed has
  is the listing: process ↔ container ↔ ports ↔ arguments.
- **Timestamps are arrival time** (`Containers.fs:149, 341`). A Delete arrives about 5 min after
  `exited_at` for crash-looping containers (max 30 min), so sort by `exited_at`.
- **Scrubbing gap.** One process (cert-manager's ACME solver) carries a bare `token` argument
  unscrubbed. The agent's default sensitive words cover `access_token`/`auth_token` but not
  `token`. This one is public by design, but a real secret passed the same way would be stored.
- **A real finding on jose.** The crowdsec agent DaemonSet crash-loops on all three nodes (exit 1,
  about 11 deletes/h per node, the 5-minute back-off rhythm); it accounts for most nonzero exits.
  About half of the deleted containers never appeared in a 10 s snapshot, so only `owner_uid` →
  the pod in `k8s_resources` can name them.

Panel candidates: a live process list per host with drill-down to container, ports and service; a
restart and OOM timeline from `container_events`; an image inventory per node.

## Kubernetes

Window 15.9 h from 21:03. Frames are `zstd1x_protobuf`; nothing went to `raw_payloads`.

| table | rows jose / ninjapaw | rows/h jose / ninjapaw | on disk | fed by |
|---|---|---|---|---|
| `k8s_resources` | 334 k / 167 k | 21.0 k / 10.5 k | 10.4 MiB (1.24 GiB raw) | `/api/v2/orch`, 21 typed collectors |
| `k8s_manifests` | 415 k / 217 k | 26.1 k / 13.7 k | 75 MiB (4.93 GiB raw) | `/api/v2/orchmanif` |
| `k8s_cluster` | 5.7 k / 5.7 k | ~380 each | 0.3 MiB | collector type 46 |
| `k8s_resources_current` | 1,187 / 554 keys | – | 0.6 MiB | materialized view |

- **Producers.** The cluster agent sends most kinds; pods come from the node agents and carry
  `host_name`. Pod rows without a host are the final notice of a deleted pod.
- **Every object is resent every 180–195 s**, changed or not, plus an extra send on change.
  Exceptions: the `kube-system` Namespace and the cluster summary every 10 s; Nodes every
  15–135 s (their `resourceVersion` moves with each heartbeat); KubeletConfiguration every 600 s.
- **Almost all of it is repetition.** Only 0.6% (ninjapaw) and 2.8% (jose) of manifest rows carry a
  new `(uid, resource_version)`; for `k8s_resources`, 0.7% and 1.0%.
- **Kinds.** 21 on jose, 20 on ninjapaw; the largest on jose are ReplicaSet (95 k rows for 310
  objects), Pod, ClusterRole, ServiceAccount, ClusterRoleBinding. No VPA, HPA or LimitRange.
  `k8s_manifests` adds CRDs, KubeletConfiguration, EndpointSlice, custom resources and the cluster
  summary.
- **CRDs are 56% of manifest content** (2.53 of 4.49 GiB), about 113 per cluster, 34–43 KiB on
  average, up to 600 KiB, and almost never changing. On disk `content` compresses 67x.
- **Custom resources**: Flux Kustomization, HelmChart, HelmRepository, GitRepository and the
  DatadogAgent on jose; only the DatadogAgent on ninjapaw. Kustomization writes a new status about
  every 40 s per object (8,130 versions for 23 objects).

### How to read it

- **`k8s_resources`**: one row per object per send, keyed `(cluster_id, kind, namespace, name,
  collected_at)`. `object` is the agent's summary model, not the Kubernetes object: a Pod has
  `metadata, IP, nodeName, phase, containerStatuses, status, tags, resourceRequirements, QOSClass,
  startTime, scheduledTime, conditions`, and no env or YAML.
  - Always filled: `labels`, `annotations`, `tags`. `counts` for workload and RBAC kinds (for Pods
    restarts and ready/total containers). `phase` for Pods only; `status` for Pod, Namespace, Node;
    `condition_*` for Pod, Deployment, Job, PDB, Node. `metrics` is always empty.
  - The `last-applied-configuration` annotation arrives already redacted by the agent.
- **`k8s_manifests`**: one row per object per send. `content` is the Kubernetes object's JSON;
  `type` is the agent's numeric kind (1 Pod … 5 cluster, 20 CRD, 21 custom resource,
  28 EndpointSlice, 29 KubeletConfiguration). `tags` merges frame, wrapper and manifest tags, and
  `tag_sources` says which attached each. `is_terminated = 1` marks a deleted object's final
  notice.
- **`k8s_cluster`**: one row per cluster about every 10 s. Nothing changed all night: jose 3 nodes,
  36 CPUs, k3s v1.36.5, with `nvidia.com/gpu` as an extended resource; ninjapaw 1 node, RKE2
  v1.36.5. `nodes_capacity` keeps quantity strings (`…Ki`). `nodes_region`, `cluster_tags` and
  `metrics` are empty.
- **`k8s_resources_current`**: the latest row per `(kind, namespace, name)`. Read it with `FINAL`
  (jose had 15 unmerged duplicates). Deleted objects stay as their last row with
  `deletion_timestamp` set (jose: 78 Pods, 17 PVs, 17 PVCs and a few workloads). Pod status on
  jose: 129 Running, 62 Completed, 17 Terminating ghosts, 5 Error, 4 CrashLoopBackOff.

### Decoded, stored, surprising

- **`kind` is empty on about 21% of manifest rows**: types 5, 21 and 28. `manifestRows` takes
  `Kind = manifest.Kind` (`api/src/Intake/Routers/Kubeops.fs:677`), which the agent leaves empty
  for these types even though `content` has `"kind"` (except type 5). Filtering by kind cannot find
  Flux, DatadogAgent or EndpointSlice objects.
- **Migration 0004 is not deployed.** The production ledger stops at 0003, and
  `k8s_resources_current` has no TTL, so deleted objects stay forever. Even with the 1-day TTL,
  about 10% of jose pod keys have a last row older than 15 minutes, so a "now" query must also
  filter on `collected_at` or `deletion_timestamp IS NULL`.
- **Same-timestamp duplicates.** The cluster-summary and `kube-system` Namespace manifests are
  sometimes stored twice with identical `collected_at` and `resource_version` but different
  `group_id` (about 15% of cluster-summary sends on ninjapaw, 6% on jose). They are two agent
  messages; the cause is unknown.
- **The type-5 manifest duplicates `k8s_cluster`**, every 10 s, one `resource_version` all night.
- Secrets and ConfigMaps are not collected.
- **Pod and workload manifests carry container env values in plain text.** In a 10-minute sample,
  jose had 1,965 literal values and ninjapaw 722; about 2% were masked, because the agent masks
  only names on its sensitive list. `k8s_manifests.content` has to be treated as sensitive.
- Never filled: `manifest_host`, `extra_attributes`, `header_timestamp`, `org_id` (always 0);
  `host_name`/`node_name` in manifests only on node-agent pods.
- 114 ninjapaw Pod rows have no creation time; probably RKE2 static pods.
- `collected_at` is NinjaCat's clock (`DateTime.UtcNow` in `handleFrame`), not the agent's.

Panel candidates: a cluster inventory from `k8s_resources_current` (FINAL, live objects only) with
pod status, restarts and CrashLoopBackOff; a node capacity view; a "what changed" timeline from the
distinct `(uid, resource_version)` versions in `k8s_manifests`, with a diff between two versions.

## Everything else

| table | finding |
|---|---|
| `check_runs` | 277 k rows, 26 checks, 11 hosts. 65 k rows have an empty host (KSM job checks, `clickhouse.*`). `kubernetes_state.job.complete` is 19% of the table: 15 finished jobs re-reported every 15 s. |
| `events` | 10.9 k. 79% from the cluster agent's apiserver collector (one event per object, reasons bundled in the text), 20% containerd (create, destroy, 5 OOMs), 30 agent start/stop, 17 from the Operator (empty `host`). `extra` and `related_event_id` never set. |
| `apm_telemetry` | 7.2 k rows, all `producer = agent-telemetry`; no tracers send yet. |
| `agent_checks` | 9,016 rows, **`status` NULL on every row** (decoder bug). |
| `agent_metadata` | 384 rows, cluster agents only: cluster-agent config and the cluster-check dispatch map every 10 min, secrets masked by the agent. |
| `raw_payloads` | 2,371 rows: 2,367 Datadog Operator metadata (`no_schema`), 4 real decode failures. |
| `synthetics_results` | 386 rows, exactly 24/h, **every column empty**. |
| `hosts` | one row per host, refreshed every ~10 min, plus a stale `jose1`. |
| `schema_migrations` | 0001–0003; 0004 not deployed. |

- **events.** Flux noise dominates (`ReconciliationSucceeded` 3.5 k, `ArtifactUpToDate` 750,
  `GitOperationSucceeded` 360). Next is `DNSConfigForming`, 2.1 k warnings from kube-system static
  pods on the ninjapaw node about every 5 min: a real resolv.conf nameserver-limit problem there.
  The useful signals are `BackOff` (591), `Unhealthy` (144), `Killing` (90), `FailedScheduling`
  (16) and the OOMs. One `FailedScheduling` event from 2026-10-03 was stored 33 times, identical in
  every field: nothing deduplicates events.
- **apm_telemetry.** Each batch is stored as one parent row (404 rows, ~29 KB, the whole batch)
  plus one row per entry (6,558, ~1.7 KB), so every payload is stored twice. `hostname` is the
  literal `x` on every row, because that is what the agent sends; the sender is in
  `payload.metrics.agent_metadata.hostname` (a pod name). The payload holds `{name: {tags, type,
  value}}` metrics: check runs and timings, forwarder successes and errors per endpoint, logs,
  tagger and workloadmeta counts, and DBM job timings. It shows two 500s from `series_v2` between
  21:08 and 21:53, and that node agents sent no `metadata_v1` transactions.
- **agent_metadata.** No node-agent inventories arrive because both DatadogAgent manifests disable
  inventories (`DD_INVENTORIES_ENABLED`). That is deliberate, but it also hides `check_metadata`,
  including the DBM integrations'.
- **agent_checks.** The agent sends status as a string in position 3 (`OK`, `WARNING`, …;
  confirmed in an earlier capture). `agentCheckRow` reads it with `Json.int64Of`
  (`api/src/Intake/Routers/Api.fs:675`) and drops it. Position 5 (the warning text) and 6 (the
  instance tags) land in `positional_extra`. The column type `Nullable(Int64)` and
  `api/docs/tables/api.md` are wrong too.
- **raw_payloads.** Three metadata variants the decoder does not know, all well-formed:
  `datadog_operator_metadata` (once a minute per cluster: versions, leader flag, feature flags),
  `datadog_operator_crd_metadata` (every ~5 min per CR, the full spec; 3–5 distinct specs all
  night) and `datadog_operator_helm_metadata` (jose only, the chart's values with the keys null).
  They fill a failures table at ~120 rows/h. The 4 real failures are the log OOMs and the
  pass-through zstd bodies at 21:05:56 (`api/src/Intake/Http.fs:102-117`), one of them on
  `/intake/`.
- **synthetics_results.** No test is configured anywhere. `handleBatch` likely accepts a JSON
  `null` or `{}` item and `syntheticsResultRow` writes an all-default row
  (`api/src/Intake/Routers/Evp.fs:53, 65-81, 404`); the row keeps no host or body, so the producer
  is unknown.
- **hosts.** `network` is the text `null` (gohai's network goes to `gohai_extra['network']` as
  designed). The renamed `jose1` row stays because `hosts` has no TTL.
- **check_runs alerts.** A Longhorn trim CronJob on jose fails `cronjob.on_schedule_check` and
  `cronjob.complete`. `ntp.in_sync` is UNKNOWN on every jose host and OK only on ninjapaw, which
  suggests NTP egress is blocked on jose.

Panel candidates: an events strip on charts (warnings and errors, Flux successes muted); a fleet
page with hosts, agent versions, cluster-agent and Operator flags, the cluster-check map and
forwarder errors per endpoint.

## Follow-ups

Data lost or wrong:

1. **ClickHouse outages lose acknowledged writes**: 1,915 `metrics` rows at 21:06 and 38 at 07:28
   (`ninjacat.storage.rows.failed`). Already in TODO.md.
2. **Intake OOM on startup bursts**: two 1.1–1.4 MB log batches failed with
   `OutOfMemoryException` (`Logs.parseItems`), and two bodies were stored still compressed because
   `Body.decompress` failed in the same crunch and passed them through (`api/src/Intake/Http.fs:102-117`).
   Raise the memory limit or bound batch parsing, and record a failed decompression as such.
3. **`agent_checks.status` is NULL on every row**: read position 3 as a string, change the column
   type in a new migration, and name positions 5 and 6 (`api/src/Intake/Routers/Api.fs:650-677`).
4. **Empty `synthetics_results` rows**: send null or empty items to `raw_payloads`
   (`unexpected_shape`), then capture one to find the producer (`api/src/Intake/Routers/Evp.fs:53,
   65-81, 404`).
5. **`k8s_manifests.kind` empty on 21% of rows**: take it from `content`, or from `type` (5 →
   Cluster) (`api/src/Intake/Routers/Kubeops.fs:677`).
6. **Deploy migration 0004** (`k8s_resources_current` TTL), and give every "current" read a
   freshness or deletion filter.
7. **DBM envelope gaps**: fill `database_instance` from `tags` for Postgres `dbmmetrics`, and
   `host`/`database_instance` from `data.database_instance` for `dbmhealth`, in `decodeEnvelope`
   or `toRow` (`api/src/Intake/Routers/Dbm.fs:77, 204`). Record the engine on every row (`dbms`,
   else `ddsource`, else the array prefix).
8. **Log `status` is the stream, not the level**: parse JSON messages into `attributes` and map
   `level`/`LogLevel` → status, `msg`/`Message` → message, `ts`/`time` → timestamp
   (`api/src/Intake/Routers/Logs.fs` `toRow`); optionally recognise ClickHouse's and the agent's
   text formats.
9. **Host tags are not joined into metric queries**: `{kube_cluster_name:…}` misses ~70% of points
   (`api/src/Engine/Query/Compile.fs`). Logs have the same gap: no cluster tag at all.
10. **`metrics.resources` is never read**: expose `device` and `database_instance` as tags in
    `Compile.fs`, and add `resources` to the `series_id` hash (new migration).
11. **Metric names with `/` cannot be queried**: normalise at intake as Datadog does
    (`api/src/Storage/Rows/Metrics.fs`) or let `api/src/Engine/MetricQuery/Parser.fs:178` accept
    them.
12. **`as_count()` undercounts RATE points with interval 0** (the 12 `dd.*.count` names,
    `Compile.fs:61`).
13. **Identical events stored repeatedly** (33 copies of one apiserver event): deduplicate on
    aggregation key, text and timestamp.
14. **Host renames split history** (`jose1` → `jose1-jose`): a TTL or "inactive" marker on `hosts`.
15. **Duplicate cluster-summary and `kube-system` manifests** with one timestamp and different
    `group_id`: cause unknown.

Decoder gaps:

16. **DBM, the rest** (TODO.md §4): decode `<dbms>_rows` into a narrow table or metric points, and
    record there that the rows are deltas; decode ClickHouse `query_completion`, `query_error` and
    `storage_health`, and `dbmhealth` `name`/`status`/`category`.
17. **Operator metadata**: add `datadog_operator_metadata`, `_crd_metadata` and `_helm_metadata` to
    `metadataVariants` (`api/src/Intake/Routers/Api.fs:948`).
18. **apm_telemetry `hostname` is always `x`**: promote `payload.metrics.agent_metadata.hostname`.
19. **Name deleted containers**: join `container_events.owner_uid` to the pods in `k8s_resources`.
20. **Realtime processes**: answer `ActiveClients > 0` only while a panel user watches a host
    (`api/src/Intake/Routers/Process.fs:856-866`), or stop expecting the two stats tables.
21. **A `received_at` column on `logs`** (new migration), so agent-side delay is measurable.
22. **Sensitive content**: treat `k8s_manifests.content` as sensitive (plain-text env values), and
    scrub bare `token`/`--token` arguments in `processes` before the panel shows them.
23. **Self-monitoring**: use the node as `host` with the pod as a tag, and register `SelfMonitor`
    on the query host (only `IntakeHost.fs:85` does).
24. **Stale docs**: `api/docs/tables/dbm.md`, `logs.md`, `process.md`, `kubernetes.md` and
    `api.md` still describe the Go server; rewrite them against the F# routers and
    `0001_initial.sql`, with the shapes above. Rename or document `dbm_events.db_instance` (it is
    the logical database) and fix the `dbm_type` comment (`0001_initial.sql:1852, 1879`).

Storage cost:

25. **Stop the log loop**: set NinjaCat's ClickHouse logger to `information`
    (`helm/ninjacat/templates/clickhouse.yaml`) or exclude that container from log collection.
    About 80% of log volume.
26. **`metrics_1m` is larger than `metrics`**: `LowCardinality` tag keys and values or a per-series
    tag table, reconsider `value_last`/`value_events`, add a coarser long-retention tier
    (`api/schema/migrations/0002_metrics_rollup.sql`). At 400 days it projects to ~600 GB.
27. **`metrics.pod_name`** (25% of the table): `LowCardinality` with ZSTD, or drop it.
28. **Unchanged Kubernetes manifests**: 97–99% repeat a stored `resource_version`. Store only new
    versions (or a `ReplacingMergeTree` keyed on them), and drop the type-5 manifest that
    duplicates `k8s_cluster`. CRD repeats alone are 56% of the content.
29. **dbmmetrics repeats the query text every 10 s** (45% of `dbm_events`); fixed by item 16.
    `pg_settings`/`pg_extension` repeat unchanged (~95%): keep only changes. Merge split
    `pg_databases` payloads per cycle, and note that `-rw`/`-ro` store everything twice.
30. **`processes.request_id`/`snapshot_id`** (~20% of the table): drop in a new migration
    (`api/src/Storage/Rows/Process.fs`). Rebuild `containers` with `ORDER BY (tenant_id, host, id,
    timestamp)` so its float columns compress.
31. **apm_telemetry stores every batch twice**: drop the parent copy or the children.
32. **`raw_payloads` fills with well-formed Operator metadata** at ~120 rows/h until item 17.
33. Hide `n_o_i_n_d_e_x.*` metrics from listings; make `content_type` consistent
    (KubeletConfiguration says `application/json`, everything else `json`).

Deployment and agent configuration (not NinjaCat code):

34. jose Postgres: create `pg_stat_statements` in the database the agent connects to. That brings
    back dbmmetrics and FQT, and stops ~9 k log lines/h from the agent and ~4 k from the servers.
35. All Postgres instances: create the `datadog` schema and `explain_statement`, so plans arrive;
    it is also the precondition for column statistics. Consider a larger
    `track_activity_query_size` on jose.
36. ClickHouse DBM: grant the agent's user what EXPLAIN needs (error 497,
    `helm/ninjacat/templates/clickhouse.yaml:45`) and set the instance's database, or accept the
    failures (error 60). Exclude the agent's own user from samples and metrics, about half of all
    ClickHouse DBM rows. Raise the activity interval from 1 s to 10 s, a ~10x cut of the largest
    track.
37. Drop the overlapping `cri` or `containerd` check and the Operator's Go runtime metrics: about
    15–20% of points.
38. Decide whether node-agent inventories stay disabled; they hide `check_metadata`.
39. Tag `cnpg.*` with `kube_cluster_name` and give `clickhouse.*`/`clickhouse_prom.*` a host, or
    join on `database_instance`/`endpoint` in the panel.
40. Cluster issues found along the way: crowdsec agent crash-looping on all jose nodes, NTP on
    jose, the DNS nameserver limit on the ninjapaw node, the failing Longhorn trim CronJob.
41. To exercise `sketches`: enable `histogram_buckets_as_distributions` on kubelet or apiserver, or
    send one DogStatsD distribution.

## Appendix: Queries

All run read-only (`clickhouse-client --readonly=1`) against the `ninjacat` database.

```sql
-- Table sizes
SELECT table, sum(rows), formatReadableSize(sum(bytes_on_disk))
FROM system.parts WHERE database = 'ninjacat' AND active GROUP BY table;

-- Column sizes, to find what costs disk
SELECT table, name, formatReadableSize(sum(data_compressed_bytes))
FROM system.columns WHERE database = 'ninjacat' AND table IN ('metrics', 'metrics_1m', 'processes')
GROUP BY table, name ORDER BY sum(data_compressed_bytes) DESC;

-- DBM: rows and bytes per track and instance
SELECT track, dbm_type, kind, host, database_instance, count() c,
       formatReadableSize(sum(length(event))) bytes
FROM dbm_events GROUP BY ALL ORDER BY track, host;

-- DBM: the shape of every event, one level down
SELECT track, dbm_type, kind, kv.1 key, JSONType(kv.2) t, count()
FROM (SELECT track, dbm_type, kind, arrayJoin(JSONExtractKeysAndValuesRaw(event)) kv FROM dbm_events)
GROUP BY ALL ORDER BY ALL;

-- Metrics by namespace
SELECT splitByChar('.', metric)[1] ns, count(), uniqExact(series_id), uniqExact(metric)
FROM metrics GROUP BY ns ORDER BY 2 DESC;

-- Rollup check: raw points against the rollup's count
SELECT (SELECT count() FROM metrics WHERE timestamp >= '2026-10-16 01:00:00' AND timestamp < '2026-10-16 05:00:00'),
       (SELECT countMerge(points) FROM metrics_1m WHERE minute >= '2026-10-16 01:00:00' AND minute < '2026-10-16 05:00:00');

-- Logs: who produces them
SELECT tags['kube_namespace'][1] ns, tags['kube_container_name'][1] ctr, count() c, avg(length(message))
FROM logs GROUP BY ALL ORDER BY c DESC LIMIT 40;

-- Kubernetes: how much of the manifest stream is new
SELECT cluster_name, count(), uniqExact(uid, resource_version) FROM k8s_manifests GROUP BY 1;

-- Kubernetes: manifests without kind
SELECT cluster_name, type, JSONExtractString(content, 'kind') k, count()
FROM k8s_manifests WHERE kind = '' GROUP BY ALL;

-- Container exits and how late the Delete arrives
SELECT host, exit_code, count(), avg(dateDiff('second', exited_at, timestamp))
FROM container_events WHERE object_kind = 'Container' GROUP BY ALL;

-- What fell back to raw_payloads
SELECT intake, reason, path, count(), avg(body_bytes) FROM raw_payloads GROUP BY ALL;

-- Applied ClickHouse migrations
SELECT version, name FROM schema_migrations;
```
