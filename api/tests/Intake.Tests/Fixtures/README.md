# Golden fixtures

`go/<TestName>/<n>.json` are requests the Go server's own tests sent through
its intake, recorded with what Go answered and what it handed to storage:

- `request`: method, host, URL, headers and the body as it arrived;
- `response`: status, headers, body (`body_json` when it is JSON);
- `sends`: each batch written — the table (`to`), its INSERT statement, the
  rows as records (`rows`) and as the column values given to the ClickHouse
  driver (`args`). `null` when the Go test did not capture writes;
- `routes`: the Go route sets of the engine that served the request. Only
  the tests know these names (`Golden/Replay.fs`); the server routes by host;
- `volatile`: paths that differ between two runs of the same test (receive
  times, minted ids) and are not compared.

`GoldenTests.fs` replays each request against the F# intake and compares all
of it. `overrides.json` lists differences accepted for a fixture, each with
its reason:

    { "TestName/001": { "ignore": ["/sends/0/rows/0/Note"], "why": "…" } }

A fixture with an `edited` key no longer says what Go did: the F# server
deliberately does something else there, the expectation was changed by hand,
and the key says what changed and why.

They were recorded by the Go server's own tests (two runs, merged, which is
how `volatile` was found) and are frozen: they are what the Go server did at
the time it was replaced. The Go server, and the recorder with it, was removed
on 2026-10-02; its last commit is in the history of `server/`.

`go/TestPortProbe*` were recorded the same way from a probe written during the
port (every Kubernetes object kind, sparse and full, bad frames, container
lifecycle and image payloads, Kubernetes action batches), to cover what the Go
server's own tests did not send.

## Real senders

`real/` holds payloads recorded from real senders against this server, not
from the Go one. `RealPayloadTests.fs` and `UsmTests.fs` post them and assert
what must hold: every event becomes a row, nothing is left raw, no key is
marked undecoded.

| files | sender |
|---|---|
| `browser-sdk-7.15.0-*.ndjson` | `@datadog/browser-rum` 7.15.0 in Chromium and Firefox |
| `<check>-<version>-<database>-<track>.json` | the agent's DBM integrations (agent 7.84.0) against Postgres 18, MySQL 8.4, MariaDB 11.8, SQL Server 2022, Oracle 23, MongoDB 8 and ClickHouse 26.8: one event of every kind each produced |
| `agent-7.84.0-sdsresult-*.bin` | the agent's data security check scanning three Postgres tables |
| `agent-7.84.0-ndm*.json`, `-netpath.json` | the SNMP check against net-snmp, three traps, NetFlow v5, a traceroute |
| `agent-7.84.0-intake-resources.json` | the resources check every agent posts to `/intake/` |
| `system-probe-7.84.0-*-aggregations.bin` | what system-probe put inside connections between lab containers: HTTP, Postgres, Redis |

How they were recorded, and how to record them again for a newer agent:
`lab/README.md`. The API key in the recorded URLs is replaced by zeros. A file
ending in a DBM track's name is picked up by the tests on its own.
