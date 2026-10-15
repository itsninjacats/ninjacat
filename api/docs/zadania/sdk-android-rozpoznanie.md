# dd-sdk-android — rozpoznanie

**Rozpoznanie 2026-09-20. Zero kodu.** To samo, co `browser-sdk-rozpoznanie.md`,
tylko dla `DataDog/dd-sdk-android` (Android, Android TV, Wear, Automotive —
jeden SDK). Nacisk na **różnice wobec przeglądarki**, bo hipoteza była taka,
że większość dziwactw browser-sdk to ograniczenia przeglądarki, a nie decyzje
Datadoga. Hipoteza się potwierdziła — szczegóły w ostatniej sekcji.

Klon: `scratchpad/dd/dd-sdk-android`, commit `65614b6` (2026-09-17), wersja
**3.15.0-SNAPSHOT** (`build-logic/src/main/kotlin/com/datadog/gradle/config/AndroidConfig.kt:22`);
ostatnie wydanie 3.14.0 z 2026-09-09 (`CHANGELOG.md:1`). Do porównania
schematów użyłem `scratchpad/dd/rum-events-format` (master, `ab3e7c5`,
2026-09-08 — ten sam commit, który przypina `browser-sdk/package.json:49`).
Ścieżki niżej są względem korzenia klonu. Nasze pliki — względem `server/`.

Skrót dla niecierpliwych: **da się przekierować** (`useCustomEndpoint`, pełny
URL, per kanał), **klucz idzie nagłówkiem** `DD-API-KEY`, **ten sam schemat
RUM** z `rum-events-format` (Android trzyma snapshot JSON-ów i generuje z nich
Kotlin przy buildzie).

---

## 1. Czy da się zmusić SDK, żeby wysyłał do nas

**Tak, bez forka, prościej niż w przeglądarce.** Nie ma `proxy`, nie ma
`ddforward`, nie ma regexu na domenę.

### `site` — zamknięty enum, nie string

`DatadogSite` (`dd-sdk-android-core/src/main/kotlin/com/datadog/android/DatadogSite.kt:19-69`)
to enum z dziesięcioma wartościami; host powstaje z nazwy
(`:76-79`: `browser-intake-$siteName-datadoghq.com`, kilka wyjątków wpisanych
ręcznie), a `intakeEndpoint = "https://$intakeHostName"` (`:82`). `useSite`
przyjmuje tylko ten enum (`core/configuration/Configuration.kt:178-181`).
Regexu jak w przeglądarce nie ma, bo nie ma czego walidować — stringa nie da
się podać. `site` nie nadaje się do przekierowania, ale nie musi.

### `useCustomEndpoint` — to jest właściwa droga

Każdy kanał ma własne `useCustomEndpoint(endpoint: String)` i dokumentuje to
jako *„the full endpoint url, e.g.: https://example.com/rum/upload"*:

| kanał | setter | pole |
|---|---|---|
| RUM | `features/dd-sdk-android-rum/src/main/kotlin/com/datadog/android/rum/RumConfiguration.kt:306-309` | `customEndpointUrl` |
| Logs | `features/dd-sdk-android-logs/src/main/kotlin/com/datadog/android/log/LogsConfiguration.kt:33-36` | `customEndpointUrl` |
| Trace | `features/dd-sdk-android-trace/src/main/kotlin/com/datadog/android/trace/TraceConfiguration.kt:38-41` | `customEndpointUrl` |
| Trace stats | `TraceConfiguration.kt:83-86` `useCustomStatsEndpoint` | `customStatsEndpointUrl` |
| Session Replay (segmenty i zasoby) | `features/dd-sdk-android-session-replay/src/main/kotlin/com/datadog/android/sessionreplay/SessionReplayConfiguration.kt:107-110` | `customEndpointUrl` |
| Profiling | `features/dd-sdk-android-profiling/src/main/java/com/datadog/android/profiling/ProfilingConfiguration.kt:64-67` | `customEndpointUrl` |
| Flags | `features/dd-sdk-android-flags/src/main/kotlin/com/datadog/android/flags/FlagsConfiguration.kt:18-19` | `customExposureEndpoint`, `customEvaluationEndpoint` |

**Żadnej walidacji stringa.** Każda fabryka żądań robi to samo:

```kotlin
val intakeUrl = customEndpointUrl ?: (context.site.intakeEndpoint + "/api/v2/rum")   // RumRequestFactory.kt:67
```

czyli własny URL **zastępuje host razem ze ścieżką**. Potem dokleja się query
(tylko RUM i logi, §2). Z `useCustomEndpoint("https://android-intake.ninjacat.pl/api/v2/rum")`
dociera dokładnie:

```
POST https://android-intake.ninjacat.pl/api/v2/rum?ddsource=android
```

Datadog sam tak kieruje SDK na `MockWebServer` w testach instrumentowanych:
`instrumented/integration/src/main/kotlin/com/datadog/android/sdk/integration/RuntimeConfig.kt:66-101`
(cztery buildery, cztery `useCustomEndpoint`). To gotowy wzorzec dla naszej
dokumentacji użytkownika. Ścieżkę wybieramy sami, więc wystarczy trzymać
natywną (`/api/v2/<track>`), żeby jeden handler obsłużył Androida i
ewentualnie przeglądarkę przez `proxy`-funkcję.

### Pułapka: tylko HTTPS z zaufanym certyfikatem

Współdzielony `OkHttpClient` (`core/internal/CoreFeature.kt:134-146`) ma
`ConnectionSpec.RESTRICTED_TLS` (TLS 1.2/1.3, wąska lista szyfrów) i
`Protocol.HTTP_2, HTTP_1_1`. Cleartext HTTP włącza tylko
`needsClearTextHttp` (`:662-665`), a ten ustawia wyłącznie
`internal fun allowClearTextHttp()` (`Configuration.kt:315-320`), wystawiony
przez `_InternalProxy.allowClearTextHttp` (`_InternalProxy.kt:98-100`) z
komentarzem *„DO NOT USE … subject to change without notice"* (`:21-27`).
Technicznie wywoływalne z aplikacji, ale to nie jest droga do dokumentowania.
`useSite` dodatkowo zeruje tę flagę (`Configuration.kt:179`).

Wniosek: nasz intake dla Androida **musi** mieć certyfikat, któremu ufa
systemowy magazyn Androida (publiczne CA), albo aplikacja musi mieć własny
`network_security_config` z naszym CA. **Do sprawdzenia:** przecięcie
`RESTRICTED_TLS` z domyślnymi szyframi Go `crypto/tls` — na papierze
ECDHE+AES-GCM/ChaCha20 jest po obu stronach, ale to trzeba zobaczyć w
handshake'u.

### Co custom endpoint NIE przekierowuje

Remote config: `DatadogSite.remoteConfigurationEndpoint =
https://sdk-configuration.<host intake'u>` (`DatadogSite.kt:86-87`),
`GET …/v1/<remoteConfigurationId>.json`
(`core/internal/remote/RemoteConfigService.kt:83-85`, `:317`), bez żadnego
nagłówka uwierzytelniającego (`RemoteConfigFetcher.kt:76-84` — goły `GET`).
Działa tylko po `setRemoteConfigurationId` (`Configuration.kt:310-313`), więc
domyślnie tego ruchu nie ma. Tak samo jak w przeglądarce — osobna sprawa.

### Inne mechanizmy, odnotowane

- `Configuration.Core.proxy: java.net.Proxy` + `proxyAuth`
  (`Configuration.kt:41-42`, `CoreFeature.kt:676-680`) — zwykły proxy HTTP
  na poziomie OkHttp. Inny cel niż nasz; działa razem z custom endpointem.
- `additionalConfig["_dd.source"]` i `["_dd.sdk_version"]`
  (`Datadog.kt:455-456`, `core/internal/DatadogCore.kt:579-589`) — nadpisują
  `source` i wersję. Tego używają wrappery (Flutter, React Native, KMP), więc
  z tego samego SDK przyjdzie `ddsource=flutter` i `DD-EVP-ORIGIN: flutter`.

---

## 2. Kanały, hosty, uwierzytelnianie

### Uwierzytelnienie: nagłówek, nie query — potwierdzone

Poświadczenie nazywa się tak samo jak w przeglądarce: `clientToken`, pierwszy
argument `Configuration.Builder` (`Configuration.kt:66-73`). Trafia do
`DatadogContext.clientToken`, a każda fabryka wkłada je w nagłówek:

```kotlin
RequestFactory.HEADER_API_KEY to context.clientToken      // "DD-API-KEY", api/net/RequestFactory.kt:48
```

Stałe nagłówków: `dd-sdk-android-core/src/main/kotlin/com/datadog/android/api/net/RequestFactory.kt:34-79`.
Uploader dodatkowo waliduje wartość nagłówka (puste / znaki spoza ASCII →
`InvalidTokenError` bez wysyłki, `core/internal/data/upload/DataOkHttpUploader.kt:147-154`).

Nasz `RequireAPIKey` czyta `Dd-Api-Key` z nagłówka (`intake/apikey_mw.go:34`).
Go kanonizuje nazwy nagłówków, więc `DD-API-KEY` z Androida **przejdzie przez
nasze middleware bez zmian**. Wariant z query potrzebny jest tylko dla
przeglądarki.

### Nagłówki wspólne dla wszystkich kanałów

```
DD-API-KEY: <clientToken>
DD-EVP-ORIGIN: android                    (albo flutter / react-native / … przez _dd.source)
DD-EVP-ORIGIN-VERSION: <wersja SDK>
DD-REQUEST-ID: <uuid v4>
Content-Type: <z fabryki, niżej>
Content-Encoding: gzip                    (wszystko poza multipart, §3)
User-Agent: Datadog/<sdk> (Linux; U; Android <os>; <model> Build/<id>)
```

`User-Agent`: `DataOkHttpUploader.kt:116-124` — najpierw `System.getProperty("http.agent")`
po sanityzacji, fallback na format wyżej; nagłówek jest zarezerwowany (`:177-189`).
Tylko RUM dokłada `DD-IDEMPOTENCY-KEY: sha1(ciało)` (`RumRequestFactory.kt:42, 83-85, 100-104`).

### Query string — prawie pusty

- RUM: `?ddsource=<source>`, a przy ponowieniu `&ddtags=retry_count:<n>,retry_after:<poprzedni kod>`
  (`RumRequestFactory.kt:57-70, 89-97`). Odpowiednik przeglądarkowych
  `_dd.retry_count`/`_dd.retry_after`, inna nazwa.
- Logs: `?ddsource=<source>` (`LogsRequestFactory.kt:59-68`).
- Reszta: **nic**. Custom URL idzie jak jest.

Nie ma `dd-api-key`, `dd-evp-origin*`, `dd-request-id`, `batch_time`,
`_dd.api`, `dd-evp-encoding` — wszystko, co przeglądarka upycha w query,
Android wysyła nagłówkami albo wcale.

### Tabela kanałów

Wszystkie POST. Host domyślny to ten sam `browser-intake-<site>` co w
przeglądarce (`DatadogSite.kt:24-79`) — Datadog nie ma osobnego hosta
mobilnego.

| kanał | ścieżka | Content-Type | ciało | gdzie |
|---|---|---|---|---|
| RUM (+ telemetria, + crash) | `/api/v2/rum?ddsource=android` | `text/plain;charset=UTF-8` | NDJSON | `rum/internal/net/RumRequestFactory.kt:36-54, 138` |
| Logs | `/api/v2/logs?ddsource=android` | `application/json` | **tablica JSON** `[{…},{…}]` | `log/internal/net/LogsRequestFactory.kt:48-56, 85-87` |
| Traces | `/api/v2/spans` | `text/plain;charset=UTF-8` | NDJSON, każda linia `{"spans":[<span>],"env":"…"}` | `trace/internal/net/TracesRequestFactory.kt:31-48`; `trace/internal/domain/event/SpanEventSerializer.kt:28-38` |
| Trace client stats (3.14+, za `statsComputationEnabled`) | `/api/v0.2/stats` | `application/msgpack` | msgpack z polem `Stats` | `trace/internal/net/ClientStatsRequestFactory.kt:29-47`; `domain/metrics/StatsPayload.kt:14, 54` |
| Session Replay — segmenty | `/api/v2/replay` | `multipart/form-data` | N × `segment` + `event` | `sessionreplay/internal/net/SegmentRequestBodyFactory.kt:22-50` |
| Session Replay — zasoby | `/api/v2/replay` | `multipart/form-data` | N × `image` + `event` | `ResourceRequestBodyFactory.kt:125-170, 201-217` |
| Profiling | `/api/v2/profile` | `multipart/form-data` | `event` (event.json) + `perfetto.proto` + `rum-mobile-events.json` | `profiling/internal/ProfilingRequestFactory.kt:77-104, 115-120` |
| Flags — exposures | `/api/v2/exposures` | `text/plain;charset=UTF-8` | NDJSON | `flags/internal/net/ExposuresRequestFactory.kt:30-48` |
| Flags — evaluations | `/api/v2/flagevaluation` | `text/plain;charset=UTF-8` | NDJSON | `EvaluationsRequestFactory.kt:41-59` |
| Remote config | `GET sdk-configuration.<host>/v1/<id>.json` | — | — | `RemoteConfigService.kt:83-85` |

`Content-Type` ustawia SDK jawnie (`Request.contentType`, `api/net/Request.kt:21`,
`DataOkHttpUploader.kt:168-175`), nie platforma. Dla multipart OkHttp dokleja
`; boundary=…`.

Gdzie lądują rzeczy, które nie mają własnego kanału:

- **Telemetria SDK** — na track RUM jako `type: "telemetry"`, pisana
  writerem feature'u RUM (`telemetry/internal/TelemetryEventHandler.kt:62, 157-160`).
  Jak w przeglądarce: strumień `/api/v2/rum` ma dwa rodzaje rekordów.
- **Crash JVM** — `DatadogExceptionHandler` wysyła `JvmCrash.Rum` do feature'u
  RUM (`core/.../error/internal/DatadogExceptionHandler.kt:66-78`,
  `RumFeature.kt:451, 698`) → zdarzenie `type: "error"` z `is_crash`. Grep
  `LOGS_FEATURE_NAME` po `error/` i `ndk/` w core jest pusty — **do logów crash
  nie idzie**. Po zapisie SDK odpala `UploadWorker` (`:102-105`), więc crash
  zwykle dociera od razu.
- **Crash NDK** — sygnał łapie natywny handler (`features/dd-sdk-android-ndk/.../NdkCrashReportsFeature.kt:71, 132`),
  zapisuje raport na dysk; **przy następnym uruchomieniu** `DatadogNdkCrashHandler`
  wysyła komunikat `ndk_crash` do feature'u RUM (`core/.../ndk/internal/DatadogNdkCrashHandler.kt:134-145`),
  a `DatadogLateCrashReporter` zamienia go w RUM `error` na podstawie
  ostatniego `view` (`rum/internal/DatadogLateCrashReporter.kt:85-98`). ANR —
  analogicznie, kategoria `ANR` (`:146-163`). Czyli na `/api/v2/rum` przyjdą
  błędy z `date` sprzed restartu aplikacji.
- **WebView** (`features/dd-sdk-android-webview`) — zdarzenia z JS SDK w
  WebView są przepisywane przez natywny SDK i wysyłane **tą samą fabryką i
  na ten sam URL** (`webview/WebViewTracking.kt:209, 231, 253` — bierze
  `requestFactory` z natywnego feature'u). Zdarzenie zachowuje `source: browser`,
  a mapper dokłada `container: {source: "android", view: {id}}` i podmienia
  `application.id`/`session.id` na natywne
  (`webview/internal/rum/WebViewRumEventMapper.kt:34-45, 63-73, 109-110`).
  W strumieniu Androida mogą więc być rekordy przeglądarkowe.

### Kody odpowiedzi — 202 albo nic

`DataOkHttpUploader.kt:206-237` mapuje kod na status:

```
202                      -> Success                  (usuń batch)
401, 403                 -> InvalidTokenError        (usuń batch, log ERROR "token is invalid")
408, 429                 -> HttpClientRateLimiting   (ponów)
400, 413                 -> HttpClientError          (usuń batch)
500, 502, 503, 504, 507  -> HttpServerError          (ponów)
inne, w tym 200          -> UnknownHttpError         (usuń batch, log ERROR + telemetria)
```

`shouldRetry` per status: `UploadStatus.kt:18-36`; usunięcie batcha:
`DataUploadTask.kt:126` (`deleteBatch = !status.shouldRetry`). Czyli **`200`
nie jest sukcesem** — dane dojdą, SDK je skasuje, ale zaloguje błąd i wyśle
telemetrię o „unexpected status code". Mock Datadoga w testach odpowiada `202`
(`instrumented/integration/src/androidTest/kotlin/com/datadog/android/sdk/okhttp/RecordingDispatcher.kt:39`).
Nasze `202 {}` z `HandleLogs` pasuje; każdy nowy handler dla Androida ma
odpowiadać **202**.

---

## 3. Format ciała

### Batch na dysku → jedno żądanie

Tak, SDK zapisuje wszystko na dysk zanim wyśle. Format pliku to TLV:
blok `META` + blok `EVENT` na zdarzenie, `type` 2 B + `length` 4 B big-endian
(`core/internal/persistence/file/batch/PlainBatchFileReaderWriter.kt:30-55, 350-352`).
`META` to per-zdarzeniowe metadane (`api/storage/RawBatchEvent.kt:15-18`) —
dla RUM `{type, viewId, documentVersion}` (`rum/internal/domain/event/RumEventMeta.kt`),
dla profilingu bajty perfetto. Limity (`persistence/file/FilePersistenceConfig.kt:22-28`):
plik ≤ 5 MB, ≤ 1000 zdarzeń, zdarzenie ≤ 1 MB, plik zamykany po
`BatchSize.windowDurationMs` (3/10/35 s, `core/configuration/BatchSize.kt:15-25`),
pliki starsze niż 18 h kasowane, łącznie ≤ 512 MB.

**Jeden plik = jedno żądanie HTTP** (`DataUploadTask.kt:77-89` czyta
`readNextBatch`, `DataOkHttpUploader.kt:49-58` buduje z niego jedno `Request`).
Upload rusza tylko gdy jest sieć i bateria (`DataUploadTask.kt:46, 91-102`:
nie w trybie oszczędzania energii, bateria naładowana/ładuje się/powyżej
progu), do `maxBatchesPerJob` plików na cykl, dopóki są sukcesy (`:48-60`).
Odstęp: `UploadFrequency` 500/2000/5000 ms × 1…10
(`core/internal/configuration/DataUploadConfiguration.kt:15-22`), po błędzie
×1.10, po `IOException` minuta (`DefaultUploadSchedulerStrategy.kt:48-54, 63-64`).

Co to zmienia w tym, co dociera: po powrocie do sieci **seria żądań**, każde
do 5 MB / 1000 zdarzeń, ze zdarzeniami sprzed godzin (do 18 h). `date` w
zdarzeniach to czas zapisania, nie wysyłki. Opcjonalne szyfrowanie na dysku
(`Configuration.Core.encryption`, `EncryptedBatchReaderWriter.kt:26-40` szyfruje,
`:63-64` deszyfruje przy odczycie) **nie zmienia** tego, co idzie po sieci.

### RUM — NDJSON z deduplikacją `view`

`RumRequestFactory.kt:36-41`: zdarzenia z batcha sklejone `\n`, bez końcowego
`\n` (`:138`) — identycznie jak w przeglądarce. Przed sklejeniem
`RumViewEventFilter.filterOutRedundantViewEvents`
(`rum/internal/domain/event/RumViewEventFilter.kt:17-65`) zostawia per
`view.id` tylko `view` o najwyższym `documentVersion` (czyta to z bloku `META`,
nie parsuje JSON-a). Między batchami ten sam `view.id` wraca z rosnącym
`_dd.document_version` — jak w przeglądarce. `view_update` istnieje w
pipeline (3.14), ale domyślny `RumViewEventWriteConfig.AlwaysFullView`
(`RumFeature.kt:881`) go **nie emituje**; setter jest `internal`
(`RumConfiguration.kt:513`). Nie spodziewać się `view_update` z aplikacji
w produkcji, dopóki Datadog tego nie przełączy.

### Logs — tablica JSON, nie NDJSON

`LogsRequestFactory.kt:48-56, 85-87`: prefiks `[`, separator `,`, sufiks `]`,
`Content-Type: application/json`. Nasz `decodeJSONList` w `HandleLogs`
oczekuje tablicy — dla Androida **pasuje bez splittera NDJSON**. Ale pole
czasu to `date` jako **string ISO-8601** (`features/dd-sdk-android-logs/src/main/json/log/log-schema.json`,
`properties.date`: *„ISO-8601 String"*), a `HandleLogs` czyta `timestamp`
(`intake/router_logs.go:79`). Przeglądarka miała `date` jako liczbę ms —
trzy warianty jednego pola w trzech źródłach. Wymagane pola logu Androida:
`message, status, date, service, logger, _dd, ddtags, device, os`
(`log-schema.json`, `required`); `status` z enumu
`critical|error|warn|info|debug|trace|emergency`.

### Traces — spany po jednym w kopercie

`SpanEventSerializer.kt:28-38`: **każdy span osobno** owinięty w
`{"spans":[span],"env":"<env>"}`, potem `TracesRequestFactory` skleja te
koperty `\n` (`:42-48, 66`). Czyli linia NDJSON = jeden span. Schemat spanu:
`features/dd-sdk-android-trace/src/main/json/trace/span-schema.json`
(`SpanEvent`: `trace_id, span_id, parent_id` jako **stringi**, `duration`,
`start` w ns, `error` int, `meta`, `metrics`). To nie jest format agenta
(msgpack v0.4) ani OTLP — trzeci format spanów u nas.

### Kompresja — `Content-Encoding: gzip`, po ludzku

`GzipRequestInterceptor` (`core/internal/data/upload/GzipRequestInterceptor.kt:37-66, 94-95`)
gzipuje **każde** ciało i ustawia `Content-Encoding: gzip`, z dwoma wyjątkami:
`MultipartBody` i ciało, które już ma `Content-Encoding`. Interceptor wchodzi
tylko w buildzie release biblioteki (`CoreFeature.kt:667-674`; debug daje
`CurlInterceptor`), a opublikowany AAR jest release — więc w praktyce:

- RUM, logs, spans, stats, exposures, flagevaluation: **zawsze gzip**, RFC 1952.
- replay, profile (multipart): **nigdy** na poziomie HTTP.

Nasz `Decompress()` klucza po `Content-Encoding` (`intake/body.go:30-32`)
i zna gzip — **ruch z Androida rozpakuje bez zmian**. Mock Datadoga robi to
samo (`RecordingDispatcher.kt:53-58, 72-79` — `GZIPInputStream` gdy
`Content-Encoding: gzip`).

Wewnątrz multipart replay każdy `segment` jest osobno deflate'owany
(`sessionreplay/internal/net/BytesCompressor.kt:18-37`): `Deflater` z
`SYNC_FLUSH` + pusty `FULL_FLUSH`/`finish` jako „fake checksum" — komentarz
mówi wprost, że to pod sposób dekompresji w dogweb i konkatenację strumieni.
Ten sam trik co worker przeglądarki. `compress/zlib` z `body.go` to przeczyta.

---

## 4. Session Replay na Androidzie

**Istnieje** (`features/dd-sdk-android-session-replay` + `-compose`, `-material`)
i format jest **ten sam pomysł, inne szczegóły**: multipart, część binarna +
część JSON, ale rekordy to nie DOM.

### Segmenty — `SegmentRequestBodyFactory.kt:22-50`

```
segment   (N razy)  application/octet-stream, filename "file<i>"
                    = zlib(<MobileSegment JSON> + "\n")
event     (1 raz)   application/json, filename "blob"
                    = TABLICA JSON metadanych segmentów, każdy z
                      raw_segment_size i compressed_segment_size
```

Różnice wobec przeglądarki: **wiele segmentów w jednym żądaniu** (jeden na
kombinację `application/session/view` znalezioną w batchu —
`BatchesToSegmentsMapper.kt:64-83`), część `event` to **tablica**, obie części
mają `filename`. Nasz `profParts` (`intake/router_profiling.go:152-167`)
czyta `MultipartReader` bez `ParseMultipartForm`, więc ani tablica, ani
`filename` przy `event` mu nie przeszkadzają.

Zawartość segmentu po rozpakowaniu (`BatchesToSegmentsMapper.kt:132-148`):

```
{"application":{"id":…},"session":{"id":…},"view":{"id":…},"start":…,"end":…,
 "records_count":N,"has_full_snapshot":true,"source":"android","records":[…]}\n
```

`index_in_view` jest `null` (TODO w kodzie, `:139-140`). Schemat:
`features/dd-sdk-android-session-replay/src/main/json/schemas/session-replay/mobile/segment-schema.json`
(`MobileSegment` = `segment-metadata-schema.json` + `records`); `source`
z enumu `android|ios|flutter|react-native|kotlin-multiplatform|maui`
(`segment-metadata-schema.json`).

Rekordy: `full_snapshot` **type 10**, `incremental_snapshot` **type 11**
(`mobile/full-snapshot-record-schema.json`, `incremental-snapshot-record-schema.json`),
plus wspólne `meta` 4, `focus` 6, `view_end` 7, `visual_viewport` 8
(`session-replay/common/*-record-schema.json`). Snapshot to lista
**wireframe'ów** (`shape`, `text`, `image`, `placeholder`, `webview`,
`embedded-content` — `mobile/*-wireframe-schema.json`), nie drzewo DOM.
Mapper rozpoznaje też przeglądarkowy full snapshot type 2
(`BatchesToSegmentsMapper.kt:151-155, 191`), bo WebView wpuszcza rekordy
przeglądarkowe do tego samego strumienia (`WebViewTracking.kt:231`).

Na dysku leży `EnrichedRecord` `{application_id, session_id, view_id, records}`
(`sessionreplay/internal/processor/EnrichedRecord.kt:27-47`); grupowanie w
segmenty i sortowanie po `timestamp` dzieje się dopiero przy budowie żądania
(`:92-110`). Nas to nie dotyczy, ale tłumaczy, czemu jedno żądanie niesie
kilka segmentów.

### Zasoby — `ResourceRequestBodyFactory.kt:125-170, 201-217`

```
image     (N razy)  <mime z metadanych, domyślnie image/png>, filename = identyfikator zasobu
event     (1 raz)   application/json, filename "blob"
                    = {"application":{"id":…},"type":"resource"}
```

To samo, co przeglądarkowe zasoby canvas. Osobny feature (`ResourcesFeature`),
osobne batche, ten sam URL. Rozróżnienie po obecności części `image` vs
`segment`, jak w mocku przeglądarkowym.

Rekomendacja bez zmian wobec browser-sdk: trzymać bajty `segment`, dekodować
`event`; rekordów wireframe nie parsować w intake'u.

---

## 5. Czy to ten sam schemat zdarzeń RUM — TAK

**Tak, wprost: dd-sdk-android generuje modele z tych samych schematów JSON z
`DataDog/rum-events-format`, co browser-sdk.** Dowody:

1. Skrypt klonujący: `build-logic/src/main/kotlin/com/datadog/gradle/utils/JsonSchemaGenerationTasks.kt:18`
   — `RUM_EVENTS_FORMAT_REPO = "https://github.com/DataDog/rum-events-format.git"`;
   `cloneRumEventsFormat` (`:36-51`) klonuje `ref = dd.rum.schema.ref ?: "master"`
   i kopiuje podfolder. Zadania per moduł:
   - RUM: `features/dd-sdk-android-rum/build.gradle.kts:134-140`
     `schemas/rum → src/main/json/rum`; telemetria `:142-148`
     `schemas/telemetry → src/main/json/telemetry`.
   - Session Replay: `features/dd-sdk-android-session-replay/build.gradle.kts:83-114`
     (root bez browser/profiling/rum, `session-replay/mobile`, `session-replay/common`).
   - Profiling: `features/dd-sdk-android-profiling/build.gradle.kts:107-114`
     `schemas/profiling` bez `browser`.
   Kopiowanie usuwa tylko linie logów/lomboka (`gitclone/GitCloneDependenciesTask.kt:119-143`),
   treść schematów jest nietknięta.
2. Snapshoty JSON są **commitowane** (`git ls-files features/dd-sdk-android-rum/src/main/json`
   → 28 plików). Kotlin **nie** — generuje go przy buildzie własny plugin
   (`build-logic/src/main/kotlin/com/datadog/gradle/plugin/jsonschema/`,
   `GenerateJsonSchemaTask.kt:33-36`) do `build/generated/<task>/`
   (`JsonSchemaGenerationTasks.kt:66-68, 86-91`). Dlatego w drzewie nie ma
   `ViewEvent.kt` z nagłówkiem „generated" — jest `src/main/json/rum/view-schema.json`
   i mapowanie nazw (`rum/build.gradle.kts:150-177`): `action-schema.json →
   ActionEvent`, `error → ErrorEvent`, `resource → ResourceEvent`, `view →
   ViewEvent`, `view_update → ViewUpdateEvent`, `long_task → LongTaskEvent`,
   `vital-app-launch → VitalAppLaunchEvent`, `vital-operation-step →
   VitalOperationStepEvent`, `timeseries-memory/cpu → Timeseries*Event`;
   telemetria `:179-189`. Wygenerowany kształt (Gson `toJson`/`fromJsonObject`,
   `additionalProperties` jako mapa) widać na fixture'ach testowych pluginu:
   `build-logic/src/test/kotlin/com/example/model/Person.kt`.
3. `_common-schema.json` w klonie Androida jest **bajt w bajt identyczny** z
   masterem `rum-events-format` (`diff` pusty): `source` enum
   `android, ios, browser, flutter, react-native, roku, unity,
   kotlin-multiplatform, electron, cpp, maui`, `_dd.format_version` `const: 2`,
   te same pola wspólne (`date, application, service, version, build_version,
   build_id, ddtags, session, source, view, usr, account, tab, connectivity,
   display, synthetics, ci_test, os, device, _dd, context, stream`).

**Wniosek dla nas: generator typów Go ze schematów `rum-events-format`
pokryje Androida.** Dyskryminator `type` jest ten sam; Android emituje
`action | error | long_task | resource | view | vital | timeseries | telemetry`
(`view_update` jest w schemacie, ale domyślnie wyłączony — §3; `transition`
w ogóle nie jest w snapshocie Androida). `vital` ma `type: "vital"` i podtyp
w `vital.type` (`_vital-common-schema.json`); `timeseries` ma `type: "timeseries"`
(`timeseries-cpu-schema.json`).

### Ale: snapshot, nie pin — wersje się rozjeżdżają

`dd.rum.schema.ref` nigdzie nie jest ustawiony (grep po `.properties`, `.yml`,
`.kts` pusty), więc snapshot to „master w dniu, gdy ktoś ostatnio odpalił
`cloneRumSchema`". W klonie z 17 września snapshot jest **starszy** niż master
`ab3e7c5` z 8 września (ten sam commit przypina browser-sdk):

- brak w Androidzie: `rum/_graphql-schema.json`, `_stream-schema.json`,
  `_trace-schema.json`, `transition-schema.json`, `vital-duration-schema.json`;
- różnią się (tylko dodatki w masterze): `view-schema.json` (ref do
  `_stream`), `error-schema.json` (`graphql`, `wasm_modules`, `source_type`
  `browser+wasm`/`nodejs`), `action`, `resource`, `long_task`, `view_update`,
  `_view-performance`, `_view-properties`, `_profiling-internal-context`;
- telemetria: wszystkie pięć plików z drobnymi różnicami;
- replay mobile: master ma `composition-layer-*` i `shape-gradient-*`, których
  Android jeszcze nie zna; `full-snapshot-record`, `incremental-data`,
  `shape-style` różnią się.

Dla generatora oznacza to dwie rzeczy: (a) generować z **mastera** (nadzbiór),
(b) tolerować brak nowych pól i nieznane pola — telefony w terenie chodzą na
SDK sprzed miesięcy, każdy z innym snapshotem. Schematy mają
`additionalProperties` na `context`/`usr`/`account`, a generowane typy
Androida i tak niosą mapę dodatkowych pól, więc to nie jest nowy problem.

### Co NIE pochodzi z `rum-events-format`

- **Logi**: `features/dd-sdk-android-logs/src/main/json/log/log-schema.json`
  — lokalny schemat Androida, bez zadania klonującego (`logs/build.gradle.kts:81-86`
  tylko generuje). `rum-events-format` nie ma katalogu `log`.
- **Spany**: `features/dd-sdk-android-trace/src/main/json/trace/span-schema.json`
  — lokalny (`trace/build.gradle.kts:100-105`).
- **Flagi**: `features/dd-sdk-android-flags/src/main/json/flags/` — lokalne.
- **Remote config**: `dd-sdk-android-core/src/main/json/rc/android.json`,
  `mobile.json` — lokalne (`core/build.gradle.kts:46-50`).

Czyli generator z `rum-events-format` daje RUM, telemetrię, replay i profiling;
logi i spany Androida trzeba by wziąć z tych dwóch plików w repo SDK.

---

## 6. Czego nie da się ustalić z kodu

Do ustalenia wyłącznie przez złapanie ruchu z urządzenia/emulatora
(`capture.RequestLogger` przy `DEBUG=true`):

- **TLS**: czy `RESTRICTED_TLS` dogaduje się z naszym Go serwerem i czy łańcuch
  certyfikatu jest akceptowany przez system (nie widać tego w kodzie SDK).
- **Nagłówki dodane przez OkHttp**: `Accept-Encoding`, `Content-Length` vs
  chunked (gzip idzie strumieniowo — `GzipRequestInterceptor.kt:72-90` nie
  zna długości), `Connection`, `Host`; realny `User-Agent` (`http.agent` vs
  fallback); czy `boundary` multipart ma cudzysłowy.
- **Realne wypełnienie zdarzeń RUM** — `device`, `os`, `connectivity`,
  `_dd.configuration`, `accessibility` w `view`; które pola `error` przychodzą
  dla crashy JVM/NDK/ANR; częstość `timeseries`.
- **Proporcja powtórzeń `view`** między batchami i realne rozmiary batchy przy
  domyślnym `BatchSize.MEDIUM` / `UploadFrequency.AVERAGE`.
- **Rozmiary segmentów replay**, liczba części `segment` na żądanie, mime typy
  zasobów, obecność rekordów WebView w segmentach mobilnych.
- **Perfetto** — czy `perfetto.proto` to surowy trace protobuf, czy coś
  opakowanego; `ProfilingBatchMetadata` tylko przenosi bajty.
- **Stats msgpack** — czy `StatsPayload` jest zgodny z
  `pb.ClientStatsPayload` z `agent-payload` (pole `Stats` sugeruje, że tak;
  `HandleAPMStats` w `intake/router_trace.go:286` już to dekoduje dla
  agenta). Do porównania na próbce.
- **Odpowiedź prawdziwego intake'u** — kod (zakładam 202) i ciało.
- **Wrappery** (Flutter, RN, KMP, MAUI): jaki `_dd.source` i `_dd.sdk_version`
  ustawiają i czy zmieniają coś w ciele poza `source`. Wymaga zajrzenia w ich
  repozytoria, nie w to.
- **Wersje SDK w terenie** i odpowiadające im snapshoty schematów.

---

## Różnice wobec przeglądarki

Najkrócej: wszystko, co w browser-sdk wynikało z braku nagłówków, CORS i
`sendBeacon`, na Androidzie **znika**. Zostają różnice formatu ciała.

| aspekt | browser-sdk 7.13 | dd-sdk-android 3.14 | dla nas |
|---|---|---|---|
| poświadczenie | `clientToken` w query `dd-api-key` | `clientToken` w nagłówku **`DD-API-KEY`** | `RequireAPIKey` działa jak jest; wariant query tylko dla przeglądarki |
| przekierowanie | `proxy` funkcja/string, `ddforward`, regex na `site` | `useCustomEndpoint(pełny URL)` per kanał; `site` to enum | prościej; dokumentować pełny URL `/api/v2/<track>` |
| transport | `fetch` cors / `sendBeacon`, brak nagłówków | OkHttp, HTTPS-only (`RESTRICTED_TLS`), nagłówki jawne | potrzebny publicznie zaufany cert; CORS niepotrzebny |
| kompresja | query `dd-evp-encoding=deflate`, zlib, tylko RUM opt-in i replay | **`Content-Encoding: gzip`** zawsze (poza multipart) | `Decompress()` działa bez zmian |
| metadane żądania | query `dd-evp-origin`, `dd-evp-origin-version`, `dd-request-id`, `batch_time`, `_dd.api` | nagłówki `DD-EVP-ORIGIN`, `DD-EVP-ORIGIN-VERSION`, `DD-REQUEST-ID`, `DD-IDEMPOTENCY-KEY` (RUM); query tylko `ddsource` | — |
| retry | `_dd.retry_count`, `_dd.retry_after` w query | `ddtags=retry_count:N,retry_after:CODE` w query (RUM) | inne nazwy, ta sama informacja |
| kod odpowiedzi | wszystko poza 408/429/5xx = sukces | **tylko 202** = sukces; 200 = błąd, batch skasowany | odpowiadać 202 |
| utrata przy błędzie | CORS → status 0 → cicha utrata | brak sieci → retry z dysku do 18 h; 4xx → kasuje z logiem | — |
| RUM ciało | NDJSON `text/plain` (charset od przeglądarki) | NDJSON `text/plain;charset=UTF-8` | ten sam splitter |
| logi ciało | NDJSON `text/plain`, `date` liczba ms | **tablica JSON** `application/json`, `date` **string ISO-8601** | `decodeJSONList` pasuje; `date` vs `timestamp` do ogarnięcia |
| spany | brak | NDJSON `{"spans":[…],"env"}` na `/api/v2/spans`; stats msgpack `/api/v0.2/stats` | nowy handler, trzeci format spanów |
| replay | 1 segment, `event` obiekt bez filename, rekordy DOM (type 2) | N segmentów `file<i>`, `event` **tablica** z filename `blob`, wireframe'y (type 10/11) | `profParts` czyta oba; treści nie parsować |
| profiling | `event` + `wall-time.json` (JSON) | `event` + `perfetto.proto` (binarny) + `rum-mobile-events.json` | inny załącznik |
| wyjście ze strony | dwa żądania (deflate + plain) | nie ma pojęcia „wyjścia"; crash → RUM error, NDK/ANR przy następnym starcie | stare `date` po restarcie |
| telemetria | na tracku RUM, `type: telemetry` | tak samo | — |
| WebView | — | zdarzenia `source: browser` z `container.source: android` w strumieniu Androida | jeden strumień, dwa źródła |
| schemat RUM | `rum-events-format` przypięty do commitu w `package.json` | ten sam `rum-events-format`, snapshot JSON w repo bez pinu | jeden generator; tolerować dryf wersji |
| schemat logów/spanów | `logsEvent.types.ts` z JSON Schema w browser-sdk | lokalne `log-schema.json`, `span-schema.json` | osobne źródła prawdy |
| remote config | `sdk-configuration.…/v1/<id>.json`, własna opcja proxy | to samo, bez opcji przekierowania | poza zakresem |
| dysk | brak (poza `sendBeacon`) | TLV batche, ≤5 MB/1000 zdarzeń, ≤18 h, ≤512 MB, gating sieć+bateria | seria żądań po reconnect |

### Co jest wspólne dla Androida, a czego nie mamy

1. Host: własny prefiks w `routes.go` (Datadog używa tego samego
   `browser-intake-<site>` dla obu — możemy zrobić jeden host dla „SDK
   klienckich" albo dwa; użytkownik i tak podaje pełny URL).
2. `/api/v2/spans` — handler dla JSON-owych spanów z kopertą `{"spans","env"}`.
3. `/api/v0.2/stats` — sprawdzić, czy istniejący `HandleAPMStats`
   (`intake/router_trace.go:286`) zje msgpack z SDK; ścieżka i klucz
   nagłówkiem są te same co u agenta, więc może zadziałać bez zmian.
4. `HandleLogs`: `date` jako string ISO obok `timestamp` (i `date` ms z
   przeglądarki).
5. `202` w każdym handlerze RUM/replay/profile/flags.
6. Certyfikat z publicznego CA na hoście intake'u dla urządzeń.

Nie trzeba: middleware kluczy z query, `Access-Control-*`, dekompresji po
parametrze query, rozpakowywania `ddforward`, obsługi `OPTIONS` — to wszystko
było przeglądarkowe.

---

## Odniesienia — skrót

| co | gdzie |
|---|---|
| enum site, host, remote config host | `dd-sdk-android-core/src/main/kotlin/com/datadog/android/DatadogSite.kt:19-88` |
| `useCustomEndpoint` (RUM) | `features/dd-sdk-android-rum/src/main/kotlin/com/datadog/android/rum/RumConfiguration.kt:306-309` |
| budowa URL i nagłówków RUM, NDJSON, idempotency | `features/dd-sdk-android-rum/src/main/kotlin/com/datadog/android/rum/internal/net/RumRequestFactory.kt:36-104, 138` |
| logi: tablica JSON, `application/json` | `features/dd-sdk-android-logs/src/main/kotlin/com/datadog/android/log/internal/net/LogsRequestFactory.kt:48-68, 85-87` |
| spany: koperta, NDJSON | `features/dd-sdk-android-trace/src/main/kotlin/com/datadog/android/trace/internal/net/TracesRequestFactory.kt:31-48`, `…/internal/domain/event/SpanEventSerializer.kt:28-38` |
| stats msgpack | `…/trace/internal/net/ClientStatsRequestFactory.kt:29-47` |
| stałe nagłówków | `dd-sdk-android-core/src/main/kotlin/com/datadog/android/api/net/RequestFactory.kt:34-79` |
| uploader, User-Agent, kody odpowiedzi | `dd-sdk-android-core/src/main/kotlin/com/datadog/android/core/internal/data/upload/DataOkHttpUploader.kt:116-124, 147-237` |
| retry per status | `…/data/upload/UploadStatus.kt:18-36`; `DataUploadTask.kt:46-60, 91-126` |
| gzip | `…/data/upload/GzipRequestInterceptor.kt:37-66, 94-95`; `CoreFeature.kt:667-674` |
| TLS / cleartext | `CoreFeature.kt:134-146, 656-684`; `Configuration.kt:178-181, 315-320`; `_InternalProxy.kt:98-100` |
| `_dd.source` override | `Datadog.kt:455-458`; `DatadogCore.kt:573-596`; `CoreFeature.kt:825` |
| TLV na dysku, limity | `…/persistence/file/batch/PlainBatchFileReaderWriter.kt:30-55, 350-352`; `…/persistence/file/FilePersistenceConfig.kt:22-28` |
| dedupe `view` w batchu | `…/rum/internal/domain/event/RumViewEventFilter.kt:17-65` |
| replay: multipart segmentów | `features/dd-sdk-android-session-replay/src/main/kotlin/com/datadog/android/sessionreplay/internal/net/SegmentRequestBodyFactory.kt:22-61` |
| replay: grupowanie w segmenty | `…/sessionreplay/internal/net/BatchesToSegmentsMapper.kt:36-155` |
| replay: deflate z trailerem | `…/sessionreplay/internal/net/BytesCompressor.kt:18-37` |
| replay: zasoby | `…/sessionreplay/internal/net/ResourceRequestBodyFactory.kt:28-56, 125-170, 201-217` |
| profiling multipart | `features/dd-sdk-android-profiling/src/main/java/com/datadog/android/profiling/internal/ProfilingRequestFactory.kt:67-120` |
| flagi | `features/dd-sdk-android-flags/src/main/kotlin/com/datadog/android/flags/internal/net/{Exposures,Evaluations}RequestFactory.kt` |
| telemetria na tracku RUM | `features/dd-sdk-android-rum/src/main/kotlin/com/datadog/android/telemetry/internal/TelemetryEventHandler.kt:62, 157-160` |
| crash JVM / NDK / ANR | `dd-sdk-android-core/…/error/internal/DatadogExceptionHandler.kt:66-105`; `…/ndk/internal/DatadogNdkCrashHandler.kt:134-145`; `…/rum/internal/DatadogLateCrashReporter.kt:85-98, 146-163` |
| WebView | `features/dd-sdk-android-webview/src/main/kotlin/com/datadog/android/webview/WebViewTracking.kt:206-253`; `…/internal/rum/WebViewRumEventMapper.kt:34-73, 109-110` |
| klon `rum-events-format` | `build-logic/src/main/kotlin/com/datadog/gradle/utils/JsonSchemaGenerationTasks.kt:18-51, 53-92` |
| zadania klonowania i mapowanie nazw | `features/dd-sdk-android-rum/build.gradle.kts:134-189`; `…-session-replay/build.gradle.kts:83-125`; `…-profiling/build.gradle.kts:107-124` |
| generator Kotlin | `build-logic/src/main/kotlin/com/datadog/gradle/plugin/jsonschema/GenerateJsonSchemaTask.kt`; przykład wyjścia `build-logic/src/test/kotlin/com/example/model/Person.kt` |
| snapshoty schematów | `features/dd-sdk-android-rum/src/main/json/{rum,telemetry}/`, `…-session-replay/src/main/json/schemas/`, `…-profiling/src/main/json/profiling/mobile/` |
| schematy lokalne (nie z rum-events-format) | `features/dd-sdk-android-logs/src/main/json/log/log-schema.json`; `features/dd-sdk-android-trace/src/main/json/trace/span-schema.json` |
| referencyjny mock Datadoga | `instrumented/integration/src/androidTest/kotlin/com/datadog/android/sdk/okhttp/RecordingDispatcher.kt:25-80`; `…/sdk/integration/RuntimeConfig.kt:66-101` |
| master `rum-events-format` do diffów | `scratchpad/dd/rum-events-format` (`ab3e7c5`), pin w `scratchpad/dd/browser-sdk/package.json:49` |
| nasze: klucz z nagłówka | `intake/apikey_mw.go:34` |
| nasze: dekompresja po nagłówku | `intake/body.go:30-32` |
| nasze: `timestamp` vs `date` | `intake/router_logs.go:79` |
| nasze: multipart do reużycia | `intake/router_profiling.go:92, 152-167` |
