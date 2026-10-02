# Datadog Agent — co wysyła, dokąd i jak to okiełznać

Wszystko poniżej **zmierzone na agencie 7.83.2**, nie wzięte z dokumentacji —
Datadog nie publikuje mapowania „który payload leci pod który adres".
Ustalone przez podsłuchanie ruchu własnym odbiornikiem.

---

## 1. Endpointy, w które uderza agent

Zaobserwowane wywołania (liczba z jednej sesji testowej):

| Metoda | Ścieżka | N | Content-Type | Kto wysyła |
|---|---|---|---|---|
| POST | `/api/v2/series` | 35 | `x-protobuf` | core agent + agent-data-plane |
| POST | `/intake/` | 28 | `json` | core agent |
| POST | `/api/v1/check_run` | 25 | `json` | core agent |
| POST | `/api/v1/series` | 14 | `json` | core agent + ADP |
| POST | `/api/beta/sketches` | 9 | `x-protobuf` | core agent |
| POST | `/api/v1/collector` | 8 | `x-protobuf` + własna ramka | process-agent |
| POST | `/api/v1/metadata` | 4 | `json` | core agent |
| HEAD | `/support/flare` | 4 | — | Go-http-client |
| GET | `/api/v1/validate` | 4 | `json` | core agent |
| GET | `/_health` | — | — | Go-http-client |

Do tego, nieobserwowane bo logi były wyłączone, ale potwierdzone w binarce:

| POST | `/api/v2/logs` | logi, format aktualny |
| POST | `/v1/input` | logi, format starszy |

### Co niesie który

- **`/intake/`** — metadane hosta: nazwa, wersja agenta, system, tagi, konfiguracja,
  migawka procesów oraz `gohai` (inwentarz sprzętu jako **zagnieżdżony JSON w stringu**).
  Leci co ~20 s niezależnie od zmian.
- **`/api/v1|v2/series`** — metryki liczbowe. Ta sama treść, dwa formaty.
  v2 = protobuf (domyślny), v1 = JSON (`use_v2_api.series=false`).
- **`/api/v1/check_run`** — statusy checków. `0=OK 1=WARNING 2=CRITICAL 3=UNKNOWN`
  (potwierdzone w `datadog_checks/base/types.py` w obrazie agenta).
- **`/api/beta/sketches`** — metryki typu distribution jako DDSketch.
  Źródło: DogStatsD z sufiksem `|d`.
- **`/api/v1/collector`** — process-agent. Jedyny z własną 16-bajtową ramką
  i jedyny, który oczekuje odpowiedzi w tej samej ramce (`ResCollector`).

### Przemiatanie diagnostyczne

Przy starcie agent wysyła puste ciała (`{}`) na **wszystkie** powyższe ścieżki,
z nagłówkiem `X-Requested-With: datadog-agent-diagnose`. Interesuje go tylko kod
odpowiedzi. Stąd liczniki wyższe niż liczba paczek z danymi.

---

## 2. Dokąd agent próbuje wyjść na świat

Zmierzone przez ustawienie `DD_PROXY_HTTPS` na własny adres i przechwycenie
żądań `CONNECT`. **22 różne hosty w jednej sesji.**

### Ruch podstawowy — zawsze obecny

| Host | Co przez niego leci | Czym sterować |
|---|---|---|
| `7-83-2-app.agent.datadoghq.com` | metryki, `/intake/`, check_runy, sketches. Poddomena zawiera **wersję agenta** (7.83.2) | `DD_DD_URL` |
| `app.datadoghq.com` | walidacja klucza API (`/api/v1/validate`) | `DD_DD_URL` |
| `1-5-2-adp.agent.datadoghq.com` | **agent-data-plane** — osobny proces w Ruście, własna wersja w poddomenie (1.5.2) | tylko proxy |
| `config.datadoghq.com` | **remote configuration — kanał W DÓŁ.** Zmiany konfiguracji z chmury, w k8s także akcje na klastrze | `DD_REMOTE_CONFIGURATION_ENABLED` |
| `instrumentation-telemetry-intake` | telemetria samego agenta | `DD_TELEMETRY_ENABLED` |
| `trace.agent.datadoghq.com` | ślady APM oraz Data Streams Monitoring | `DD_APM_DD_URL` |
| `7-83-2-flare.agent.datadoghq.com` | paczki diagnostyczne — tylko przy ręcznym `datadog-agent flare` | — |

### Pipeline'y produktowe — odzywają się po włączeniu funkcji

| Host | Produkt | Flaga z §4 |
|---|---|---|
| `contlcycle-intake` | Live Containers — zdarzenia cyklu życia | `DD_CONTAINER_LIFECYCLE_ENABLED` |
| `contimage-intake` | inwentarz obrazów kontenerów | `DD_CONTAINER_IMAGE_ENABLED` |
| `sbom-intake` | SBOM, skanowanie podatności | `DD_SBOM_ENABLED` |
| `dbm-metrics-intake` | Database Monitoring (sześć typów zdarzeń) | `database_monitoring.*` |
| `ndm-intake` | Network Devices — metadane urządzeń | `network_devices.metadata` |
| `ndmflow-intake` | netflow | `network_devices.netflow` |
| `snmp-traps-intake` | trapy SNMP | `network_devices.snmp_traps` |
| `netpath-intake` | Network Path — traceroute'y | `DD_NETWORK_PATH_ENABLED` |
| `resources-intake` | zasoby ogólne | — |
| `llmobs-intake` | LLM Observability | — |
| `http-synthetics` | testy Synthetics | — |
| `agentdiscovery-intake` | Service Discovery | `DD_DISCOVERY_ENABLED` |
| `kubeops-intake` | orchestrator explorer — manifesty k8s (§7) | `DD_ORCHESTRATOR_EXPLORER_ENABLED` |
| `data-obs-intake` | Data Observability | — |
| `event-management-intake` | zarządzanie zdarzeniami | — |

### Niezwiązane z konfiguracją agenta

```
yum.datadoghq.com       apt.datadoghq.com
keys.datadoghq.com      install.datadoghq.com
```

Repozytoria pakietów — instalator i aktualizacje. Żadna flaga ich nie dotyczy.

### Jak powstaje adres — i jak to wykorzystać

Wzorzec z binarki: `https://%s.%s` = **zaszyty prefiks + `DD_SITE`**.

Prefiksów (`contlcycle-intake`, `sbom-intake`…) ani schematu `https://`
nie podmienisz — są w kodzie. **Ale `DD_SITE` może wskazywać na Ciebie.**

Zmierzone: `DD_SITE=ninjacat.ninjacats.eu` daje

```
resources-intake.ninjacat.ninjacats.eu
contlcycle-intake.ninjacat.ninjacats.eu
sbom-intake.ninjacat.ninjacats.eu
trace.agent.ninjacat.ninjacats.eu
7-83-2-app.agent.ninjacat.ninjacats.eu
```

Czyli **`DD_SITE` jest trzecią drogą** — obok `dd_url` i proxy — i jako jedyna
z konfiguracyjnych obejmuje pipeline'y, które `dd_url` nie mają. Kierujesz DNS
na siebie, a strumienie rozróżniasz po nagłówku `Host`.

**Haczyk z certyfikatem.** Nazwy mają różną głębokość:

```
resources-intake.ninjacat.ninjacats.eu     <- jeden poziom
trace.agent.ninjacat.ninjacats.eu          <- dwa poziomy
7-83-2-app.agent.ninjacat.ninjacats.eu     <- dwa poziomy
```

Wildcard TLS obejmuje **tylko jeden poziom**, więc `*.ninjacat.ninjacats.eu`
nie pokryje `trace.agent....`. Potrzebny certyfikat z dwoma wpisami SAN:

```
*.ninjacat.ninjacats.eu
*.agent.ninjacat.ninjacats.eu
```

Let's Encrypt zrobi to przez wyzwanie DNS-01.

**Poddomeny z wersją** (`7-83-2-app`, `1-5-2-adp`) powstają z wersji binarki,
więc zmienią się przy każdej aktualizacji agenta — kolejny powód, żeby nie
opierać blokowania na liście nazw hostów.

### Skąd to mapowanie

Agent przy starcie sam wypisuje, który typ danych leci pod jaki host:

```
newHTTPPassthroughPipeline | Initialized event platform forwarder pipeline.
  eventType=container-lifecycle  mainHosts=contlcycle-intake.datadoghq.com
  eventType=dbm-samples          mainHosts=dbm-metrics-intake.datadoghq.com
  eventType=network-path         mainHosts=netpath-intake.datadoghq.com
  ...
```

Wystarczy `DD_LOG_LEVEL=debug` przy starcie, żeby zobaczyć pełną listę
dla swojej wersji agenta.

## 3. Co da się przekierować

Pięć podsystemów ma własny, nadpisywalny URL:

```bash
DD_DD_URL=http://ninjacat:8080                      # metryki, /intake/, check_run, sketches
DD_PROCESS_CONFIG_PROCESS_DD_URL=http://ninjacat:8080  # process-agent
DD_APM_DD_URL=http://ninjacat:8080                  # ślady APM
DD_LOGS_CONFIG_LOGS_DD_URL=ninjacat:8080            # logi
DD_LOGS_CONFIG_USE_HTTP=true
DD_LOGS_CONFIG_LOGS_NO_SSL=true
DD_ORCHESTRATOR_EXPLORER_ORCHESTRATOR_DD_URL=http://ninjacat:8080  # zasoby k8s
```

Plus trzy niszowe z własnym `dd_url`: `apm_config.telemetry`,
`evp_proxy_config`, `multi_region_failover`.

### Czego przekierować się NIE DA

Sprawdzone w binarce: pipeline'y event platform mają **wyłącznie**
`<prefiks>.additional_endpoints` i `<prefiks>.enabled`. **Żadnego `dd_url`.**

```
container_lifecycle   container_image   sbom
database_monitoring.metrics|samples|activity
network_devices.metadata|snmp_traps|netflow
network_path.forwarder    service_discovery.forwarder
runtime_security_config.endpoints
```

`additional_endpoints` **dokłada** odbiorców, nie podmienia — wysyłka pójdzie
do Ciebie **i** do Datadoga. Czyli do przechwytywania bezużyteczne.

---

## 4. Wyłączanie tego, czego nie da się przekierować

Każda flaga z opisem, co realnie gasi i dokąd ten strumień by poleciał.
Kolumna „intake" pochodzi z **logów startowych samego agenta** — przy starcie
wypisuje on mapowanie typu danych na host (`newHTTPPassthroughPipeline ...
eventType=X mainHosts=Y`), więc to nie jest domysł.

### Pipeline'y event platform

| Flaga | Co gasi | Intake |
|---|---|---|
| `DD_CONTAINER_LIFECYCLE_ENABLED=false` | Live Containers — zdarzenia start/stop kontenerów, podów i tasków ECS wraz z kodem wyjścia | `contlcycle-intake` |
| `DD_CONTAINER_IMAGE_ENABLED=false` | inwentarz obrazów: registry, tagi, digest, rozmiar, warstwy, historia budowania | `contimage-intake` |
| `DD_SBOM_ENABLED=false` | Software Bill of Materials — lista pakietów i bibliotek w formacie CycloneDX, podstawa skanowania podatności | `sbom-intake` |
| `DD_SBOM_CONTAINER_IMAGE_ENABLED=false` | SBOM liczony dla obrazów kontenerów | `sbom-intake` |
| `DD_SBOM_HOST_ENABLED=false` | SBOM liczony dla systemu hosta | `sbom-intake` |
| `DD_ORCHESTRATOR_EXPLORER_ENABLED=false` | manifesty zasobów Kubernetes — pody, deploymenty, nody, serwisy (patrz §8) | `kubeops-intake` |
| `DD_DISCOVERY_ENABLED=false` | Service Discovery — automatyczne wykrywanie usług działających na hoście | `agentdiscovery-intake` |
| `DD_NETWORK_PATH_ENABLED=false` | Network Path — traceroute'y między hostami, mapa tras sieciowych | `netpath-intake` |
| `DD_COMPLIANCE_CONFIG_ENABLED=false` | CSPM — benchmarki CIS, audyt konfiguracji pod kątem zgodności | nieobserwowany |
| `DD_RUNTIME_SECURITY_CONFIG_ENABLED=false` | CWS — wykrywanie zagrożeń w czasie rzeczywistym przez eBPF: exec, otwarcia plików, wywołania systemowe | nieobserwowany |
| `DD_SERVICE_MONITORING_CONFIG_ENABLED=false` | USM — analiza ruchu między usługami (wymaga system-probe) | przez system-probe |
| `DD_GPU_MONITORING_ENABLED=false` | metryki kart graficznych | zwykłe metryki |

Nie mieliśmy ich włączonych, ale te pipeline'y istnieją i mają własne intake'y —
warto wiedzieć, że są, gdyby kiedyś ktoś je włączył:

```
dbm-metrics-intake       Database Monitoring: probki zapytan, plany, aktywnosc sesji,
                         metadane, zdrowie, statystyki kolumn (szesc typow zdarzen)
ndm-intake               Network Devices Monitoring: metadane urzadzen + ndmconfig
ndmflow-intake           netflow
snmp-traps-intake        trapy SNMP
resources-intake         zasoby ogolne (genresources)
http-synthetics          testy Synthetics
llmobs-intake            LLM Observability
data-obs-intake          Data Observability (do-query-results)
event-management-intake  zarzadzanie zdarzeniami
trace.agent              Data Streams Monitoring (ten sam host co APM)
```

### Kanał sterujący — to jest ważne

| Flaga | Co gasi | Host |
|---|---|---|
| `DD_REMOTE_CONFIGURATION_ENABLED=false` | **kanał W DÓŁ** — zmiany konfiguracji przysyłane z chmury, a w Kubernetesie także akcje `delete_pod` / `restart_deployment` (patrz §8) | `config.datadoghq.com` |
| `DD_INVENTORIES_ENABLED=false` | raportowanie konfiguracji agenta i listy integracji | przez `/intake/` |
| `DD_TELEMETRY_ENABLED=false` | telemetria samego agenta wysyłana do Datadoga | `instrumentation-telemetry-intake` |
| `DD_MULTI_REGION_FAILOVER_ENABLED=false` | przełączanie na zapasowy region Datadoga | własny `dd_url` |
| `DD_EVP_PROXY_CONFIG_ENABLED=false` | proxy dla bibliotek śledzących — agent pośredniczy w ich ruchu do Datadoga | własny `dd_url` |

**`DD_REMOTE_CONFIGURATION_ENABLED` to ta, na której zależy najbardziej.**
Bez niej nie ma kanału, którym Datadog wysyła polecenia do Twojej infrastruktury.

### Całe podsystemy

| Flaga | Co gasi |
|---|---|
| `DD_APM_ENABLED=false` | trace-agent — zbieranie śladów aplikacji, port 8126 |
| `DD_LOGS_ENABLED=false` | zbieranie logów (domyślnie i tak wyłączone) |
| `DD_PROCESS_CONFIG_PROCESS_COLLECTION_ENABLED=false` | inwentarz procesów na `/api/v1/collector` — **razem z tym znika legacy zstd**, który psuł nam dekodowanie |
| `DD_PROCESS_CONFIG_CONTAINER_COLLECTION_ENABLED=false` | inwentarz kontenerów tą samą drogą |
| `DD_PROCESS_CONFIG_PROCESS_DISCOVERY_ENABLED=false` | lekkie wykrywanie procesów bez pełnych metryk |
| `DD_SYSTEM_PROBE_CONFIG_ENABLED=false` | system-probe — moduł eBPF pod ruch sieciowy, USM i CWS |
| `DD_OTELCOLLECTOR_ENABLED=false` | wbudowany kolektor OpenTelemetry |
| `DD_PROMETHEUS_SCRAPE_ENABLED=false` | automatyczne zbieranie metryk z endpointów Prometheusa |

### Czego nie wyłączysz

**`agent-data-plane`** nie ma flagi wyłączającej proces. Jest
`data_plane.dogstatsd.enabled`, ale to tylko przełącznik, kto obsługuje port 8125 —
ADP czy stary pipeline w Go. Proces i tak wstanie i i tak wyśle swoją sondę
`n_o_i_n_d_e_x.datadog.agent.data_plane.preflight_mode.probe` na
`1-5-2-adp.agent.datadoghq.com`.

**Repozytoria pakietów** (`yum`, `apt`, `keys`, `install`.datadoghq.com) nie mają
z konfiguracją agenta nic wspólnego — to instalator i aktualizacje.

**Flare** (`7-83-2-flare.agent`) odzywa się tylko przy ręcznym `datadog-agent flare`.

## 5. Jak naprawdę zagwarantować, że nic nie wyjdzie

Konfiguracja **nie daje gwarancji** — to osiemnaście flag do rewidowania przy
każdej wersji agenta, a nowy pipeline pojawi się bez pytania.

Dwa sposoby, oba sprawdzone:

### Proxy — przechwytuje WSZYSTKO

```bash
DD_PROXY_HTTPS=http://twoj-host:8080
DD_PROXY_HTTP=http://twoj-host:8080
```

Zmierzone: przez proxy przeszły **wszystkie 22 hosty**, łącznie z pipeline'ami
bez `dd_url` i z agent-data-plane, który ignorował inne ustawienia.

Wymaga obsługi metody **`CONNECT`** po Twojej stronie — agent wysyła
`CONNECT host:443` z adresem w formie *authority* (`URL.Path` jest **pusty**,
więc router po ścieżce tego nie złapie; Gin zwraca 404).

### Sieć odcięta — gwarancja twarda

```bash
docker network create --internal ninjacat-lab
```

Zmierzone z wnętrza kontenera w takiej sieci:

```
DNS api.datadoghq.com:     BRAK
TCP 443 do Datadoga:       ODCIETE
curl https://...:          000
host.docker.internal:      NIEWIDOCZNY
```

Kontener fizycznie nie ma trasy na zewnątrz — niezależnie od tego, co agent
sobie wymyśli. Kosztem jest to, że odbiornik musi siedzieć w tej samej sieci
jako kontener.

**Zalecenie:** oba naraz. Sieć jako bezpiecznik, proxy jako to, co zbiera dane.
Flagi wtedy tylko wyciszają szum, a nie odpowiadają za bezpieczeństwo.

---

## 6. Do zrobienia po naszej stronie

### Weryfikacja klucza API — obowiązkowa przed wystawieniem na świat

Agent wysyła klucz w nagłówku **`Dd-Api-Key`** przy każdym żądaniu
(a nie w query stringu). Bez sprawdzania tego każdy może wpychać dowolne
metryki pod dowolny hostname.

Punkty, które to muszą egzekwować:

- wszystkie `POST` z danymi (`/intake/`, `series`, `check_run`, `sketches`, `collector`, `logs`)
- `GET /api/v1/validate` — **to on decyduje, czy agent uzna klucz za poprawny**;
  `403` zatrzymuje całą wysyłkę, `200` ją przepuszcza

Klucz powinien mapować się na tenanta — patrz `tenant_id` w schemacie ClickHouse.

### Obsługa CONNECT

Jeśli ninjacat ma działać jako proxy (patrz §5), potrzebuje:
odpowiedzieć `200 Connection Established`, przejąć połączenie przez
`http.Hijacker`, a dalej albo terminować TLS własnym certyfikatem
(widać treść), albo przepuszczać bajty i logować same adresy.

---

## 7. SDK i biblioteki śledzące — co wysyłają przez agenta

Wszystko poniżej **wyciągnięte z binarki `trace-agent`** w obrazie 7.83.2.
To są ścieżki, które agent **wystawia lokalnie** (domyślnie port 8126) dla
bibliotek działających w Twoich aplikacjach.

### Endpointy wystawiane dla SDK

| Ścieżka | Co przyjmuje |
|---|---|
| `/v0.3/traces` `/v0.4/traces` `/v0.5/traces` `/v0.7/traces` | ślady — kolejne wersje formatu, `v0.4` najpowszechniejsza |
| `/v0.2/stats` `/v0.6/stats` | statystyki policzone po stronie klienta (agregaty span'ów) |
| `/v0.7/config` | **remote config DLA biblioteki** — Datadog zdalnie zmienia sampling, włącza profiler, dodaje instrumentację |
| `/profiling/v1/input` | profiler ciągły: CPU, pamięć, blokady |
| `/debugger/v1/input` `/debugger/v2/input` | Dynamic Instrumentation |
| `/debugger/v1/diagnostics` | diagnostyka DI |
| `/symdb/v1/input` | Symbol Database |
| `/telemetry/proxy/api/v...` | telemetria samej biblioteki śledzącej |
| `/dogstatsd/v1/proxy` `/dogstatsd/v2/proxy` | metryki z aplikacji przekazywane przez agenta |
| `/evp_proxy/v1/` … `/v4/` | **generyczne proxy** — patrz niżej |

### Produkty kryjące się za mniej oczywistymi nazwami

**Dynamic Instrumentation** — dodawanie „logpointów" do **działającej** aplikacji
bez restartu i bez zmiany kodu. W UI klikasz linijkę, mówisz „loguj tu wartość
zmiennej", biblioteka wstrzykuje to w locie.

**Symbol Database** — SDK odsyła mapę symboli kodu (klasy, metody, zmienne),
żeby UI wiedziało, gdzie te logpointy da się wstawić.

**`/v0.7/config`** — to remote config nie dla agenta, tylko dla biblioteki
w Twojej aplikacji. Osobny kanał sterujący, osobny od `config.datadoghq.com`.

### `evp_proxy` — worek bez dna

```
/evp_proxy/v1/  /v2/  /v3/  /v4/
```

EVP = Event Platform. SDK wysyła cokolwiek pod tę ścieżkę i podaje docelowy
podhost w nagłówku, a agent forwarduje **nie znając formatu**.

Dzięki temu nowe produkty Datadoga nie wymagają aktualizacji agenta — dokładają
ścieżkę po stronie SDK i backendu.

Dla nas: **nie da się tego „zaimplementować w całości"**, bo nie ma
zdefiniowanej zawartości. Sensowna odpowiedź to `200` plus zalogowanie,
co przyszło.

### RUM idzie inaczej — z pominięciem agenta

Szukaliśmy w binarkach `browser-intake` i `session-replay` — **nie ma ich**.

RUM (przeglądarkowy i mobilny) strzela **bezpośrednio do Datadoga**, z kluczem
klienckim osadzonym w kodzie strony. To logiczne: agent siedzi w serwerowni,
a przeglądarka użytkownika jest w internecie.

Konsekwencja: RUM to **osobny projekt**, nie rozszerzenie tego, co budujemy.
Wymagałby publicznego endpointu i implementacji formatu `browser-sdk`
(otwarty, więc do odczytania) oraz innego modelu bezpieczeństwa —
klucz kliencki jest z definicji publiczny.

### Kolejność wdrażania, gdyby kiedyś

```
1. /v0.4/traces          najczesciej uzywana wersja, dobry punkt startu
2. /v0.6/stats           bez tego UI nie pokaze statystyk
3. /telemetry/proxy      bezpiecznie zignorowac — odpowiedz 200
4. /profiling /debugger /symdb   osobne produkty, pomijalne
5. evp_proxy             odpowiadaj 200 i loguj
6. RUM                   osobny projekt
```

### Co jest otwarte, a co nie

Zweryfikowane: `LICENSE` w obrazie mówi `agent 7.83.2 license: "Apache-2.0"`,
a wśród zależności nie ma komponentów Datadoga na licencji komercyjnej.

Wzorzec jest konsekwentny — **otwarte jest to, co działa u klienta**:

```
datadog-agent      agent, process-agent, trace-agent, security-agent
agent-payload      definicje protobuf
integrations-core  251 integracji w Pythonie (leza w obrazie jako zrodlo)
dd-trace-*         biblioteki sledzace
browser-sdk        RUM przegladarkowy
dd-sdk-ios/android RUM mobilny
datadog-operator   operator k8s
```

**Zamknięta jest cała platforma**: przyjmowanie danych, magazyn, silnik zapytań,
UI, alerty, implementacje wykrywania anomalii.

Powód nie jest ideowy: kod agenta działa na cudzych maszynach, w bankach
i szpitalach, z dostępem do produkcji i eBPF. Zamknięta binarka nie przeszłaby
audytu. Otwartość jest warunkiem sprzedaży.

Stąd asymetria widoczna w całym tym dokumencie: **kod otwarty, protokół
nieudokumentowany**. Otwierają to, co muszą, nie to, co ułatwiłoby konkurencję.

*(Licencję agenta sprawdziliśmy w obrazie. Lista pozostałych repozytoriów
pochodzi z wiedzy, nie z weryfikacji — warto zerknąć do repo, gdyby miało
to znaczenie dla decyzji.)*

---

## 8. Kubernetes — osobna historia

Ustalenia z sesji, przydatne gdy ninjacat ma kiedyś obsłużyć klaster.

### Trzy niezależne strumienie

| Co | Skąd | Dokąd | Nasz status |
|---|---|---|---|
| `kubernetes.*` — CPU, RAM, sieć, dyski | kubelet, agent na node'cie | `/api/v2/series` | **obsłużone** |
| `kubernetes_state.*` — stany podów, replik | **Cluster Agent** | `/api/v2/series` | obsłużone tym samym kodem |
| manifesty zasobów (specyfikacje podów) | orchestrator explorer | `kubeops-intake` | **brak** |

Dwa z trzech to zwykłe metryki i wpadają w istniejący handler bez zmian.
Osobnej roboty wymaga tylko orchestrator explorer.

### Nazewnictwo metryk i tagów

Namespace `kubernetes` jest ustawiany w `kubelet.py:182` (`self.NAMESPACE = 'kubernetes'`),
a nazwy powstają jako `kubernetes.` + reszta. Domyślnie zbierane:

```python
DEFAULT_ENABLED_GAUGES = ['memory.cache', 'memory.usage', 'memory.swap',
                          'memory.working_set', 'memory.rss', 'filesystem.usage']
DEFAULT_POD_LEVEL_METRICS = ['network.*']
```

plus budowane dynamicznie `kubernetes.cpu.capacity`, `kubernetes.memory.capacity`,
`kubernetes.{cpu,memory}.{requests,limits}`, `kubernetes.containers.<metryka>.<stan>`.

Tagi — wyciągnięte z binarki agenta:

```
kube_cluster_name   kube_namespace     kube_deployment
kube_replica_set    kube_daemon_set    kube_stateful_set
kube_service        kube_container_name kube_job
```

**Uwaga:** tag to `kube_deployment`, nie `deployment_name`.

Metryki typu `kubernetes.pod.up` **nie istnieją**. Stany podów przychodzą
z checku `kubernetes_state` jako `kubernetes_state.pod.status_phase` i pokrewne —
a to jest check **Cluster Agenta**, nie node'owego, bo wymaga dostępu do API serwera.

### Typy zasobów w process-agencie

Ramka process-agenta (`/api/v1/collector`) niesie nie tylko procesy.
Pełna lista typów z `agent-payload/process/message.go`:

```
12  Proc                     41  Pod                  55  RoleBinding
22  Connections              42  ReplicaSet           56  ClusterRole
23  ResCollector (ODPOWIEDZ) 43  Deployment           57  ClusterRoleBinding
27  RealTime                 44  Service              58  ServiceAccount
39  Container                45  Node                 59  Ingress
40  ContainerRealTime        46  Cluster              60  ProcEvent
53  ProcDiscovery            47  Job                  61  Namespace
                             48  CronJob              80  Manifest
                             49  DaemonSet            81  ManifestCRD
                             50  StatefulSet          82  ManifestCR
                             51  PersistentVolume     83  VerticalPodAutoscaler
                             52  PersistentVolumeClaim 84 HorizontalPodAutoscaler
                             54  Role                 85  NetworkPolicy
                                                      86  LimitRange
                                                      87  StorageClass
                                                      88  PodDisruptionBudget
                                                      200 ECSTask
```

Czyli rozpoznanie typu z nagłówka ramki mówi, co przyszło. Odpowiadamy zawsze
typem **23 (`ResCollector`)** — implementacja w `apiserver/api_server.go`,
funkcja `resCollectorResponse()`.

### Dwukierunkowość — Datadog wysyła polecenia DO klastra

To nie jest tylko zbieranie danych. `agent-payload/proto/kubeactions/kubeactions.proto`
definiuje akcje idące **w dół**:

```protobuf
oneof action {
    DeletePodParams          delete_pod          = 10;
    RestartDeploymentParams  restart_deployment  = 11;
    PatchDeploymentParams    patch_deployment    = 12;
    GetResourceParams        get_resource        = 13;
    RollbackDeploymentParams rollback_deployment = 14;
    PatchDaemonSetParams     patch_daemonset     = 15;
    PatchStatefulSetParams   patch_statefulset   = 16;
}
```

Kanałem jest **remote configuration** (`config.datadoghq.com`) — ten sam,
którym lecą zmiany konfiguracji. Komentarz w proto potwierdza wprost:
`ActionID ... is different from the RC metadata.id which is tied to the RC config lifecycle`.

Pola audytowe: `action_id`, `timestamp` oraz `requested_by` (e-mail zlecającego,
wymagany „for audit trail").

**Konsekwencja bezpieczeństwa:** weryfikacja tego e-maila odbywa się po stronie
Datadoga. Agent w klastrze dostaje protobuf z polem tekstowym i wykonuje.
Kto przejmie konto albo wejdzie w ten kanał, może kasować pody i cofać deploymenty.
Dlatego `DD_REMOTE_CONFIGURATION_ENABLED=false` w §4 jest istotne.

### Autoskalowanie — backend decyduje, ile replik

`agent-payload/proto/autoscaling/kubernetes/recommender.proto`:

```protobuf
message WorkloadRecommendationReply {
  int32 targetReplicas = 3;          // ile replik ma byc
  optional int32 lowerBoundReplicas = 4;
  optional int32 upperBoundReplicas = 5;
}
message WorkloadRecommendationConstraints {
  int32 minReplicas = 1;
  int32 maxReplicas = 2;
}
```

Klaster pyta, chmura odpowiada liczbą. Sensowne, bo dobre skalowanie wymaga
historii z tygodni i korelacji między serwisami — czego klaster u siebie nie ma.

Te payloady, w odróżnieniu od intake'owych, jadą **JSON-em przez remote config
i są walidowane schematem**. Stąd w repo katalog `jsonschema/` z dokładnie
czterema plikami, wszystkimi od autoskalowania:

```
WorkloadRecommendationsRequest.json    ClusterAutoscalingValuesList.json
WorkloadRecommendationsReply.json      WorkloadValuesList.json
```

I stąd adnotacje `(protoc.gen.jsonschema.field_options).required` w tych protos —
mają podwójne życie: definicja w proto, transport JSON-owy.

### Cluster Agent to osobny byt

Obraz `datadog/agent:7` zawiera siedem binarek:

```
agent  agent-data-plane  process-agent  security-agent
system-probe  system-probe-lite  trace-agent
```

**Cluster Agenta wśród nich nie ma** — to osobny obraz (`datadog/cluster-agent`)
i osobna binarka. Operator Datadoga stawia obraz node'owy jako DaemonSet na
każdym węźle (dokładnie ten, który badaliśmy), a Cluster Agenta jako osobny Deployment.

Dobra wiadomość: jeśli ninjacat obsłuży ten agent, obsłuży każdy node w klastrze
postawionym operatorem. Cluster Agent wymagałby osobnego rozpoznania —
i to on odpowiada za orchestrator explorer, `kubernetes_state.*` oraz
za pośredniczenie w remote configu (`/datadog.api.v1.AgentSecure/ClientGetConfigs`).

### gRPC wewnętrzne

Agent ma gRPC, ale **wyłącznie do rozmowy własnych procesów** po lokalnym gnieździe —
to nie jest ruch do chmury. Z binarki:

```
/datadog.api.v1.Agent/GetHostname
/datadog.api.v1.AgentSecure/ClientGetConfigs        <- remote config
/datadog.api.v1.AgentSecure/StreamKubeMetadata      <- metadane k8s
/datadog.api.v1.AgentSecure/TaggerStreamEntities    <- strumien tagow
/datadog.procmgr.ProcessManager/ListSBOM
/datadog.remoteagent.flare.v1.FlareProvider/GetFlareFiles
```

Te protos leżą w repo `datadog-agent` (`pkg/proto`), **nie** w `agent-payload`.

---

## Uwaga metodologiczna

W całym `agent-payload` jest **387 wiadomości i 1 usługa gRPC**
(`StatefulLogsService`, eksperymentalne kodowanie logów ze słownikiem).

Czyli protos opisują **kształt danych, nie API**. Mapowanie „ten payload leci
pod ten URL" nie istnieje w publicznych źródłach — trzeba je zdobyć obserwacją.
Stąd ten dokument.
