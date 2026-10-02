# Tags are a multiset, and the schema now says so

**Decision.** Every Datadog *tag* column becomes
`Map(LowCardinality(String), Array(LowCardinality(String)))`.

Kubernetes labels and annotations, and the gohai host metadata maps, stay
ordinary maps. Their keys are unique by their own API contract; tags are not.

## The problem

A Datadog tag is not a key-value property. It is a label in a set — one string,
which by convention contains a colon. Nothing forbids two tags sharing the part
before that colon, and real systems produce them constantly: a pod belonging to
two services carries `kube_service:a` and `kube_service:b`.

Storing tags as `Map(key, value)` silently discards one of them. `tagsToMap`
writes `out[k] = v`; the last tag wins and the earlier one is gone at write
time, with no error and no counter.

This was not hypothetical. Measured against a live Kubernetes cluster:

- **every single log entry** in a captured batch lost a tag — 21 of 21, each
  dropping one of two `kube_service` values;
- one check in twelve arrived with 21 tags that a map reduced to 20;
- `k8s_resources`, which happened to use `Array(String)`, kept both values and
  so proves what the map was losing.

The consequence is a query that answers confidently and wrongly. "Everything
for service X" silently omits any pod whose `kube_service:X` was the tag that
lost the race.

## Why this shape

Four candidates, measured on an identical 800k-row slice of real cluster
telemetry:

| | Map(LC,LC) | **Map(LC,Array(LC))** | Array(String)+keys |
|---|---:|---:|---:|
| On disk | 15.22 MiB | **15.23 MiB** | 45.63 MiB |
| Enumerate keys | 299 ms | 284 ms | 49 ms |
| Values for one key | 202 ms | 328 ms | 564 ms |
| Filter by one tag | 201 ms | 317 ms | 499 ms |
| Keeps duplicates | no | **yes** | yes |

Fidelity turns out to be free: 15.23 MiB against 15.22. The array offsets
compress to nothing because they are uniform.

The array-of-strings option was the intuitive answer and it is the worst one —
three times the disk and slowest at two of the three questions, because
flattening to `"k:v"` throws away the LowCardinality dictionary.

The cost of the chosen shape is about 60% on value lookup and filtering: 317 ms
instead of 201 ms across 800k rows. That is acceptable because this is not the
hot path — the hot path is the write, and the hot *reads* go through the
materialised columns below, which beat every map variant by roughly 17x.

## What this changes

Queries move from equality to membership:

```sql
-- before
WHERE tags['kube_service'] = 'clickhouse'
-- after
WHERE has(tags['kube_service'], 'clickhouse')
```

Materialised columns keep working, indexed into the first element:

```sql
service LowCardinality(String) MATERIALIZED tags['service'][1]
```

They stay the right way to filter by a hot tag. A tag that is genuinely
single-valued reads exactly as before.

## What is deliberately not done

**A facet table.** Enumerating keys is 284 ms here, and a materialised
`tag_keys` column would cut it to 53 ms — at 63% more disk, paid on every
point. 153 distinct keys do not need 800k rows to be counted. When facets
matter, they belong in a small aggregating table maintained by a materialised
view, not in a column repeated per datapoint.

**Foreign maps.** Kubernetes forbids duplicate label and annotation keys, and
gohai metadata is a plain object. Converting those would cost the same 60% for
a problem they cannot have.
