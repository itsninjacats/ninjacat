# Datadog Agent — kompletny spis endpointów HTTP/gRPC/socketowych

**Cel dokumentu:** umożliwić napisanie w Go serwera-odbiornika, który przyjmie *wszystko*, co ekosystem Datadog Agenta wysyła do backendu Datadoga, oraz odtworzyć te endpointy, które Agent sam wystawia.

**Źródła:** checkout `DataDog/datadog-agent` + `DataDog/agent-payload`. Wszystko poniżej zostało odczytane ze źródeł; miejsca niezweryfikowane są jawnie oznaczone w §6.

---

## 0. Konwencje, które trzeba znać zanim przeczytasz tabele

### 0.1 Jak powstaje host

Prawie żaden host nie jest literałem. Wzorzec:

```
pkg/config/utils/endpoints.go:315  GetMainEndpoint(cfg, prefix, ddURLKey)
  → jeśli cfg[ddURLKey] jest ustawiony i niepusty  → zwróć go DOSŁOWNIE (prefix ignorowany)
  → w przeciwnym razie BuildURLWithPrefix(prefix, cfg["site"])   // prosta konkatenacja
  → fallback: prefix + constants.DefaultSite ("datadoghq.com")
```

Konsekwencje dla reimplementacji:

* Kolumna `host prefix` w tabelach to **prefiks doklejany do `site`**, np. `contlcycle-intake.` + `datadoghq.com`.
* Ustawienie `config key` (np. `dd_url`) podmienia **cały** URL — prefiks znika. To jest jedyna wspierana droga przekierowania ruchu na własny serwer.
* `convert_dd_site_fqdn.enabled` dokleja **kropkę końcową** do znanych site'ów (`app.datadoghq.com.`). Odbiornik musi tolerować trailing dot w nagłówku `Host`.
* `BuildURLWithPrefix` robi normalizację IDNA.

### 0.2 Przepisanie hosta na wersjonowany subdomain (tylko forwarder core)

`comp/forwarder/defaultforwarder/impl/default_forwarder.go:336` woła `utils.AddAgentVersionToDomain(domain, "app")`, które **podmienia pierwszą etykietę hosta** na `<major>-<minor>-<patch>-app.agent`:

```
https://app.datadoghq.com  →  https://7-72-0-app.agent.datadoghq.com
```

ALE tylko gdy host pasuje do `ddURLRegexp = ^app(\.mrf)?\.<site>\.?$` (endpoints.go:381). **Własny `dd_url` NIE pasuje i zostaje nietknięty.** Czyli: odbiornik pod custom `dd_url` widzi swój goły host; odbiornik udający Datadoga musi obsłużyć obie pisownie.

Ten sam mechanizm z sufiksem `"flare"` daje `<maj>-<min>-<patch>-flare.agent.<site>` dla flare'ów.

### 0.3 Event Platform (EvP) — ścieżka jest składana, nie literalna

`comp/logs-library/client/http/destination.go:505`:

```go
url.Path = fmt.Sprintf("%s/api/v2/%s", endpoint.PathPrefix, endpoint.TrackType)
```

* `TrackType` to `intakeTrackType` z deskryptora pipeline'u (`comp/forwarder/eventplatform/impl/pipelines_*.go`).
* `PathPrefix` jest pusty, chyba że użytkownik wpisał ścieżkę w `<prefix>.logs_dd_url`.
* Gdy `<prefix>.use_v2_api = false` (domyślnie `true` wszędzie) ścieżka degraduje się do `PathPrefix + "/v1/input"`.

Każdy track ma własny zestaw kluczy konfiguracyjnych z prefiksem `<endpointsConfigPrefix>`: `.dd_url`, `.logs_dd_url`, `.additional_endpoints`, `.use_compression`, `.compression_kind`, `.zstd_compression_level`, `.batch_*`, `.logs_no_ssl`, `.use_v2_api`.

### 0.4 Strategia batchowania w EvP — decyduje Content-Type

`comp/forwarder/eventplatform/impl/epforwarder.go:391`:

* `contentType == application/x-protobuf` (albo `useStreamStrategy: true`) → **StreamStrategy**: dokładnie **jeden komunikat na request**, ciało = `compress(bytes)`. Klucze `batch_max_size` / `batch_max_content_size` są wtedy martwe.
* `application/json` → **BatchStrategy** + `arraySerializer` (`comp/logs-library/sender/serializer.go`): ciało to `[` + surowe bajty komunikatów rozdzielone `,` + `]`, całość kompresowana jako jeden strumień.

### 0.5 Kompresja

| kind | Content-Encoding | uwagi |
|---|---|---|
| `zstd` | `zstd` | domyślny wszędzie (`constants.DefaultCompressorKind`, `DefaultLogCompressionKind`) |
| `zlib` | **`deflate`** | wymusza fallback z v3 na v2 dla metryk (`zlibForcesV2`) |
| `gzip` | `gzip` | |
| `none` / noop | brak nagłówka **lub** `identity` | `impl-noop/no_strategy.go:39` zwraca literał `"identity"` → nagłówek JEST wysyłany na ścieżce logów |

**Pułapka:** `comp/logs/agent/config/config_keys.go:124-142` — jeśli `<prefix>.additional_endpoints` jest niepuste, a `compression_kind` nie został jawnie ustawiony, agent **po cichu przełącza się na gzip**. Odbiornik musi obsłużyć zstd, gzip i identity dla każdego tracku EvP.

### 0.6 Multi-Region Failover (MRF) — nie jest endpointem, jest regułą hosta

`pkg/config/utils/endpoints.go:344` `GetMRFLogsEndpoint`:

```
jeśli prefix kończy się na ".logs."  → prefix + "mrf."
w przeciwnym razie                   → prefix + "logs.mrf."
```

Czyli:
* infra: `app.` → `app.mrf.` (`GetMRFInfraEndpoint`)
* czyste logi: `agent-http-intake.logs.` → `agent-http-intake.logs.mrf.`
* **wszystkie tracki EvP**: `dbm-metrics-intake.` → `dbm-metrics-intake.logs.mrf.`, `sbom-intake.` → `sbom-intake.logs.mrf.`, `ndm-intake.` → `ndm-intake.logs.mrf.` itd.
* APM ma własną regułę: `trace.agent.` + `mrf.` = `trace.agent.mrf.` (`comp/trace/config/impl/setup.go:51,53`)
* Remote Config: `config.mrf.<multi_region_failover.site>` — **osobna instancja serwisu RC** z własną bazą `remote-config-ha.db`, własnymi rootami TUF i kluczem `multi_region_failover.api_key`.

Ścieżki, ciała, nagłówki są identyczne jak w primary. To **dual-ship**, nie redirect. Gating: `multi_region_failover.enabled` + per-payload flaga (`IsMRFAllow` dla logów, `multi_region_failover.failover_metrics` / `.failover_logs` / `.failover_apm`, plus `multi_region_failover.metric_allowlist`). Klucz przekierowania: `multi_region_failover.dd_url` / `.site`.

### 0.7 FIPS proxy

`pkg/config/setup/config.go:546-665` (`setupFipsEndpoints` / `setupFipsLogsConfig`) przy `fips.enabled` **nadpisuje** `*_dd_url` / `logs_dd_url` na `http(s)://<fips.local_address>:<fips.port_range_start + N>` dla ~16 kanałów. To nie jest endpoint — to globalne przekierowanie, które unieważnia wszystkie hosty z tabel poniżej.

### 0.8 Nagłówki wspólne

**Forwarder core** (`transaction.go`, `resolver/domain_resolver.go:460`):
`DD-Api-Key`, `DD-Agent-Version`, `User-Agent: datadog-agent/<ver>`, `X-DD-Agent-Attempts` (licznik retry), opcjonalnie `DD-Allow-Arbitrary-Tags`.
Dla resolvera lokalnego (cluster-agent): `Authorization: Bearer <cluster_agent.auth_token>` zamiast `DD-Api-Key`.

**EvP / logs** (`destination.go:354-380`):
`DD-API-KEY`, `Content-Type`, `User-Agent: datadog-agent/<ver>`, `Content-Encoding` (gdy != ""), `DD-EVP-ORIGIN: agent` + `DD-EVP-ORIGIN-VERSION: <ver>`, `dd-message-timestamp` (ms), `dd-current-timestamp` (ms). `DD-PROTOCOL` **tylko** gdy pipeline podał niepusty protokół (czyste logi: `agent-json`; compliance: `agent-json`; reszta tracków: brak).

**Process-agent** (`pkg/process/runner/submitter.go:365-382`):
`Content-Type: application/x-protobuf`, `X-DD-Agent-Timestamp`, `X-Dd-Hostname`, `X-Dd-Processagentversion`, `X-Dd-ContainerCount`, `X-DD-Agent-Start-Time`, `X-DD-Payload-Source`, `X-DD-Processes-Enabled`, `X-DD-Service-Discovery-Enabled`, oraz `X-DD-Request-ID` **tylko dla checków `process` i `connections`**.

---

## 1. OUTBOUND — tabela zbiorcza (agent → Datadog)

`konfig` = klucz, który przekierowuje. `import` = czy typ payloadu da się zaimportować z `agent-payload/v5` lub `datadog-agent/pkg/proto`.

### 1.1 Metryki, service checki, eventy, metadata (forwarder core)

| host prefix | ścieżka | metoda | content-type | typ Go | import | konfig |
|---|---|---|---|---|---|---|
| `app.` | `/api/v2/series` | POST | `application/x-protobuf` | `gogen.MetricPayload` | tak — `github.com/DataDog/agent-payload/v5/gogen` | `dd_url` |
| `app.` | `/api/intake/metrics/v3/series` | POST | `application/x-protobuf` | `intake_v3.Payload` | tak — `agent-payload/v5/metrics/intake_v3` | `dd_url` |
| `app.` | `/api/v1/series` | POST | `application/json` | `[]*metrics.Serie` (ręczny JSON) | **nie** | `dd_url` |
| `app.` | `/api/beta/sketches` | POST | `application/x-protobuf` | `gogen.SketchPayload` | tak — `agent-payload/v5/gogen` | `dd_url` |
| `app.` | `/api/intake/metrics/v3/sketches` | POST | `application/x-protobuf` | `intake_v3.Payload` | tak | `dd_url` |
| `app.` | `/api/intake/metrics/v3beta/sketches` | POST | `application/x-protobuf` | `intake_v3.Payload` | tak | `serializer_experimental_use_v3_api.sketches.beta_route` (ścieżka!) + `dd_url` |
| `app.` | `/api/v1/check_run` | POST | `application/json` | `servicecheck.ServiceChecks` | **nie** | `dd_url` |
| `app.` | `/intake/` | POST | `application/json` | multipleks (4 kształty) | **nie** | `dd_url` |
| `app.` | `/api/v1/metadata` | POST | `application/json` | `*<component>impl.Payload` (11 wariantów) | **nie** | `dd_url` |
| `<ver>-flare.agent.` | `/support/flare[/{case_id}]` | HEAD→POST | `multipart/form-data` | brak (ręczny multipart) | **nie** | `dd_url` |
| `api.` | `/api/v1/validate` | GET | — | brak ciała | n/d | `dd_url` |
| `api.` | `/api/v2/intake-key` | POST | `application/json` | brak ciała (puste) | **nie** (odpowiedź) | `dd_url` |
| `api.` | `/api/v1/query` | GET | — | brak ciała | **nie** (`zorkian/go-datadog-api`) | `external_metrics_provider.endpoint` |

### 1.2 Process / Orchestrator

| host prefix | ścieżka | metoda | content-type | typ Go | import | konfig |
|---|---|---|---|---|---|---|
| `process.` | `/api/v1/collector` | POST | `application/x-protobuf` | `*process.CollectorProc` (typ 12) **lub** `*process.CollectorRealTime` (typ 27) | tak — `agent-payload/v5/process` | `process_config.process_dd_url` |
| `process.` | `/api/v1/container` | POST | `application/x-protobuf` | `*process.CollectorContainer` (39) / `*process.CollectorContainerRealTime` (40) | tak | `process_config.process_dd_url` |
| `process.` | `/api/v1/connections` | POST | `application/x-protobuf` | `*process.CollectorConnections` (22) | tak | `process_config.process_dd_url` |
| `process.` | `/api/v1/discovery` | POST | `application/x-protobuf` | `*process.CollectorProcDiscovery` (53) | tak | `process_config.process_dd_url` |
| `orchestrator.` | `/api/v2/orch` | POST | `application/x-protobuf` | 26× `*process.Collector<Kind>` | tak | `orchestrator_explorer.orchestrator_dd_url` |
| `orchestrator.` | `/api/v1/orchestrator` | POST | `application/x-protobuf` | j.w. | tak | j.w. (+ `orchestrator_explorer.use_legacy_endpoint`) |
| `orchestrator.` | `/api/v2/orchmanif` | POST | `application/x-protobuf` | `*process.CollectorManifest{,CRD,CR}` (80/81/82) | tak | j.w. |

### 1.3 APM / trace-agent

| host prefix | ścieżka | metoda | content-type | typ Go | import | konfig |
|---|---|---|---|---|---|---|
| `trace.agent.` | `/api/v0.2/traces` | POST | `application/x-protobuf` | `pb.AgentPayload` | tak — `datadog-agent/pkg/proto/pbgo/trace` | `apm_config.apm_dd_url` |
| `trace.agent.` | `/api/v0.2/stats` | POST | `application/msgpack` | `*pb.StatsPayload` (msgp, **nie** protobuf) | tak — j.w. | `apm_config.apm_dd_url` |
| `trace.agent.` | `/api/v0.1/pipeline_stats` | POST | passthrough | opaque (DSM z tracera) | **nie** | `apm_config.apm_dd_url` |
| `trace.agent.` | `/_health` | GET | `application/json` | brak | n/d | `apm_config.apm_dd_url` (diagnostyka) |
| `intake.profile.` | `/api/v2/profile` | POST | passthrough (multipart) | opaque | **nie** | `apm_config.profiling_dd_url` |
| `intake.profile.` | `/v1/input` | POST | (dd-trace-go) | opaque pprof | **nie** | `internal_profiling.profile_dd_url` |
| `instrumentation-telemetry-intake.` | `/api/v2/apmtelemetry` | POST | `application/json` | 4 producentów, patrz §4.6 | **nie** | `apm_config.telemetry.dd_url` / `agent_telemetry.dd_url` |
| `http-intake.logs.` | `/api/v2/logs` | POST | passthrough | opaque (DI snapshots) | **nie** | `apm_config.debugger_dd_url` |
| `debugger-intake.` | `/api/v2/debugger` | POST | multipart **lub** `application/json` | `[]uploader.DiagnosticMessage` / `[]json.RawMessage` / symdb multipart | **nie** | `apm_config.debugger_diagnostics_dd_url`, `apm_config.symdb_dd_url` |
| `data-obs-intake.` | `/api/v1/lineage?api-version=2` | POST | passthrough | OpenLineage RunEvent JSON | **nie** | `ol_proxy_config.dd_url` |
| `<subdomena z nagłówka>` | dowolna (evp_proxy) | dowolna | passthrough | opaque | **nie** | `evp_proxy_config.dd_url` |
| `<ver>-flare.` | `/api/ui/support/serverless/flare` | POST | `multipart/form-data` | opaque | **nie** | brak (tylko `site`) |

### 1.4 Logi

| host prefix | ścieżka | metoda | content-type | typ Go | import | konfig |
|---|---|---|---|---|---|---|
| `agent-http-intake.logs.` | `/api/v2/logs` | POST | `application/json` | `[]processor.jsonPayload` (nieeksportowany) | **nie** | `logs_config.logs_dd_url` |
| `agent-http-intake.logs.` | `/v1/input` | POST | `application/json` | j.w. | **nie** | j.w. (`use_v2_api:false`) |
| `agent-intake.logs.` **:10516/:443** | — (raw TLS TCP) | — | — | `pb.Log` (tylko `dev_mode_use_proto`) / tekst RFC5424 | tak dla proto — `agent-payload/v5/pb` | `logs_config.logs_dd_url` |
| `http-intake.logs.` | `/api/v2/logs` | POST | `application/json` | `jsonServerlessInitPayload` | **nie** | `logs_config.logs_dd_url` (serverless) |
| `runtime-security-http-intake.logs.` | `/api/v2/secruntime` | POST | `application/json` | koperta logów + easyjson CWS | **nie** | `runtime_security_config.endpoints.logs_dd_url` |
| `runtime-security-http-intake.logs.` | `/api/v2/secinfo` | POST | `application/json` | j.w. (track remediacji) | **nie** | j.w. |
| `cspm-intake.` | `/api/v2/compliance` | POST | `application/json` | koperta + `*compliance.CheckEvent` / `ResourceLog` | **nie** | `compliance_config.endpoints.logs_dd_url` |

### 1.5 Event Platform — pozostałe tracki

Wszystkie: POST, ścieżka `= /api/v2/<track>`, `DD-EVP-ORIGIN: agent`, kompresja zstd domyślnie.

| host prefix | ścieżka | content-type | typ Go | import | konfig (`<prefix>.logs_dd_url`) |
|---|---|---|---|---|---|
| `agentdiscovery-intake.` | `/api/v2/agentdiscovery` | x-protobuf | `*agentdiscovery.AgentDiscoveryPayloadBatch` | tak | `config_files_discovery.forwarder.` |
| `contlcycle-intake.` | `/api/v2/contlcycle` | x-protobuf | `*contlcycle.EventsPayload` | tak | `container_lifecycle.` |
| `contimage-intake.` | `/api/v2/contimage` | x-protobuf | `*contimage.ContainerImagePayload` | tak | `container_image.` |
| `sbom-intake.` | `/api/v2/sbom` | x-protobuf | `*sbom.SBOMPayload` | tak | `sbom.` |
| `sds-intake.` | `/api/v2/sdsresult` | x-protobuf | `*sds.SdsResultPayload` | **NIE — patrz sprostowanie niżej** | `sds_result.forwarder.` |
| `resources-intake.` | `/api/v2/genresources` | x-protobuf | opaque (`[]byte` z integracji) | **nie** | `genresources.` |
| `kubeops-intake.` | `/api/v2/kubeactions` | json | `[]ActionResultEvent` | **nie** | `kubeactions.forwarder.` |
| `ndm-intake.` | `/api/v2/ndm` | json | `[]metadata.NetworkDevicesMetadata` | **nie** | `network_devices.metadata.` |
| `ndm-intake.` | `/api/v2/ndmconfig` | json | `[]report.NCMPayload` | **nie** | `network_devices.config_management.forwarder.` |
| `snmp-traps-intake.` | `/api/v2/ndmtraps` | json | `[]map[string]any` (`{"trap":{…}}`) | **nie** | `network_devices.snmp_traps.forwarder.` |
| `ndmflow-intake.` | `/api/v2/ndmflow` | json | `[]payload.FlowPayload` | tak\* — `comp/netflow/payload` (własny go.mod) | `network_devices.netflow.forwarder.` |
| `netpath-intake.` | `/api/v2/netpath` | json | `[]payload.NetworkPath` | tak\* — `pkg/networkpath/payload` (własny go.mod) | `network_path.forwarder.` |
| `dbm-metrics-intake.` | `/api/v2/databasequery` | json | `[]json.RawMessage` | **nie** | `database_monitoring.samples.` |
| `dbm-metrics-intake.` | `/api/v2/dbmmetrics` | json | `[]json.RawMessage` | **nie** | `database_monitoring.metrics.` |
| `dbm-metrics-intake.` | `/api/v2/dbmmetadata` | json | `[]json.RawMessage` | **nie** | `database_monitoring.metrics.` |
| `dbm-metrics-intake.` | `/api/v2/dbmhealth` | json | `[]json.RawMessage` | **nie** | `database_monitoring.metrics.` |
| `dbm-metrics-intake.` | `/api/v2/dbmcolumnstatistics` | json | `[]json.RawMessage` | **nie** | `database_monitoring.metrics.` |
| `dbm-metrics-intake.` | `/api/v2/dbmactivity` | json | `[]json.RawMessage` | **nie** | `database_monitoring.activity.` |
| `data-obs-intake.` | `/api/v2/query-actions` | json | `[]json.RawMessage` | **nie** | `data_observability.forwarder.` |
| `trace.agent.` | `/api/v2/data_streams_messages` | json | `[]json.RawMessage` | **nie** | `data_streams.forwarder.` |
| `event-management-intake.` | `/api/v2/events` | json | `map[string]any` (JSON:API-like) | **nie** | `event_management.forwarder.` |
| `softinv-intake.` | `/api/v2/softinv` | json | `[]softwareinventoryimpl.Payload` | **nie** | `software_inventory.forwarder.` |
| `http-synthetics.` | `/api/v2/synthetics` | json | `[]*common.TestResult` | **nie** | `synthetics.forwarder.` |

> **Sprostowanie (2026-09-20, weryfikacja przy pisaniu `router_security.go`):**
> `sds.SdsResultPayload` **nie jest importowalny**. Pakiet `pkg/proto/pbgo/sds`
> istnieje wyłącznie na gałęzi `main` klonu datadog-agenta. Żadna opublikowana
> wersja modułu `pkg/proto` go nie zawiera — sprawdzone przez `go list -m -versions`
> i przez zawartość `pkg/proto@v0.83.2/pbgo/`, gdzie są tylko: `core`,
> `dogstatsdhttp`, `languagedetection`, `mocks`, `privateactionrunner`, `process`,
> `procmgr`, `sbom`, `trace`. Do czasu publikacji handler czyta ten payload przez
> `protowire` i loguje układ pól.

\* — typy w osobnych modułach Go **poza** `agent-payload` i `pkg/proto`; importowalne, ale nie z modułów wskazanych w zadaniu.

### 1.6 Security / CWS / SBOM / health

| host prefix | ścieżka | metoda | content-type | typ Go | import | konfig |
|---|---|---|---|---|---|---|
| `cws-intake.` | `/api/v2/secdump` | POST | `multipart/form-data` + `Content-Encoding: gzip` | `*dumpsv1.SecDump` | tak — `agent-payload/v5/cws/dumpsv1` | `runtime_security_config.activity_dump.remote_storage.endpoints.logs_dd_url` |
| `agenthealth-intake.` | `/api/v2/agenthealth` | POST | `application/json` | `*healthplatform.HealthReport` | tak — `agent-payload/v5/healthplatform` | `dd_url` |
| `sourcemap-intake.` | `/api/v2/srcmap` | POST | `multipart/form-data` | brak (ELF + JSON event) | **nie** | `profiling::symbol_uploader::symbol_endpoints[].site` |
| `api.` | `/api/v2/profiles/symbols/query` | POST | `application/json` (JSON:API) | `*SymbolsQueryRequest` | **nie** | j.w. |
| `otlp.` | `/v1development/profiles` | POST | `application/x-protobuf` | OTLP `ExportProfilesServiceRequest` | tak — `go.opentelemetry.io/collector/pdata/pprofile` | `apm_config.profiling_dd_url` (site) |
| `otlp.` | `/v1/metrics` | POST | `application/x-protobuf` | OTLP `ExportMetricsServiceRequest` | tak — `.../pdata/pmetric/pmetricotlp` | j.w. |

### 1.7 Private Action Runner (PAR)

Host zawsze `api.<site>`. **Uwaga:** `dd_url` NIE przekierowuje — `ExtractSiteFromURL` dopasowuje tylko znane domeny Datadoga, więc custom host daje pusty site i URL `https://api./…`. Jedyne realne przekierowanie: env `DD_INTERNAL_PAR_USE_DD_URL_FOR_OPMS=true` (opisane w kodzie jako *internal-only, test*).

| ścieżka | metoda | content-type | typ Go | import | enable |
|---|---|---|---|---|---|
| `/api/unstable/on_prem_runners` | POST | `application/vnd.api+json` | `par.CreateRunnerRequest` | **nie** | `private_action_runner.enabled` + `api_key_only_enrollment:false` |
| `/api/unstable/on_prem_runners/api_key_only` | POST | `application/vnd.api+json` | `par.CreateRunnerRequest` | **nie** | `api_key_only_enrollment:true` (**domyślne**) |
| `/api/v2/on-prem-management-service/workflow-tasks/dequeue` | POST | `application/json` | `DequeueJSONRequest` (JSON:API) | **nie** | `private_action_runner.enabled` |
| `/api/v2/on-prem-management-service/workflow-tasks/publish-task-update` | POST | `application/json` | `PublishTaskUpdateJSONRequest` | **nie** | j.w. |
| `/api/v2/on-prem-management-service/workflow-tasks/heartbeat` | POST | `application/json` | `HeartbeatJSONRequest` | **nie** | j.w. |
| `/api/v2/on-prem-management-service/runner/health-check` | GET | `application/json` (bez ciała) | — | n/d | j.w. |
| `/api/v2/actions/connections` | POST | `application/vnd.api+json` | `autoconnections.ConnectionRequest` | **nie** | j.w. |

### 1.8 Hosty spoza schematu `prefix + site`

| host (literał) | ścieżka | metoda | uwagi |
|---|---|---|---|
| `install.datadoghq.com` / `install.datad0g.com` | `/v2/`, `/v2/{repo}/manifests/{tag}`, `/v2/{repo}/blobs/{digest}` | GET | OCI Distribution, fleet installer. Klucz: `installer.registry.url`, mirror: `installer.mirror` |
| `install.datadoghq.com` | `/btfs/{platform}/{ver}/{arch}/{kernel}.btf.tar.xz` | GET | **hardcoded**, brak klucza konfiguracyjnego. `system_probe_config.remote_config_btf_enabled` |
| `install.datadoghq.com` | `/` | HEAD | diagnostyka, klucz `installer.registry.url` |
| `yum.datadoghq.com`, `apt.datadoghq.com`, `keys.datadoghq.com` | `/` | HEAD | diagnostyka, **brak** klucza przekierowania |
| `eudm-intake.<site>` | `/api/v2/aiusage` | POST | binarka Rust `cmd/ai_prompt_logger`, przez lokalny `evp_proxy` |
| `llmobs-intake.<site>` | `/api/v2/llmobs` | POST | **tylko sonda diagnostyczna**, ciało `nil` |
| `<registry>` (gcr.io/datadoghq, docker.io/datadog, public.ecr.aws/datadog, …) | `/v2/{repo}/manifests/{tag}` | HEAD | cluster-agent, `admission_controller.auto_instrumentation.container_registry` |

---

## 2. INBOUND — tabela zbiorcza (Datadog → agent)

„Inbound" = dane płyną w dół. Transport zwykle nadal inicjuje agent.

| host prefix | ścieżka / metoda | protokół | typ odpowiedzi | import | konfig / enable |
|---|---|---|---|---|---|
| `config.` | `POST /api/v0.1/configurations` | HTTP, `application/x-protobuf` | `*core.LatestConfigsResponse` | tak — `pkg/proto/pbgo/core` | `remote_configuration.rc_dd_url` / `remote_configuration.enabled` |
| `config.` | `GET /api/v0.1/org` | HTTP, protobuf | `*core.OrgDataResponse` | tak | j.w. |
| `config.` | `GET /api/v0.1/status` | HTTP, protobuf | `*core.OrgStatusResponse` | tak | j.w. |
| `config.mrf.` | j.w. ×3 | j.w. | j.w. | tak | `multi_region_failover.remote_configuration.rc_dd_url` |
| `config.` | `GET /api/v0.2/ping-pong` | HTTP | — (tylko 200) | n/d | `remote_configuration.no_websocket_echo:false` |
| `config.` | `GET /api/v0.2/echo-test` | **WebSocket** | ramki opaque | n/d | j.w. |
| `config.` | `GET /api/v0.2/echo-test-alpn` | **WS + ALPN `dd-rc-v1`** | ramki opaque | n/d | j.w. + TLS |
| `config.` | `/datadog.config.RcEcho/RunEchoTest` | **gRPC bidi stream** | `core.RunEchoTestResponse` | tak | j.w. |
| `config.` **:8042** | — | **raw TLS TCP, 2B LE length prefix** | bajty | n/d | j.w. |
| `config.` | `GET /_health` | HTTP | — | n/d | diagnostyka |
| `intake.synthetics.` | `GET /api/unstable/synthetics/agents/tests` | HTTP, JSON | `{tests:[SyntheticsTestConfig]}` | **nie** | `synthetics.collector.enabled` |
| `api.` | `POST /api/v2/…/workflow-tasks/dequeue` | HTTP, JSON:API | `*types.Task` + zagnieżdżony protobuf | częściowo | `private_action_runner.enabled` |
| `install.datadoghq.com` | OCI manifests/blobs | HTTP | manifesty OCI, warstwy tar+zstd | **nie** | `installer.registry.url` |
| `install.datadoghq.com` | `/btfs/…` | HTTP | tar.xz + SHA256 z katalogu RC | n/d | `remote_config_btf_enabled` |

### 2.1 Produkty Remote Config

Jedno POST `/api/v0.1/configurations` przenosi **wszystkie** produkty; nazwa produktu to segment w ścieżce pliku TUF `datadog/<org_id>/<PRODUCT>/<config_id>/<file>` (regexp `pkg/remoteconfig/state/path.go:18`). To **nie są osobne endpointy**. Zweryfikowane produkty i typy zawartości:

| produkt | typ zawartości `File.raw` | pakiet |
|---|---|---|
| `K8S_ACTIONS` | `kubeactions.KubeActionsList` (**protojson**, `DiscardUnknown:true`) | `agent-payload/v5/kubeactions` |
| `CONTAINER_AUTOSCALING_VALUES` | `kubernetes.WorkloadValuesList` (**encoding/json**, nie protojson!) | `agent-payload/v5/autoscaling/kubernetes` |
| `CONTAINER_AUTOSCALING_SETTINGS` | `model.AutoscalingSettingsList` | wewnętrzny, wymaga redeklaracji |
| `CLUSTER_AUTOSCALING_VALUES` | `model.ClusterAutoscalingValuesList` | wewnętrzny (agent-payload ma **przestarzały** odpowiednik) |
| `DO_QUERY_ACTIONS` | JSON (schema w integrations-core) | brak |
| `DSM_KAFKA_ACTIONS` | JSON | brak |
| `DATA_SECURITY_DB_SCAN_TASKS` | JSON | brak |
| `BTF_DD` | JSON: `{arch:{distro:{release:{kernel:{sha256}}}}}` | brak |
| `LIVE_DEBUGGING`, `LIVE_DEBUGGING_SYMBOLDB` | opaque (dla subskrypcji dyninst) | brak |
| `AP_RUNNER_KEYS` | `types.RawKey` JSON | brak |
| `INSTALLER_CONFIG`, `UPDATER_CATALOG_DD`, `UPDATER_TASK` | JSON (fleet) | częściowo — `pkg/fleet/catalog` |
| `AGENT_CONFIG`, `AGENT_TASK`, `APM_SAMPLING`, … | — | — |

**Krytyczne dla reimplementacji:** odpowiedź musi przejść pełną weryfikację TUF (`pkg/config/remote/uptane/client.go`) — podpisane `config_metas` i `director_metas`, zgodne długości i hashe SHA256 target files, `OrgUUID` zgodny z tym z `/api/v0.1/org`. Pusty obiekt **nie zadziała**. Klucze rootów nadpisywalne: `remote_configuration.config_root`, `remote_configuration.director_root`, `remote_configuration.key`. Interwał odpytywania: domyślnie 60 s, ale **backend może go nadpisać** przez `agent_refresh_interval` w `targets.custom` (`pkg/config/remote/service/util.go:111`), o ile użytkownik nie ustawił `remote_configuration.refresh_interval` jawnie. Pole `backend_client_state` musi być odsyłane z powrotem w kolejnym żądaniu.

---

## 3. LOCAL — serwery, które Agent wystawia

Nic z tej sekcji nie dociera do backendu Datadoga. Odbiornik zastępujący Datadoga tego **nie potrzebuje**; potrzebuje tego reimplementacja Agenta.

### 3.1 trace-agent receiver (domyślnie `127.0.0.1:8126`, `apm_config.receiver_port`; też UDS i named pipe)

| ścieżka | metoda | content-type | typ Go | import |
|---|---|---|---|---|
| `/spans`, `/v0.1/spans` | POST | JSON (msgpack → 415) | `[]*pb.Span` | tak — `pkg/proto/pbgo/trace` |
| `/v0.2/traces` | POST | JSON | `pb.Traces` | tak |
| `/v0.3/traces` | POST | msgpack/JSON | `pb.Traces` | tak |
| `/v0.4/traces` | POST/PUT | msgpack/JSON | `pb.Traces` | tak |
| `/v0.5/traces` | POST | msgpack (słownikowy) | `pb.Traces` przez `UnmarshalMsgDictionary` | tak |
| `/v0.7/traces` | POST | msgpack | `pb.TracerPayload` (msgp!) | tak |
| `/v1.0/traces` | POST | msgpack | `idx.InternalTracerPayload` | tak — `pkg/proto/pbgo/trace/idx` |
| `/v0.6/stats` | POST | msgpack | `*pb.ClientStatsPayload` | tak |
| `/services`, `/v0.1…/v0.4/services` | dowolna | dowolna | **no-op**, zwraca `"OK\n"` | n/d |
| `/info` | GET | (sniffed text/plain) | `api.infoPayload` | **nie** |
| `/v0.7/config` | POST | JSON | `core.ClientGetConfigsRequest/Response` | tak |
| `/profiling/v1/input` | POST | passthrough | opaque | **nie** |
| `/telemetry/proxy/` | POST | passthrough | opaque | **nie** |
| `/v0.1/pipeline_stats` | POST | passthrough | opaque | **nie** |
| `/openlineage/api/v1/lineage` | POST | passthrough | opaque | **nie** |
| `/evp_proxy/v1/` … `/v4/` | dowolna | passthrough | opaque | **nie** |
| `/debugger/v1/input`, `/debugger/v1/diagnostics`, `/debugger/v2/input` | POST | passthrough | opaque | **nie** |
| `/symdb/v1/input` | POST | passthrough | opaque | **nie** |
| `/dogstatsd/v1/proxy`, `/dogstatsd/v2/proxy` | POST | tekst DogStatsD | brak | n/d |
| `/tracer_flare/v1` | POST | multipart passthrough | opaque | **nie** |

**Odpowiedzi:** v0.1–v0.3 i wszystkie `/services` → literał `"OK\n"`. v0.4/v0.5/v0.7/v1.0 → `application/json` `{"rate_by_service":{…}}` (albo literał `{}` gdy `Datadog-Rates-Payload-Version` się zgadza). `/v0.6/stats` → 200 z **pustym** ciałem. Każda odpowiedź niesie `Datadog-Agent-Version` i `Datadog-Agent-State` (SHA-256 ciała `/info`) — **oprócz samego `/info`**.

### 3.2 core agent CMD API (`cmd_host`:`cmd_port`, domyślnie `127.0.0.1:5001`, HTTPS + IPC auth token)

Wszystko montowane pod prefiksem `/agent` (`comp/api/api/apiimpl/server_cmd.go:44`). To jest najczęstszy błąd w spisach: trasy rejestruje się jako `/status`, a na drucie są jako `/agent/status`.

| ścieżka | metoda | typ / uwagi |
|---|---|---|
| `/agent/status`, `/agent/status/section/{c}`, `/agent/status/sections` | GET | text/plain lub JSON (`?format=`) |
| `/agent/config`, `/config/without-defaults`, `/config/list-runtime`, `/config/{setting}` (GET/POST) | GET/POST | YAML / JSON; POST = `application/x-www-form-urlencoded`, pole `value` |
| `/agent/flare` | POST | `flarehelpers.ProfileData` |
| `/agent/diagnose` | POST | `diagnose.Config` → `diagnose.Result` (**bez tagów json**, PascalCase!) |
| `/agent/tagger-list` | GET | `types.TaggerListResponse` |
| `/agent/workload-list` | GET | `workloadmeta.WorkloadDumpResponse` |
| `/agent/secrets`, `/agent/secret/refresh` | GET | text/template |
| `/agent/config-check` | GET | AD config check |
| `/agent/metadata/v5`, `/gohai`, `/inventory-agent`, `/inventory-host`, `/inventory-checks`, `/host-gpu`, `/host-system-info`, `/ha-agent`, `/security-agent`, `/system-probe`, `/package-signing`, `/software`, `/agent-telemetry` | GET | po jednym `*Payload` na komponent, **żaden nie jest importowalny** |
| `/agent/health-platform/issues` | GET | `{count, issues:map[string]*healthplatform.Issue}` |
| `/agent/stream-event-platform` | POST | chunked text stream, `diagnostic.Filters` |
| `/agent/dogstatsd-stats` | GET | `map[ckey.ContextKey]metricStat` |
| `/agent/dogstatsd-contexts-dump` | POST | ścieżka do pliku zstd |
| `/agent/ncm/config`, `/agent/ncm/rollback` | GET/POST | NCM |
| `/agent/networkdevices/connectivity-check` | POST | `connectivity.Request` |
| `/agent/gui/intent` | GET | token base64 (TTL 30 s) |
| `/agent/version`, `/agent/hostname`, `/agent/stop` | GET/POST | |
| `/agent/coverage` | GET | **tylko build tag `e2ecoverage`** |

### 3.3 IPC API (osobny listener, `agent_ipc.port` / `agent_ipc.socket_path`, mTLS z `RequireAndVerifyClientCert`)

| ścieżka | metoda | typ |
|---|---|---|
| `/config/v1/` | GET | `map[string]interface{}` (tylko klucze z `api.AuthorizedConfigPathsCore`) |
| `/config/v1/<key>` | GET | goła wartość JSON |

Pułapka: `logs_config.additional_endpoints` jest stringifikowane (`[]map[string]string`), a klient parsuje `is_reliable` z powrotem przez `strconv.ParseBool`.

### 3.4 system-probe (UDS `system_probe_config.sysprobe_socket` / named pipe; host w URL to atrapa `sysprobe`)

Router montuje każdy moduł pod `/<module_name>/` (`pkg/system-probe/api/module/router.go:26`).

| moduł | ścieżki |
|---|---|
| `network_tracer` | `/connections`, `/register`, `/network_id`, `/debug/net_maps`, `/debug/net_state`, `/debug/ebpf_maps`, `/debug/conntrack/cached`, `/debug/conntrack/host`, `/debug/process_cache`, `/debug/{kafka,postgres,redis,http2,http}_monitoring`, `/debug/usm_telemetry`, `/debug/usm/{traced_programs,blocked_processes,clear_blocked,attach-pid,detach-pid}`, (Windows) `/iis_tags`, `/process_cache_tags` |
| `process` | `/stats` (POST, protobuf lub JSON wg `Accept`), `/service` i `/network` = **stuby TODO** |
| `discovery` | `/services`, `/status`, `/state` |
| `gpu` | `/check`, `/driver-events`, `/prm-metrics`, `/nvml-release`, `/debug/*` |
| `ebpf` | `/check` |
| `oom_kill_probe` | `/check` |
| `tcp_queue_length_tracer` | `/check` |
| `software_inventory` | `/check` (Windows/darwin) |
| `notable_events` | `GET /check`, `POST /ack` (darwin) |
| `logon_duration` | `/check` (darwin) |
| `noisy_neighbor` | `/check` |
| `compliance` | `/dbconfig?pid=` |
| `language_detection` | `/detect` — **GET z ciałem protobuf** |
| `windows_crash_detection` | `/check` |
| `dynamic_instrumentation` | `/check`, `/debug/{goprocs,stats,state,diagnostics,config,symdb}` |
| `privileged_logs` | `POST /open` |
| `traceroute` | `/traceroute/{host}` → pełna ścieżka `/traceroute/traceroute/{host}` |
| `ping` | `/ping/{host}` → pełna ścieżka `/ping/ping/{host}` |
| (root) | `/config`, `/config/without-defaults`, `/config/by-source`, `/config/list-runtime`, `/config/{setting}`, `/debug/stats`, `/debug/pprof/`, `/debug/vars`, `/telemetry`, `POST /agent-restart` (jedyna trasa z IPC auth), `/debug/ebpf_btf_loader_info`, `/debug/dmesg`, `/debug/selinux_sestatus`, `/debug/selinux_semodule_list` |

### 3.5 DogStatsD (ingest lokalny)

| transport | adres | framing |
|---|---|---|
| UDP | `bind_host`:`dogstatsd_port` (8125), lub `:8125` gdy `dogstatsd_non_local_traffic` | datagram, linie `\n` |
| UDS datagram | `dogstatsd_socket` (linux/aix: `/var/run/datadog/dsd.socket`) | j.w., chmod 0722 |
| UDS **stream** | `dogstatsd_stream_socket` | **4-bajtowy LE uint32 length prefix**, potem dokładnie tyle bajtów |
| named pipe | `\\.\pipe\<dogstatsd_pipe_name>` | linie `\n` |
| HTTP (eksperyment) | `dogstatsd_experimental_http.listen_address` (127.0.0.1:8125 **TCP**) | `POST /series`, `POST /sketches`, ciało = `dogstatsdhttp.Payload` (protobuf kolumnowy) |

### 3.6 cluster-agent (`0.0.0.0:cluster_agent.cmd_port`, domyślnie 5005, TLS≥1.3, token DCA)

Root mux: `GET /version`, `/hostname`, `POST /flare`, `POST /stop`, `/status*`, `/config*`, `/config-check`, `/autoscaler-list`, `/local-autoscaling-check`, `/tagger-list`, `/workload-list`, `/metadata/cluster-agent`, `/metadata/cluster-checks`.

`/api/v1/` (StripPrefix): `/annotations/node/{n}`, `/info/node/{n}`, `/tags/node/{n}`, `/uid/node/{n}`, `/tags/namespace/{ns}`, `/metadata/namespace/{ns}`, `/tags/pod`, `/tags/pod/{n}`, `/tags/pod/{n}/{ns}/{pod}`, `/cluster/id`, `/languagedetection`, `/clusterchecks*`, `/endpointschecks/configs[/{node}]`, `/instrumentation/{configs,status}`; wariant CloudFoundry: `/tags/cf/apps/{n}`, `/cf/apps[/{guid}]`, `/cf/orgs`, `/cf/org_quotas`.

`/api/v2/series` — POST, `gogen.MetricPayload`, autoscaling failover (node agent → DCA).

External Metrics Provider: osobny listener HTTPS `external_metrics_provider.port` (**domyślnie 8443**, nie 443) obsługujący `apis/external.metrics.k8s.io/v1beta1` — **nie ma `custom.metrics.k8s.io`**, mimo nazwy pakietu `custommetrics`.

### 3.7 gRPC — `datadog.api.v1.AgentSecure` (core agent, cmd_port, mTLS)

`ClientGetConfigs`, `ClientGetConfigsHA`, `GetConfigState`, `GetConfigStateHA`, `ResetConfigState`, `CreateConfigSubscription`, `StreamConfigEvents`, `TaggerStreamEntities`, `TaggerFetchEntity`, `TaggerGenerateContainerIDFromOriginInfo`, `WorkloadmetaStreamEntities`, `StreamKubeMetadata`, `AutodiscoveryStreamConfig`, `DogstatsdCaptureTrigger`, `DogstatsdSetTaggerState`, `GetHostTags`, `WorkloadFilterEvaluate`, `RegisterRemoteAgent` (deprecated), `RefreshRemoteAgent` (deprecated), `ReportHealthIssue`, `ResolveHealthIssue`.

Inne serwisy na **tym samym** listenerze: `datadog.api.v1.Agent/GetHostname`; `datadog.remoteagent.v1.RemoteAgent/{RegisterRemoteAgent,RefreshRemoteAgent,ReportRemoteAgentEvent}`; `datadog.remoteagent.command.v1.RemoteCommandProvider/{ListCommands,ExecuteCommand}` (proxy do sub-agentów).

**Kierunek odwrócony** — core agent jest *klientem*, sub-agent serwerem (transport: unix / `https://127.0.0.1:<losowy>` / `vsock://cid:port`, mTLS + bearer + nagłówek `session_id`):
`datadog.remoteagent.status.v1.StatusProvider/GetStatusDetails`, `…telemetry.v1.TelemetryProvider/GetTelemetry`, `…flare.v1.FlareProvider/GetFlareFiles` (klient ustawia `MaxCallRecvMsgSize(64 MiB)`).

Pozostałe gRPC: `datadog.sbom.SBOMCollector/GetSBOMStream` (CWS cmd socket → workloadmeta), `datadog.process.ProcessEntityStream/StreamEntities` (process-agent → core, **tylko Windows**, port `process_config.language_detection.grpc_port`, domyślnie 6262), `datadog.procmgr.ProcessManager/*` (8 metod, serwer w **Rust** `dd-procmgrd`, UDS, plaintext), `datadog.privateactionrunner.executor.Executor/{RunAction,Health}` (UDS/named pipe, mTLS).

### 3.8 Pozostałe lokalne

* CWS: `/api.SecurityModuleEvent/{GetEventStream,GetActivityDumpStream}` i `/api.SecurityAgentAPI/{SendEvent,SendActivityDumpStream}` — UDS, codec `vtproto` (`application/grpc+vtproto`), bez TLS. Który serwis jest zarejestrowany zależy od `runtime_security_config.event_grpc_server` (`""` / `security-agent` / `system-probe`). `SecurityModuleCmd` jest na **innym** sockecie: `runtime_security_config.cmd_socket` lub wyprowadzony jako `<dir>/cmd-<basename>`.
* health probe: `0.0.0.0:health_port` (**0 = wyłączone**, domyślnie wyłączone) — `/live`, `/ready`, `/startup` i catch-all `/` → `health.Status` (klucze **`Healthy`/`Unhealthy`**, PascalCase).
* GUI: `GUI_host:GUI_port` (darwin/windows 5002, linux -1) — `/`, `/auth?intent=`, `/view/`, `/agent/*`, `/checks/*`.
* expvar/pprof/telemetry: core `expvar_port` (5000), process-agent `process_config.expvar_port` (6062), system-probe `system_probe_config.debug_port`, cluster-agent (loopback only).
* installer daemon: UDS `/opt/datadog-packages/run/installer.sock` lub pipe `\\.\pipe\DD_INSTALLER` — `GET /status`, `POST /catalog`, `/config_catalog`, `/{pkg}/install|remove|experiment/*|config_experiment/*`, `/debug/pprof/*`. Middleware wymaga `Content-Type: application/json` **na każdym żądaniu, także GET i bez ciała** → inaczej 415.
* OTLP receiver: `otlp_config.receiver.protocols.{grpc,http}.endpoint` (**domyślnie `localhost:4317` / `localhost:4318`**, nie 0.0.0.0) + wewnętrzny gRPC trace-agenta na `otlp_config.traces.internal_port` (5003).
* serverless-init microVM lifecycle: `:9000/aws/lambda-microvms/runtime/v1/{ready,validate,run,resume,suspend,terminate}`.
* admission controller (cluster-agent, port 8000): webhooki wołane przez kube-apiserver — poza zakresem census (ruch K8s).

---

## 4. Sekcje per-host (outbound) — to, czego potrzebuje odbiornik

### 4.1 `app.<site>` → `<maj>-<min>-<patch>-app.agent.<site>`

**Klucz:** `dd_url` (+ `additional_endpoints` mapa `url → []api_key`, fan-out tego samego ciała; MRF: `multi_region_failover.dd_url`).
**Enable:** per-payload `enable_payloads.{series,sketches,service_checks,events,json_to_v1_intake}`, `inventories_enabled`, `enable_metadata_collection`.

Wybór trasy dla serii (`pkg/serializer/metrics.go`):

```
use_v2_api.series == false                    → /api/v1/series (JSON)
use_v3_api.series.enabled:
    true                                      → /api/intake/metrics/v3/series
    "datadog_only" (DOMYŚLNE)                 → v3 tylko gdy IsDatadogURL(GetConfigName())
    false                                     → /api/v2/series
serializer_compressor_kind == zlib            → wymusza /api/v2/series
resolver.IsLocal() (cluster-agent)            → wymusza /api/v2/series
```

**Konsekwencja praktyczna:** odbiornik trzeciej strony wskazany przez `dd_url` **nie jest** Datadog URL → `datadog_only` = false → dostaje **`/api/v2/series`**. Musi go zaimplementować. `/api/intake/metrics/v3/series` jest potrzebny tylko gdy operator jawnie ustawi `use_v3_api.series.enabled: true`.

Analogicznie dla sketchy: domyślnie `/api/beta/sketches`; `/api/intake/metrics/v3/sketches` wymaga wpisania URL-a do `serializer_experimental_use_v3_api.sketches.endpoints`.

**`/intake/` jest multipleksem** — odbiornik musi rozpoznawać po kształcie JSON, nie po ścieżce:
1. eventy: `{"apiKey":"", "events":{"<source_type_name|api>":[…]}, "internalHostname":"<host>"}` (pole `apiKey` **celowo puste**, klucz jest w nagłówku)
2. shutdown event: ten sam kształt, wysyłany synchronicznie, `Retryable=false`
3. host metadata: `*hostimpl.Payload` (uwaga: pole `gohai` to **string** z zagnieżdżonym JSON-em; `resources` to legacy nazwa dla processes)
4. agentchecks metadata: `*collectorimpl.Payload`
5. legacy processes metadata: dowolny `interface{}`

**`/api/v1/metadata`** to też multipleks — 11 komponentów rozróżnianych po kluczu najwyższego poziomu: `agent_metadata`, `host_metadata`, `check_metadata`+`logs_metadata`+`files_metadata`, `system_probe_metadata`, `security_agent_metadata`, `signing_metadata`, `host_system_info_metadata`, `host_gpu_metadata`, `ha_agent_metadata`, `datadog_cluster_agent_metadata`, `clustercheck_metadata`.

### 4.2 `api.<site>`

**Klucz:** `dd_url` (dla `/api/v1/validate`, `/api/v2/validate`, `/api/v2/intake-key`) — ale z zastrzeżeniami:

* `/api/v1/validate` — `getAPIDomain()` (`forwarder_health.go:205`) dopasowuje regexpem `([a-z]{2,}\d{1,2}\.)?(datadoghq\.[a-z]+|ddog-gov\.com)\.?$` i przepisuje na `https://api.` + dopasowanie. **Host niepasujący (czyli twój) zostaje bez zmian** — musisz serwować `/api/v1/validate` na tym samym hoście co `dd_url`. Klucz API jest w **query param `api_key`**, nie w nagłówku. Wariant CLI (`pkg/cli/subcommands/experimental/check.go:185`) używa nagłówka `DD-API-KEY` i ignoruje `dd_url` (buduje z `site`). Akceptuj oba.
* `/api/v2/validate` — trace-agent, OPM. **`apm_config.apm_dd_url` NIE działa**, używa core'owego `dd_url`. Odpowiedź musi być `{"data":{"id":"<uuid>"}}` z **niepustym** `data.id`; 4 próby (backoff 1s/2s/4s), potem rezygnacja na stałe. Brak klucza enable — `EnableOPMFetch = true` jest zahardkodowane w `comp/trace/config/impl/setup.go:173`.
* `/api/v2/intake-key` — delegated auth. Ciało żądania **puste** (`bytes.NewBuffer([]byte(""))`) mimo `Content-Type: application/json`; auth w `Authorization: Delegated <proof>` (AWS SigV4). Odpowiedź: `{"data":{"attributes":{"api_key":"…"}}}`, pusty `api_key` = błąd. Enable: `<prefix>.delegated_auth.org_uuid` niepuste (prefiksy: root, `logs_config`, `remote_configuration`, `evp_proxy_config`, `ol_proxy_config`).
* `/api/v1/query` — cluster-agent, External Metrics. Wymaga **klucza aplikacji**, nie tylko API key. Zapytania są łączone przecinkiem w jeden request; odpowiedź musi mieć `query_index` (0-based) na każdej serii, inaczej seria zostanie odrzucona. Timestampy w `pointlist` są w **milisekundach**; agent wybiera **przedostatni** niepusty punkt. Chunkowanie: `maxCharactersPerChunk = 7000` znaków escapowanego query (żeby uniknąć HTTP 414).

### 4.3 `process.<site>`

**Klucz:** `process_config.process_dd_url` + `process_config.additional_endpoints`.
**Enable:** `process_config.process_collection.enabled` / `.container_collection.enabled` / `.process_discovery.enabled` / `network_config.enabled`.

Wszystkie cztery ścieżki dzielą framing i odpowiedź. **Jedna ścieżka = dwa typy komunikatu**: `/api/v1/collector` niesie `CollectorProc` (bajt typu 12) i `CollectorRealTime` (27); `/api/v1/container` niesie 39 i 40. Rozróżnienie **wyłącznie po bajcie 2 nagłówka**, nie po ścieżce ani po `Endpoint.Name`.

**Odpowiedź ma znaczenie.** `pkg/process/runner/runner.go:501` (`ignoreResponseBody()` zwraca `false` dla każdego checku) parsuje ciało przez `model.DecodeMessage`, wymaga `Header.Type == 23 (TypeResCollector)`. Minimalna poprawna odpowiedź:

```
16-bajtowy nagłówek V3 {0x03, 0x00 (raw protobuf!), 0x17, 0x00, orgID int32 LE = 0, ts int64 LE}
+ proto.Marshal(&process.ResCollector{Status:&process.CollectorStatus{ActiveClients:0, Interval:10}})
```

`ActiveClients > 0` przełącza agenta w tryb realtime (2 s); `Interval` ustawia okres RT. Puste ciało → log `"Could not decode response body"` przy **każdym** submicie i licznik błędów, ale dostarczenie nadal liczy się jako sukces.

Reasemblacja: każdy chunk niesie `GroupId` (wspólny dla przebiegu) i `GroupSize` (liczba chunków). Limity: `process_config.max_per_message` (100, cap 10000), `process_config.max_message_bytes` (1 000 000, cap 4 000 000).

### 4.4 `orchestrator.<site>`

**Klucz:** `orchestrator_explorer.orchestrator_dd_url` (fallback `process_config.orchestrator_dd_url`), `*_additional_endpoints`.
**Enable:** `orchestrator_explorer.enabled` + build tag `orchestrator` (jest w `AGENT_TAGS` i `CLUSTER_AGENT_TAGS`) + środowisko K8s/ECS.

Jeden komunikat na request; ten sam framing 16-bajtowy co process-agent. Nagłówki specyficzne: `X-Dd-Orchestrator-ClusterID`, `DD-EVP-ORIGIN: agent`, `DD-EVP-ORIGIN-VERSION`. Odpowiedź nieparsowana (brak `CompletionHandler`) → 200 z pustym ciałem wystarczy. **Uwaga na retry:** 404 jest **ponawiane** celowo (komentarz w `transaction.go:450`) — odbiornik nie może zwracać 404 na nieznaną trasę, bo agent będzie retrywał w nieskończoność.

Typy komunikatów: Pod=41, ReplicaSet=42, Deployment=43, Service=44, Node=45, Cluster=46, Job=47, CronJob=48, DaemonSet=49, StatefulSet=50, PV=51, PVC=52, Role=54, RoleBinding=55, ClusterRole=56, ClusterRoleBinding=57, ServiceAccount=58, Ingress=59, Namespace=61, VPA=83, HPA=84, NetworkPolicy=85, LimitRange=86, StorageClass=87, PodDisruptionBudget=88, ECSTask=**200**. Manifesty: 80/81/82 na `/api/v2/orchmanif`.

(Enum `NodeType` z `pkg/orchestrator/model/types.go` — K8sPod=1…, ECSTask=150 — to **telemetria wewnętrzna**, nie bajt na drucie.)

### 4.5 `trace.agent.<site>`

**Klucz:** `apm_config.apm_dd_url`; nadpisywane całkowicie przez `observability_pipelines_worker.traces.url` / `vector.traces.url`; `apm_config.additional_endpoints` daje fan-out; MRF → `trace.agent.mrf.<multi_region_failover.site>`.

* `/api/v0.2/traces` — `Content-Encoding: zstd` w core/trace-agent i otel-agent (`fx-zstd`), **`gzip` tylko w host-profiler** (`fx-gzip`). Honoruj nagłówek. Dodatkowy nagłówek `X-Datadog-Reported-Languages` (lista rozdzielona `|`). Flush przy 3 200 000 B nieskompresowanych (`MaxPayloadSize`) lub co 5 s. Dwa writery (`TraceWriter` i `TraceWriterV1`) piszą ten sam `pb.AgentPayload`, różniąc się tylko polem: `TracerPayloads` (field 5) vs `IdxTracerPayloads` (field 11). Odbiornik musi obsłużyć oba.
* `/api/v0.2/stats` — **msgpack + gzip**, nie protobuf. `msgp.Encode` na `*pb.StatsPayload`, klucze mapy PascalCase: `AgentHostname`, `AgentEnv`, `Stats`, `AgentVersion`, `ClientComputed`, `SplitPayload`. Split przy 4000 wpisach, wtedy `SplitPayload=true`.
* `/api/v0.1/pipeline_stats` — czysty reverse proxy. **Ścieżka wejściowa lokalnie to `/v0.1/pipeline_stats`, wyjściowa to `/api/v0.1/pipeline_stats`** — `setTarget` robi `r.URL = u`, więc ścieżka klienta jest **odrzucana**, nie doklejana.

### 4.6 `instrumentation-telemetry-intake.<site>` — cztery różne produkcje na jednej ścieżce

`/api/v2/apmtelemetry` obsługuje:

| producent | źródło | klucz konfiguracyjny | ciało |
|---|---|---|---|
| proxy tracerów | `pkg/trace/api/telemetry.go` | `apm_config.telemetry.dd_url` | opaque, passthrough z biblioteki tracującej |
| onboarding trace-agenta | `pkg/trace/telemetry/collector.go:105` | `apm_config.telemetry.dd_url` | `{"request_type":"apm-onboarding-event",…}` |
| agent telemetry | `comp/core/agenttelemetry/impl/sender.go:40` | `agent_telemetry.dd_url` | `{"request_type":"agent-metrics"/"message-batch"/"agent-logs"/"agent-bsod",…}`, **zstd** |
| cluster-agent RC | `pkg/clusteragent/telemetry/collector.go:152` | `apm_config.telemetry.dd_url` | `{"request_type":…,"payload":{"event_name":"agent.k8s.patch"/"agent.k8s.mutate",…}}`, nagłówek **`DD-API-KEY`** (wielkie litery) |
| fleet installer | `pkg/fleet/installer/telemetry/client.go:29` | brak (z `site`) | spany OTel-like |

Rozróżniaj po polu `request_type`, nie po ścieżce. Uwaga na wielkość liter nagłówka klucza: `DD-Api-Key` (trace-agent) vs `DD-API-KEY` (cluster-agent).

Proxy: `/telemetry/proxy/` jest `StripPrefix`-owane, a **reszta ścieżki jest przekazywana dosłownie**, więc odbiornik musi akceptować dowolny subpath, nie tylko `/api/v2/apmtelemetry`. Scrubbing sekretów działa **tylko** gdy `DD-Telemetry-Request-Type: injection-metadata` i ścieżka == `/api/v2/apmtelemetry`; skompresowane ciało w tym przypadku nie da się sparsować i zostaje **zastąpione literałem `{}`** przy zachowanym nagłówku `Content-Encoding`.

### 4.7 `config.<site>` — Remote Config

Pięć transportów na jednym hoście:

1. **HTTP** `/api/v0.1/configurations` (POST, protobuf), `/api/v0.1/org` (GET), `/api/v0.1/status` (GET). Nagłówki: `Content-Type: application/x-protobuf` (ustawiany też na GET-ach bez ciała!), `DD-Api-Key`, opcjonalnie `DD-PAR-JWT`, `DD-Application-Key`, `User-Agent`.
2. **Preflight** `GET /api/v0.2/ping-pong` — musi zwrócić dokładnie 200, inaczej wszystkie cztery testy transportowe są pomijane.
3. **WebSocket** `/api/v0.2/echo-test` i `/api/v0.2/echo-test-alpn` (ten drugi z ALPN `dd-rc-v1`, wymaga TLS).
4. **gRPC** `/datadog.config.RcEcho/RunEchoTest` — bidi stream, `grpc-timeout` 7 dni (celowo, żeby Envoy nie ucinał).
5. **Raw TLS TCP** na porcie **8042** tego samego hosta, framing 2B LE length prefix, max 8192 B.

Wszystkie echo-testy: serwer **wysyła pierwszy**, agent odsyła identyczne bajty. Deadline 5 minut odnawiany przy każdym odczycie/zapisie — serwer musi wysłać PING lub ramkę danych co najmniej raz na 5 min. WS ma magiczne payloady `set_compress_on` / `set_compress_off` (dosłowne porównanie bajtów). Cykl: raz przy starcie, potem co 24 h. Kill switch dla **wszystkich czterech**: `remote_configuration.no_websocket_echo` (mimo nazwy).

Wymagania TLS: `api.NewHTTPClient` **odmawia** budowy klienta dla `http://` chyba że `remote_configuration.no_tls: true`, i odmawia przy `skip_ssl_validation` chyba że `remote_configuration.no_tls_validation: true`.

### 4.8 Logi

Wybór transportu (`comp/logs/agent/config/config.go:134-143`):

```
HTTP gdy:  force_use_http || use_http || logs_dd_url zaczyna się od http(s):// || OPW enabled
           || (test łączności HTTP OK && !shouldUseTCP())
shouldUseTCP() = force_use_tcp || use_tcp || socks5_proxy_address || additional_endpoints niepuste
```

**Uwaga:** samo ustawienie `logs_config.additional_endpoints` przełącza cały pipeline na TCP, bez żadnej jawnej zgody użytkownika. TCP idzie na `agent-intake.logs.<site>` z portem z zahardkodowanej mapy: **10516** dla `.com`, **443** dla `.eu`, inaczej `logs_config.dd_port`.

Framing TCP (`comp/logs-library/client/tcp/`): `prefixer` dokleja `<api_key>` + spację (0x20), potem `delimiter`:
* `dev_mode_use_proto: false` (nietypowe) → `lineBreakDelimiter`, jeden `\n`
* `dev_mode_use_proto: true` (**DOMYŚLNE**) → `lengthPrefixDelimiter`, 4-bajtowy **big-endian** uint32 liczący `apikey + spacja + protobuf`

Czyli domyślna ramka TCP: `[4B BE len][api_key][0x20][proto.Marshal(pb.Log)]`. Agent **nigdy nie czyta** z socketu poza detekcją EOF — serwer nie odsyła nic.

Ciało HTTP: tablica JSON obiektów `{message, status, timestamp (ms), hostname, service, ddsource, ddtags}`. `ddtags` to **string** rozdzielony przecinkami, nie tablica. Typ `processor.jsonPayload` jest **nieeksportowany**.

`parseURL` (`config.go:544`) usuwa prefiks ścieżki jeśli jest dokładnie `/v1/input` lub `/api/v2/logs` — nie da się ich zdublować.

### 4.9 Fleet installer / OCI

`PackageURL()` (`pkg/fleet/installer/oci/download.go:585`) twardo koduje host:
* `oci://install.datadoghq.com/<pkg-bez-prefiksu-datadog->-package:<version>` (prod)
* `oci://install.datad0g.com/…` tylko gdy `site == "datad0g.com"`

Fallbacki prod: `install.datadoghq.com`, potem `gcr.io/datadoghq`. Przekierowanie: `installer.registry.url` / env `DD_INSTALLER_REGISTRY_URL` (+ per-image `DD_INSTALLER_REGISTRY_URL_<IMAGE>`) zastępuje host i **wyłącza fallbacki**. `installer.mirror` / `DD_INSTALLER_MIRROR` instaluje `mirrorTransport`, który **sam odpowiada 200 na `/v2/`** bez przekazywania do mirrora.

Wymagania dla odbiornika udającego rejestr:
* `/v2/` → 200 (lub 401 z `WWW-Authenticate` żeby wymusić auth)
* tag musi wskazywać **index** (multi-platform), nie pojedynczy manifest; `downloadIndex` filtruje po `runtime.GOOS`/`GOARCH` i dokładnym `Platform.Variant` (`""`, albo `"fips"` przy `env.FIPSMode`)
* manifest musi mieć adnotacje `com.datadoghq.package.name`, `.version`, `.size`, inaczej `Download()` zwraca błąd
* media typy warstw: `application/vnd.datadog.package.layer.v1.tar+zstd`, `.config.layer.v1.tar+zstd`, `.installer.layer.v1` (**bez** `+zstd`, pisana jako goły plik binarny 0700), `.extension.layer.v1.tar+zstd`
* limit warstwy: `layerMaxSize = 3 GiB`
* retry: 3 próby, 1 s odstępu, tylko dla błędów sieciowych (`net.OpError` `Temporary()`, `ECONNRESET`, `"connectex"`, HTTP/2 `INTERNAL_ERROR` — znany quirk GCR)
* `mirrorTransport` nadpisuje `Content-Type` odpowiedzi manifestu wartością pola `mediaType` z ciała JSON (obejście dla Nexusa) — więc nagłówek manifestu jest wybaczający, ale ciało musi być poprawnym JSON-em OCI

`/btfs/…` — **brak jakiegokolwiek klucza konfiguracyjnego**, host zahardkodowany w `pkg/ebpf/config.go:119`, bez auth, przez `http.DefaultClient`. SHA256 pobranego pliku musi zgadzać się z katalogiem dostarczonym przez produkt RC `BTF_DD`, inaczej `errBTFHashMismatch`.

---

## 5. Framing i kodowanie — wszystko, co nie jest „POST protobuf albo JSON"

### 5.1 16-bajtowy nagłówek agent-payload (process, orchestrator)

`agent-payload/process/message.go:479` `encodeHeaderV3`, little-endian:

| offset | rozmiar | pole | wartość |
|---|---|---|---|
| 0 | 1 | Version | 3 (`MessageV3`) |
| 1 | 1 | Encoding | 4 (`MessageEncodingZstd1xPB`, cgo) / 5 (`ZstdPBxNoCgo`) / 0 (raw proto) / 1 (JSON) / 2 (legacy, odrzucany) |
| 2 | 1 | Type | dyskryminator typu komunikatu |
| 3 | 1 | SubscriptionID | 0 (nieużywane) |
| 4 | 4 | OrgID int32 | 0 (nieużywane) |
| 8 | 8 | Timestamp int64 | często 0 — `EncodePayload` go nie ustawia |

Bajty 16..N = zstd(proto.Marshal(msg)).

**Kluczowe:** `Content-Encoding` **NIE jest wysyłany**. Kompresja żyje w bajcie 1 nagłówka. Nie wolno przepuszczać ciała przez transportowy dekompresor HTTP.

`ReadHeader` obsługuje też V1 (nagłówek 4 B) i V2 (8 B) — serwer może odesłać `ResCollector` w najprostszej formie V1/encoding 0.

Wyjątek: `TypeCollectorProcEvent` (60) w `pkg/process/util/api/payload.go:36` jest marszalowany **bez nagłówka**, gołym `proto.Marshal`. Obecnie nic nie produkuje tego typu, ale gałąź istnieje.

Caveat cgo: `MessageEncodingZstd1xPB` wymaga cgo (`DataDog/zstd`); build nocgo zwraca błąd przy enkodowaniu. Odbiornik w Go bez cgo powinien dekompresować sam przez `klauspost/compress/zstd` po odczytaniu nagłówka, zamiast polegać na `process.DecodeMessage`.

### 5.2 Kolumnowy protobuf v3 (metryki) — konkatenacja niezależnych strumieni

`pkg/serializer/internal/metrics/iterable_series_v3.go:finishPayload()`. Ciało **nie jest** jednym skompresowanym protobufem. Jest to:

```
compress( varint tag field 3 (MetricData, wiretype 2) + varint len(NIESKOMPRESOWANE MetricData) )
++ dla każdej niepustej kolumny i ∈ 1..26:
     compress( varint tag field i wiretype 2 + varint len(NIESKOMPRESOWANA kolumna i) )
     ++ compressedBytes(kolumna i)
```

Odbiornik musi użyć dekompresora, który przezroczyście łączy kolejne ramki (zstd i gzip to robią). Po dekompresji bajty są dokładnie poprawnym `intake_v3.Payload`. **Długości w prefiksach opisują dane nieskompresowane** — dekompresja musi poprzedzać parsowanie.

Wewnątrz: słowniki (`dictNameStr`=1 … `dictUnitStr`=25) to blob `varint len + wartość`; indeksy są **base-1** (0 = puste); kolumny referencyjne (`nameRefs`, `tagsetRefs`, `timestamps`, `sketchBinKeys`, …) są **delta-enkodowane zigzag**; kolumna `types` pakuje `metricType | valueType | metricFlags | tagCardinality` po nibble'u. Mapowanie kolumna→numer pola protobuf jest 1:1. Pole 1 jest `reserved`, pole 2 (`Metadata`) agent nigdy nie zapisuje.

Referencyjny dekoder: `test/fakeintake/aggregator/metricReaderV3.go` (drzewo testowe, ale to najlepsza dostępna specyfikacja).

Sketche v3: `sum, min, max` to trzy kolejne wartości w kolumnie wybranej przez `valueType`, a `cnt` **zawsze** ląduje w `valsSint64` na końcu; `avg` nie jest wysyłane (intake liczy `sum/cnt`). `interval` dla sketchy jest zawsze 0.

### 5.3 Ręcznie enkodowany protobuf (molecule) dla v2

`/api/v2/series` i `/api/beta/sketches` **nie** wołają `Marshal()` na strukturze gogen. Używają `github.com/richardartoul/molecule` i piszą pola numerami skopiowanymi z `.proto`. Wynik jest bajt-w-bajt zgodny z `gogen.MetricPayload` / `gogen.SketchPayload`, więc `proto.Unmarshal` działa.

`stream.NewCompressor` dostaje **pusty header, footer i separator**, więc po dekompresji dostajesz gołą konkatenację wpisów `field 1, wiretype 2` = poprawny `MetricPayload`. Żadnego prefiksu długości.

Sketche mają jeden wyjątek: footer to jawnie zapisane **puste pole 2** (`0x12 0x00`, `CommonMetadata`) — naśladownictwo wyjścia gogoproto. Nieszkodliwe dla standardowego parsera.

**Pułapka w sketchach:** pole `k` w `Dogsketch` to **packed sint32 (zigzag)**, pole `n` to packed uint32. Pomylenie zigzagu po cichu psuje sketch. Pole 3 (`distributions`) nigdy nie jest zapisywane.

### 5.4 msgpack tam, gdzie spodziewasz się protobufa

* `/api/v0.2/stats` (outbound) — `*pb.StatsPayload` przez `msgp.Encode` + gzip. Typ jest wygenerowany z protobufa, ale na drucie jest **msgpack z kluczami PascalCase**.
* `/v0.6/stats` (local) — `*pb.ClientStatsPayload` przez `msgp.Decode`, **bez kompresji** (brak obsługi `Content-Encoding` w całym `pkg/trace/api`).
* `/v0.7/traces` (local) — `pb.TracerPayload` przez `UnmarshalMsg` (msgpack map ze stringowymi kluczami), nie protobuf.
* `/v0.5/traces` — msgpack `[słownik_stringów, [[span…]]]`, gdzie span to tablica **dokładnie 12 elementów**: Service, Name, Resource, TraceID, SpanID, ParentID, Start, Duration, Error, Meta(`map[uint32]uint32`), Metrics(`map[uint32]float64`), Type. Indeksy > `len(dict)` → błąd.
* `/v1.0/traces` — msgpack **mapa, której klucze są numerami pól (uint)**. Pole 1 to streaming string table i **musi przyjść pierwsze**, inaczej `"Unexpected strings attribute, strings must be sent first"`. Pola 2..8 używają `UnmarshalStreamingString`: wartość to albo inline string (dopisywany do tabeli) albo uint-indeks do tabeli.

### 5.5 Podwójna kompresja / kompresja w nietypowym miejscu

* **`/api/v2/secdump`**: `gzip( multipart/form-data )`. Nagłówek `Content-Encoding: gzip` obejmuje **całe** ciało multipart, nie pojedyncze części. Część `dump` deklaruje `Content-Type: application/json`, ale zawiera **binarny protobuf** (`dumpsv1.SecDump`) — to błąd w agencie, ignoruj nagłówek części. Ciało jest strumieniowane przez `io.Pipe`, więc leci **chunked bez Content-Length**.
* **`/api/v2/srcmap`**: albo objcopy kompresuje sekcje debug zstd-em *wewnątrz* ELF-a i ciało HTTP leci nieskompresowane, albo cały multipart jest owinięty w zstd z `Content-Encoding: zstd`. Nigdy oba. Odbiornik musi rozgałęzić się na obecność nagłówka.
* **`/debugger/v1/diagnostics`** i **symdb**: multipart, w symdb część `file` to `gzip(JSON)` z własnym `Content-Type: application/gzip`.
* **Logi/EvP**: kompresja jest **strumieniowa**, nałożona na `arraySerializer` — bajty `[`, `,`, `]` przechodzą przez kompresor razem z zawartością.

### 5.6 Multipart

| endpoint | części (w kolejności!) |
|---|---|
| `/support/flare` | `case_id`, `email`, `source`, `rc_task_uuid`, `flare_source`, `tags`×N, **plik `flare_file`**, potem `agent_version`, `hostname` |
| `/api/v2/secdump` | `event` (JSON `ActivityDumpHeader`), `dump` (filename `dump.protobuf`) |
| `/api/v2/srcmap` | `elf_symbol_file`, potem `event` (JSON) |
| `/debugger/v1/diagnostics` | `event` (filename `event.json`, JSON array) |
| symdb `/api/v2/debugger` | `file` (filename `file.gz`, gzip JSON), potem `event` |
| `/api/v2/profile` | passthrough z tracera (dd-trace-go) |

**Uwaga na flare:** `agent_version` i `hostname` są zapisywane **po** pliku ZIP. Parser strumieniowy nie może zakładać, że metadane przyjdą przed plikiem. `ContentLength = -1` → `Transfer-Encoding: chunked`.

### 5.7 Endpointy, których odpowiedź MUSI być niepusta / sensowna

| endpoint | wymóg |
|---|---|
| `/api/v1/collector`, `/container`, `/connections`, `/discovery` | ramkowany `ResCollector` (typ 23). Puste ciało = spam błędów + brak trybu RT |
| `/api/v0.1/configurations` | pełny, podpisany zestaw metadanych TUF + target files. `{}` nie przejdzie weryfikacji |
| `/api/v0.1/org` | `OrgDataResponse` z **niepustym** `uuid` — pusty zostanie **zapisany na trwałe** w bolt store do następnej rotacji roota |
| `/api/v0.1/status` | `{enabled:true, authorized:true}` (bajty `08 01 10 01`); puste ciało → RC uznane za wyłączone |
| `/api/v2/validate` | `{"data":{"id":"<niepusty>"}}` |
| `/api/v2/intake-key` | `{"data":{"attributes":{"api_key":"<niepusty>"}}}` |
| `/api/v1/query` | seria z `query_index` i `pointlist`; brak serii = metryka nieważna |
| `/api/v2/profiles/symbols/query` | JSON:API array `SymbolFile`; steruje tym, czy agent wyśle symbole na `/api/v2/srcmap` |
| `/api/unstable/on_prem_runners*` | JSON:API z `runner_id` + `org_id`; dokładnie **200**, nie 201 |
| `/api/v2/…/workflow-tasks/dequeue` | pusty 200 = „brak zadania" (normalne). Zadanie musi mieć podpisany `signed_envelope`, który agent weryfikuje po SHA256 **surowych bajtów** — nie wolno re-marshalować |
| `/support/flare` | 200 + `Content-Type: application/json` + ciało JSON. Inny content-type = błąd nawet przy 200. Wcześniej HEAD musi zwrócić 200 **lub 404** |
| `/v0.4+/traces` (local) | `{"rate_by_service":{…}}` lub `{}` |
| OCI manifests/blobs | poprawne JSON-y OCI + zgodne hashe |
| `/btfs/…` | tar.xz o SHA256 zgodnym z katalogiem RC |
| `/api/v2/…/runner/health-check` | ciało ignorowane, ale nagłówki `X-Server-Time` i `X-Retry-After-Ms` są czytane |

Wszędzie indziej: 200/202 z pustym ciałem wystarcza. Kody statusu, na które forwarder reaguje: **400 i 413 → trwały drop**; **403 → odświeżenie sekretu + drop/retry**; **404 i reszta ≥400 → retry**. EvP/logi: **400/401/403/413 → trwały drop**, `>400` → retry z backoffem.

### 5.8 Inne osobliwości

* `/language_detection/detect` (system-probe): **GET z niepustym ciałem protobuf**. Złamie każdy framework, który zrzuca ciało na GET-ach.
* `/api/v1/validate` (forwarder): `fmt.Sprintf("%s%s?api_key=%s", domain, endpoints.V1ValidateEndpoint, apiKey)` — `V1ValidateEndpoint` to **struct**, nie string. `%s` formatuje go domyślną reprezentacją, więc dosłowny URL nie jest czystym `/api/v1/validate?…`. Zweryfikuj na realnym buildzie.
* `/dogstatsd/v1|v2/proxy`: linie są przekazywane **w trakcie czytania** jako datagramy UDP. Żądanie, które później przekroczy limit (100 000 linii → 413), zdążyło już dostarczyć wcześniejsze linie. Retry klienta duplikuje metryki.
* evp_proxy: **wszystkie** nagłówki klienta są kasowane poza allowlistą `{Content-Type, Accept-Encoding, Content-Encoding, User-Agent, DD-CI-PROVIDER-NAME, DD-EVP-ORIGIN, DD-EVP-ORIGIN-VERSION}`. `X-Datadog-EVP-Subdomain` jest **konsumowany i usuwany** — routuj po `Host`/SNI. Walidacja znaków: ścieżka tylko `[A-Za-z0-9/_\-+]` (**kropka niedozwolona**), query po `QueryUnescape` `[A-Za-z0-9/_\-+@?&=.:"\[\]]`.
* Limit ciała w proxy (profiling, pipeline_stats, openlineage) jest nakładany **tylko przy wielu endpointach**; przy pojedynczym celu ciało leci streamem bez limitu.
* `/api/v1/lineage` używa **`Authorization: Bearer <api_key>`**, nie `DD-API-KEY` — bo tak robi klient OpenLineage OSS.
* `/v1development/profiles` i `/v1/metrics` (host-profiler) dzielą **ten sam obiekt eksportera** — te same 4 nagłówki (`dd-api-key`, `dd-evp-origin`, `dd-evp-origin-version`, `dd-otel-metric-config`) lecą na obie ścieżki.
* `tracer_flare`: host wyprowadzany regexpem z gołego `site`; **regionalne site'y (`us3.datadoghq.com`) nie pasują** i produkują URL z pustym hostem → `http: no Host in request URL`. Działa tylko dla `datadoghq.com`, `.eu`, `datad0g.com/.eu`, `ddog-gov.com`.
* Klucz API w **query stringu** występuje w dokładnie jednym miejscu: `/api/v1/validate` z forwarder health. Wszędzie indziej to nagłówek.

---

## 6. Typy payloadu, których NIE da się zaimportować

Kryterium: typ nie leży w `github.com/DataDog/agent-payload/v5/...` ani w `github.com/DataDog/datadog-agent/pkg/proto/...`.

| endpoint | typ | co trzeba zrobić |
|---|---|---|
| `/api/v1/series` | koperta `{"series":[…]}` strumieniowana jansiterem; element = `metrics.Serie` | Zadeklaruj `{metric, points [][2]any, tags []string, host, device?, type, interval, source_type_name?, unit?}`. **Kolejność pól jest stała.** `type` ∈ `gauge|rate|count`. `device` tylko gdy niepuste. Timestampy obcinane do sekund. Serie z `NoIndex=true` są **pomijane** na tej ścieżce. `metrics.Serie.Tags` to `tagset.CompositeTags` z własnym marshallerem — import niepraktyczny |
| `/api/v1/check_run` | `servicecheck.ServiceCheck` | 6 pól, zawsze wszystkie, w kolejności: `check, host_name, timestamp, status, message, tags`. `tags` to **`null`** (nie `[]`) gdy brak. Pusty flush = `[]` |
| `/intake/` (eventy) | brak typu — ręczny jsoniter | `{apiKey:"", events:{<src>:[{msg_title,msg_text,timestamp,priority?,host,tags?,alert_type?,aggregation_key?,source_type_name?,event_type?}]}, internalHostname}` |
| `/api/v1/metadata` | 11× `*impl.Payload` | Pola `Metadata` są typów nieeksportowanych. Traktuj jako wolny JSON, klucz najwyższego poziomu dyskryminuje |
| `/intake/` host metadata | `payload.HostMetadata` (`pkg/opentelemetry-mapping-go/inframetadata/payload`, osobny moduł) | Importowalny z tamtego modułu, ale nie z dwóch wskazanych. `gohai` to **string z JSON-em**; `resources` to legacy nazwa |
| logi HTTP | `processor.jsonPayload` (**nieeksportowany**) | 7 pól: `message, status, timestamp (ms), hostname, service, ddsource, ddtags` |
| logi serverless | `jsonServerlessInitPayload` (nieeksportowany) | j.w. minus/plus `service` z `omitempty` |
| `/api/v2/kubeactions` | `ActionResultEvent` (dwie identyczne deklaracje!) | `agent-payload/v5/kubeactions` ma `KubeActionResult`, ale **nic go nie używa** — redeklaruj 15 pól |
| `/api/v2/ndm` | `metadata.NetworkDevicesMetadata` | 13 pól top-level, czysty JSON ze stabilnymi tagami |
| `/api/v2/ndmconfig` | `report.NCMPayload` | `{namespace, configs[], inventories[], collect_timestamp, agent_hostname}`; `content` to surowy tekst konfiguracji urządzenia |
| `/api/v2/ndmtraps` | anonimowa `map[string]any` | `{"trap":{ddsource,ddtags,timestamp,uptime,snmpTrapOID,snmpTrapName,snmpTrapMIB,variables[],+flattened}}`; v1 dodaje `enterpriseOID,genericTrap,specificTrap` |
| `/api/v2/softinv` | `softwareinventoryimpl.Payload` | `{hostname, host_software:{software:[software.Entry]}}` |
| `/api/v2/synthetics` | `common.TestResult` | Zagnieżdża `payload.NetworkPath` (importowalny z własnego modułu) |
| `/api/v2/events` (EvP) | anonimowa mapa | Koperta `{data:{type:"event",attributes:{…}}}`, 3 warianty (notable/change/logon) |
| `/api/v2/dbm*`, `/api/v2/query-actions`, `/api/v2/data_streams_messages`, `/api/v2/genresources` | **brak typu w Go** | Bajty pochodzą z integracji (Python/Rust) przez `SubmitEventPlatformEvent`. Przyjmuj `[]json.RawMessage`. Dyskryminator dla DBM: pole `dbm_type` / `kind` |
| `/api/v2/compliance` | `compliance.CheckEvent`, `ResourceLog` | Zagnieżdżone jako **escapowany string** w polu `message` koperty logów — podwójne dekodowanie |
| `/api/v2/secruntime` | `events.BackendEvent` + `serializers.EventSerializer` | easyjson, sklejone tekstowo (`mergeJSON`), potem w `message` |
| `/api/v2/secdump` część `event` | `profile.ActivityDumpHeader` | `{host, service, ddsource, ddtags, dns_names}` — `dns_names` to side-channel, nie ma go w protobufie |
| `/api/v2/srcmap` część `event` | `symbolUploadRequestMetadata` (nieeksportowany) | `{arch, gnu_build_id, go_build_id, file_hash, type, symbol_source, origin, origin_version, filename}` |
| `/api/v2/profiles/symbols/query` | `SymbolsQueryRequest` / `SymbolFile` | JSON:API, `{buildIds, arch}` → `[{id, buildId, symbolSource, buildIdType}]` |
| `/api/v2/apmtelemetry` | `OnboardingEvent`, `ApmRemoteConfigEvent`, `agenttelemetryimpl.Payload`, `telemetryRequest` | 4 różne; `telemetryRequest` jest nieeksportowany i to tylko częściowy dekod |
| `/support/flare` odpowiedź | `flareResponse` (nieeksportowany) | `{case_id int, error string, request_uuid string}` |
| PAR (wszystko) | `par.CreateRunnerRequest/Response`, `DequeueJSONRequest`, `PublishTaskUpdateJSONRequest`, `HeartbeatJSONRequest`, `types.Task` | Wewnętrzne. Tylko enumy (`actionsclient.Client`, `errorcode.ActionPlatformErrorCode`) i `privateactions.*` są w `pkg/proto`. **Enumy serializują się jako LICZBY**, nie nazwy |
| `/api/unstable/synthetics/agents/tests` | `common.SyntheticsTestConfig` | Ma **własny `UnmarshalJSON`** czytający top-level `subtype` (`UDP`/`TCP`/`ICMP`) żeby wybrać typ `config.request`. Nieznany subtype wywala **cały** poll |
| `/api/v1/query` | `[]datadog.Series` | Z `gopkg.in/zorkian/go-datadog-api.v2` (third-party, niezwendorowany) |
| RC: `CONTAINER_AUTOSCALING_SETTINGS`, `CLUSTER_AUTOSCALING_VALUES` | `model.*List` | Zależą od CRD `datadog-operator`; `agent-payload` ma **przestarzały** odpowiednik dla cluster autoscaling (brak `type`/`manifest`) |
| `/debugger/*`, `/symdb/*` | `uploader.DiagnosticMessage`, `decode.message` (nieeksportowany), `symdb/uploader.Scope` | Wszystkie za `//go:build linux && bpf` |
| lokalne: `types.TaggerListResponse`, `workloadmeta.WorkloadDumpResponse`, `v1.MetadataResponse`, `health.Status`, `api.infoPayload`, `diagnose.Result`, `clusterchecks/types.*` | — | Wewnętrzne; wszystkie to proste struktury JSON |

**Typy jawnie potwierdzone jako importowalne** (sprawdzone w checkoucie `agent-payload`): `gogen.MetricPayload`, `gogen.SketchPayload`, `intake_v3.Payload`, `process.Collector*` + `ResCollector` + `CollectorStatus` + `Message`/`MessageHeader`/`EncodeMessage`/`DecodeMessage`, `contlcycle.EventsPayload`, `contimage.ContainerImagePayload`, `sbom.SBOMPayload` (+ `cyclonedx_v1_4.Bom`), `agentdiscovery.AgentDiscoveryPayloadBatch`, `healthplatform.HealthReport`, `dumpsv1.SecDump`, `kubeactions.KubeAction/KubeActionsList`, `autoscaling/kubernetes.WorkloadValuesList`, `pb.Log`. Z `pkg/proto` (osobny moduł publiczny `github.com/DataDog/datadog-agent/pkg/proto`): `trace.AgentPayload`, `trace.StatsPayload`, `trace.ClientStatsPayload`, `trace.TracerPayload`, `trace.Traces`, `idx.InternalTracerPayload`, `core.LatestConfigsRequest/Response`, `core.OrgDataResponse`, `core.OrgStatusResponse`, `core.ClientGetConfigsRequest/Response`, `sds.SdsResultPayload`, `dogstatsdhttp.Payload`, `process.ParentLanguageAnnotationRequest`.

---

## 7. Co pozostaje niepewne lub niezweryfikowane

**Rozbieżności między źródłami, rozstrzygnięte na korzyść dowodu z grepa:**

1. **Pięć tras forwardera uznanych za martwe.** Krytyk #1 twierdził, że w spisie brakuje `/api/v2/events`, `/api/v2/service_checks`, `/api/v2/host_metadata`, `/api/v1/sketches` i `/api/intake/metrics/v3beta/series`. Weryfikacja: wszystkie są zadeklarowane w `endpoints.go`, ale jedynym konsumentem symbolu jest `initEndpointExpvars()` w `impl/telemetry.go`, które czyta wyłącznie `endpoint.Name` do rejestracji licznika expvar. Żadne `createHTTPTransactions` ich nie przekazuje. Dla `v3beta/series` jest mocniej: `metricsShadowSampleRate()` (`metrics.go:113`) ma bezwarunkowy `if kind == metricsKindSeries { return 0 }` **przed** odczytem konfiguracji, a schema `core_schema.yaml:8940-8970` nie zawiera podsekcji `series` — więc nie istnieje klucz, którym dałoby się to włączyć. **Wniosek:** nie implementuj tych tras; realne odpowiedniki to odpowiednio `/intake/`, `/api/v1/check_run`, `/intake/`, `/api/beta/sketches`, `/api/intake/metrics/v3/series`. Gdybyś chciał zapas na przyszłość — to pięć tras, które są „o jeden commit" od ożycia (poza `v3beta/series`, które wymaga zmiany kodu, nie konfiguracji).

2. **`intake.profile.` vs `otlp.` dla host-profilera.** Jeden raport twierdził, że host-profiler wysyła profile na `intake.profile.`. Weryfikacja `comp/host-profiler/collector/impl/agentprovider/config_builder.go:56` pokazuje `endpointFormat = "https://otlp.%s"`, identycznie w shipowanych YAML-ach (`host-profiler-config.yaml`, Helm, Operator). Ciąg `intake.profile.` występuje tam tylko w komentarzu przykładowym i w fixture'ach testowych standalone konwertera. `apm_config.profiling_dd_url` służy host-profilerowi wyłącznie do **wyekstrahowania site'u**, nie jako URL.

3. **Payload `/api/v2/dbmmetrics` i sąsiadów.** Jeden raport podawał `oracle.MetricsPayload`. To tylko jeden z producentów (i to za build tagiem `oracle`); pozostałe to integracje Pythonowe wysyłające dowolny JSON. Odbiornik **musi** parsować heterogeniczną tablicę.

4. **`/api/v1/collector` payload.** Jeden raport podawał `*process.CollectorPod`. To jest nieprawda — `CollectorPod` idzie na `/api/v2/orch`. Na `/api/v1/collector` jest `CollectorProc`/`CollectorRealTime`.

**Niezweryfikowane ze źródeł (brak kodu w checkoucie lub w cache modułów):**

* **Format ciała `/api/v2/profile`** i `intake.profile.<site>/v1/input`. Budują je `github.com/DataDog/dd-trace-go/v2/profiler` (niezwendorowany, nieobecny w lokalnym cache). Agent jest tylko reverse proxy / przekazuje URL. Wiemy, że to multipart (`flare_file`-podobne części `event` + pprof), ale kolejność, nazwy pól i Content-Encoding części **nie zostały potwierdzone**.
* **Content-Type/Content-Encoding OTLP receivera** (`localhost:4317/4318`) i eksportera OTLP host-profilera. `otlpreceiver` i `otlphttpexporter` v0.159.0 nie są zwendorowane. Przyjęto poziom specyfikacji OTLP (`application/x-protobuf` / `application/json`, gzip/zstd akceptowane). Nie zweryfikowano też, czy klient OTLP parsuje `partial_success` w odpowiedzi.
* **Dokładny kształt dokumentu JSON:API** produkowanego przez `github.com/DataDog/jsonapi` v0.13.0 (PAR enrollment, symbols/query, `/api/v2/actions/connections`, dequeue). Tagi struktur zostały odczytane; konkretne nazwy członów `attributes` wynikają z tagów, ale sam marshaller nie był inspekcjonowany.
* **Ścieżka `/v2/{repo}/blobs/{digest}`.** `go-containerregistry` nie jest zwendorowany. Że ścieżka manifestu ma kształt `/v2/<repo>/manifests/<ref>` jest potwierdzone pośrednio przez `isManifestPath()` w `pkg/fleet/installer/oci/mirror.go:84`; kształt blobs wynika ze specyfikacji OCI, nie z kodu w repo.
* **`/v1development/profiles` jako sufiks ścieżki.** Nie występuje jako literał w datadog-agent — jest domyślnym per-signal sufiksem `otlphttpexporter` gdy ustawiono tylko `endpoint`. Wywnioskowane z README i fixture'ów, nie odczytane z kodu biblioteki.
* **Zachowanie `%s` na `transaction.Endpoint`** w `forwarder_health.go:286` — patrz §5.8. Potrzebuje weryfikacji na realnym buildzie.
* **Reachability build-tagów** nie była sprawdzana dla każdego modułu system-probe. Część jest linux-only (`bpf`), część darwin-only (`notable_events`, `logon_duration`), część windows-only (`windows_crash_detection`, `software_inventory`, `iis_tags`).
* **`RemoteCommandProvider` po stronie sub-agenta.** Core agent rejestruje serwer-proxy i sam jest klientem, ale w tym checkoucie **żaden sub-agent nie implementuje serwera wewnętrznego** (tylko testy). Mechanizm istnieje dla zewnętrznych remote agentów.
* **Pięć schematów payloadu bez producenta:** m.in. `agent-payload` `process/events.proto` (`CollectorProcEvent`, typ 60 — jedyny z gałęzią kodowania **bez nagłówka**), `kubeactions.KubeActionResult`, `intake_v3.Response`. Nikt ich nie wysyła; budowanie odbiornika pod nie to strata czasu, ale ich obecność sygnalizuje, że gałęzie kodujące istnieją.

**Ostrzeżenie metodologiczne:** w tym środowisku `rg -rn "wzorzec"` jest parsowane jako ripgrep `--replace` z wartością `"n"`, co **po cichu podmienia dopasowane fragmenty w wyjściu** (`install.datadoghq.com` drukowane jako `n.com`). Kilka wcześniejszych przebiegów mogło tak działać. Przy weryfikacji używaj `rg -n` albo `grep -rn`.

**Świadomie pominięte (poza zakresem):** wywołania do Kubernetes API / kubelet / metadanych chmurowych, webhooki admission controllera (to kube-apiserver dzwoni do agenta), rekomendator autoskalowania pod adresem z adnotacji CRD (URL w pełni klienta), rejestry OCI stron trzecich przy auto-instrumentacji, `/api/v2/rum` (martwy kod, jawnie oznaczony jako *abandoned*), `mutate-pod` (ścieżka nie istnieje w repo), oraz cały `test/fakeintake` (narzędzie testowe, choć bywa użyteczne jako referencyjny dekoder v3).