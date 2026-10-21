# Labs

Real senders pointed at NinjaCat, to see what they actually put on the wire.
Two kinds:

- `k8s/` — a kind cluster running the whole stack with a real agent in it
  (its own README).
- `server.sh`, `dbm/`, `net/` — single-purpose runs against the dev stack's
  databases, described here. They are how the payloads under
  `backend/intake/tests/fixtures/real/` were recorded.

## The server

```sh
docker compose up -d postgres clickhouse migrate   # the dev stack's databases
lab/server.sh up                                   # builds backend/, starts every intake program, mints a temporary key
…runs…
lab/server.sh down                                 # removes all of it
```

The lab server is the server's image (built with `PROFILE=dev`), one container
per intake program, behind a Traefik that routes each host name as the dev
stack's does (`dev/intake-routes.yml`). The agents reach only that router, on a
network with **no route out** (`--internal`), so nothing can reach Datadog
whatever the agent is told. It writes to a scratch ClickHouse database,
`ninjacat_lab` (migrated by `ninjapaws_migrate` first), under the tenant `lab`.

Every program runs with `DEBUG=true` and dumps each request to `lab/captures/`
(gitignored), as your own user, once it has been answered: one file per
request, named `<time>_<method>_<path>_<n>.txt` (`n` counts each program's
requests), and a line in `index.log`. A file holds the status, the time taken,
the sender's address (behind the router, the router's), the request line and
headers (an API key
in a header, the query or the path is left out) and the body decompressed: JSON
pretty-printed, protobuf walked field by field without a schema, text as it is,
anything else as a hexdump. `backend/intake/src/capture.rs` writes them.

```
# time:     2026-10-21T00:50:01.790696Z
# client:   172.18.0.1
# status:   202
# took:     2.8 ms
# body:     61 B raw -> 48 B decompressed (zstd)

POST /api/v2/series HTTP/1.1
Host: app.ninjacat.lab:8080
accept: */*
dd-api-key: <redacted>
content-type: application/x-protobuf
content-encoding: zstd
user-agent: datadog-agent/7.84.0
content-length: 61

---------------- BODY ----------------
(protobuf, walked without a schema: field_number: value)

1: {
  1: {
    1: "host"
    2: "h1"
  }
  2: "system.cpu.idle"
  4: {
    1: 4602678819172646912 (f64 0.5)
    2: 1790151330
  }
}
```

## Database Monitoring

```sh
lab/dbm/run.sh postgres      # or mysql, mariadb, sqlserver, oracle, mongo, clickhouse
DURATION=400 lab/dbm/run.sh oracle
lab/dbm/export.py <name> '<from, UTC>' '<to, UTC>'
```

One run starts the database, creates the monitoring user the integration
asks for, starts a real agent with that integration and `dbm: true`, runs
queries for `DURATION` seconds (110 by default) and prints the check's status.
`export.py` then writes one event of every kind the run produced into the
fixtures, as `<name>-<track>.json`.

Run them one at a time: SQL Server and Oracle want about 2 GB each. Oracle
collects query metrics and plans once a minute and needs two rounds, so give
it a longer run.

`lab/dbm/run-sds.sh` runs the agent's data security check (Sensitive Data
Scanner) against a Postgres with an e-mail address and a card number planted
in it. It needs `shared_library_check.enabled` and `data_security.enabled`;
in production the scan tasks arrive through Remote Configuration, here they
are a check config.

## Network

```sh
lab/net/run.sh         # an SNMP device (net-snmp), three traps, NetFlow v5
lab/net/run-probe.sh   # system-probe: connections, service monitoring, traceroute
```

`run-probe.sh` starts a **privileged** agent that loads eBPF programs into
the host's kernel and sees every connection on the machine, not only the
lab's. Everything it sends stays in the scratch database; do not turn what it
recorded into a fixture without cutting it down to the lab's own traffic.
