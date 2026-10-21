# Configuring the Datadog agent for NinjaCat

Settings ready to paste. The reasoning behind them is in `datadog-agent.md`.

## One server, many configuration keys

Datadog sends each kind of signal to a **different host**:

```
api.<site>                     metrics, hosts, checks, sketches, events
trace.agent.<site>             APM traces and their stats
http-intake.logs.<site>        logs
process.<site>                 process inventory
```

At Datadog these are separate services. NinjaCat is one process with **one list of paths**;
they do not collide, so splitting by host would gain nothing.

The consequence: every agent subsystem has its own configuration key, and all of them must be
set to the same address.

## Option A: explicit addresses

Needs neither wildcard DNS nor a wildcard certificate.

```yaml
# datadog.yaml
api_key: <key from the NinjaCat panel>

dd_url: http://ninjacat:8080                 # metrics, /intake/, check_run, sketches, events

apm_config:
  enabled: true
  apm_dd_url: http://ninjacat:8080           # /api/v0.2/traces + /api/v0.2/stats

logs_enabled: true
logs_config:
  logs_dd_url: ninjacat:8080                 # NOTE: no scheme, host:port
  use_http: true                             # without it logs go over their own TCP+TLS
  logs_no_ssl: true                          # for http, not https

process_config:
  process_collection:
    enabled: true
  process_dd_url: http://ninjacat:8080       # /api/v1/collector

orchestrator_explorer:
  enabled: true
  orchestrator_dd_url: http://ninjacat:8080  # /api/v2/orch — Kubernetes resources, a SEPARATE key
```

As environment variables (Docker, Kubernetes):

```bash
DD_API_KEY=<key>
DD_DD_URL=http://ninjacat:8080
DD_APM_DD_URL=http://ninjacat:8080
DD_APM_ENABLED=true
DD_LOGS_ENABLED=true
DD_LOGS_CONFIG_LOGS_DD_URL=ninjacat:8080
DD_LOGS_CONFIG_USE_HTTP=true
DD_LOGS_CONFIG_LOGS_NO_SSL=true
DD_PROCESS_CONFIG_PROCESS_COLLECTION_ENABLED=true
DD_PROCESS_CONFIG_PROCESS_DD_URL=http://ninjacat:8080
```

## Option B: one key, `DD_SITE` (what the Helm chart sets up)

The agent builds each address as `<hard-coded prefix>.<site>`. If `site` points at NinjaCat,
every pipeline reaches it on its own:

```bash
DD_SITE=ninjacat.example.com
```

Measured: this gives `api.ninjacat.example.com`, `trace.agent.ninjacat.example.com`,
`http-intake.logs.ninjacat.example.com` and so on.

The gain: **one key instead of six**, and it also covers the pipelines that have no `dd_url`
of their own.

The cost:

- wildcard DNS: `*.ninjacat.example.com` → NinjaCat
- a TLS certificate for the bare name and every intake host (the chart lists them; a TLS
  wildcard does not match the two-label ones such as `trace.agent.`)

## Which key feeds which endpoint

| configuration key | NinjaCat endpoints |
|---|---|
| `dd_url` | `/intake/`, `/api/v1/series`, `/api/v2/series`, `/api/v1/check_run`, `/api/v1/metadata`, `/api/beta/sketches`, `/api/v1/events`, `/api/v1/validate` |
| `apm_config.apm_dd_url` | `/api/v0.2/traces`, `/api/v0.2/stats` |
| `logs_config.logs_dd_url` | `/api/v2/logs`, `/v1/input` |
| `process_config.process_dd_url` | `/api/v1/collector` |
| `orchestrator_explorer.orchestrator_dd_url` | `/api/v2/orch`, `/api/v2/orchmanif` |
| *(none — API client only)* | `/api/v1/distribution_points` |

`dd_url` **does not redirect traces.** This is the most common mistake: the agent keeps sending
them to `trace.agent.datadoghq.com`, because the trace agent is a separate component with a
separate key. That is exactly why no trace showed up in the captures for half of the protocol
work.

## What to turn off with Option A

Subsystems that cannot be redirected, because they have only `additional_endpoints` (which
**add** a receiver rather than replace one):

```bash
DD_REMOTE_CONFIGURATION_ENABLED=false
DD_TELEMETRY_ENABLED=false
DD_INVENTORIES_ENABLED=false
DD_ORCHESTRATOR_EXPLORER_ENABLED=false
DD_CONTAINER_LIFECYCLE_ENABLED=false
DD_CONTAINER_IMAGE_ENABLED=false
DD_SBOM_ENABLED=false
DD_DATABASE_MONITORING_ENABLED=false
DD_NETWORK_DEVICES_METADATA_ENABLED=false
DD_MULTI_REGION_FAILOVER_ENABLED=false
DD_EVP_PROXY_CONFIG_ENABLED=false
```

## What the agent contacts besides the site, even with `DD_SITE` set

Measured on agent and cluster agent 7.84.1 (Datadog Operator 1.31.0) on two bare-metal
Kubernetes clusters, 2026-10-16: the agents' sockets watched from inside their pods, and the
agent's source read at that tag. Every product pipeline went to `*.<site>`. These did not:

| what | where | how often | turn it off |
|---|---|---|---|
| Connectivity checker (core agent) | `HEAD https://install.datadoghq.com`, `yum.`, `apt.`, `keys.datadoghq.com` | 30 s after start, then every 10 min and on every runtime config change | `DD_INVENTORIES_DIAGNOSTICS_ENABLED=false` |
| `ntp` check, on by default | DNS for `0-3.datadog.pool.ntp.org`, then UDP 123 to whatever pool servers those return | every 15 min | point it elsewhere (below) |
| Cloud metadata probes (hostname, host tags, network ID, cloud NTP) | `http://169.254.169.254` (AWS, GCP, Azure, Oracle, IBM) and `http://100.100.100.200` (Alibaba) | at start, with host metadata, on `agent status` | `DD_CLOUD_PROVIDER_METADATA=none` on the agent and the cluster agent |
| Images | `registry.datadoghq.com` (agent, cluster-agent, operator) | on every pull | a mirror (below) |
| Admission controller, latent | lib injection pulls `registry.datadoghq.com/dd-lib-*-init` into pods that opt in | only for opted-in pods | `admissionController` off (below) |

**Connectivity checker.** Its four URLs are hard-coded
(`pkg/diagnose/connectivity/inventoryendpoint.go`) and ignore `site`; only `install.` follows
`installer.registry.url`. The flag that stops it is `inventories_diagnostics_enabled`, which is
independent of `inventories_enabled`. No fork is needed.

**`ntp`.** `datadog.pool.ntp.org` is Datadog's vendor zone in the public NTP Pool: volunteer
servers, not Datadog's, which the pool asks vendors to use instead of the generic
`pool.ntp.org`. The agent uses it when it detects no cloud provider, and the cloud provider's
NTP when it does. A `conf.d/ntp.d/conf.yaml` of your own replaces `conf.yaml.default` (an
empty one does not: empty files are skipped):

```yaml
init_config:
instances:
  - hosts: [ntp1.hetzner.de, ntp2.hetzner.com, ntp3.hetzner.net]   # or an internal server
```

**Cloud metadata.** The default `cloud_provider_metadata` lists every provider, so on bare metal
the agent spends each probe on a timeout: `agent status` shows `gce: unable to retrieve hostname
from GCE: ... context deadline exceeded`. `169.254.169.254` is link-local and dies at the first
hop, but `100.100.100.200` is in `100.64.0.0/10` and leaves through the node's default gateway.
Matching is by name, so any value that names no provider (`none`) disables all of them; an
empty string may be read as unset.

**Images.** The Datadog Operator's default registry is `registry.datadoghq.com` (its
`pkg/images/images.go`); `gcr.io/datadoghq`, `docker.io/datadog` and `public.ecr.aws/datadog`
serve the same images and are third parties too. To keep pulls in-house, mirror the images
and either set `spec.global.registry` in the DatadogAgent plus `image.repository` in the
operator's Helm values, or leave the manifests alone and add a containerd mirror for
`registry.datadoghq.com` on the nodes (`/etc/rancher/k3s/registries.yaml`,
`/etc/rancher/rke2/registries.yaml`).

**Admission controller.** The operator turns it on by default. Its `lib.injection` webhook is
called on every pod creation outside `kube-system` and the agent's namespace (failure policy
`Ignore`, 10 s timeout), it creates a dry-run ConfigMap every 60 s as a probe, and it would
inject APM tracer libraries pulled from `registry.datadoghq.com` into any pod labelled for it.
Without APM nothing uses it. Turn it off in two steps: first `mutation`, `validation` and `probe`
set to `enabled: false` (the cluster agent then deletes its own webhook configurations), then
`admissionController.enabled: false`. Disabling it in one step leaves the webhook
configurations behind, pointing at a Service that no longer exists.

**system-probe.** The operator's `serviceDiscovery` feature, on by default, adds a privileged
`system-probe` container (SYS_ADMIN, SYS_PTRACE, NET_ADMIN, …) to every agent pod. It talks
only to the agent, but it is a privileged eBPF component that nothing in NinjaCat reads yet:
set `features.serviceDiscovery.enabled: false`. On an SELinux node its seccomp setup failed
and the pod did not start at all.

**Autodiscovery defaults.** The agent ships `auto_conf` templates that attach checks to any
container whose image they recognise (Redis, cert-manager, …). They only read, but a cluster
owner may not expect it: `DD_IGNORE_AUTOCONF="redisdb cert_manager"`. Under the operator, keep
`kubernetes_state` in that list, since the variable replaces the operator's value.

## A guarantee that nothing leaves

All of the above is configuration, not a guarantee. If you need certainty:

```yaml
# docker-compose
networks:
  lab:
    internal: true        # no route to the world, DNS does not resolve
```

Measured: DNS fails, TCP is refused, `curl` returns `000`. It is the only empirically
verified method that also covers the agent-data-plane, which ignores some settings.

A middle ground is to route everything through a proxy:

```bash
DD_PROXY_HTTPS=http://interceptor:8080
```

It catches **all** outgoing traffic, including pipelines without a `dd_url`. That is how we
found the full set of 22 hosts the agent tries to reach. On Kubernetes, an egress
NetworkPolicy on the agent's namespace that allows only NinjaCat (plus DNS, the kubelet and the
apiserver) does the same for the pods, though not for image pulls, which the node makes.
