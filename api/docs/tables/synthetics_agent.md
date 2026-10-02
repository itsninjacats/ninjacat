# `synthetics_test_configs` — the synthetics test catalogue

Feeds: nothing yet. **Read** by `GET /api/unstable/synthetics/agents/tests` on
`intake.synthetics.<site>` (`HandleSyntheticsAgentTests`,
`intake/router_ciwebhook.go`). `schema/migrations/0017_ci_visibility.sql`
creates the table, `apps/storage/rows_ci.go` is the writer.

This is the one table in the tree that exists to be read rather than written,
and the one endpoint in the intake whose answer **drives an agent loop**
instead of acknowledging something the agent sent.

## The two directions of synthetics

They are easy to confuse because both hostnames contain the word:

| host | direction | route | table |
|---|---|---|---|
| `http-synthetics.<site>` | agent → us | `POST /api/v2/synthetics` | `synthetics_results` (`docs/tables/evp.md`) |
| `intake.synthetics.<site>` | us → agent | `GET /api/unstable/synthetics/agents/tests` | `synthetics_test_configs` |

The results intake already works. This is the scheduler side: what the agent
should run in the first place.

`routes_test.go` pins that neither host serves the other's path — the failure
this prevents is a poller getting a 202 with an empty object from the results
handler and treating it as an empty test list forever.

## What the agent does

Primary source, read directly from the module cache at
`datadog-agent@v0.0.0-20260521051500-e70f483e68d2`:
`comp/syntheticstestscheduler/impl/testpoller.go` and
`comp/syntheticstestscheduler/common/data.go`. Gated by
`synthetics.collector.enabled`.

The poller builds its own URL — the agent does **not** derive this one from
`dd_url` or any per-product override:

```go
endpoint: "https://intake.synthetics." + config.site + "/api/unstable/synthetics/agents/tests"
```

Then, every **2 seconds** (`pollingFrequency`), with a 10 second client
timeout:

- `GET` with query `?agent_hostname=<hostname>&agent_version=<version>`
- header `DD-API-KEY: <api key>` — the same header our default guard reads, so
  no special auth wiring is needed
- **any status other than 200 is a failure.** Five consecutive failures
  (`maxConsecutiveErrors`) flip the poller unhealthy and hand scheduling to an
  in-memory fallback.

So the handler answers `200`, not `202`. Nothing is stored for the request: it
carries no telemetry, and a raw row every two seconds per agent would bury the
table it landed in. The two query parameters are logged.

## The response shape is unforgiving

```go
type testPollerResponse struct {
    Tests []common.SyntheticsTestConfig `json:"tests"`
}
```

decoded in **one** `json.NewDecoder(resp.Body).Decode(&response)` call. And
`SyntheticsTestConfig` has a custom `UnmarshalJSON` that switches on the
top-level `subtype` to pick the type of `config.request`:

```go
switch payload.Protocol(tmp.Subtype) {
case payload.ProtocolUDP:  /* UDPConfigRequest  */
case payload.ProtocolTCP:  /* TCPConfigRequest  */
case payload.ProtocolICMP: /* ICMPConfigRequest */
default:
    return fmt.Errorf("unknown subtype: %s", tmp.Subtype)
}
```

Because it is one `Decode` over the whole body, **an unknown subtype in one
test discards every test in the response**, not just that one. That is the
single most important fact about this endpoint: adding a test whose subtype is
not exactly `UDP`, `TCP` or `ICMP` silently stops the agent running anything at
all, and the only symptom is a debug log.

We answer `{"tests":[]}` — an array, not `null`. Both decode, but the array is
what the field means and what a capture should show. It is the correct answer
to "what should I run" when the answer is nothing, and it keeps the poller
healthy and ready for the moment there is something.

`civSyntheticsTestsResponse.Tests` is `[]json.RawMessage` rather than a typed
list for exactly this reason: whoever fills it later must be able to build each
element from a row and **drop one that fails validation without taking the rest
of the response with it**.

## Columns

Exactly `common.SyntheticsTestConfig`, field for field.

| column | wire field | notes |
|---|---|---|
| `tenant_id` | — | first `ORDER BY` column, as everywhere |
| `updated_at` | — | the `ReplacingMergeTree` version column; every edit rewrites the row |
| `public_id` | `public_id` | the test's identity |
| `version` | `version` | |
| `type` | `type` | |
| `subtype` | `subtype` | **`UDP` \| `TCP` \| `ICMP` only** — see above. It is not a field of the struct; the custom unmarshaller reads it off the raw JSON to choose `config.request`'s type, so it must be emitted at the top level of each test object |
| `tick_every` | `tick_every` | seconds; the struct field is `Interval` |
| `org_id` | `org_id` | |
| `main_dc` | `main_dc` | |
| `result_id` | `result_id` | |
| `run_type` | `run_type` | `"scheduled"` (`common.RunTypeScheduled`) marks a test cached for fallback execution |
| `assertions` | `config.assertions` | JSON array of `{operator, property, target, type}`. `operator` ∈ `is`/`isNot`/`moreThan`/`moreThanOrEqual`/`lessThan`/`lessThanOrEqual`; `type` ∈ `multiNetworkHop`/`latency`/`packetLossPercentage`/`jitter`; `property` ∈ `avg`/`min`/`max` and is optional |
| `request` | `config.request` | JSON object, **subtype-specific**: `UDP` = `{host, port?}`, `TCP` = `{host, port?, tcp_method}` with `tcp_method` ∈ `prefer_sack`/`syn`/`sack`, `ICMP` = `{host}`. All three also carry `source_service?`, `destination_service?`, `probe_count?`, `traceroute_count?`, `max_ttl?`, `timeout?` (seconds) |
| `enabled` | — | ours, not the agent's. A paused test is kept, not deleted: "paused" is a state a UI needs |

Both JSON columns are text rather than parallel arrays because the poller hands
them straight back out as JSON and nothing here ever filters on an assertion.

`ReplacingMergeTree(updated_at)` keyed on `(tenant_id, public_id)`. Readers need
`FINAL` or `argMax`. **No TTL**: a test configuration is a live object, like
`hosts` — it leaves only by being replaced.

## Why the table exists before anything writes to it

`HandleSyntheticsAgentTests` has to answer from *somewhere*, and a
`TODO(ninjacat)` pointing at a table that exists is a different kind of TODO
from one pointing at a table that does not. The writer is registered so the
arity and round-trip tests cover it (`TestSyntheticsTestConfigRoundTrip`) — an
unregistered table is one nobody checks, and the day something does write to it
the only new thing should be the caller.

Doing it properly means a **read**, and reads belong to the query app, not to
the intake — the same rule that keeps `HandleGitSearchCommits` answering an
empty commit list. The shape of the remaining work:

1. a panel UI writes test definitions through `panelapi` into this table;
2. the query app gains a lookup by tenant (and probably by `enabled`);
3. `HandleSyntheticsAgentTests` asks it instead of returning a constant,
   dropping any row whose `subtype` is not one of the three legal values
   **before** it reaches the response.

Step 3's filter is not optional. It is the difference between one bad row and
an agent that silently runs nothing.
