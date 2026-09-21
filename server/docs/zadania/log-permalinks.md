# Linking to a single log line

Not built. This records what was measured while answering "can we hand someone
a link to one log line", so the next person starts from numbers rather than
from the same question.

Related: [../../../docs/decisions/0001-tags-are-a-multiset.md](../../../docs/decisions/0001-tags-are-a-multiset.md)
— the same family of question: what identifies a row, and who assigns it.

## ClickHouse has no row identity

`_part` and `_part_offset` exist as virtual columns and do resolve a row:

```sql
SELECT _part, _part_offset, timestamp FROM ninjacat.logs LIMIT 1
-- 20260921_638_638_0   12   2026-09-21 15:31:34.332
```

They are useless for a permalink. Merges rewrite parts, so that offset means
something else after the next merge. A columnar store has no row identity by
construction; a row is an intersection of columns, not an object.

## The schema already answers this three different ways

Worth knowing before inventing a fourth:

| Table | Column | Where it comes from |
|---|---|---|
| `events` | `event_id UUID`, `event_id_num` | **ours** — Datadog hands the client a numeric event id, so we had to keep one |
| `metrics` | `series_id UInt64` | **derived** — `sipHash64(tenant, metric, host, tags)`; identifies the series, not the point |
| `k8s_resources` | `uid` | **foreign** — Kubernetes' own object UID |
| `logs` | none | — |

A metric point needs no id: `(series_id, timestamp)` already names it. A log
line has nothing equivalent.

## A random id would be wrong here, and that is not obvious

The reflex is a UUID per row. It breaks something we rely on.

ReplicatedMergeTree deduplicates inserts by hashing the whole block, which is
what protects us when the agent retries a batch after a timeout — and it does,
routinely. Give every row a random id and the retried batch is no longer byte
identical, so it is no longer a duplicate, and the same logs land twice.

A deterministic hash has no such problem:

```sql
log_id UInt64 MATERIALIZED sipHash64(timestamp, host, service, message)
```

The retried batch hashes identically, deduplication still fires, and the id is
stable — so a link pasted into a chat still resolves next week.

The price of determinism: two identical lines from the same host and service in
the same millisecond collapse to one id. Usually correct, since they usually
*are* one event. Adding a sequence number within the batch would separate them
and would immediately break deduplication again. For telemetry, deduplication
wins.

## Measured, on real log lines from the k8s lab

**CPU is not the problem.** 32.9k real rows, with and without the materialised
column: 32 ms against 42 ms. About 300 ns a row, confirmed again over 2M
synthetic rows. At 100k rows/s that is 3% of one core.

**Storage is the problem.**

```
without log_id   13.4 bytes/row
with log_id      21.1 bytes/row      +7.7 B, +57%
```

The full eight bytes, uncompressed. A good hash is indistinguishable from
random, so ZSTD gets nothing out of it — unlike every other column in that
table.

**Collisions are computable.** For 64 bits, roughly n²/2⁶⁵:

| rows | chance of any collision |
|---:|---:|
| 1M | ~0.000003% |
| 100M | 0.03% |
| 1B | 2.7% |
| 10B | effectively certain |

n counts rows **per partition**, not overall, because a permalink carries the
timestamp and the lookup stays inside one daily partition. And a collision
means the link shows two lines instead of one — not corruption.

`sipHash128` would end the question, at 16 bytes a row. Note it returns
`FixedString(16)`, **not** `UInt128`; declaring the column as UInt128 fails at
insert with a parse error, which is how this was discovered.

## What to do instead, for now

Do not store it. A permalink is opened rarely — someone pastes a line into a
chat now and then, nobody filters by it in a loop. Let the link carry
`(timestamp, host, service, hash)` and compute the hash at read time:

```sql
SELECT * FROM ninjacat.logs
WHERE timestamp = ? AND host = ? AND service = ?
  AND sipHash64(timestamp, host, service, message) = ?
```

The timestamp and service narrow this to a granule, so the hash runs over a
handful of rows. Zero bytes stored, zero cost on the write path, and the cost
falls on whoever actually opens the link.

Add the column when the id becomes something routinely **filtered on** rather
than occasionally pasted. At that point +57% on the logs table is buying
something.

## Open questions for whoever picks this up

- Does the same reasoning hold once logs carry parsed `attributes` (see
  [json-log-attributes.md](json-log-attributes.md))? A wider row makes the
  8-byte id proportionally cheaper.
- Should `container_events` get the same treatment? A pod restart is exactly
  the kind of event someone links to.
- If a facet or search UI ever needs stable pagination cursors, that is the
  same problem again and should reuse whatever is decided here.
