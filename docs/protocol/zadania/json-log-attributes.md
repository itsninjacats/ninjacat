# JSON log attributes — the `@` half of log search

Deferred by decision. Log parsing is a domain of its own, and opening it before
the query engine exists would mean designing two hard things at once. This
document records what the gap is, what ClickHouse can actually do about it, and
why waiting costs almost nothing.

Related: [silnik-kwerend.md](silnik-kwerend.md) — the query engine this
eventually plugs into.

## The gap

Datadog's log search syntax has two kinds of `key:value`, and they are not
interchangeable:

```
service:web              a tag or a reserved attribute
@http.status_code:500    an attribute parsed out of the log's JSON body
@duration:>1000          numeric comparison on such an attribute
```

`ninjacat.logs` stores `host`, `service`, `source`, `status`, `message` and
`tags`. The tag half is covered — `tags` is a `Map` with bloom filters on keys
and values. The `@` half has nowhere to live: `message` is a flat `String`, so
an attribute is not a queryable thing, it is a substring.

Note that `INDEX idx_message ... tokenbf_v1` does not help here. It accelerates
free-text terms, which is a different question from "which logs have
`http.status_code` at or above 500".

## What ClickHouse offers

Both options below were run against the dev instance (ClickHouse 26.8.6.5), not
taken from documentation.

### Option A — `JSONExtract*` over the existing column

Works today, no schema change:

```sql
SELECT JSONExtractInt(message, 'http', 'status_code') AS status
FROM ninjacat.logs
WHERE JSONExtractInt(message, 'http', 'status_code') >= 500
```

Nesting is expressed as successive arguments, not as a path string. The cost is
that the document is parsed for every row the query reads, every time, and no
index covers the extracted value. Acceptable once time and service have already
narrowed the scan; poor as the primary filter.

### Option B — a native `JSON` column

```sql
ALTER TABLE ninjacat.logs ADD COLUMN attributes JSON;

SELECT attributes.http.status_code, attributes.user.plan
FROM ninjacat.logs
WHERE attributes.http.status_code >= 500
```

Four properties that matter here, all confirmed:

- Paths are read with dot access, like ordinary columns, and are stored as
  separate subcolumn streams rather than one blob per row.
- Types are detected rather than stringified — an undeclared path reads back as
  `Dynamic`, not `String`.
- A path missing from a given row yields `NULL`, not an error. Logs from mixed
  sources therefore coexist without a schema union.
- `JSONAllPaths(attributes)` enumerates every path ever seen, which is how the
  panel could offer a facet list without being told the shape in advance.

Known paths can be pinned to real types while the rest stays dynamic:

```sql
attributes JSON(
    level               LowCardinality(String),
    `http.status_code`  UInt16,
    duration_ms         UInt32
)
```

This is the same move already made in `ninjacat.metrics`, where `env`, `service`
and `kube_namespace` are lifted out of the tag map as `MATERIALIZED` columns:
type the hot paths, leave the long tail dynamic.

### Which one

Option B for the real implementation. Option A is a legitimate stopgap if `@`
support is wanted before the column exists — the compiler can emit either, and
swapping the emitted predicate later does not change the query language.

## Why deferring is cheap

`ALTER TABLE ... ADD COLUMN` is a metadata operation: no table rewrite, no
backfill, instant on a table of any size. The expensive schema decision is
`ORDER BY`, and nothing here touches it.

So this stays a free decision for as long as we want it to. That is explicitly
not true of the engine choices around it.

## Open questions, for whoever picks this up

- **Where does parsing happen?** In the intake handler on write, or lazily at
  query time. Writing costs CPU on every log; reading costs it on every query
  over unparsed rows.
- **What about logs that are not JSON?** Most logs are plain lines. The column
  is simply empty for them, but the handler needs a cheap, non-throwing check
  rather than attempting a parse per line.
- **Is `message` kept alongside?** Storing the body twice doubles the bytes for
  JSON logs. Dropping the raw form loses exact reproduction, including key order
  and whitespace, which matters when someone is diffing what an agent sent.
- **Which paths get type hints?** Datadog's reserved attributes are the obvious
  first set. Everything else can stay dynamic until a facet proves hot.
- **Cardinality.** An attribute space is unbounded by construction. Whatever
  limits eventually guard tags should guard paths too.

## Done looks like

- `@path:value`, `@path:>n` and existence checks parse into the query AST and
  compile to predicates over `attributes`.
- Nested paths work to arbitrary depth.
- A log whose body is not JSON is searchable by free text exactly as it is now.
- The panel can list available attribute paths without hardcoding them.
- Absent, null and empty-string attribute values remain distinguishable — the
  same discipline the container exit code already follows.
