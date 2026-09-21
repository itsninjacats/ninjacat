# dd-sdk-ios — rozpoznanie

**Rozpoznanie 2026-09-20. Zero kodu.** Odpowiedzi na sześć pytań o to, co
`DataDog/dd-sdk-ios` wysyła i czy da się to skierować do nas — z naciskiem
na różnice wobec `browser-sdk` (`browser-sdk-rozpoznanie.md`).

Klon: `scratchpad/dd/dd-sdk-ios`, commit `62f64d7` (2026-09-18), wersja SDK
**3.17.0** (`DatadogCore/Sources/Versioning.swift:3`). Do porównania schematów
użyłem klonu `scratchpad/dd/rum-events-format`, commit `ab3e7c5` (2026-09-08).
Ścieżki niżej są względem korzenia klonu. Nasze pliki — względem `server/`.

Platformy: iOS 15+, tvOS 15+, macOS 12+, watchOS 9+, visionOS 1+
(`Package.swift:11-17`). Session Replay jest **tylko iOS**
(`DatadogSessionReplay/Sources/SessionReplay.swift:7`, `#if os(iOS)`).

---

## Streszczenie dla niecierpliwych

1. **Przekierowanie: tak, natywnie, bez forka.** Każdy moduł ma
   `customEndpoint: URL?`, który zastępuje **cały** URL (host + ścieżka).
   Walidacji domeny nie ma żadnej. `site` jest zamkniętym enumem — nie da się
   nim nic przekierować, ale nie trzeba.
2. **Klucz idzie nagłówkiem `DD-API-KEY`**, nie w query. Do tego komplet:
   `DD-EVP-ORIGIN`, `DD-EVP-ORIGIN-VERSION`, `DD-REQUEST-ID`, `Content-Type`,
   `User-Agent`, dla RUM dodatkowo `DD-IDEMPOTENCY-KEY`. Kompresja
   sygnalizowana **`Content-Encoding: deflate`**. Nasz `RequireAPIKey` i
   `Decompress()` pasują bez zmian.
3. **Ten sam schemat RUM co przeglądarka** — modele Swift są generowane z
   `DataDog/rum-events-format`, z pliku `schemas/rum-events-mobile-schema.json`
   (ten sam katalog `schemas/rum/*` co przeglądarkowy). Generator Go z tych
   schematów pokryje iOS.
4. Format ciała: RUM i spany — NDJSON; **logi — tablica JSON** (nie NDJSON);
   Session Replay — multipart jak w przeglądarce, ale `event` to **tablica**
   metadanych i segmentów może być kilka.

---

## 1. Czy da się zmusić SDK, żeby wysyłał do nas

**Tak — `customEndpoint`, per moduł.** To pierwszorzędna opcja publicznego
API, nie hack.

### Jak powstaje URL

Każdy moduł ma własny `RequestBuilder` z identycznym wzorcem:

```swift
customIntakeURL ?? context.site.endpoint.appendingPathComponent("api/v2/rum")
```

| moduł | opcja | plik z `??` |
|---|---|---|
| RUM | `RUM.Configuration.customEndpoint` (`DatadogRUM/Sources/RUMConfiguration.swift:332`) | `DatadogRUM/Sources/Feature/RequestBuilder.swift:61-63` |
| Logs | `Logs.Configuration.customEndpoint` (`DatadogLogs/Sources/Logs.swift:30`) | `DatadogLogs/Sources/Feature/RequestBuilder.swift:60-62` |
| Trace | `Trace.Configuration.customEndpoint` (`DatadogTrace/Sources/TraceConfiguration.swift:77`) | `DatadogTrace/Sources/Feature/RequestBuilder.swift:50-52` |
| Session Replay (segmenty i zasoby) | `SessionReplay.Configuration.customEndpoint` (`DatadogSessionReplay/Sources/SessionReplayConfiguration.swift:54`) | `…/RequestBuilders/SegmentRequestBuilder.swift:108-110`, `ResourceRequestBuilder.swift:84-86` |
| Profiling | `customUploadURL` (wewnętrzne, `DatadogProfiling/Sources/RequestBuilder.swift:15`) | `RequestBuilder.swift:89-91` |
| Flags (exposures / evaluation / assignments) | `customExposureEndpoint`, `customEvaluationEndpoint`, `customFlagsEndpoint` (`DatadogFlags/Sources/Flags.swift:54, 82, 94`) | `ExposureRequestBuilder.swift:46`, `EvaluationRequestBuilder.swift:57`, `FlagAssignmentsFetcher.swift:119` |
| Remote config | `Datadog.Configuration.remoteConfiguration.customURL` (`DatadogCore/Sources/DatadogConfiguration.swift:126-160`) | `DatadogCore/Sources/Core/RemoteConfigurationProvider.swift:254-259` |

Dwie konsekwencje, obie ważne:

1. `customEndpoint` zastępuje **cały URL łącznie ze ścieżką**. Z
   `customEndpoint: URL(string: "https://mobile-intake.ninjacat.pl/api/v2/rum")`
   dociera `POST /api/v2/rum?ddsource=ios`. Jeśli użytkownik poda sam host
   bez ścieżki, dostaniemy `POST /?ddsource=ios`. W dokumentacji dla
   użytkowników trzeba podać **pełne URL-e ze ścieżkami**, osobno dla
   każdego modułu. Nasz router może tolerować oba warianty, ale nie musi.
2. Query string SDK **dokleja** do podanego URL-a przez `URLComponents`
   (`DatadogInternal/Sources/Upload/URLRequestBuilder.swift:149-155`), więc
   `ddsource`/`ddtags` przychodzą normalnie.

### `site` — zamknięty enum, bez walidacji, bez znaczenia dla nas

`DatadogSite` to `enum: String` z dziewięcioma przypadkami
(`DatadogInternal/Sources/Context/DatadogSite.swift:9-37`): `us1 us3 us5 eu1
ap1 ap2 uk1 us1_fed us2_fed`. Host bierze się ze `switch`
(`:41-53`), np. `us1 → browser-intake-datadoghq.com`. **Uwaga: iOS używa tych
samych hostów `browser-intake-*` co przeglądarka**, nie żadnego
`mobile-intake`. Endpoint to `https://<host>/` (`:55-58`); remote config
`https://sdk-configuration.<host>/` (`:62-65`).

Nie ma regexu `/(datadog|ddog|datad0g|dd0g)/` ani żadnego odpowiednika (grep
po `DatadogCore`, `DatadogRUM` — pusto). Nie jest potrzebny: `site` jako enum
nie przyjmie obcej wartości, a `customEndpoint` przyjmie każdy `URL`.

### `proxyConfiguration` — HTTP proxy, nie przekierowanie

`Datadog.Configuration.proxyConfiguration: [AnyHashable: Any]?`
(`DatadogConfiguration.swift:96`) to słownik dla
`URLSessionConfiguration.connectionProxyDictionary`
(`DatadogCore/Sources/Core/Upload/URLSessionClient.swift:19`). Z podaną
nazwą użytkownika i hasłem SDK ręcznie dokleja `Proxy-Authorization: Basic …`
(`:24-28`). To klasyczne proxy HTTP(S) — żądanie nadal ma `Host:
browser-intake-datadoghq.com`. Do nas nie pasuje; odnotowane, bo użytkownicy
mogą o to pytać.

### `source` i wersja SDK — nadpisywalne

`context.source` domyślnie `"ios"`, ale cross-platformowe wrappery (Flutter,
React Native, KMP, .NET MAUI) nadpisują je przez
`additionalConfiguration["_dd.source"]`; podobnie `_dd.sdk_version`,
`_dd.variant`, `_dd.native_source_type`
(`DatadogCore/Sources/Datadog.swift:408-412`,
`DatadogInternal/Sources/Attributes/Attributes.swift:78, 83, 88, 160`). Czyli
z tego samego SDK może przyjść `ddsource=flutter` i `DD-EVP-ORIGIN: flutter`.
Nasz parser nie powinien zakładać `ios`.

### Ograniczenie platformy: ATS

**Do sprawdzenia, nie wynika z kodu SDK.** iOS App Transport Security
domyślnie wymaga HTTPS z zaufanym certyfikatem. `customEndpoint` na `http://`
lub z certyfikatem self-signed będzie odrzucany przez `URLSession`, dopóki
aplikacja nie doda wyjątku w `Info.plist`. SDK nic z tym nie robi
(`URLSessionClient.swift:13-32` — zwykła sesja `.ephemeral`). Dla NAS oznacza
to: publiczny cert (Let's Encrypt) na hoście intake'u, bez wyjątków.

---

## 2. Kanały, hosty, uwierzytelnianie

### Nagłówki — komplet

Wszystkie kanały uploadowe budują żądanie przez `URLRequestBuilder`
(`DatadogInternal/Sources/Upload/URLRequestBuilder.swift`). Nazwy nagłówków
`:22-27`, wartości `:83-112`, złożenie żądania `:165-191`:

```
POST <url>?<query>
Content-Type:           text/plain;charset=UTF-8 | application/json | multipart/form-data; boundary=<UUID>
Content-Encoding:       deflate                       tylko gdy ciało skompresowane (:171-173)
User-Agent:             <AppName>/<ver> CFNetwork (<device>; <os>/<osver>)     (:76)
DD-API-KEY:             <clientToken>                 <- uwierzytelnienie (:83-85)
DD-EVP-ORIGIN:          ios | flutter | react-native | ... (ciAppOrigin nadpisuje przy CI Visibility)
DD-EVP-ORIGIN-VERSION:  <wersja SDK>
DD-REQUEST-ID:          <UUID>
DD-IDEMPOTENCY-KEY:     <sha1 ciała>                  tylko RUM (DatadogRUM/…/RequestBuilder.swift:53)
```

Poświadczenie nazywa się tak samo jak w przeglądarce: `clientToken`
(`DatadogConfiguration.swift:58`, „RUM client token (which supports RUM,
Logging and APM) or regular client token"). Ląduje w nagłówku **`DD-API-KEY`**
— czyli w tym samym nagłówku, którego używa Agent. Jest też stała
`DD-CLIENT-TOKEN` (`:23, :88-90`), ale użyta tylko w sprawdzeniu kwoty
profilera (niżej).

**Potwierdzenie z drugiej strony:** SDK rozpoznaje własne żądania (żeby nie
śledzić ich jako zasobów RUM) właśnie po obecności nagłówka `DD-API-KEY`
lub `DD-CLIENT-TOKEN`
(`DatadogInternal/Sources/NetworkInstrumentation/NetworkInstrumentationFeature.swift:380-382`).
Nagłówek jest tożsamością żądania.

Nasz `RequireAPIKey` (`intake/apikey_mw.go:34`) czyta nagłówek — **pasuje bez
zmian**. Wariant z query, potrzebny dla przeglądarki, dla iOS jest zbędny.

### Parametry query

`URLRequestBuilder.QueryItem` zna tylko dwa (`:11-16, :194-202`):

```
ddsource=<source>                          RUM, Logs, Flags exposures/evaluation
ddtags=retry_count:<n>,retry_after:<kod>   każdy kanał, tylko przy ponowieniu
```

`ddtags` w query to **wyłącznie znaczniki ponowienia**
(`DatadogInternal/Sources/Upload/FeatureRequestBuilder.swift:45-60`):
`attempt > 0` → `retry_count:N`, a jeśli był kod odpowiedzi →
`retry_after:503`. Zwykłe `ddtags` (env, version, …) są **wewnątrz zdarzeń**,
jak w przeglądarce. Nie ma `batch_time`, `_dd.api`, `dd-evp-encoding` —
wszystko to poszło do nagłówków albo nie istnieje.

Trace i Session Replay nie wysyłają `ddsource` w ogóle (Trace:
`queryItems: []`, `DatadogTrace/…/RequestBuilder.swift:29`; SR: tylko retry,
`SegmentRequestBuilder.swift:59`).

### Tabela kanałów

Wszystko na jeden host `browser-intake-<site>` (albo `customEndpoint`).

| kanał | ścieżka | metoda | Content-Type | ciało | kompresja | query |
|---|---|---|---|---|---|---|
| RUM (+ telemetria, + crash, + WebView) | `/api/v2/rum` | POST | `text/plain;charset=UTF-8` | NDJSON | zlib, `Content-Encoding: deflate` | `ddsource` |
| Logs (+ WebView logs) | `/api/v2/logs` | POST | `application/json` | **tablica JSON** | zlib | `ddsource` |
| Trace | `/api/v2/spans` | POST | `text/plain;charset=UTF-8` | NDJSON kopert `{"spans":[…],"env":…}` | zlib | — |
| Session Replay — segmenty | `/api/v2/replay` | POST | `multipart/form-data` | `segment`×N + `event` | segment zlib; ciało **nie** | — |
| Session Replay — zasoby obrazów | `/api/v2/replay` | POST | `multipart/form-data` | `image`×N + `event` | całe ciało zlib | — |
| Profiling | `/api/v2/profile` | POST | `multipart/form-data` | `event` + `profile.pprof` [+ `rum-mobile-events.json`] | całe ciało zlib | — |
| Profiling — kwota | `quota.<host>/api/v2/profiling/quota?session_id=…` | GET | — | — | — | nagłówek **`DD-CLIENT-TOKEN`**, `Accept: application/vnd.api+json` |
| Flags — exposures | `/api/v2/exposures` | POST | `text/plain;charset=UTF-8` | NDJSON | **brak** (`compress: false`, `ExposureRequestBuilder.swift:42`) | `ddsource` |
| Flags — evaluation | `/api/v2/flagevaluation` | POST | do sprawdzenia | do sprawdzenia | do sprawdzenia | `ddsource` |
| Flags — assignments | `preview.ff-cdn.<site>/precompute-assignments` | — | — | — | — | inny host (`FlagAssignmentsFetcher.swift:124-133`) |
| Remote config | `sdk-configuration.<host>/v1/<id>.json` | GET | — | — | — | bez poświadczeń, `markAsInternal` (`RemoteConfigurationProvider.swift:254-263`) |

Źródła: RUM `DatadogRUM/Sources/Feature/RequestBuilder.swift:20, 38-58`;
Logs `DatadogLogs/Sources/Feature/RequestBuilder.swift:17, 35-57`; Trace
`DatadogTrace/Sources/Feature/RequestBuilder.swift:17, 27-47`; SR
`SegmentRequestBuilder.swift:57-105`, `ResourceRequestBuilder.swift:44-81`;
Profiling `DatadogProfiling/Sources/RequestBuilder.swift:44-86`,
`Models/ProfileAttachments.swift:13-15`; kwota
`ProfilingQuotaChecker.swift:58, 140-156`.

Flags to moduł świeży (kanały `exposures`/`flagevaluation` w przeglądarce
nie miały producenta); tu producent jest. Exposures prześledzone
(`ExposureRequestBuilder.swift:15, 27, 42`), evaluation — **do sprawdzenia**,
jeśli w ogóle nas interesuje.

### Kompresja — jak i kiedy

`uploadRequest(with:compress:)` ma domyślnie `compress: true`
(`URLRequestBuilder.swift:165`). Wtedy:

- ciało → `Deflate.encode` = nagłówek zlib `78 5e` + surowy deflate (poziom 5,
  Apple `Compression`) + Adler-32 big-endian
  (`DatadogInternal/Sources/Upload/DataCompression.swift:42-51`). Poprawny
  strumień RFC 1950 — `compress/zlib` z `intake/body.go:87-94` go przeczyta.
- nagłówek `Content-Encoding: deflate` (`URLRequestBuilder.swift:172`).
- **Wyjątek:** gdy kompresja powiększyłaby ciało (`data.count <= header+raw+
  checksum`, `DataCompression.swift:48`), `encode` zwraca `nil` i SDK wysyła
  **surowe ciało bez `Content-Encoding`** (`:174-175`). Dla małych batchy
  (jedno zdarzenie) to się zdarza. Serwer musi obsłużyć oba warianty —
  `Decompress()` (`intake/body.go:32`) kluczuje po nagłówku, więc robi to
  poprawnie.

Kto wyłącza kompresję: segmenty SR (`compress: false`,
`SegmentRequestBuilder.swift:105` — bo segmenty są już zlib w środku) i Flags
exposures (`ExposureRequestBuilder.swift:42`). Zasoby SR i profile
kompresują całe ciało multipart (`ResourceRequestBuilder.swift:81`,
`DatadogProfiling/…/RequestBuilder.swift:86`) — czyli **multipart pod
deflate**, najpierw rozpakować, potem parsować boundary. `profParts`
(`intake/router_profiling.go:152-167`) czyta `MultipartReader` z ciała — jeśli
jest za `Decompress()`, zadziała.

Referencyjny parser Datadoga w testach robi dokładnie to:
`TestUtilities/Sources/Helpers/Decompression.swift:70-81` —
`Content-Encoding == "deflate"` → `zlib.decode(body)`; potem
`RUMEventMatcher.fromNewlineSeparatedJSONObjectsData`
(`TestUtilities/Sources/Matchers/RUMEventMatcher.swift:32-38`) dzieli po `\n`;
`SpanMatcher.swift:58-59` tak samo; `LogMatcher.swift:88` dekoduje tablicę.

### Kody odpowiedzi — co SDK rozumie

`DatadogCore/Sources/Core/Upload/DataUploadStatus.swift:10-55`:

```
202                        sukces, batch skasowany            (jedyny kod „accepted")
400 401 403 413            błąd klienta, batch skasowany, bez ponowienia
408 429 500 502 503 504 507 ponowienie tego samego batcha
inne (w tym 200!)          .unexpected → bez ponowienia, batch skasowany
```

`200` nie jest na liście — trafia w `.unexpected`, co też kończy się
skasowaniem batcha, więc **działa**, ale `DataUploadError(status:)`
(`:113-124`) dla `.unexpected` zwraca `nil`… bo `HTTPResponseStatusCode(
rawValue: 200)` to `nil`. Czyli 200 = cichy sukces. Mimo to odpowiadajmy
**`202`**, bo to jedyny kod, który SDK nazywa po imieniu, a `HandleLogs` już
tak robi. Na 401/403 SDK loguje ostrzeżenie o tokenie i site
(`DataUploadWorker.swift:155`).

Ponowienia nie mają limitu prób; rośnie tylko odstęp
(`DataUploadWorker.swift:127-131`, `delay.increase()`), a batch żyje na dysku
do 18 h (`PerformancePreset.swift:134`, `maxFileAgeForRead`).

### Transport

- `URLSession` `.ephemeral`, bez cache, bez ciasteczek
  (`URLSessionClient.swift:14-18`). Zero CORS, zero preflightu — to
  aplikacja natywna.
- Jeden upload naraz, synchronicznie na kolejce workera
  (`DataUploader.swift:44-93`).
- Upload zablokowany, gdy: brak sieci, bateria < 10 % i nie ładuje, Low Power
  Mode (`DataUploadConditions.swift:12-15, 30-50`). Więc dane przychodzą
  **paczkami po powrocie online**, z `ddtags=retry_count:…` jeśli wcześniej
  nie wyszło.
- Odstępy: `uploadFrequency` `.frequent/.average/.rare` → 0,5/2/5 s bazowo,
  mnożnik 1–10× adaptacyjnie (`PerformancePreset.swift:88-94, 96-104`).
  `batchProcessingLevel` `.low/.medium/.high` → 5/20/100 batchy na cykl
  (`DatadogConfiguration.swift:41-52`).

---

## 3. Format ciała

### Ramkowanie: trzy różne, nie jedno

`DataFormat(prefix:suffix:separator:)`
(`DatadogInternal/Sources/Upload/DataFormat.swift:35-53`) składa
`prefix + zdarzenie₁ + sep + zdarzenie₂ + … + suffix`. Bez końcowego
separatora.

| kanał | `DataFormat` | wynik |
|---|---|---|
| RUM | `("", "", "\n")` (`DatadogRUM/…/RequestBuilder.swift:20`) | NDJSON, jak przeglądarka |
| Trace | `("", "", "\n")` (`DatadogTrace/…/RequestBuilder.swift:17`) | NDJSON kopert |
| Logs | `("[", "]", ",")` (`DatadogLogs/…/RequestBuilder.swift:17`) | **`[{…},{…}]` — tablica JSON** |

Logi w tablicy to dobra wiadomość: `parseLogs → decodeJSONList`
(`intake/router_logs.go`) oczekuje tablicy — **iOS pasuje od razu**, w
przeciwieństwie do przeglądarki.

### Limity batcha

`PerformancePreset.swift:130-136`, komentarze SDK mówią „backend limit":
plik (= batch = jedno żądanie) **≤ 5 MB**, **≤ 1 000 zdarzeń**, zdarzenie
**≤ 1 MB**. Wiek pliku do zamknięcia: `batchSize` `.small/.medium/.large` →
3/10/35 s (`:78-84`).

### Zapis na dysk — tak, i to zmienia kształt

Każde zdarzenie jest **od razu** serializowane do JSON i dopisywane do pliku
batcha w formacie TLV (`DatadogCore/Sources/Core/TLV/TLVBlock.swift:31-49`:
`u16 typ | u32 długość | bajty`, little-endian). Typy bloków: `0x00` event,
`0x01` metadata poprzedzającego eventu (`Storage+TLV.swift:16-22`).
Opcjonalnie szyfrowane per blok (`FileWriter.swift:104-123`, `DataEncryption`
użytkownika). Przy uploadzie `FileReader` odczytuje bloki, deszyfruje i
zwraca `[Event(data:metadata:)]` (`DatadogInternal/Sources/Upload/Event.swift`).

Co z tego wynika dla tego, co dociera:

1. **Zdarzenie jest zamrożone w chwili zapisu.** Żaden późniejszy stan
   (nowy `view.id`, nowy `usr`) nie zmienia już zapisanego JSON-a. Batch
   wysłany po 3 dniach offline zawiera `date` sprzed 3 dni — czyli
   **timestampy w batchu mogą być stare**; nasz serwer nie może ich odrzucać
   ani zastępować czasem odbioru.
2. **Metadata jest do filtrowania, nie do wysyłki.** `RUMViewEventsFilter`
   (`DatadogRUM/Sources/Feature/RUMViewEventsFilter.swift:24-36, 39-69`)
   przy budowie żądania: dla tego samego `view.id` w batchu zostawia
   **tylko ostatnią** wersję (poza zdarzeniami z accessibility), i wyrzuca
   początkowy `view` z `time_spent = 1 ns`, jeśli nic po nim nie przyszło.
   Efekt jak upsert w przeglądarce: w jednym żądaniu jeden `view` per id,
   między żądaniami ten sam id wraca z rosnącym `_dd.document_version`.
3. **Consent.** `TrackingConsent.pending` trzyma dane w osobnym katalogu do
   decyzji, `.granted` przenosi je do kolejki, `.notGranted` kasuje
   (`DatadogInternal/Sources/Context/TrackingConsent.swift:16-23`). Dla
   serwera: po `granted` może przyjść paczka wszystkiego od uruchomienia.
4. **Batch nie jest łączony między funkcjami** ani między plikami — jedno
   żądanie = jeden plik jednego modułu. Retry ponawia dokładnie ten sam
   plik, więc ciało jest bajt w bajt to samo (poza `DD-REQUEST-ID`, nowym
   `ddtags=retry_count`, i nowym boundary multipart).
5. Katalogi są rozdzielone per `site` i instancja SDK
   (`Datadog.swift:432-436`, `CoreDirectory(… site:)`). Zmiana site w
   aplikacji nie „przenosi" starych batchy.

### RUM

Jedna linia = jedno zdarzenie ze schematu (§5). Dyskryminator `type`. Na
tracku `rum` lądują cztery źródła:

- właściwe zdarzenia RUM (`RUMViewScope.swift:663, 794, 898` itd.),
- **telemetria SDK** — `TelemetryDebugEvent`/`TelemetryErrorEvent`/
  `TelemetryConfigurationEvent`/`TelemetryUsageEvent`, pisane do scope'u
  funkcji RUM (`DatadogRUM/Sources/Integrations/TelemetryReceiver.swift:
  67-97, 309-314`), `type: "telemetry"`. Jak w przeglądarce.
- **crash reporting** — nie ma własnego kanału HTTP. `CrashReportSender`
  publikuje raport na szynie (`DatadogCrashReporting/Sources/
  CrashReportSender.swift:44-56`), a `CrashReportReceiver` w RUM pisze
  `RUMErrorEvent` z `error.is_crash = true` i, jeśli trzeba, syntetyczny
  `RUMViewEvent` dla poprzedniej sesji
  (`DatadogRUM/Sources/Integrations/CrashReportReceiver.swift:122-139,
  162-180`). Wymaga włączonego RUM (komunikat `:51-52`). Crash z poprzedniego
  uruchomienia przychodzi **przy następnym starcie**, z `date` sprzed
  crasha. Moduł Logs crashy **nie** dostaje (grep po `DatadogLogs/Sources`
  za `Crash`/`DDCrashReport` — pusto); jedyna droga to RUM.
- **WebView** — zdarzenia z browser-sdk osadzonego w `WKWebView`
  przepisywane przez `WebViewEventReceiver`
  (`DatadogRUM/Sources/Integrations/WebViewEventReceiver.swift:64-148`):
  SDK podmienia `application.id` i `session.id` na natywne, koryguje `date`
  o offset NTP, dokleja `container: {source: "ios", view: {id}}`,
  merguje `ddtags`. Ale `source` zdarzenia zostaje **`browser`**, a `_dd.
  browser_sdk_version` zostaje. Czyli w jednym żądaniu z `DD-EVP-ORIGIN:
  ios` mogą być zdarzenia z `source: "browser"`. Nie zakładać
  `source == DD-EVP-ORIGIN`. Analogicznie logi WebView na track `logs`
  (`DatadogLogs/Sources/Feature/MessageReceivers.swift:65-105`).

### Logs

`LogEvent` (`DatadogLogs/Sources/Log/LogEventEncoder.swift:13`) jest
**pisany ręcznie**, nie generowany — schemat logów nie jest w
`rum-events-format`. Klucze najwyższego poziomu (`:176-240`): `date`
(**ms epoch, liczba**), `status`, `message`, `service`, `env`, `ddtags`
(string po przecinku), `version`, `build_version`, `build_id`, `_dd`,
`logger.name/version/thread_name`, `usr.*`, `account.*`,
`network.client.*`, `error.kind/message/stack/source_type/fingerprint/
binary_images`, plus atrybuty użytkownika spłaszczone na tym samym poziomie.

Wobec `HTTPLogItem`: `message`, `service`, `ddtags` — 1:1; `hostname` — brak
(telefon); `ddsource` — z query. **Ta sama pułapka co w przeglądarce:**
`date` zamiast `timestamp` (`intake/router_logs.go:79` czyta `timestamp`).
Reszta w `AdditionalProperties`.

### Trace — to NIE format Agenta

`/api/v2/spans` to intake EVP, nie `/v0.4/traces` Agenta. Ciało to NDJSON, a
każda linia to koperta `{"spans":[<span>],"env":"<env>"}`
(`DatadogTrace/Sources/Span/SpanEventEncoder.swift:11-31`; dziś zawsze jeden
span w kopercie, `:25-27`, pisane w `DDSpan.swift:165-166`). Span jest
**płaski, z kropkowanymi kluczami** (`SpanEventEncoder.swift:122-171`):

```
trace_id (hex, low 64 bit), span_id, parent_id ("0" dla roota), name, service,
resource, type:"custom", start (ns), duration (ns), error (0|1),
metrics._top_level, metrics._sampling_priority_v1, metrics._dd.agent_psr,
meta._dd.source, meta._dd.origin, meta._dd.p.tid, meta._dd.p.dm, meta.version,
meta.tracer.version, meta.usr.*, meta.account.*, meta.network.client.*,
meta.device, meta.os (obiekty), meta.<tag użytkownika>
```

`trace_id` jako hex (`:186`), `meta.device`/`meta.os` jako zagnieżdżone
obiekty pod kluczem z kropką. Nasz dekoder spanów agentowych (msgpack,
`pb.TracerPayload`) tego nie przeczyta — to inny kształt. Wolumen z
aplikacji mobilnych jest zwykle mały; generyczny odczyt jak w
`router_evp.go` wystarczy na start.

---

## 4. Session Replay na iOS

**Istnieje, tylko iOS, multipart jak w przeglądarce — ale z dwiema różnicami
w kształcie.** `SegmentRequestBuilder.swift:76-105`:

```
segment   ×N   Content-Disposition: form-data; name="segment"; filename="file<i>"
               Content-Type: application/octet-stream
               = zlib( JSON segmentu + "\n" )
event     ×1   Content-Disposition: form-data; name="event"; filename="blob"
               Content-Type: application/json
               = TABLICA metadanych: [ {segment bez "records", + raw_segment_size, + compressed_segment_size}, … ]
```

Różnice wobec przeglądarki:

1. **Kilka segmentów w jednym żądaniu.** Batch z dysku zawiera segmenty
   różnych widoków; `merge()` (`SegmentJSON.swift:158-179`) skleja te z tym
   samym `view.id` w jeden, więc po jednym `segment` per widok, a
   `filename="file0"`, `"file1"`, … po kolei. Przeglądarka: jeden segment,
   `filename="<session>-<start>"`.
2. **`event` to tablica**, jeden element per segment, w tej samej kolejności
   co części `segment`. Przeglądarka: pojedynczy obiekt. Do tego `event` ma
   `filename="blob"` — `mime/multipart` w Go poda go jako plik, nie wartość
   formularza (odwrotnie niż w przeglądarce). `profParts` czyta
   `MultipartReader`, więc obojętne.

Zawartość segmentu po rozpakowaniu (`SegmentJSON.swift:120-132`):

```
{"application":{"id":…},"session":{"id":…},"view":{"id":…},"source":"ios",
 "start":…,"end":…,"has_full_snapshot":…,"records":[…],"records_count":N}\n
```

Bez `creation_reason` i `index_in_view` (są w przeglądarce). Kompresja:
zlib poziom 6, `Z_SYNC_FLUSH` + `Z_FINISH`
(`DatadogSessionReplay/Sources/Writers/SRCompression.swift:14-27`) — komentarz
mówi wprost, że po to, by odbiorca mógł **konkatenować** kolejne kawałki i
inflate'ować raz. Ta sama filozofia co w przeglądarce: trzymać bajty.

Rekordy są **inne niż rrweb**: mobilny schemat to wireframe'y (`type: 10`
FullSnapshot mobilny vs `2` przeglądarkowy, `SegmentJSON.swift:25-30`),
generowane z `schemas/session-replay-mobile-schema.json` →
`session-replay/mobile/segment-schema.json` (`SRDataModels.swift:3137`,
`run.py:23`). `SRSegment.Source` nie zawiera `browser`
(`SRDataModels.swift:2266-2273`). Jeśli kiedyś będziemy dekodować rekordy,
mobilne i przeglądarkowe to dwa różne schematy w jednym repo.

Drugi wariant na tej samej ścieżce — zasoby obrazów
(`ResourceRequestBuilder.swift:63-81`): `image`×N (`filename` = identyfikator
zasobu, `Content-Type` z zasobu) + `event` = `{"type":"resource",
"application":{"id":…}}` (`Models/EnrichedResource.swift:12-23`). Rozróżnienie
po części `image` vs `segment`, jak w przeglądarce. **Całe ciało** tego
wariantu jest pod `Content-Encoding: deflate` (`:81`), segmentów — nie
(`:105`).

Nie ma odpowiednika `has_replay` w query ani `ddsource` — tożsamość tylko z
nagłówków i z `event`.

---

## 5. Czy to ten sam schemat zdarzeń RUM — TAK

Wprost: **dd-sdk-ios generuje modele RUM z tych samych schematów JSON z
`DataDog/rum-events-format`, co browser-sdk.** Dowody:

1. Nagłówek generowanego pliku: `DatadogInternal/Sources/Models/RUM/
   RUMDataModels.swift:7` — „This file was generated from JSON Schema. Do
   not modify it directly." Stopka `:15898` — „Generated from
   https://github.com/DataDog/rum-events-format/tree/c6b13a3e…". To samo dla
   ObjC (`DatadogRUM/Sources/DataModels/RUMDataModels+objc.swift:17549`) i
   SR (`SRDataModels.swift:3137`, inny SHA `7cd262a4…`).
2. Generator: `tools/rum-models-generator/run.py:19` klonuje
   `https://github.com/DataDog/rum-events-format.git` (`:121-128`), czyta
   `schemas/rum-events-mobile-schema.json` (`:22`) i
   `schemas/session-replay-mobile-schema.json` (`:23`). CLI w Swifcie
   (`tools/rum-models-generator/Sources/CodeGeneration/…`) parsuje JSON
   Schema draft-07.
3. Makefile: `make rum-models-generate GIT_REF=master` i `rum-models-verify`
   (`Makefile:360-380`); `docs/CONVENTIONS.md:71` — „Never hand-edit.
   Regenerate". Weryfikacja w CI wymusza zgodność z upstreamem.
4. Enum `source` w wygenerowanym kodzie (`RUMDataModels.swift:1205-1217`):
   `android ios browser flutter react-native roku unity
   kotlin-multiplatform electron cpp maui` — **identyczna lista** jak w
   `rumEvent.types.ts` przeglądarki.
5. Nie ma submodułu (`.gitmodules` brak) ani zależności pakietowej —
   generator klonuje repo ad hoc, a wynik jest wersjonowany w Swift.

### Mobilny vs pełny schemat — co się różni

W `rum-events-format` (klon `ab3e7c5`) są cztery punkty wejścia:
`rum-events-schema.json`, `rum-events-browser-schema.json`,
`rum-events-mobile-schema.json`, `rum-events-electron-schema.json`.
Porównanie `mobile` z `rum-events-schema.json`:

| `oneOf` | pełny | mobilny |
|---|---|---|
| `rum/action-schema.json` | tak | tak |
| `rum/transition-schema.json` | tak | **nie** |
| `rum/error`, `long_task`, `resource`, `view`, `view_update` | tak | tak |
| `rum/vital-duration`, `vital-operation-step`, `vital-app-launch` | tak | tak |
| `telemetry-events-schema.json` | nie | **tak** (telemetria wprost w unii) |
| `rum/timeseries-memory`, `timeseries-cpu` | nie | **tak** |

Wszystkie `$ref` wskazują na **te same pliki `schemas/rum/*.json`** — mobilny
schemat to inna unia tych samych składowych, nie inne definicje. Wspólne
pola (`_common-schema.json`: `date`, `application`, `session`, `source`,
`view`, `usr`, `_dd.format_version = 2`, …) są jedne.

Wygenerowane typy Swift (`RUMDataModels.swift`): `RUMActionEvent`,
`RUMErrorEvent`, `RUMLongTaskEvent`, `RUMResourceEvent`,
`RUMTimeseriesCpuEvent`, `RUMTimeseriesMemoryEvent`, `RUMViewEvent`,
`RUMViewUpdateEvent`, `RUMVitalAppLaunchEvent`, `RUMVitalDurationEvent`,
`RUMVitalOperationStepEvent`, `TelemetryConfigurationEvent`,
`TelemetryDebugEvent`, `TelemetryErrorEvent`, `TelemetryUsageEvent`.
Producentów `view_update` i `timeseries` **w tym SDK już są**
(`RUMViewEvent+Update.swift:22`, `Timeseries/TimeseriesSessionCollector.swift`),
więc na tracku `rum` z iOS mogą przyjść typy, których przeglądarka (bez
flagi beta) nie wysyła: `view_update`, `timeseries`.

**Wniosek dla naszego generatora Go:** generować z `rum-events-schema.json`
**plus** `telemetry-events-schema.json` i dwa `timeseries-*` — albo po prostu
z unii `browser ∪ mobile`. Wtedy jeden zestaw typów Go pokrywa przeglądarkę,
iOS i (do potwierdzenia przez drugiego agenta) Androida. Uwaga na
`additionalProperties`: generowany Swift ma `[String: AnyCodable]` dla
`context`, `usr`, `account`, `feature_flags` (`RUMDataModels.swift:343-372,
2726-2745`) — to samo, co `[k: string]: unknown` w TS.

### Co NIE jest ze schematów

- **Logi** — `LogEvent` ręczny (§3). Nie ma schematu logów w
  `rum-events-format`.
- **Spany** — `SpanEvent` ręczny (§3).
- **Crash report** (`DatadogInternal/Sources/Models/CrashReporting/*`) —
  ręczny, ale na wyjściu i tak staje się `RUMErrorEvent` ze schematu.
- **Remote config** (`Models/RC/RCDataModels.swift`) — generowany z
  prywatnego `dd-go` (`run.py:26-29`), nieistotny (to odpowiedź, nie intake).

---

## 6. Czego nie da się ustalić z kodu

Do złapania z prawdziwego urządzenia (`capture.RequestLogger`, `DEBUG=true`):

- **Czy CFNetwork dokłada własne nagłówki** (`Accept`, `Accept-Encoding`,
  `Accept-Language`, `Connection`, `Content-Length` vs chunked) i czy
  zmienia wielkość liter w `DD-API-KEY`. SDK ustawia swoje, ale sesja może
  dołożyć.
- **Kiedy realnie brakuje `Content-Encoding`** (małe batche, `encode → nil`).
  Z kodu wynika, że przy jednym zdarzeniu RUM ~1 KB kompresja się opłaca,
  więc pewnie prawie zawsze jest — ale to trzeba zobaczyć.
- **Rzeczywiste wypełnienie zdarzeń** — które pola opcjonalne przychodzą dla
  `resource`/`error`/`vital`/`timeseries` z iOS; jak wygląda
  `_dd.configuration` w telemetrii; ile jest `view_update` vs `view`.
- **Rozkład timestampów** — ile batchy przychodzi z `date` starszym niż
  minuty/godziny (offline, consent pending, crash z poprzedniego startu).
- **Wielkości i liczba części w żądaniach SR** — ile segmentów per żądanie
  w praktyce, mime typy `image`.
- **Format ciała Flags exposures/evaluation** — nie prześledzone.
- **Odpowiedź prawdziwego intake'u** (ciało przy 202, nagłówki) — SDK nie
  czyta ciała, ale warto odwzorować.
- **ATS i certyfikat** — czy `customEndpoint` na naszym hoście przechodzi bez
  wyjątków w `Info.plist` (powinno z publicznym certem).
- **Co robią wrappery** (Flutter/RN/KMP/MAUI) z `customEndpoint` — czy w
  ogóle go wystawiają. Poza zakresem tego repo.
- **`DD-EVP-ORIGIN` przy CI Visibility** (`ciAppOrigin`) — wartość nie jest
  w tym repo.

---

## Różnice wobec przeglądarki

To jest lista rzeczy, które w `browser-sdk-rozpoznanie.md` wymagały pracy po
naszej stronie, i co z nich zostaje dla iOS.

| temat | przeglądarka | iOS | dla nas |
|---|---|---|---|
| przekierowanie | `proxy` (string z `ddforward`, albo funkcja); `site` walidowane regexem | `customEndpoint: URL` per moduł; pełny URL ze ścieżką; `site` zamknięty enum, bez regexu | prościej: dokumentować pełne URL-e; ewentualnie tolerować brak ścieżki |
| host domyślny | `browser-intake-<site>` | **ten sam** `browser-intake-<site>` | jeden prefiks w `routes.go` obsłuży oba |
| uwierzytelnienie | `dd-api-key` w **query** | **nagłówek `DD-API-KEY`** | `RequireAPIKey` bez zmian; wariant z query tylko dla przeglądarki |
| nagłówki | zero (unikanie preflightu) | `Content-Type`, `User-Agent`, `DD-API-KEY`, `DD-EVP-ORIGIN`, `DD-EVP-ORIGIN-VERSION`, `DD-REQUEST-ID`, RUM: `DD-IDEMPOTENCY-KEY` | metadane są tam, gdzie w ruchu Agenta |
| sygnał kompresji | query `dd-evp-encoding=deflate` | **`Content-Encoding: deflate`** | `Decompress()` bez zmian |
| kiedy kompresja | RUM tylko z `compressIntakeRequests`, logi nigdy | RUM, logi, spany, zasoby SR, profile — **domyślnie zawsze** (chyba że nie zmniejsza) | obsłużyć brak nagłówka przy małych ciałach |
| ramkowanie logów | NDJSON | **tablica JSON** | `decodeJSONList` pasuje od razu |
| ramkowanie RUM | NDJSON, bez końcowego `\n` | NDJSON, bez końcowego `\n` | ten sam splitter |
| `Content-Type` RUM | `text/plain` (od przeglądarki) | `text/plain;charset=UTF-8` (jawnie) | — |
| CORS | konieczny `Access-Control-Allow-Origin`; brak = cicha utrata | nie istnieje | nic |
| `sendBeacon`, dwa żądania przy wyjściu | tak | nie; za to **bufor na dysku**, upload po powrocie online, retry bez limitu do 18 h | stare `date` w batchach; idempotencja po `DD-IDEMPOTENCY-KEY` możliwa |
| retry w query | `_dd.retry_count`, `_dd.retry_after` (RUM) | `ddtags=retry_count:N,retry_after:<kod>` (każdy kanał) | inne nazwy, ta sama semantyka |
| `batch_time`, `_dd.api` | są (RUM) | brak | — |
| kody odpowiedzi | wszystko poza 408/429/5xx = sukces | **202** jedyny nazwany sukces; 400/401/403/413 kasują; 408/429/5xx/507 ponawiają | odpowiadać 202 |
| trace | brak w browser-sdk | `/api/v2/spans`, NDJSON kopert `{"spans":[…],"env"}`, span płaski z kluczami `meta.*`/`metrics.*` | nowy kształt, nie format Agenta |
| Session Replay: `event` | jeden obiekt, bez `filename` | **tablica**, `filename="blob"` | parser po `MultipartReader` obojętny; dekodować jako tablicę |
| Session Replay: segmenty | jeden per żądanie, `filename=<session>-<start>` | **N per żądanie** (jeden per widok), `filename=file<i>` | dopasować `segment[i]` do `event[i]` |
| Session Replay: rekordy | rrweb (`type: 2` FullSnapshot) | wireframe'y (`type: 10`), osobny schemat mobilny | trzymać bajty; jeśli dekodować — dwa schematy |
| Session Replay: zasoby | `image` + `event`, bez kompresji | `image`×N + `event`, **całe ciało deflate** | rozpakować przed multipartem |
| profiling | `event` + `wall-time.json` (JSON) | `event.json` + **`profile.pprof`** (+ `rum-mobile-events.json`), całe ciało deflate | `profDecodeProfile` (pprof) **ma zastosowanie**, inaczej niż w przeglądarce |
| profiling kwota | `DD-CLIENT-TOKEN`, preflight | `DD-CLIENT-TOKEN`, `Accept: application/vnd.api+json`, bez preflightu | — |
| telemetria | na tracku `rum` | na tracku `rum` | to samo |
| crash reporting | n/d | `RUMErrorEvent` (+ syntetyczny `view`) przy **następnym starcie** | to samo co error RUM |
| WebView | n/d | zdarzenia browser-sdk przepisane na track iOS, `source: "browser"` zostaje | nie wnioskować `source` z `DD-EVP-ORIGIN` |
| `source` | `browser` (lub `flutter`/`unity`) | `ios` domyślnie, nadpisywalne (`flutter`, `react-native`, `kotlin-multiplatform`, `maui`) | j.w. |
| schemat RUM | `rum-events-format`, `rum-events-browser-schema.json` | **`rum-events-format`, `rum-events-mobile-schema.json`** — te same `schemas/rum/*` | jeden generator Go; dołożyć `telemetry` i `timeseries` |
| typy zdarzeń, które realnie przyjdą | `view action error resource long_task vital` (+ `view_update` za flagą) | to samo **plus** `view_update`, `timeseries`, `telemetry` | — |
| logi: model | `logsEvent.types.ts` (schemat) | `LogEvent` ręczny | `date` zamiast `timestamp` — ta sama poprawka |

### Co jest wspólne i czego jeszcze nie mamy — wersja iOS

Z listy dla przeglądarki (§5 tamtego dokumentu) dla iOS zostaje:

1. Host w `routes.go` — ten sam co dla przeglądarki (`browser-intake.` albo
   własny; użytkownik i tak poda pełny URL).
2. ~~Middleware kluczy z query~~ — niepotrzebne.
3. ~~Dekompresja po `dd-evp-encoding`~~ — niepotrzebne, `Content-Encoding`.
4. ~~CORS~~ — niepotrzebne.
5. Splitter NDJSON dla RUM i spanów — ten sam co dla przeglądarki. Logi —
   już pasują.
6. ~~`ddforward`~~ — niepotrzebne.

Nowe, specyficzne dla iOS:

7. Dekoder spanów EVP (`/api/v2/spans`) — generyczny odczyt.
8. SR: `event` jako tablica, dopasowanie do N segmentów.
9. Dekompresja **przed** parsowaniem multipartu (zasoby SR, profile).
10. Tolerancja starych `date` (offline, crash z poprzedniego startu).

---

## Odniesienia — skrót

| co | gdzie |
|---|---|
| `site` enum, hosty, endpoint | `DatadogInternal/Sources/Context/DatadogSite.swift:9-65` |
| `customEndpoint` RUM / Logs / Trace / SR | `DatadogRUM/Sources/RUMConfiguration.swift:332`, `DatadogLogs/Sources/Logs.swift:30`, `DatadogTrace/Sources/TraceConfiguration.swift:77`, `DatadogSessionReplay/Sources/SessionReplayConfiguration.swift:54` |
| `clientToken`, `proxyConfiguration`, remote config | `DatadogCore/Sources/DatadogConfiguration.swift:58, 96, 126-160` |
| nagłówki, query, deflate + `Content-Encoding` | `DatadogInternal/Sources/Upload/URLRequestBuilder.swift:22-27, 83-112, 165-191, 194-202` |
| zlib `78 5e` + adler | `DatadogInternal/Sources/Upload/DataCompression.swift:42-51` |
| retry `ddtags` | `DatadogInternal/Sources/Upload/FeatureRequestBuilder.swift:45-60` |
| ramkowanie | `DatadogInternal/Sources/Upload/DataFormat.swift:35-53` |
| RUM request | `DatadogRUM/Sources/Feature/RequestBuilder.swift:20, 38-63` |
| Logs request (tablica) | `DatadogLogs/Sources/Feature/RequestBuilder.swift:17, 35-62` |
| Trace request, koperta, klucze spanu | `DatadogTrace/Sources/Feature/RequestBuilder.swift:17, 27-52`, `DatadogTrace/Sources/Span/SpanEventEncoder.swift:11-31, 122-171, 186-198` |
| SR segmenty multipart, merge, JSON segmentu | `DatadogSessionReplay/Sources/Feature/RequestBuilders/SegmentRequestBuilder.swift:57-110`, `SegmentJSON.swift:120-132, 158-179` |
| SR zlib (poziom 6, sync flush) | `DatadogSessionReplay/Sources/Writers/SRCompression.swift:14-27` |
| SR zasoby | `DatadogSessionReplay/Sources/Feature/RequestBuilders/ResourceRequestBuilder.swift:44-86`, `Models/EnrichedResource.swift:12-23` |
| profiling multipart, nazwy załączników | `DatadogProfiling/Sources/RequestBuilder.swift:44-91`, `Models/ProfileAttachments.swift:13-15` |
| profiling kwota (`DD-CLIENT-TOKEN`) | `DatadogProfiling/Sources/ProfilingQuotaChecker.swift:58, 140-156` |
| kody odpowiedzi, retry | `DatadogCore/Sources/Core/Upload/DataUploadStatus.swift:10-55`, `DataUploadWorker.swift:104-160` |
| warunki uploadu (sieć, bateria) | `DatadogCore/Sources/Core/Upload/DataUploadConditions.swift:12-50` |
| URLSession ephemeral, proxy | `DatadogCore/Sources/Core/Upload/URLSessionClient.swift:13-32` |
| TLV na dysku, limity 5 MB / 1000 / 1 MB | `DatadogCore/Sources/Core/TLV/TLVBlock.swift:31-49`, `Storage+TLV.swift:10-22`, `PerformancePreset.swift:78-94, 130-136` |
| filtr `view` w batchu | `DatadogRUM/Sources/Feature/RUMViewEventsFilter.swift:24-69` |
| consent | `DatadogInternal/Sources/Context/TrackingConsent.swift:16-23` |
| `source`, `sdkVersion` z `additionalConfiguration` | `DatadogCore/Sources/Datadog.swift:408-412`, `DatadogInternal/Sources/Attributes/Attributes.swift:78-88, 160` |
| telemetria na track `rum` | `DatadogRUM/Sources/Integrations/TelemetryReceiver.swift:67-97, 309-314` |
| crash → `RUMErrorEvent` | `DatadogCrashReporting/Sources/CrashReportSender.swift:44-56`, `DatadogRUM/Sources/Integrations/CrashReportReceiver.swift:122-180` |
| WebView → track `rum` / `logs` | `DatadogRUM/Sources/Integrations/WebViewEventReceiver.swift:64-148`, `DatadogLogs/Sources/Feature/MessageReceivers.swift:65-105` |
| model logu (ręczny) | `DatadogLogs/Sources/Log/LogEventEncoder.swift:13, 176-240` |
| **generowane modele RUM** | `DatadogInternal/Sources/Models/RUM/RUMDataModels.swift:7, 1205-1217, 15898` |
| generowane modele SR | `DatadogSessionReplay/Sources/Models/SRDataModels.swift:3137` |
| generator | `tools/rum-models-generator/run.py:19-23, 121-128, 187-200`, `Makefile:360-380`, `docs/CONVENTIONS.md:71` |
| schematy upstream | `scratchpad/dd/rum-events-format/schemas/rum-events-mobile-schema.json`, `rum-events-schema.json`, `session-replay-mobile-schema.json` |
| SDK rozpoznaje własne żądania po nagłówku | `DatadogInternal/Sources/NetworkInstrumentation/NetworkInstrumentationFeature.swift:380-382` |
| referencyjny parser Datadoga (testy) | `TestUtilities/Sources/Helpers/Decompression.swift:70-81`, `Matchers/RUMEventMatcher.swift:32-38`, `Matchers/SpanMatcher.swift:58-59`, `Matchers/LogMatcher.swift:88`, `Matchers/SRRequestMatcher.swift` |
| nasze: klucz z nagłówka — pasuje | `intake/apikey_mw.go:34` |
| nasze: dekompresja po nagłówku — pasuje | `intake/body.go:32, 87-94` |
| nasze: logi jako tablica — pasuje; `timestamp` vs `date` — nie | `intake/router_logs.go:79` |
| nasze: multipart do reużycia | `intake/router_profiling.go:92, 152-167` |
