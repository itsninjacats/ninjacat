# NinjaCat end-to-end lab

A single-node [kind](https://kind.sigs.k8s.io/) cluster that runs the whole
NinjaCat stack (ClickHouse, Postgres, the Go server, the panel, a TLS proxy)
plus a real Datadog agent pointed at it. The agent collects genuine cluster
telemetry and ships it into NinjaCat, so the full intake path is exercised
exactly as in production.

> **WARNING — your default kube context is a REAL REMOTE CLUSTER.**
> Never run a bare `kubectl` command against this lab. Every command must
> pass the context explicitly:
>
> ```sh
> kubectl --context kind-ninjacat-lab -n ninjacat-lab get pods
> ```

## Start

```sh
./scripts/up.sh
```

## Host ports

| Host port | What |
|-----------|------|
| 8888      | Panel (browser) — `http://localhost:8888` |
| 18123     | ClickHouse HTTP — `curl 'localhost:18123/?user=ninjacat&password=ninjacat&query=SELECT+1'` |
| 18081     | NinjaCat panel API — `curl localhost:18081/ping` |

## Verify data is flowing

```sh
./scripts/verify.sh              # defaults to kind-ninjacat-lab
```

Prints row counts per ClickHouse table plus samples (metric names, newest
logs, k8s resource kinds) that prove real agent data arrived.

## Pointing a Datadog agent at NinjaCat

`datadog-values.yaml` is the interesting file here, and it is heavily
commented. Three things in it cost real time to work out, all of which fail by
appearing to work:

**The agent needs to trust us, and `SSL_CERT_FILE` is how.** The agent composes
`https://<product>.<site>` for every endpoint that has no `dd_url` of its own —
container lifecycle, container images and the orchestrator among them, which
are exactly the payloads worth testing. The agent is written in Go, so one
mounted CA in `SSL_CERT_FILE` covers every HTTP client inside every container,
including the ones that never read `DD_SKIP_SSL_VALIDATION`. Note the volume
belongs under `agents.volumeMounts`, not `datadog.volumeMounts` — the latter is
not a key this chart defines, so it is accepted and silently ignored while
`datadog.env` works, leaving containers pointed at a CA file that is not there.

**A file in `confd` does not replace the bundled `auto_conf.yaml`, it runs
beside it.** Filtering `kube_apiserver_metrics` this way produced one instance
reporting 1,833 samples a run and another, untouched, still reporting 12,834 —
so the filter worked perfectly and changed nothing. `datadog.ignoreAutoConfig`
is what suppresses the bundled config.

**This check wants `ignore_metrics`, not `exclude_metrics`.** The latter is the
OpenMetrics v2 spelling; it parses, loads without complaint, and filters
nothing.

Why filter in the agent at all, rather than in NinjaCat's writer: because
dropping or rewriting what an agent sent would make NinjaCat a lossy
approximation of Datadog rather than a replacement for it. The operator asked
for that data by configuring the check. Datadog puts the same knob in the same
place. See `docs/decisions/0001-tags-are-a-multiset.md` for the same principle
applied to tags.

## Upgrading the release

Use `--reset-values` when re-applying `datadog-values.yaml`. `--reuse-values`
merges with whatever the previous revision held, so a single bad upgrade stays
in the release and later revisions inherit it — in one case removing the node
agent DaemonSet entirely while `helm template` locally still rendered it.

## Tear down

```sh
kind delete cluster --name ninjacat-lab
```

Everything is in-memory (`emptyDir`), so deleting the cluster removes all
state. Nothing persists between runs.
