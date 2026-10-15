# Produkty Datadoga i ich endpointy

Każdy produkt Datadoga ma własny intake: własny host, własny klucz
konfiguracji, własny format. Ten plik jest ich spisem.

Zebrane ze źródeł agenta i z przechwyconego ruchu. **Pięć rodzin**, każda
z własnym hostem, własnym kluczem konfiguracji i własnym formatem.

Legenda: ✅ obsługujemy · ⬜ nie obsługujemy · 💤 zadeklarowane w agencie,
jeszcze nieużywane

---

## 1. Forwarder główny

**host:** `api.<site>` oraz `<wersja>-app.agent.<site>`
**config:** `dd_url` / `DD_DD_URL`

```
✅ POST /intake/                                 metadane hosta, gohai, zdarzenia agenta
✅ POST /api/v1/series                           metryki, JSON
✅ POST /api/v2/series                           metryki, protobuf
✅ POST /api/v1/check_run                        statusy checkow
✅ POST /api/v1/metadata                         metadane integracji (u nas zaslepka)
✅ POST /api/beta/sketches                       szkice DDSketch
✅ GET  /api/v1/validate                         sprawdzenie klucza

💤 POST /api/v2/events                           <- KOLIZJA z event-management-intake
💤 POST /api/v2/service_checks
💤 POST /api/v1/sketches                         oznaczone "unused for now"
⬜ POST /api/v2/host_metadata
⬜ POST /api/intake/metrics/v3/series             <- NASTEPNY format glownego strumienia
⬜ POST /api/intake/metrics/v3beta/series
⬜ POST /api/intake/metrics/v3/sketches
⬜ POST /api/intake/metrics/v3beta/sketches
```

Rodzina `v3` jest najważniejsza z nieobsługiwanych: to kolejne pokolenie
formatu metryk, czyli strumienia, na którym stoi wszystko inne.

---

## 2. Process-agent i orchestrator

**host:** `process.<site>` (procesy), `kubeops-intake.<site>` (orchestrator)
**config:** `process_config.process_dd_url` / `DD_PROCESS_CONFIG_PROCESS_DD_URL`
**config orchestratora:** `orchestrator_explorer.orchestrator_dd_url`
/ `DD_ORCHESTRATOR_EXPLORER_ORCHESTRATOR_DD_URL` — **osobny klucz**

Jedyna rodzina z własną 16-bajtową ramką zamiast gołego protobufa. Ta sama
ramka niesie **37 typów wiadomości**, z czego 28 to zasoby Kubernetesa.

```
✅ POST /api/v1/collector          typ 12  CollectorProc
⬜ POST /api/v1/container          typy 39, 40
⬜ POST /api/v1/connections        typ 22
⬜ POST /api/v1/discovery          typ 53
⬜ POST /api/v2/orch               typy 41..88  <- zasoby k8s
⬜ POST /api/v2/orchmanif          typy 80..82  <- manifesty
⬜ POST /api/v1/orchestrator       wersja starsza, wlaczana przez
                                  orchestrator_explorer.use_legacy_endpoint
```

Orchestrator ma **własny `dd_url`**, niezależny od `process_dd_url`. To piąty
przekierowywalny podsystem — wcześniejsze notatki w `datadog-agent.md`
i `konfiguracja-agenta.md` mówią o czterech i wymagają poprawki.

Typy zasobów k8s: Pod 41, ReplicaSet 42, Deployment 43, Service 44, Node 45,
Cluster 46, Job 47, CronJob 48, DaemonSet 49, StatefulSet 50, PV 51, PVC 52,
Role 54, RoleBinding 55, ClusterRole 56, ClusterRoleBinding 57,
ServiceAccount 58, Ingress 59, Namespace 61, Manifest 80, ManifestCRD 81,
ManifestCR 82, VPA 83, HPA 84, NetworkPolicy 85, LimitRange 86,
StorageClass 87, PodDisruptionBudget 88, ECSTask 200.

---

## 3. Trace-agent

**host:** `trace.agent.<site>`
**config:** `apm_config.apm_dd_url` / `DD_APM_DD_URL`

```
✅ POST /api/v0.2/traces                  slady, protobuf, SPROBKOWANE
✅ POST /api/v0.2/stats                   statystyki APM, msgpack, KOMPLETNE
⬜ POST /api/v2/apmtelemetry              proxy dla telemetrii bibliotek sledzacych
⬜ POST /api/v2/data_streams_messages     Data Streams Monitoring (przez event platform)
```

---

## 4. Event platform

**host:** `<produkt>-intake.<site>`
**config:** **BRAK `dd_url`** — tylko `<prefiks>.enabled` oraz
`<prefiks>.additional_endpoints`, a to **dokłada** odbiorcę zamiast podmieniać.

Ścieżka powstaje jednym wzorem:

```go
url.Path = fmt.Sprintf("%s/api/v2/%s", endpoint.PathPrefix, endpoint.TrackType)
```

```
✅ http-intake.logs.<site>        POST /api/v2/logs            trackType "logs"
⬜ contlcycle-intake.<site>       POST /api/v2/contlcycle      cykl zycia kontenera
⬜ contimage-intake.<site>        POST /api/v2/contimage       inwentarz obrazow
⬜ kubeops-intake.<site>          POST /api/v2/kubeactions     akcje na klastrze
⬜ sbom-intake.<site>             POST /api/v2/sbom            podatnosci
⬜ dbm-metrics-intake.<site>      POST /api/v2/dbmmetrics
                                  POST /api/v2/dbmactivity
                                  POST /api/v2/databasequery
                                  POST /api/v2/dbmmetadata
                                  POST /api/v2/dbmhealth
                                  POST /api/v2/dbmcolumnstatistics
⬜ ndm-intake.<site>              POST /api/v2/ndm, /api/v2/ndmconfig
⬜ snmp-traps-intake.<site>       POST /api/v2/ndmtraps
⬜ ndmflow-intake.<site>          POST /api/v2/ndmflow
⬜ netpath-intake.<site>          POST /api/v2/netpath
⬜ resources-intake.<site>        POST /api/v2/genresources
⬜ agentdiscovery-intake.<site>   POST /api/v2/agentdiscovery
⬜ softinv-intake.<site>          POST /api/v2/softinv
⬜ data-obs-intake.<site>         POST /api/v2/query-actions
⬜ sds-intake.<site>              POST /api/v2/sdsresult
⬜ http-synthetics.<site>         POST /api/v2/synthetics
⬜ event-management-intake.<site> POST /api/v2/events          <- KOLIZJA z forwarderem
⬜ trace.agent.<site>             POST /api/v2/data_streams_messages
```

**Nasze `/api/v2/logs` nie jest niczym szczególnym** — to ten sam wzór
z `trackType` równym `"logs"`. Potwierdzone: `intakeTrackType = "logs"`
w `comp/logs/agent/impl/agent.go:60`. Logi nie mają pliku `pipelines_logs.go`,
bo stałą ustawia bezpośrednio agent logów — ale mechanizm jest ten sam.

### Format ciała: WSPÓLNA JEST RURA, NIE ŁADUNEK

Event platform ujednolica adres, paczkowanie, kompresję, ponawianie
i konfigurację. **Format ciała wybiera sobie każdy produkt osobno.**

| | pipeline'ów | tracki |
|---|---|---|
| `application/json` | 17 | kubeactions, events, dbm* (6), ndm* (4), netpath, synthetics, softinv, query-actions, data_streams_messages |
| `application/x-protobuf` | 6 | contlcycle, contimage, sbom, genresources, agentdiscovery, sdsresult |

Czyli nie da się napisać jednego parsera na całą rodzinę. Routing i rozpakowanie
— tak, wspólne. Dalej **parser na produkt**, a przy protobufie jeszcze schemat,
który trzeba skądś wziąć.

To ta sama sytuacja, co na `/api/v1/series` i `/api/v2/series`: jedna ścieżka,
dwa kodowania, rozpoznawane po `Content-Type` z awaryjnym sprawdzeniem
pierwszego bajtu. Ten mechanizm mamy w `bodyIsJSON()`.

---

## 5. Kanały sterujące i pomocnicze

```
⬜ config.<site>                  GET/POST /api/v0.1/configurations   <- KANAL W DOL
                                           /api/v0.1/org
                                           /api/v0.1/status
✅ <wersja>-flare.agent.<site>    POST /support/flare                 (u nas zaslepka)
⬜ instrumentation-telemetry-intake.<site>                            telemetria agenta
```

`/api/v0.1/configurations` to jedyny endpoint, przez który dane płyną **do
agenta**, nie od niego. Opisany osobno w `zadania/remote-config-tuf.md`.

---

## 6. Wyłącznie publiczne API

Agent nigdy tu nie uderza — to dla `datadog-api-client` i skryptów
wysyłających po HTTP bez agenta.

```
ZAPIS
✅ POST /api/v1/events
✅ POST /api/v1/distribution_points       surowe wartosci, szkic budujemy my

ODCZYT
⬜ GET  /api/v1/query                     <- odblokowuje datasource Grafany
⬜ GET  /api/v1/metrics                   lista aktywnych metryk
⬜ GET  /api/v1/search                    wyszukiwarka metryk
```

---

## Jak to obsługujemy

Router w `intake/` dzieli trasy **po intake'ach**, nie trzyma ich na płaskiej
liście:

```
intake/routes.go       jedna funkcja na intake, kazda z hostem i kluczem config
intake/hostrouter.go   dyspozytor po naglowku Host
```

Dopasowanie idzie po **wiodącej etykiecie hosta**, bo tak agent buduje adresy:
`<zaszyty prefiks>.<site>`. Wdrożenie z wildcard DNS dostaje prawdziwy routing
per intake; wdrożenie z jedną nazwą trafia na router zapasowy, który ma
wszystkie trasy. Żaden z tych układów nie jest przypadkiem szczególnym.

Zmierzone:

```
Host: api.nc.test            /api/v2/series     -> 202
Host: trace.agent.nc.test    /api/v2/series     -> 404   <- nie nalezy tu
Host: trace.agent.nc.test    /api/v0.2/traces   -> 200
Host: process.nc.test        /api/v1/collector  -> 200
```

Dzięki temu kolizja `/api/v2/events` jest już rozwiązana, zanim wystąpi —
wystarczy zarejestrować ją w dwóch różnych intake'ach.

## Decyzja: co bierzemy

Jeden plik `router_*.go` na intake, tak jak `router_api.go` i pozostałe trzy,
które już są.

### Zrobione

```
router_api.go        api.<site>                dd_url
router_trace.go      trace.agent.<site>        apm_config.apm_dd_url
router_logs.go       http-intake.logs.<site>   logs_config.logs_dd_url
router_process.go    process.<site>            process_config.process_dd_url
```

### Bierzemy

```
router_kubeops.go    kubeops-intake.<site>     orchestrator_explorer.orchestrator_dd_url
                     /api/v2/orch           zasoby k8s, typy 41..88
                     /api/v2/orchmanif      manifesty
                     /api/v2/kubeactions    akcje na klastrze
```
Ramka ta sama co `/api/v1/collector`, dekoder już mamy — brakuje typów wiadomości.

```
router_containers.go contlcycle-intake.<site>  /api/v2/contlcycle   protobuf
                     contimage-intake.<site>   /api/v2/contimage    protobuf
```
Cykl życia kontenerów i inwentarz obrazów. Naturalne uzupełnienie Kubernetesa.

```
router_dbm.go        dbm-metrics-intake.<site>   szesc trackow, wszystkie JSON
                     /api/v2/dbmmetrics, dbmactivity, databasequery,
                     dbmmetadata, dbmhealth, dbmcolumnstatistics
```

```
router_ndm.go        ndm-intake.<site>         /api/v2/ndm, /api/v2/ndmconfig
                     snmp-traps-intake.<site>  /api/v2/ndmtraps
                     ndmflow-intake.<site>     /api/v2/ndmflow
                     netpath-intake.<site>     /api/v2/netpath
```
Cztery hosty, jeden produkt — urządzenia sieciowe.

```
router_resources.go  resources-intake.<site>   /api/v2/genresources   JSON
router_telemetry.go  instrumentation-telemetry-intake.<site>
```

```
router_config.go     config.<site>   /api/v0.1/configurations   <- KANAL W DOL
                                     /api/v0.1/org
                                     /api/v0.1/status
```
Jedyny, przez który dane płyną DO agenta. Warunek wstępny dla `data-obs`
i dla akcji na klastrze. Podpisywanie TUF-em: `zadania/remote-config-tuf.md`.

### Po remote configu — wtedy wrócić

```
data-obs-intake.<site>   /api/v2/query-actions
```
Monitorowanie jakości danych w bazach. Konfiguracja zapytań przychodzi
**kanałem w dół**, przez remote config — więc bez `router_config.go` ten
endpoint nie ma czego odbierać. Blisko DBM, ale osobny produkt.

### Odrzucone świadomie

```
event-management-intake   zdarzenia systemowe WINDOWS
```
Producenci: `comp/notableevents` i `comp/logonduration`, oba zespołu
`windows-products`. To dziennik zdarzeń Windows i czas logowania użytkownika
po starcie. Przy Linuksie i Kubernetesie bez zastosowania.

```
agentdiscovery-intake     odkrywanie plikow konfiguracyjnych
```
`comp/core/configfilesdiscovery`, prefiks `config_files_discovery.forwarder.`
Agent skanuje maszynę w poszukiwaniu konfiguracji integracji, żeby UI mogło
podpowiedzieć „tu jest Postgres, włącz integrację". Funkcja onboardingowa.

```
sbom-intake         skanowanie podatnosci
http-synthetics     testy syntetyczne
softinv-intake      inwentarz oprogramowania
sds-intake          Sensitive Data Scanner
```
Produkty, za które u Datadoga płaci się osobno i które mają sens przy skali,
gdzie ich cennik jest tańszy niż własny zespół.

## Co da się przekierować, a czego nie

**Pięć** podsystemów ma własny, nadpisywalny URL:

```
dd_url                                        forwarder glowny
apm_config.apm_dd_url                         trace-agent
logs_config.logs_dd_url                       logi
process_config.process_dd_url                 process-agent
orchestrator_explorer.orchestrator_dd_url     Kubernetes
```

Plus trzy niszowe: `apm_config.telemetry`, `evp_proxy_config`,
`multi_region_failover`.

**Cała reszta event platform NIE MA `dd_url`** — tylko `<prefiks>.enabled`
i `<prefiks>.additional_endpoints`, a to dokłada odbiorcę zamiast podmieniać.
Jedyny sposób, żeby je przechwycić, to `DD_SITE` plus wildcard DNS,
bo agent składa host jako `<zaszyty prefiks>.<site>`.

## Podsumowanie

```
obslugujemy       15
nie obslugujemy   ~40
```

Priorytety wśród brakujących, w kolejności ryzyka:

1. **`/api/intake/metrics/v3/*`** — gdy agent się przełączy, stracimy główny
   strumień i dowiemy się o tym tylko z logu, którego nikt nie czyta
2. **`/api/v2/events`, `/api/v2/service_checks`** — nowsze warianty czegoś,
   co już zapisujemy; ta sama cicha awaria
3. **`/api/v2/orch`** — Kubernetes, świadomie odłożony
4. **`/api/v1/query`** — nie dla agenta, tylko dla ekosystemu wokół
5. reszta event platform — produkty, których prawdopodobnie nie chcemy
