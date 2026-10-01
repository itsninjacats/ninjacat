# Labs

Real senders pointed at NinjaCat, to see what they actually put on the wire.
Two kinds:

- `k8s/` — a kind cluster running the whole stack with a real agent in it
  (its own README).
- `server.sh`, `dbm/`, `net/` — single-purpose runs against the dev stack's
  databases, described here. They are how the payloads under
  `api/tests/Intake.Tests/Fixtures/real/` were recorded.

## The server

```sh
docker compose up -d postgres clickhouse migrate   # the dev stack's databases
lab/server.sh up                                   # builds api/, starts its intake, mints a temporary key
…runs…
lab/server.sh down                                 # removes all of it
```

The lab server is the shipping image on a network with **no route out**
(`--internal`), so nothing can reach Datadog whatever the agent is told. It
writes to a scratch ClickHouse database, `ninjacat_lab`, under the tenant
`lab`, and dumps every request to `lab/captures/` (gitignored).

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
