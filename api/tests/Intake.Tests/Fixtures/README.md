# Golden fixtures

`go/<TestName>/<n>.json` are requests the Go server's own tests sent through
its intake, recorded with what Go answered and what it handed to storage:

- `request`: method, host, URL, headers and the body as it arrived;
- `response`: status, headers, body (`body_json` when it is JSON);
- `sends`: each batch written — the table (`to`), its INSERT statement, the
  rows as records (`rows`) and as the column values given to the ClickHouse
  driver (`args`). `null` when the Go test did not capture writes;
- `routes`: the Go route sets of the engine that served the request;
- `volatile`: paths that differ between two runs of the same test (receive
  times, minted ids) and are not compared.

`GoldenTests.fs` replays each request against the F# intake and compares all
of it. `overrides.json` lists differences accepted for a fixture, each with
its reason:

    { "TestName/001": { "ignore": ["/sends/0/rows/0/Note"], "why": "…" } }

They were recorded by `server/intake/fixtures_test.go` (two runs, merged by
`merge.py`, which is how `volatile` is found) and are frozen: they are what
the Go server did at the time it was replaced.

`go/TestPortProbe*` were recorded the same way from a probe written during the
port (every Kubernetes object kind, sparse and full, bad frames, container
lifecycle and image payloads, Kubernetes action batches), to cover what the Go
server's own tests did not send.
