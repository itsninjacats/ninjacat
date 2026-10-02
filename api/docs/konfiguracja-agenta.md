# Konfiguracja agenta Datadoga pod ninjacata

Gotowe ustawienia. Rozumowanie stojące za nimi jest w `datadog-agent.md` —
tutaj tylko to, co się wkleja.

## Zasada: jeden serwer, wiele kluczy konfiguracji

Datadog wysyła każdy rodzaj sygnału na **inny host**:

```
api.<site>                     metryki, hosty, checki, szkice, zdarzenia
trace.agent.<site>             slady APM i ich statystyki
http-intake.logs.<site>        logi
process.<site>                 inwentarz procesow
```

U nich to są osobne usługi. U nas to jeden proces i **jedna lista ścieżek** —
nie kolidują ze sobą, więc rozdzielanie po hostach nic by nie dało.

Konsekwencja: każdy podsystem agenta ma własny klucz konfiguracji i wszystkie
trzeba ustawić na ten sam adres.

## Wariant A: jawne adresy (zalecany)

Nie wymaga wildcard DNS ani wildcard certyfikatu.

```yaml
# datadog.yaml
api_key: <klucz z panelu ninjacata>

dd_url: http://ninjacat:8080                 # metryki, /intake/, check_run, sketches, events

apm_config:
  enabled: true
  apm_dd_url: http://ninjacat:8080           # /api/v0.2/traces + /api/v0.2/stats

logs_enabled: true
logs_config:
  logs_dd_url: ninjacat:8080                 # UWAGA: bez schematu, host:port
  use_http: true                             # bez tego logi ida wlasnym TCP+TLS
  logs_no_ssl: true                          # przy http, nie https

process_config:
  process_collection:
    enabled: true
  process_dd_url: http://ninjacat:8080       # /api/v1/collector

orchestrator_explorer:
  enabled: true
  orchestrator_dd_url: http://ninjacat:8080  # /api/v2/orch — zasoby k8s, OSOBNY klucz
```

Jako zmienne środowiskowe (Docker, Kubernetes):

```bash
DD_API_KEY=<klucz>
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

## Wariant B: jeden klucz przez `DD_SITE`

Agent składa adres jako `<zaszyty prefiks>.<site>`. Jeśli `site` wskazuje na
nas, wszystkie pipeline'y trafiają do nas same:

```bash
DD_SITE=ninjacat.firma.pl
```

Zmierzone: daje to `api.ninjacat.firma.pl`, `trace.agent.ninjacat.firma.pl`,
`http-intake.logs.ninjacat.firma.pl` i tak dalej.

Zaleta: **jeden klucz zamiast sześciu**, i obejmuje też pipeline'y, które
własnego `dd_url` nie mają.

Koszt:

- wildcard DNS: `*.ninjacat.firma.pl` → nasz adres
- certyfikat TLS z **dwoma** wpisami SAN: `ninjacat.firma.pl` ORAZ
  `*.ninjacat.firma.pl` — agent uderza i w gołą nazwę, i w prefiksowane

## Który klucz karmi który endpoint

| klucz konfiguracji | nasze endpointy |
|---|---|
| `dd_url` | `/intake/`, `/api/v1/series`, `/api/v2/series`, `/api/v1/check_run`, `/api/v1/metadata`, `/api/beta/sketches`, `/api/v1/events`, `/api/v1/validate` |
| `apm_config.apm_dd_url` | `/api/v0.2/traces`, `/api/v0.2/stats` |
| `logs_config.logs_dd_url` | `/api/v2/logs`, `/v1/input` |
| `process_config.process_dd_url` | `/api/v1/collector` |
| `orchestrator_explorer.orchestrator_dd_url` | `/api/v2/orch`, `/api/v2/orchmanif` |
| *(brak — tylko klient API)* | `/api/v1/distribution_points` |

`dd_url` **nie przekierowuje śladów.** To najczęstsza pomyłka: agent dalej
wysyła je na `trace.agent.datadoghq.com`, bo trace-agent to osobny komponent
z osobnym kluczem. Dokładnie dlatego przez pół pracy nad protokołem nie
zobaczyliśmy ani jednego śladu w przechwytach.

## Co wyłączyć

Podsystemy, których nie da się przekierować, bo mają wyłącznie
`additional_endpoints` (a te **dokładają** odbiorcę, nie podmieniają):

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

## Gwarancja, że nic nie wyjdzie na zewnątrz

Powyższe to konfiguracja, nie gwarancja. Jeśli ma być pewność:

```yaml
# docker-compose
networks:
  lab:
    internal: true        # brak trasy do swiata, DNS nie rozwiazuje
```

Zmierzone: DNS zawodzi, TCP odrzucone, `curl` zwraca `000`. To jedyny sposób
sprawdzony empirycznie, który obejmuje też agent-data-plane ignorujący część
ustawień.

Wariant pośredni — przechwycenie wszystkiego przez proxy:

```bash
DD_PROXY_HTTPS=http://przechwytywacz:8080
```

Łapie **cały** ruch wychodzący, także pipeline'y bez `dd_url`. Tak odkryliśmy
komplet 22 hostów, do których agent próbuje się dobijać.
