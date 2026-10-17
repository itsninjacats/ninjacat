# How NinjaCat watches itself

NinjaCat can be its own first customer: its server processes report traces, runtime
metrics, profiles and logs to the installation they belong to, in the same wire formats any
Datadog-instrumented program uses. Nothing in NinjaCat's source knows about it. There is no
Datadog package in `api/`, no tracing code and no special logger. Datadog's .NET tracer
(`dd-trace-dotnet`) is loaded into the processes from outside and does all of it.

There are two setups:

- **Locally**, `compose.self.yaml` copies the tracer into a volume and sets the variables
  that load it (`README.md`, "NinjaCat watching itself").
- **On Kubernetes**, the Datadog agent's cluster agent puts the tracer into the pods as
  they are created. The rest of this page is about that setup, as it runs on the ninjapaw
  cluster (Datadog Operator 2.28.1, agent and cluster agent 7.84.1, tracer 3.54.0). Where
  the two setups differ, it says so.

| What | Leaves the process | Through | Arrives at (intake host) | Stored in |
|---|---|---|---|---|
| Traces | HTTP to the agent, port 8126 | the agent's trace-agent | `trace.agent.<site>` | `spans`, `apm_stats` |
| Runtime metrics | DogStatsD to the agent, UDP 8125 | the agent | `app.<site>` | `metrics`, `metrics_1m` |
| Profiles | HTTP to the agent, port 8126 | the agent, as a proxy | `intake.profile.<site>` | `profiles` |
| Tracer telemetry | HTTP to the agent, port 8126 | the agent, as a proxy | `instrumentation-telemetry-intake.<site>` | `apm_telemetry` |
| Logs | HTTPS, straight to the intake | nothing | `http-intake.logs.<site>` | `logs` |

`<site>` is the installation's `domainRoot`, `paw.ninjacats.eu` on ninjapaw. The intake
routes each request on its `Host` header (`api/src/Intake/Routes.fs`), so a request must
name one of these hosts.

## 1. Getting the tracer into the pods

### The admission controller

Kubernetes lets a cluster register *mutating admission webhooks*. When a pod is about to
be created, the API server sends its spec to the webhook, and the webhook can change the
spec before it is stored. The Datadog cluster agent runs one of these
(`admissionController.enabled` in the DatadogAgent). On ninjapaw it acts only on pods that
opt in:

- the label `admission.datadoghq.com/enabled: "true"` opts a pod in;
- the annotation `admission.datadoghq.com/dotnet-lib.version: v3.54.0` says which tracer to
  inject, and `admission.datadoghq.com/apm-inject.version` which injector;
- the labels `tags.datadoghq.com/service` and `tags.datadoghq.com/env` become `DD_SERVICE`
  and `DD_ENV`.

The webhook adds the following to an opted-in pod. This list is what
`kubectl get pod -o yaml` shows on an injected `ninjacat-intake` pod:

- **Two init containers.**
  - `datadog-lib-dotnet-init` (image `dd-lib-dotnet-init`) copies the tracer's files into
    an `emptyDir` volume.
  - `datadog-init-apm-inject` (image `apm-inject`) copies the *injector* into the same
    volume, and into a second one an `ld.so.preload` that names it.
- **Mounts in the app container.**
  - The tracer at `/opt/datadog/apm/library`.
  - The injector at `/opt/datadog-packages/datadog-apm-inject`.
  - `/etc/ld.so.preload`.

  All three are read-only. That is why injection works with the chart's
  `readOnlyRootFilesystem`.
- **Environment.**
  - `LD_PRELOAD=/opt/datadog-packages/datadog-apm-inject/stable/inject/launcher.preload.so`.
  - `DD_AGENT_HOST=datadog-agent.datadog.svc.cluster.local`, because the DatadogAgent sets
    `agentCommunicationMode: service`.
  - `DD_SERVICE`, `DD_ENV`, `DD_ENTITY_ID` and a few `DD_INSTRUMENTATION_*` and
    `DD_INJECT_*` bookkeeping variables.
- **Annotations recording the result.** For example
  `internal.apm.datadoghq.com/injection-status: injected`.

The webhook acts only when a pod is created. A pod created while the cluster agent was
down runs without the tracer until it is recreated (`kubectl rollout restart`).

### From the preloaded library to a loaded tracer

`LD_PRELOAD` and `/etc/ld.so.preload` tell Linux's dynamic linker to load a library into
every program the container starts, before the program's own code runs. That library is
the injector's launcher. For a .NET program (here the apphosts `/app/ninjacat-api` and
`/app/ninjacat-monitors`) it makes the .NET runtime (CoreCLR) load the tracer as a
profiler at start-up. As Datadog describes the injector, it does so with the same
variables `compose.self.yaml` sets by hand; we have not looked at a running process's
environment, only at its loaded libraries (section 6):

```
CORECLR_ENABLE_PROFILING=1
CORECLR_PROFILER={846F5F1C-F9AE-4B07-969E-05C26BC060D8}
CORECLR_PROFILER_PATH=…/Datadog.Trace.ClrProfiler.Native.so
DD_DOTNET_TRACER_HOME=…
```

They are not in the pod spec, so `kubectl get pod -o yaml` does not show them. The values
in the chart must not set `CORECLR_*` or `LD_PRELOAD`, because those would replace what
the webhook set up.

The runtime then loads `Datadog.Trace.ClrProfiler.Native.so`, the tracer's native part.
That in turn loads the managed tracer (`Datadog.Trace.dll`) into the application before
`Main` runs. From then on the tracer lives inside the NinjaCat process: same memory, same
threads, same `HttpClient` stack.

### What the chart provides

The chart (`helm/ninjacat`) knows nothing about Datadog. It gives each of `intake`, `query`
and `monitors` four generic hooks:

- `podLabels` and `podAnnotations` for the opt-in above;
- `env`, a map of non-secret variables that goes into the component's ConfigMap;
- `extraEnv`, a list rendered as-is into the container's `env`. It is for what must not sit
  in a ConfigMap (`secretKeyRef`) or comes from the pod itself (`fieldRef`).

The ninjapaw values (kept with the deployment, outside this repository) fill all four. In
outline:

```yaml
x-tracer-annotations: &tracer-annotations
  admission.datadoghq.com/dotnet-lib.version: v3.54.0
  admission.datadoghq.com/apm-inject.version: "0.71.4"
  ad.datadoghq.com/logs_exclude: "true"
x-tracer-env: &tracer-env
  DD_PROFILING_ENABLED: "true"
  DD_RUNTIME_METRICS_ENABLED: "true"
  DD_INSTRUMENTATION_TELEMETRY_AGENTLESS_ENABLED: "false"
  DD_TRACE_LOG_DIRECTORY: /tmp
  DD_SITE: paw.ninjacats.eu
  DD_LOGS_DIRECT_SUBMISSION_INTEGRATIONS: ILogger
  DD_LOGS_DIRECT_SUBMISSION_MAX_QUEUE_SIZE: "10000"
x-tracer-extra-env: &tracer-extra-env
  - name: DD_API_KEY
    valueFrom: { secretKeyRef: { name: ninjacat-selfmon, key: api-key } }
  - name: NODE_NAME
    valueFrom: { fieldRef: { fieldPath: spec.nodeName } }
  - name: POD_NAME
    valueFrom: { fieldRef: { fieldPath: metadata.name } }
  - name: DD_LOGS_DIRECT_SUBMISSION_HOST
    value: $(NODE_NAME)-ninjapaw
  - name: DD_LOGS_DIRECT_SUBMISSION_TAGS
    value: env:prod,kube_cluster_name:ninjapaw,kube_namespace:ninjacat,pod_name:$(POD_NAME)

intake:
  podLabels:
    admission.datadoghq.com/enabled: "true"
    tags.datadoghq.com/env: prod
    tags.datadoghq.com/service: ninjacat-intake
  podAnnotations: *tracer-annotations
  env: *tracer-env
  extraEnv: *tracer-extra-env
# query and monitors the same, with their own service label
```

Kubernetes expands `$(NODE_NAME)` only inside an `env` entry's `value`, and only for
variables defined earlier in the same list. ConfigMap values are never expanded. This is
why the host and tags are in `extraEnv` and not in `env`.

## 2. How the tracer instruments code it was not compiled into

.NET does not run compiled machine code from the start. Assemblies hold IL, an
intermediate bytecode, and the runtime's JIT compiler turns each method into machine code
the first time it is called. The **CLR profiling API** lets a native library loaded at
start-up be told when a module loads and when a method is about to be compiled. It can
also hand the JIT different IL for that method. Profilers are built on it,
and it is what `CORECLR_PROFILER_PATH` loads.

The tracer has a list of library methods it knows, each named by assembly, type, method,
parameter types and a supported version range. When one of them is about to be compiled,
the native part rewrites its IL so that it calls into the managed tracer on entry and on
exit. In the tracer's source this is called *CallTarget*: `OnMethodBegin` and
`OnMethodEnd`. The tracer starts a span on entry and finishes it on exit, with what it read
from the arguments and the return value. Methods not on the list are left alone, so the
cost is paid only where there is something to record.

What this covers in NinjaCat:

- **ASP.NET Core.** One `aspnet_core.request` span per request, named after the route,
  e.g. `POST /api/v2/series`. ASP.NET Core already publishes request events through
  `DiagnosticListener`, so for these the tracer mostly subscribes to them rather than
  rewriting methods.
- **HttpClient.** `SocketsHttpHandler.SendAsync` is rewritten, which gives one
  `http.request` span per outgoing call, under the service `<service>-http-client`. The
  server talks to ClickHouse over HTTP, so every insert and query is one of these spans.
- **Npgsql.** Command execution is rewritten, giving the Postgres spans (services such as
  `ninjacat-intake-npgsqldatasource` and `ninjacat-monitors-postgres`).
- **Microsoft.Extensions.Logging.** The `LoggerFactory` constructor is rewritten. Section 4
  explains what the tracer adds there.

Runtime metrics (`runtime.dotnet.*`: GC, heap, thread pool, exceptions, ASP.NET Core
counters) come from the diagnostic events and meters the runtime and ASP.NET Core already
publish, which the tracer listens to. The
continuous profiler is a separate native component in the same package. It samples stacks
and allocations on its own threads.

## 3. Traces, runtime metrics and profiles

These three go through the node's Datadog agent, as they would towards Datadog.

- **Traces.** The tracer posts finished traces to the agent (`DD_AGENT_HOST`, port 8126).
  The agent's trace-agent computes statistics and forwards both to
  `trace.agent.<site>`. The intake stores spans in `spans` and the statistics in
  `apm_stats`.
- **Runtime metrics.** These are sent as DogStatsD over UDP to port 8125 of the same
  Service. The agent aggregates them and sends them with its own metrics to `app.<site>`,
  stored in `metrics` and rolled up into `metrics_1m`.
- **Profiles.** The profiler posts to the agent, which passes them through to
  `intake.profile.<site>`, stored in `profiles`.
- **Tracer telemetry.** This reports what the tracer loaded and its own health. It goes
  through the agent to `instrumentation-telemetry-intake.<site>` and is stored in
  `apm_telemetry`. `DD_INSTRUMENTATION_TELEMETRY_AGENTLESS_ENABLED=false` keeps it there:
  once `DD_API_KEY` is set (section 4), the tracer would otherwise send it straight to the
  site.

The agent learns the site from the DatadogAgent's `global.site`. It sends with its own key,
not the pods'.

## 4. Logs by direct submission

### Before: stdout through the agent

Every process writes its log through `ILogger`. The chart sets ASP.NET's JSON console
formatter (`Logging__Console__FormatterName: json` in `templates/configmap.yaml`), so each
event becomes one JSON line on stdout. Kubernetes writes stdout to a file under
`/var/log/pods`, and the agent tails that file and sends every line to
`agent-http-intake.logs.<site>`.

To the agent each line is plain text, so the stored row looks like this:

- `message` is the whole JSON line, with the level, category and fields buried inside;
- `status` comes from the stream, `info` for stdout and `error` for stderr. A warning is
  stored as `info`;
- there are no trace ids, because the console formatter does not write scopes.

### Now: from ILogger to the intake

The tracer comes with its own `ILogger` provider, `DirectSubmissionLoggerProvider`. This
is how it gets wired in:

1. When `Microsoft.Extensions.Logging`'s `LoggerFactory` is constructed, the rewritten
   constructor calls the tracer on exit.
2. The tracer calls `loggerFactory.AddProvider(...)` with its provider.
3. The tracer's provider now sits beside the console provider. The application's own
   filters still apply to both, e.g. `Microsoft.AspNetCore` at Warning
   (`api/src/Platform/Hosting.fs`).
4. Every log call reaches both providers. The console one still writes its line to stdout,
   so `kubectl logs` is unchanged.

The tracer's provider gets the event before anything turns it into text:

- the level as a value, not a word inside a string;
- the category, i.e. the logger's name, e.g. `NinjaCat.Api.Storage`;
- the message template and each named value in it, e.g. `Table`, `InFlight` and `Buffered`
  for `"writer {Table}: {InFlight} flushes in flight, deferring ({Buffered} buffered)"`;
- the exception, if there is one;
- the active scopes. The tracer opens one itself (*logs injection*, on by default) that
  carries `dd_trace_id` and `dd_span_id` whenever a span is active, so a log line written
  while handling a request names that request's trace.

It writes each event as one compact JSON object:

| Field | What it holds |
|---|---|
| `@t` | time |
| `@m` | the rendered message |
| `@l` | level; left out for Information |
| `@x` | the exception |
| `@i` | the event id |
| `Category` | the logger's name |
| the template's own named values | one field each |
| scope fields | one field each |
| `dd_service`, `dd_env`, `dd_trace_id`, `dd_span_id` | from the tracer |
| `ddsource`, `host`, `ddtags` | from the settings |

The real payload recorded locally is
`api/tests/Intake.Tests/Fixtures/real/dd-trace-dotnet-3.54.0-logs.json`.

Events wait in an in-memory queue. Every 2 seconds, up to 1000 of them are posted
uncompressed to `https://http-intake.logs.<DD_SITE>:443/api/v2/logs`, with the key in the
`DD-API-KEY` header. The intake's logs router (`api/src/Intake/Routers/Logs.fs`) maps them:

| Payload field | Becomes |
|---|---|
| `@m` | `message` |
| `@l` | `status` (`Warning` → `warn`, no `@l` → `info`) |
| `@t` | `timestamp` |
| `host` | `host` |
| `dd_service` | `service` |
| `ddsource` (`csharp`) | `source` |
| `ddtags` | `tags` |

Everything else, including `Category`, the named fields and `dd_trace_id`, stays in the
`attributes` JSON column.

### The key, the host and the tags

- **The key.** Direct submission does not go through the agent, so it needs a NinjaCat key
  of its own: a key made in the panel for the `default` tenant, for this alone. It is kept
  in the Secret `ninjacat-selfmon` and given to the pods as `DD_API_KEY`.
- **The host.** The tracer's default is the machine name, which in a pod is the pod's name.
  That would make every pod a host of its own. `DD_LOGS_DIRECT_SUBMISSION_HOST` sets it to
  the agent's name for the node, `<node>-<cluster name>` (e.g. `tiri-ninjapaw`). These
  logs then sit on the same host as the node's metrics.
- **The tags.** `DD_LOGS_DIRECT_SUBMISSION_TAGS` puts back the ones the agent's container
  collection used to add and that matter for searching: `env`, `kube_cluster_name`,
  `kube_namespace`, `pod_name`.

### The way in: the public hostname

The intake picks a router by `Host`, and the tracer cannot be told to send a different
`Host` than its URL's. So the URL must use the public name. On ninjapaw,
`http-intake.logs.paw.ninjacats.eu` resolves to the node's own address, and Traefik listens
there on host ports. A request from a pod therefore leaves the pod, reaches Traefik on the
node and comes back in through the intake's Ingress. The agent's own logs already take the
same path through `agent-http-intake.logs.paw.ninjacats.eu`. Delivery depends on public
DNS, Traefik and the certificate. There is no in-cluster fallback.

### Why the agent stops collecting these containers' stdout

If the agent kept tailing stdout, every event would be stored twice: once as text and once
structured. The pods carry the annotation `ad.datadoghq.com/logs_exclude: "true"`, which
tells the agent not to collect any of their containers' logs. It affects logs only.
Metrics, the process and container checks and APM still see these pods. The plain
`ad.datadoghq.com/exclude` would hide them from metrics too, so it is not used. Other pods,
including the panel and the databases, are collected as before.

The switch is per pod, so the `helm upgrade` that turns it on stores no line twice. A pod
made before it has no annotation and no key: the agent tails it until it stops. A pod made
by it has both from its creation. The intake and query start their new pod before the old
one stops, so the intake stays up throughout. Monitors stop one pod before starting its
replacement. If the Secret is missing, the new pods do not start; intake and query keep
their old pods, and monitors run one pod short until the Secret exists.

### The intake logging to itself

The intake's own log lines are posted to the intake. This does not grow into a loop:

- A logs request the intake can read writes no log line. ASP.NET's request logging is
  filtered to Warning, and a refused key logs below Warning.
- What the intake does log, such as a writer deferring a flush or an unhandled endpoint,
  goes out in the next batch, as one more request.
- An event the intake cannot read is kept in `raw_payloads` and logged as one warning
  (`Raw.store`). That warning goes out in the next batch. If the tracer's events ever
  stopped decoding, the result would be a steady echo of one warning per refused event,
  not a growing loop. This is how the first local run found that the logs intake refused
  events without a `message` (`TODO.md`).
- Each of those requests is also a span in the intake's own traces. An idle intake is
  never silent.

### What can be lost, and where to look then

The agent tails files, so after an outage it catches up from where it stopped. Direct
submission keeps its events in memory and gives up sooner:

- **The queue is bounded.** It holds `DD_LOGS_DIRECT_SUBMISSION_MAX_QUEUE_SIZE` events,
  10000 on ninjapaw (the default, 100000, is a lot of memory under a 512Mi limit). When
  it is full, new events are dropped.
- **Retries are short.** A batch is tried up to 5 times, backing off from half a second to
  4 seconds. A 408, a 429 or a 5xx is retried; any other 4xx (a wrong key, say) is not.
  A batch that still fails is dropped, not put back.
- **The circuit breaker.** After 10 failed batches in a row the sink drops new events
  for a period at a time between attempts, until a batch succeeds.
- **Permanent shutdown.** If those 10 failures come before any batch has ever succeeded,
  direct submission switches itself off until the process restarts. A failed batch takes
  about 10 seconds with its retries, and only a batch with something in it counts, so
  this needs a pod with lines to send and an intake out of reach for about a minute and
  a half from the pod's start, for instance after a node reboot where the intake comes
  up last. A wrong key does it faster: a 403 is not retried. Restart such pods once the
  intake is back.
- **Anything written outside `ILogger` never reaches the intake.** That includes a runtime
  crash's stack trace on stderr and lines written before the host is built. The last
  lines of a process that is killed rather than stopped are lost too; a stopped one gets
  up to 20 seconds to send them.

In all these cases the text is still on stdout. `kubectl logs` shows it, and
`kubectl logs --previous` shows it for a container that restarted. The tracer writes its
own diagnostics, including why direct submission turned off, to
`/tmp/dotnet-tracer-managed-*.log` inside the container (`DD_TRACE_LOG_DIRECTORY`).

## 5. The settings

Set by the webhook (do not set these in the values):

| Variable | What it does |
|---|---|
| `LD_PRELOAD` | Loads the injector's launcher into every program in the container. |
| `DD_AGENT_HOST` | Where traces, profiles, telemetry and DogStatsD go: the agent's Service. |
| `DD_SERVICE`, `DD_ENV` | The service and environment on everything the tracer sends, taken from the `tags.datadoghq.com/*` labels. |
| `CORECLR_*`, `DD_DOTNET_TRACER_HOME` | Load the tracer into the runtime. Left to the launcher (section 1); not in the pod spec. |

In `env` (the ConfigMap):

| Variable | What it does |
|---|---|
| `DD_PROFILING_ENABLED` | Turns the continuous profiler on. Injection does not. |
| `DD_RUNTIME_METRICS_ENABLED` | Turns runtime metrics on. Injection does not. |
| `DD_INSTRUMENTATION_TELEMETRY_AGENTLESS_ENABLED=false` | Keeps the tracer's telemetry going through the agent. Required once `DD_API_KEY` is set, since an API key makes it default to sending straight to the site. |
| `DD_TRACE_LOG_DIRECTORY=/tmp` | Where the tracer writes its own log. The root filesystem is read-only, and `/tmp` is an `emptyDir`. |
| `DD_SITE` | The site every URL the tracer builds names, including the logs intake. Without it a tracer-built URL defaults to `datadoghq.com`. |
| `DD_LOGS_DIRECT_SUBMISSION_INTEGRATIONS=ILogger` | The switch: adds the tracer's `ILogger` provider. Empty means off. |
| `DD_LOGS_DIRECT_SUBMISSION_MAX_QUEUE_SIZE` | How many events may wait in memory. Default 100000. |

In `extraEnv` (the container's own `env` list):

| Variable | What it does |
|---|---|
| `DD_API_KEY` | The NinjaCat key sent with each batch, from the Secret `ninjacat-selfmon`. Without it direct submission turns itself off. |
| `NODE_NAME`, `POD_NAME` | The node's and the pod's names, from the pod (`fieldRef`). Used only by the two below. |
| `DD_LOGS_DIRECT_SUBMISSION_HOST` | The `host` on every event: `$(NODE_NAME)-ninjapaw`. Default: the pod's name. |
| `DD_LOGS_DIRECT_SUBMISSION_TAGS` | Tags on every event (`k:v,k:v`). For logs they replace `DD_TAGS`. |

Left at their defaults:

| Variable | Default and what it does |
|---|---|
| `DD_LOGS_DIRECT_SUBMISSION_URL` | Built from `DD_SITE` as `https://http-intake.logs.<site>:443`. Locally `compose.self.yaml` sets it, because its intake speaks plain HTTP on 8080. |
| `DD_LOGS_DIRECT_SUBMISSION_MINIMUM_LEVEL` | `Information`. Applied after the application's own filters. |
| `DD_LOGS_DIRECT_SUBMISSION_SOURCE` | `csharp`. Becomes the row's `source`. |
| `DD_LOGS_DIRECT_SUBMISSION_BATCH_PERIOD_SECONDS` | 2 seconds. |
| `DD_LOGS_DIRECT_SUBMISSION_MAX_BATCH_SIZE` | 1000 events. |
| `DD_LOGS_INJECTION` | On. Puts `dd_trace_id`, `dd_span_id` and the service into each event's scope. |

The pod annotation `ad.datadoghq.com/logs_exclude: "true"` is not read by the tracer. It
is read by the agent (section 4).

The Secret is made once, before the first `helm upgrade` that names it, from a key created
in the panel (Settings → API keys):

```bash
kubectl -n ninjacat create secret generic ninjacat-selfmon --from-literal=api-key=<key>
```

Without it the three pods do not start (`CreateContainerConfigError`). This is deliberate:
a missing key would otherwise silently lose their logs.

## 6. Checking that it works

Everything below only reads.

Is the tracer injected and loaded?

```bash
kubectl -n ninjacat get pods -l app.kubernetes.io/name=ninjacat \
  -o custom-columns=POD:.metadata.name,INJECTION:'.metadata.annotations.internal\.apm\.datadoghq\.com/injection-status'
kubectl -n ninjacat exec deploy/ninjacat-intake -c intake -- grep -c Datadog.Trace.ClrProfiler.Native /proc/1/maps
```

Did direct submission start, or why not?

```bash
kubectl -n ninjacat exec deploy/ninjacat-intake -c intake -- \
  sh -c 'grep -hi "direct" /tmp/dotnet-tracer-managed-*.log | tail -n 20'
```

The ClickHouse queries below can be run with any read-only client, e.g. `clickhouse-client
--readonly=1` in the ClickHouse pod.

Logs arriving by direct submission, per service, host and level:

```sql
SELECT service, host, status, count()
FROM ninjacat.logs
WHERE tenant_id = 'default' AND source = 'csharp' AND timestamp > now() - INTERVAL 10 MINUTE
GROUP BY ALL ORDER BY service, status
```

The categories they come from:

```sql
SELECT service, toString(attributes.Category) AS category, status, count()
FROM ninjacat.logs
WHERE tenant_id = 'default' AND source = 'csharp' AND timestamp > now() - INTERVAL 1 HOUR
GROUP BY ALL ORDER BY count() DESC LIMIT 20
```

Nothing should still arrive from the agent for these containers (expect no rows a few
minutes after the pods were replaced):

```sql
SELECT service, source, count()
FROM ninjacat.logs
WHERE tenant_id = 'default' AND timestamp > now() - INTERVAL 10 MINUTE
  AND source != 'csharp' AND service IN ('ninjacat-intake', 'ninjacat-query', 'ninjacat-monitors')
GROUP BY ALL
```

Log lines joined to the request they were written in:

```sql
SELECT l.timestamp, l.status, l.message, s.resource
FROM ninjacat.logs AS l
INNER JOIN ninjacat.spans AS s ON s.trace_id_hex = toString(l.attributes.dd_trace_id)
WHERE l.tenant_id = 'default' AND l.source = 'csharp' AND l.timestamp > now() - INTERVAL 1 HOUR
  AND s.start > now() - INTERVAL 1 HOUR AND s.parent_id = 0
ORDER BY l.timestamp DESC LIMIT 10
```

Traces, runtime metrics and profiles, per service:

```sql
SELECT service, count() FROM ninjacat.spans
WHERE start > now() - INTERVAL 1 HOUR GROUP BY service;

SELECT service, uniq(metric), count() FROM ninjacat.metrics
WHERE timestamp > now() - INTERVAL 10 MINUTE AND metric LIKE 'runtime.dotnet.%' GROUP BY service;

SELECT service, count() FROM ninjacat.profiles
WHERE received_at > now() - INTERVAL 1 HOUR GROUP BY service;
```

In the panel, the same rows are on the Logs page (`/app/logs`) of the `default` tenant.
