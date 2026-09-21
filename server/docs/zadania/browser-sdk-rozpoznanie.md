# browser-sdk — rozpoznanie

**Rozpoznanie 2026-09-20. Zero kodu.** Odpowiedzi na sześć pytań o to, co
`DataDog/browser-sdk` wysyła i czy da się to skierować do nas.

Klon: `scratchpad/dd/browser-sdk`, commit `a46e96e`, wersja pakietów **7.13.0**.
Ścieżki niżej są względem `packages/` w klonie, chyba że zaznaczono inaczej.
Nasze pliki — względem `server/`.

---

## 1. Czy da się zmusić SDK, żeby wysyłał do nas

**Tak, bez forka.** Trzy mechanizmy, jeden z nich jest właściwy.

Cały adres składa jedna funkcja: `buildEndpointUrl`,
`js-core/src/transport/endpointBuilder.ts:133-165`. Kolejność rozstrzygania:

```
proxy: string    -> `${normalizeUrl(proxy)}?ddforward=${encodeURIComponent(path + '?' + parameters)}`   :145-151
proxy: function  -> proxy({ path, parameters, subdomain })                                               :153-155
brak proxy       -> `https://browser-intake-${site z '.' -> '-'}.${tld}${path}?${parameters}`            :157-164
```

### `site` — nie nadaje się

Dwa powody, oba twarde.

1. Walidacja: `js-core/src/entries/configuration.ts:361`
   `DATADOG_SITE_REGEX = /(datadog|ddog|datad0g|dd0g)/`. Pole `site` nie ma
   `strict: false` (`browser-core/src/domain/configuration/configuration.ts:282`),
   więc wartość spoza regexu to `display.error` **i przerwanie `init()`**
   (`configuration.ts:310-318`). Da się obejść nazwą zawierającą `ddog`,
   ale patrz punkt 2.
2. Host powstaje przez zamianę kropek na myślniki: `site: 'ddog.ninjacat.pl'`
   → `https://browser-intake-ddog-ninjacat.pl/…`. To **inna domena
   rejestrowalna**, nie poddomena naszej. Musielibyśmy kupić
   `browser-intake-ddog-ninjacat.pl`. Nie.

Lista znanych site'ów (`js-core/src/transport/intakeSites.ts:2-11`) jest tylko
typem TS z `(string & {})` — nie ogranicza niczego w runtime. Ogranicza regex.

### `proxy` jako string — działa, ale zmienia kształt żądania

`InitConfiguration.proxy` (`browser-core/src/domain/configuration/configuration.ts:128`).
Z `proxy: 'https://rum.ninjacat.pl/dd'` do nas dociera:

```
POST https://rum.ninjacat.pl/dd?ddforward=%2Fapi%2Fv2%2Frum%3Fddsource%3Dbrowser%26dd-api-key%3Dpub_...%26dd-evp-origin-version%3D7.13.0%26dd-evp-origin%3Dbrowser%26dd-request-id%3D...%26batch_time%3D...%26_dd.api%3Dfetch
```

Zweryfikowane, nie zakładane: `endpointBuilder.ts:146` —
`ddforward` niesie **całą oryginalną ścieżkę razem z jej query stringiem**,
zakodowaną `encodeURIComponent`. Klucz (`dd-api-key`) jest **wewnątrz**
`ddforward`, nie w zewnętrznym query. Serwer musi zdekodować `ddforward`,
sparsować go jako URL i dopiero z niego czytać ścieżkę i parametry. Dokładnie
tak robi mock Datadoga w e2e:
`test/e2e/lib/framework/intakeProxyMiddleware.ts:110-118`
(`new URL(ddforward, 'https://example.org')`).

Drugi parametr, `ddforwardSubdomain=<sub>`, dokleja się tylko gdy budowany
adres ma poddomenę (`:147-149`). W 7.13.0 jedyny taki przypadek to
`quota` (profiling, niżej). Remote config **nie idzie przez `proxy`** —
`browser-rum-core/src/domain/configuration/remoteConfiguration.ts:343-353`
buduje URL bez `proxy`, ma własną opcję `remoteConfigurationProxy`
(`configuration.ts:162`).

`normalizeUrl` (`js-core/src/util/urlPolyfill.ts:13-15`) rozwiązuje adres
względem `document.baseURI`, więc `proxy: '/dd'` też działa — to jest sposób
na ruch **same-origin**, bez CORS w ogóle, jeśli strona i intake stoją pod
jedną domeną.

### `proxy` jako funkcja — to jest właściwa droga

`ProxyFn` (`endpointBuilder.ts:30`) jest publicznym typem w
`InitConfiguration`. Funkcja dostaje `{ path, parameters, subdomain }` i
zwraca gotowy URL:

```js
proxy: ({ path, parameters }) => `https://browser-intake.ninjacat.pl${path}?${parameters}`
```

Efekt: żądanie **identyczne z natywnym** — ta sama ścieżka `/api/v2/<track>`,
te same parametry w zewnętrznym query, `dd-api-key` czytelny bez
rozpakowywania `ddforward`. Host wybieramy sami, więc pasuje do konwencji
`routes.go` (jeden host, jeden router). Jeden handler obsłuży i to, i
ewentualny wariant ze stringiem (wystarczy sprawdzić, czy jest `ddforward`).

Rekomendacja: dokumentować użytkownikom **funkcję**, string tolerować.

### Replica (dual shipping)

`replica` (`configuration.ts:212`, `@internal`) zawsze celuje w US1
(`endpointBuilder.ts:99-116`), z tym samym `proxy`. Nieistotne dla nas,
odnotowane dla kompletności.

---

## 2. Kanały, hosty, uwierzytelnianie

Wszystkie kanały HTTP idą na **jeden host** `browser-intake-<site>` i ścieżkę
`/api/v2/<track>` (`endpointBuilder.ts:85`). `TrackType` (`:10`):
`logs | rum | replay | profile | exposures | flagevaluation | debugger`.

`exposures` i `flagevaluation` — zadeklarowane, **bez producenta w tym
repozytorium** (grep po całym drzewie znajduje tylko typ i changelog).
Pomijam.

### Parametry query — wspólne (`endpointBuilder.ts:171-199`)

```
ddsource=browser                        (flutter | unity | dd_debugger)
dd-api-key=<clientToken>                <- uwierzytelnienie
dd-evp-origin-version=<wersja SDK>
dd-evp-origin=browser
dd-request-id=<uuid v4>
dd-evp-encoding=deflate                 tylko gdy ciało skompresowane
```

Tylko dla `rum` (`:190-196`):

```
batch_time=<ms epoch>
_dd.api=fetch | beacon
_dd.retry_count=<n>  _dd.retry_after=<status>   tylko przy ponowieniu
```

**`ddtags` w query nie ma.** Nasz `rum-i-monitorowanie-aplikacji.md` cytuje
`&ddtags=` z `pkg/opentelemetry-mapping-go/otlp/rum/rum.go:302-329` — to
agentowy translator OTLP→RUM (wołany z `otlp/logs/translator.go:123`), nie
SDK. SDK wkłada `ddtags` **do wnętrza zdarzenia**
(`browser-rum-core/src/domain/assembly.ts:94`, `rumEvent.types.ts:1113`;
logi: `browser-logs/src/domain/assembly.ts:58`). Do poprawienia w tamtym
dokumencie.

### Uwierzytelnienie: query string, nie nagłówek — potwierdzone

`fetchStrategy` (`browser-core/src/transport/httpRequest.ts:130-140`):

```ts
fetch(fetchUrl, { method: 'POST', body: payload.data, mode: 'cors' })
```

**Zero nagłówków.** Komentarz `:15-22` mówi wprost: bez Content-Type, żeby
uniknąć preflightu i móc użyć `sendBeacon`. Klucz jest wyłącznie w
`dd-api-key` w query. Nasz `RequireAPIKey` (`intake/apikey_mw.go:34`) czyta
tylko nagłówek — potrzebny wariant czytający query.

Jedyny wyjątek z nagłówkiem: sprawdzenie kwoty profilera,
`browser-rum/src/domain/profiling/quotaCheck.ts:52-58` —
`GET https://quota.browser-intake-<site>/api/v2/profiling/quota?session_id=…`
z nagłówkiem `DD-CLIENT-TOKEN`. Ten jeden **wymusza preflight OPTIONS**.
Dotyczy tylko profilingu.

### Transport i CORS

- `send` → `fetch` z `mode: 'cors'`, bez credentials (domyślne `same-origin`,
  więc cross-origin bez ciasteczek).
- `sendOnExit` → `navigator.sendBeacon` gdy ciało < limitu
  (`httpRequest.ts:103-119`), inaczej `fetch`. Limit: 16 KiB dla batchy
  (`:13`), 60 000 B dla replay (`segmentCollection.ts:18`), 0 dla zasobów
  replay (`datadogRecorder.ts:46` — zasoby nigdy beaconem).
- Ciała to `text/plain` albo `multipart/form-data` → **simple request, bez
  preflightu**. Odpowiedź musi jednak nieść `Access-Control-Allow-Origin`,
  inaczej `fetch` odrzuca.

**Pułapka**: `shouldRetryRequest` (`sendWithRetryStrategy.ts:139-147`) ponawia
tylko przy 408, 429, 5xx, albo `status 0` **gdy `navigator.onLine === false`**.
Błąd CORS to `status 0` przy `onLine === true` → SDK uznaje to za **sukces**
i porzuca dane bez ponowienia. Brak nagłówka CORS = cicha utrata, nie błąd.
Nasz serwer dziś nie zna CORS (grep `Access-Control` po `server/` — pusto).

Co do kodu odpowiedzi: SDK akceptuje wszystko poza 408/429/5xx. Mock e2e
odpowiada `200` z pustym ciałem (`intakeProxyMiddleware.ts:105`). Nasze
`202 {}` z `HandleLogs` będzie w porządku. **Do sprawdzenia:** co odpowiada
prawdziwy intake (nie wynika z kodu SDK).

### Tabela kanałów

| kanał | ścieżka | metoda | Content-Type ciała | ciało | kompresja |
|---|---|---|---|---|---|
| RUM (+ telemetria SDK) | `/api/v2/rum` | POST | `text/plain` | NDJSON | `deflate` gdy `compressIntakeRequests: true` (domyślnie `false`, `browser-rum-core/…/configuration.ts:498`) |
| Logs | `/api/v2/logs` | POST | `text/plain` | NDJSON | nigdy (`startLogsBatch.ts:21` — batch bez enkodera) |
| Session Replay — segmenty | `/api/v2/replay` | POST | `multipart/form-data` | `segment` + `event` | segment **zawsze** zlib |
| Session Replay — zasoby canvas | `/api/v2/replay` | POST | `multipart/form-data` | `image` + `event` | brak |
| Profiling | `/api/v2/profile` | POST | `multipart/form-data` | `event` + `wall-time.json` | załącznik zlib gdy worker |
| Profiling — kwota | `quota.…/api/v2/profiling/quota` | GET | — | — | nagłówek `DD-CLIENT-TOKEN` |
| Debugger | `/api/v2/debugger` | POST | `text/plain` | NDJSON, `ddsource=dd_debugger` (`browser-debugger/src/transport/startDebuggerBatch.ts:7`) | brak |
| Remote config | `sdk-configuration.…/v1/<id>.json` | GET | — | — | bez `proxy`; osobna opcja |

Content-Type `text/plain`: dla stringa ustawia go **przeglądarka**
(`text/plain;charset=UTF-8` — zachowanie `fetch`, nie kod SDK); dla
skompresowanego ciała SDK jawnie tworzy `Blob` z `type: 'text/plain'`
(`batch.ts:132-146`, z komentarzem dlaczego).

Telemetria SDK (błędy wewnętrzne, konfiguracja, usage) leci **na track `rum`**
niezależnie od tego, czy to SDK RUM czy Logs (`telemetry.ts:219-230`), jako
zdarzenia `type: 'telemetry'` (`:186-202`). Czyli strumień `/api/v2/rum`
zawiera dwa rodzaje rekordów.

Most do aplikacji mobilnych (`eventBridge`) nie jest HTTP — pomijam.

---

## 3. Format ciała

### NDJSON (rum, logs, debugger)

`browser-core/src/transport/batch.ts`:
- każde zdarzenie to `jsonStringify(message)`; kolejne doklejane z `\n`
  (`:58`, `:80`, `:102`). **Bez końcowego `\n`**, bez tablicy.
- limit rekordu 256 KiB (`:18`), rekord większy jest porzucany z ostrzeżeniem.
- flush: 50 wiadomości (`flushController.ts:23`), 16 KiB (`:112`),
  30 s (`:17`), koniec sesji, wyjście ze strony.
- **upsert**: zdarzenia `view` są podmieniane w batchu po `view.id`
  (`startRumBatch.ts:96`, `:114`, `:121`), więc w jednym żądaniu nie ma dwóch
  `view` o tym samym id — ale między żądaniami ten sam `view.id` wraca
  wielokrotnie z rosnącym `_dd.document_version`. Za flagą
  `betaEnableViewUpdates` zamiast pełnego `view` przychodzi diff
  `type: 'view_update'` (`:142-167`).

Mock Datadoga parsuje to dokładnie tak:
`intakeProxyMiddleware.ts:166-180` — `inflateSync` jeśli deflate, potem
`split('\n').map(JSON.parse)`.

### Kompresja — jak ją rozpoznać

**Nie ma nagłówka `Content-Encoding`** (SDK nie ustawia nagłówków). Sygnał to
parametr query `dd-evp-encoding=deflate` (`endpointBuilder.ts:186-188`).
Mock e2e: `content-encoding || dd-evp-encoding` (`:116`). Nasz `Decompress()`
(`intake/body.go:32`) klucza po nagłówku, więc **tego ruchu nie rozpakuje** —
trzeba czytać parametr.

Format: worker (`browser-worker/src/boot/startWorker.ts`) używa pako
`Deflate` z `Z_SYNC_FLUSH` (`:63-64`) i dokleja własny trailer: pusty blok
deflate `03 00` + Adler-32 (`:100-113`). Wynik to **poprawny strumień zlib**
(nagłówek `78 xx`, mock sprawdza to w `:374-380`). `compress/zlib` z
`body.go:87-94` go przeczyta. Enkoder składa kawałki + trailer w
`browser-rum/src/domain/deflate/deflateEncoder.ts:56-64`.

Kiedy deflate występuje:
- RUM: tylko `compressIntakeRequests: true` (`preStartRum.ts:162`).
- Logs: nigdy.
- Replay: zawsze (enkoder deflate jest wymagany, `datadogRecorder.ts:25`).
- Profiling: załącznik, gdy worker działa; inaczej surowy JSON — mock
  wykrywa po magicznych bajtach (`:300-305`).

**Do sprawdzenia:** co robi SDK, gdy worker nie wstanie (CSP bez
`worker-src blob:`) — czy RUM wraca do identity encodera, czy replay się nie
uruchamia. Z `rumPublicApi.ts:679-681` wynika fallback na identity dla RUM;
dla replay nie prześledziłem do końca.

### Przy wyjściu ze strony — dwa żądania

Gdy enkoder jest asynchroniczny (deflate) i strona się zamyka, batch wysyła
**dwa** żądania: skompresowane to, co worker zdążył zwrócić, i osobno
niezakodowany tekst tego, co czekało (`batch.ts:86-108`). Pierwsze ma
`dd-evp-encoding=deflate`, drugie nie. Oba beaconem, jeśli mieszczą się w
limicie.

### Multipart (replay, profile)

Opisany w §4 i niżej. Konwencja Datadoga: część `event` z JSON-em opisu +
załączniki nazwane w `event.attachments` — ta sama, co w `/api/v2/profile`
z tracerów (`intake/router_profiling.go:66-75`).

---

## 4. Session Replay

**Tak, multipart z binarką.** `buildReplayPayload.ts:24-41`:

```
segment   Blob application/octet-stream, filename `${session.id}-${start}`
          = strumień zlib; po rozpakowaniu JEDEN obiekt JSON
event     Blob application/json, bez filename
          = metadata segmentu + raw_segment_size + compressed_segment_size
```

Zawartość `segment` po rozpakowaniu (`segment.ts:55-56`, `:71`):

```
{"records":[<record>,<record>,...],"start":…,"end":…,"creation_reason":"init",
 "records_count":N,"has_full_snapshot":true,"index_in_view":0,"source":"browser",
 "application":{"id":…},"session":{"id":…},"view":{"id":…}}\n
```

Czyli `{"records":[` + rekordy po przecinku + `],` + metadata bez `{` + `\n`.
Rekordy to typy z `browser-rum/src/types/sessionReplay.ts` (rrweb-podobne:
FullSnapshot `type: 2`, IncrementalSnapshot, Meta, Focus, ViewEnd,
VisualViewport, Frustration, Change). Format snapshotów ma dwie wersje
(`format: 0` drzewo DOM, `format: 1` lista zmian `[kod, ...]`).

Wolumen: segment zamykany co 5 s (`segmentCollection.ts:13`), przy 60 000 B
skompresowanych (`:18`), przy zmianie widoku, przy wyjściu.

Ważne zdanie z komentarza `segmentCollection.ts:20-26`: segmenty są
*„stored without any processing from the intake"* i deflate jest tak
skonstruowany, żeby dało się **konkatenować strumienie zlib** (trailer
`03 00` + adler właśnie po to, `startWorker.ts:97-98`). Datadog sam nie
dekoduje tego na wejściu — trzyma bajty. To sugeruje, co my powinniśmy
robić: przechowywać `segment` jak przyszedł, dekodować `event`.

Drugi wariant na tej samej ścieżce — zasoby canvas
(`buildResourcePayload.ts:8-13`):

```
image     Blob (typ z canvas), filename = hash
event     {"application":{"id":…},"type":"resource"}
```

Rozróżnienie po obecności części `image` vs `segment`, jak w mocku
(`intakeProxyMiddleware.ts:215-231`, obraz **nie** jest kompresowany).

Uwaga na `event` bez `filename`: `mime/multipart` w Go poda to jako część
z `Content-Type: application/json`, a `ParseMultipartForm` wrzuciłby ją do
`Form` (wartość), nie `File`. Nasz `profParts` (`router_profiling.go:152-167`)
czyta `MultipartReader` bezpośrednio — nie ma tego problemu.

---

## 5. Czy pokrywa się z tym, co już mamy

Uczciwie, kanał po kanale.

### Logs → `datadogV2.HTTPLogItem` — **tak, z małym adapterem**

Zdarzenie (`browser-logs/src/logsEvent.types.ts:1-194`) kontra pola modelu:

| SDK | HTTPLogItem | uwaga |
|---|---|---|
| `message` | `Message` | 1:1 |
| `service` | `Service` | 1:1 |
| `ddtags` (string, po przecinku) | `Ddtags` | 1:1, `splitDDTags` działa |
| — | `Hostname` | przeglądarka nie ma hosta; pusty |
| — (w query `ddsource=browser`) | `Ddsource` | trzeba przepisać z query |
| `date` (ms) | AdditionalProperties | **nie `timestamp`** — `HandleLogs:79` czyta `timestamp`, tu będzie `date` |
| `status`, `origin`, `view`, `session_id`, `usr`, `error`, `http`, `logger`, `_dd` | AdditionalProperties | generowany unmarshaller zbierze |

Dwie zmiany poza modelem: ramkowanie NDJSON (`parseLogs` →
`decodeJSONList` oczekuje tablicy lub obiektu, tu jest linia po linii) i
`date` zamiast `timestamp`. Reszta `HandleLogs` pasuje jak jest.

### RUM → **nie ma opublikowanego typu Go. Nigdzie.**

Sprawdzone trzy miejsca:

1. `datadog-api-client-go v2.65.0`, `api/datadogV2/model_rum_event.go` —
   `RUMEvent{Attributes *RUMEventAttributes, Id, Type}` z
   `RUMEventAttributes{Attributes map[string]interface{}, Service, Tags,
   Timestamp}` (`model_rum_event_attributes.go:14-28`). To kształt
   **odpowiedzi API wyszukiwania** (`/api/v2/rum/events`), nie zdarzenia
   z intake'u. Zdarzenie intake'u to `{type:'view', date, application,
   session, view, _dd:{format_version:2,…}, …}` — płaskie, z `type` jako
   dyskryminatorem. Wciskanie tego w `RUMEventAttributes.AdditionalProperties`
   byłoby naciągane: model nie deklaruje ani jednego pola, które by pasowało.
   Pozostałe 200+ plików `model_rum_*` to konfiguracja aplikacji, metryki,
   filtry retencji — nie zdarzenia.
2. `agent-payload/v5` — nic z RUM.
3. Agent: `pkg/opentelemetry-mapping-go/otlp/rum/rum.go` buduje payload jako
   `map[string]any` (`ConstructRumPayloadFromOTLP`, `:60-75`). Datadog sam
   nie ma tu typu.

Źródłem prawdy jest JSON Schema w `DataDog/rum-events-format`
(`rumEvent.types.ts:1-3` — „DO NOT MODIFY, run json-schemas:sync";
`constants.go:8-9` w agencie wskazuje to samo repo). Dałoby się z niego
wygenerować Go, ale to jest dokładnie „dokładanie nowych struktur", którego
właściciel nie chce, a schemat ma `[k: string]: unknown` na każdym poziomie.

**Wniosek: czytać generycznie.** `json.RawMessage` na linię + `map[string]any`
+ pola tożsamości po udokumentowanych nazwach — czyli konwencja z
`router_evp.go` (`evpObject`, `evpPath`, `evpKeys`). Dyskryminator `type`:
`action | transition | error | long_task | resource | view | view_update |
vital` (`rumEvent.types.ts:8-16`) plus `telemetry`. Wspólne pola najwyższego
poziomu (`CommonProperties`, `rumEvent.types.ts:~1095-1520`): `date`,
`application.id`, `service`, `version`, `session.id/type/has_replay`,
`source`, `view.id/url/referrer/name`, `usr`, `account`, `connectivity`,
`display`, `os`, `device`, `_dd.format_version=2`, `_dd.browser_sdk_version`,
`context`, `ddtags`.

### Telemetria SDK (na tracku rum) → generycznie, jak RUM

Typ w `browser-core/src/domain/telemetry/telemetryEvent.types.ts` (1053
linii), też z JSON Schema. Ten sam los co RUM.

### Session Replay → **nowy kształt, ale nie nowe struktury**

Multipart czyta `profParts` (`router_profiling.go:152-167`) bez zmian.
`segment` rozpakowuje `compress/zlib` (`body.go` już go importuje). Po
rozpakowaniu — brak typu Go, i **Datadog sam go nie parsuje na wejściu**
(§4). Trzymać bajty, dekodować `event` (metadata) generycznie. Nie widzę
powodu, żeby dekodować rekordy rrweb w intake'u.

### Profiling → `profParts` + `profDecodeEvent` bez zmian, załącznik inny

`event` to JSON (`buildProfileEvent.ts:42-55`: `family: 'chrome'`,
`format: 'json'`, `version: 4`, `attachments: ['wall-time.json']`,
`application/session/view` ids) — `profDecodeEvent` (`router_profiling.go:92`)
zwraca `map[string]any`, pasuje. Załącznik `wall-time.json` to **JSON**
(`BrowserProfilerTrace`, `types/profiling.ts:130`), opcjonalnie zlib —
**nie pprof**, więc `profDecodeProfile` nie ma zastosowania; wystarczy
sniff `78 xx` jak w mocku i `compress/zlib`.

### Debugger → `HandleDebugger` prawie pasuje

Wariant JSON w `HandleDebugger` (`router_profiling.go:221-224`,
`dbgDecodeLogs:273`) oczekuje tablicy; z przeglądarki przyjdzie NDJSON. Ta
sama poprawka ramkowania co w logach. Poza zakresem na teraz.

### Co jest wspólne dla wszystkich kanałów, a czego nie mamy

1. Host: własny prefiks w `routes.go` (np. `browser-intake.`), a użytkownik
   podaje `proxy` jako funkcję (§1).
2. Middleware kluczy z **query** (`dd-api-key`) — obok `RequireAPIKey`.
   Client token jest publiczny w kodzie strony, więc to inny rodzaj
   poświadczenia niż klucz agenta; decyzja właściciela
   (już w `rum-i-monitorowanie-aplikacji.md`, „Do rozstrzygnięcia").
3. Dekompresja po `dd-evp-encoding`, nie po `Content-Encoding`.
4. `Access-Control-Allow-Origin` w odpowiedziach; `OPTIONS` tylko dla kwoty
   profilera.
5. Splitter NDJSON — jedna funkcja, trzy kanały.
6. Ewentualnie rozpakowanie `ddforward`, jeśli tolerujemy `proxy` jako
   string.

Podsumowanie pkt 5: **logi tak (HTTPLogItem), profile tak (nasze helpery
multipart), replay i debugger — nasze helpery, brak typu; RUM — brak typu
gdziekolwiek, generyczny odczyt jak w `router_evp.go`.**

---

## 6. Czego nie wiadomo z kodu SDK

Do ustalenia wyłącznie przez złapanie prawdziwego ruchu (`capture.RequestLogger`
przy `DEBUG=true`, `routes.go:201-204`):

- **Dokładne nagłówki** wystawione przez przeglądarki: `Content-Type` z
  `charset` dla stringa, `boundary` multipart, obecność `Content-Length` przy
  `sendBeacon`, `Origin`/`Referer`. SDK nie ustawia nic — wszystko jest
  zachowaniem przeglądarki i różni się między Chrome/Firefox/Safari.
- **Realne wypełnienie zdarzeń RUM.** Typy mają `[k: string]: unknown` na
  każdym poziomie; które pola faktycznie przychodzą dla `resource`, `error`,
  `vital` w praktyce — tylko z próbki.
- **Proporcja `view` do `view_update`** i częstość powtórzeń tego samego
  `view.id` między batchami — decyduje o tym, czy przechowujemy każdą wersję,
  czy ostatnią.
- **Udział `_dd.api=beacon`** i rozdwojonych żądań przy wyjściu (§3).
- **Rozmiary segmentów replay** i mime typy zasobów canvas w praktyce.
- **Odpowiedź prawdziwego intake'u** (status, ciało). SDK tego nie wymaga,
  ale warto odwzorować.
- **Zachowanie bez workera** (CSP) — RUM identity vs replay wyłączony.
- **Czy przeglądarki robią preflight dla `sendBeacon` z `multipart/form-data`**
  — standard mówi nie, Safari bywa inny.
- **Format `dd-evp-origin-version`** przy buildach z `sdkVersion`/`variant`
  (opcje `@internal`, `configuration.ts:249-256`).

---

## Odniesienia — skrót

| co | gdzie |
|---|---|
| budowa URL, `ddforward`, host z `site` | `js-core/src/transport/endpointBuilder.ts:133-165` |
| parametry query | `endpointBuilder.ts:171-199` |
| regex `site` | `js-core/src/entries/configuration.ts:361, 404-408` |
| brak nagłówków, `mode: cors` | `browser-core/src/transport/httpRequest.ts:130-140` |
| `sendBeacon` | `httpRequest.ts:103-119` |
| NDJSON, `text/plain` Blob | `browser-core/src/transport/batch.ts:58, 132-153` |
| limity flush | `browser-core/src/transport/flushController.ts:17, 23, 112` |
| retry / cicha utrata przy CORS | `browser-core/src/transport/sendWithRetryStrategy.ts:139-147` |
| zlib + trailer | `browser-worker/src/boot/startWorker.ts:63-64, 100-113` |
| replay multipart | `browser-rum/src/domain/segmentCollection/buildReplayPayload.ts:24-41` |
| replay segment JSON | `browser-rum/src/domain/segmentCollection/segment.ts:55-56, 71` |
| replay zasoby | `browser-rum/src/domain/segmentCollection/buildResourcePayload.ts:8-13` |
| profiling multipart | `browser-rum-core/src/transport/formDataTransport.ts:42-69` |
| profiling kwota (nagłówek!) | `browser-rum/src/domain/profiling/quotaCheck.ts:32-58` |
| telemetria na tracku rum | `browser-core/src/domain/telemetry/telemetry.ts:186-230` |
| remote config bez proxy | `browser-rum-core/src/domain/configuration/remoteConfiguration.ts:343-353` |
| referencyjny parser Datadoga | `test/e2e/lib/framework/intakeProxyMiddleware.ts` (cały) |
| typy zdarzeń | `browser-rum-core/src/rumEvent.types.ts`, `browser-logs/src/logsEvent.types.ts`, `browser-rum/src/types/sessionReplay.ts`, `browser-rum/src/types/profiling.ts` |
| Go: RUMEvent to odpowiedź API | `$GOMODCACHE/…/datadog-api-client-go/v2@v2.65.0/api/datadogV2/model_rum_event.go`, `model_rum_event_attributes.go:14-28` |
| nasze: klucz z nagłówka | `intake/apikey_mw.go:34` |
| nasze: dekompresja po nagłówku | `intake/body.go:32` |
| nasze: `timestamp` vs `date` | `intake/router_logs.go:79` |
| nasze: multipart do reużycia | `intake/router_profiling.go:92, 152-167` |
| nasze: generyczny odczyt do reużycia | `intake/router_evp.go` (`evpList`, `evpObject`, `evpPath`) |

---

## 7. Typy Go dla zdarzeń RUM — `rumevents/` (uzupełnienie 2026-09-20)

Wniosek z §5 („RUM — brak typu gdziekolwiek, czytać generycznie") jest
**nieaktualny**. Datadog publikuje formalny schemat, a §5 przeoczył, że
wygenerowanie z niego Go nie jest wymyślaniem własnych struktur.

### Skąd schematy

`browser-sdk/packages/browser-rum-core/src/rumEvent.types.ts` ma nagłówek
„DO NOT MODIFY IT BY HAND. Run `yarn json-schemas:sync`", a
`scripts/json-schemas.ts:81` wskazuje źródło: publiczne repozytorium
**`DataDog/rum-events-format`** (JSON Schema draft-07). To z niego generują
swoje modele SDK przeglądarkowy (TS), iOS (Swift), Android (Kotlin) i
Electron — my dokładamy wariant Go z **tych samych plików**.

W klonie browser-sdk nie ma `node_modules`, więc schematy sklonowano z sieci
do `scratchpad/dd/rum-events-format` i ustawiono na commit, który
browser-sdk przypina w `package.json:49`:

```
ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671   2026-09-08  "Rename StringResourceId to StringRoleResourceId (#443)"
```

Repozytorium ma 197 plików schematów: `rum/` (25), `telemetry/` (8),
`session-replay/` (~150), `profiling/` (9) i roboty korzeniowe.

### Co pokrywa `rum-events-schema.json`

Root RUM to `oneOf` po dyskryminatorze `type`: `action`, `transition`,
`error`, `long_task`, `resource`, `view`, `view_update`, `vital` — czyli
**wszystkie 8 wariantów z `rumEvent.types.ts`**. `vital` jest zagnieżdżonym
`oneOf` po `vital.type`: `duration`, `operation_step`, `app_launch` (ten
ostatni tylko mobile; wariant browser-owy `rum-events-browser-schema.json`
go pomija, my bierzemy pełny, bo intake dostaje ruch ze wszystkich SDK).

**Telemetria** (`type: 'telemetry'`, ten sam track `/api/v2/rum`) ma własny
root `telemetry-events-schema.json`: `oneOf` po `telemetry.type`
(`configuration`, `usage`) i `telemetry.status` (`error`, `debug` — dla logów
`telemetry.type: 'log'` nie jest wymagane i próbki Datadoga go nie mają).
`usage.feature` to ~40 wariantów obiektowych.

**Timeseries** (`type: 'timeseries'`, mobile, `timeseries.name`: `cpu` |
`memory`) — root mobilny `rum-events-mobile-schema.json` je dołącza, więc
generator też.

### Dwie unie: browser i mobile (i dwie mniejsze)

Repozytorium publikuje cztery rooty złożone z tych samych plików
`schemas/rum/*.json`, inaczej dobranych. Mobilne SDK (dd-sdk-ios
`tools/rum-models-generator/run.py:19-23`, Android, Flutter, RN, Unity, KMP,
cpp, maui) generują z `rum-events-mobile-schema.json`; browser-sdk z
`rum-events-browser-schema.json`, a w testach waliduje przeciw generycznemu
`rum-events-schema.json`. Generator składa **sumę wszystkich czterech** —
jeden typ Go na wariant — i zapisuje pochodzenie: metoda `SchemaRoots()`
w interfejsie `Event` oraz linia „Listed by:" w doc-komentarzu każdego
wariantu. `TestSchemaRoots` przypina poniższą tabelę.

| wariant | generic | browser | mobile | electron |
|---|:-:|:-:|:-:|:-:|
| action | x | x | x | |
| transition | x | x | | |
| error | x | x | x | x |
| long_task | x | x | x | |
| resource | x | x | x | x |
| view | x | x | x | x |
| view_update | x | x | x | x |
| vital/duration | x | x | x | x |
| vital/operation_step | x | x | x | x |
| vital/app_launch | x | | x | |
| telemetry (4 warianty) | | | x | |
| timeseries/memory, cpu | | | x | |

Konsekwencje dla storage: `timeseries` i `vital/app_launch` **nie przyjdą
z przeglądarki**; `transition` **nie przyjdzie z mobile**. Enum `source`
jest wspólny dla wszystkich rootów (`rum/_common-schema.json`).

Uwaga do telemetrii: root browser-owy jej nie wymienia, bo browser-sdk
generuje dla niej osobny plik TS (`telemetryEvent.types.ts` z
`telemetry-events-schema.json`) — ale wysyła ją na ten sam track `rum`
(§2, `telemetry.ts:186-230`). `SchemaRoots()` oddaje wiernie, co mówią
rooty (tylko mobile); w praktyce telemetria przychodzi z każdego SDK.

**Session Replay ma własny schemat** (`session-replay-browser-schema.json`
→ `segment` → `records[]`, ~110 plików, rekurencyjne drzewo DOM) i osobny
schemat mobilny. Pierwotnie pominięty; właściciel zdecydował, że generujemy —
patrz §8 (`rumevents/replay`).

### Jak wygenerowano

Własny generator `rumevents/internal/gen/main.go` (stdlib, ~1900 linii po
rozszerzeniu o replay, §8; wspólny runtime w `rumevents/internal/jsonx`).
Uzasadnienie w jego nagłówku; skrót: `go-jsonschema` nie scala `allOf`
(a cały schemat to łańcuchy `allOf` redeklarujących `view`, `_dd`, `action`
w kolejnych fragmentach), `quicktype` przepuszcza liczby przez float64 —
i oba gubią nieznane klucze oraz nie mają dyskryminatora. Post-processing
byłby większy niż generator.

Reguły:

- `allOf` scalane rekurencyjnie. Obiekt zadeklarowany raz, we wspólnym
  fragmencie, dostaje jeden wspólny typ (`RumCommonSession`, `RumCommonUsr`,
  `RumViewPerformanceCLS`); obiekt rozszerzany przez wariant dostaje typ
  wariantu (`RumActionEventView` ma `in_foreground`, `RumCommonView` nie).
  Generator sprawdza, że jedna nazwa = jeden kształt.
- **Każda struktura ma `AdditionalProperties map[string]any`** i własne
  `UnmarshalJSON`/`MarshalJSON` (wzorzec z `datadog-api-client-go`): nieznane
  klucze na każdym poziomie przeżywają round-trip.
- `integer` → `int64`, `number` → `json.Number`, nieznane wartości dekodowane
  z `UseNumber()`. Nic nie przechodzi przez float64.
- Pola opcjonalne to wskaźniki z `omitzero` (Go ≥1.24): rozróżniamy brak
  klucza, `[]` i zero. Pola wymagane skalarne są wartościami (`Date int64`,
  `Type string`) — jeśli wymagane pole brakuje w wejściu, przy zapisie wyjdzie
  zero; to jedyny przypadek, w którym round-trip nie jest bajt-w-bajt
  równoważny, i dotyczy tylko zdarzeń niezgodnych ze schematem.
- Enumy to `string` z listą wartości w komentarzu (nowa wartość enumu nie może
  wywalić dekodowania). `oneOf` skalarów (`action.id`: string | string[]) to
  `any`. Zagnieżdżone `oneOf` obiektów (`usage.feature`) są spłaszczone do
  jednej struktury z sumą pól.
- `Decode([]byte) (Event, error)` wybiera wariant **wyłącznie po
  dyskryminatorach** wyliczonych ze schematu (`type`, `vital.type`,
  `telemetry.type`, `telemetry.status`, `timeseries.name`), z poszanowaniem
  `required`. Brak dopasowania → `*UnknownEventError` z `type`; surowy JSON
  zostaje u wołającego. Interfejsy: `Event` ⊃ `RumEvent` ⊃ `RumVitalEvent`,
  `TelemetryEvent`, `RumTimeseriesEvent`.

Wynik: 16 wariantów, 148 struktur, ~7,9k linii w `rum_gen.go`,
`telemetry_gen.go`, `event_gen.go`, każdy z nagłówkiem
`Code generated from DataDog/rum-events-format@<commit>. DO NOT EDIT.`

### Jak odtworzyć

```
git clone https://github.com/DataDog/rum-events-format
git -C rum-events-format checkout ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671
RUM_EVENTS_FORMAT=$PWD/rum-events-format go generate ./rumevents/
```

Z ustawionym `RUM_EVENTS_FORMAT` test `TestGeneratedCodeIsUpToDate`
regeneruje do katalogu tymczasowego i porównuje bajt w bajt z repo; bez
zmiennej jest pomijany. Podbicie wersji schematów = nowy `checkout`,
`go generate`, przegląd diffu.

### Test na prawdziwych danych

W browser-sdk **nie ma fixtur JSON** — testy budują zdarzenia fabryką
`packages/browser-rum-core/test/fixtures.ts` (`createRawRumEvent`, częściowe)
i walidują je Ajv-em wprost przeciw `rum-events-schema.json`
(`test/formatValidation.ts`, e2e: `test/e2e/lib/helpers/validation.ts`),
dokładając sztuczny kontekst. Pełne, referencyjne przykłady to
`rum-events-format/samples/` — 15 RUM + 6 telemetrii, walidowane w CI
Datadoga (`scripts/validate.mjs`). Skopiowane do `rumevents/testdata/samples/`
(Apache-2.0). `rumevents_test.go` sprawdza dla każdej próbki: właściwy typ,
round-trip bez utraty czegokolwiek, nieznane pola na 7 poziomach
zagnieżdżenia, `2^53+1` i `MaxInt64` w polach `int64`, `number`, mapach i
polach nieznanych, przetrwanie `[]`, wybór wariantu po dyskryminatorze przy
mylącym kształcie, oraz błędy dla nieznanego `type`/`vital.type`/`status`.
Warianty bez próbek upstream (`operation_step`, `app_launch`, timeseries) mają
dokumenty syntetyczne.

### Czego nie zrobiono (celowo)

Pakiet **nie jest podpięty** do `intake/` — router RUM (host, klucz z query,
`dd-evp-encoding`, CORS, NDJSON, §5 „co wspólne") to osobna decyzja.

---

## 8. Session Replay → typy Go (`rumevents/replay`) (uzupełnienie 2026-09-20)

Decyzja właściciela: generujemy też replay. Ten sam commit schematów
(`ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671`), ten sam generator z presetem
`replay`, osobny podpakiet.

### Dlaczego osobny podpakiet

Replay to inny track (`/api/v2/replay`), inne ciało (segment JSON w zlib
wewnątrz multipartu, §4), inna przestrzeń dyskryminatorów (`type` rekordu
i `source` danych to **liczby**, nie stringi) i ~2× więcej typów niż RUM.
Trzymanie tego obok `RumEvent` zaśmieciłoby przestrzeń nazw (`Segment`,
`Wireframe`, `TextNode`…) i ruszało dostarczone już API `rumevents`.
Wspólny runtime (dekodowanie z `UseNumber`, `AdditionalProperties`, probe
dyskryminatorów) wylądował w `rumevents/internal/jsonx` i jest używany przez
oba pakiety.

### Schematy: browser i mobile — oba istnieją, oba pokryte

- Root: `session-replay/segment-schema.json` = `oneOf [BrowserSegment,
  MobileSegment]`, dyskryminator `source` (`"browser"` vs enum
  `android | ios | flutter | react-native | kotlin-multiplatform | maui`).
  Generyczny `session-replay-schema.json` to tylko katalog `allOf` dla
  TypeScriptu, nie unia — nie jest rootem.
- **Browser** (`session-replay-browser-schema.json` → `browser/segment`):
  rekordy rrweb-podobne po `type` (2 full snapshot w dwóch formatach, 3
  incremental, 4 meta, 6 focus, 7 view end, 8 visual viewport, 9 frustration,
  12 change), dane inkrementalne po `source` (0 mutation, 1/6 mousemove,
  2 mouse interaction, 3 scroll, 4 viewport resize, 5 input, 7 media, 8
  stylesheet, 9 pointer), drzewo DOM po `type` węzła (0 document, 1 doctype,
  2 element, 3 text, 4 cdata, 11 fragment).
- **Mobile** (`session-replay-mobile-schema.json` → `mobile/segment`):
  potwierdzone — rekordy to **wireframe'y**, `type: 10` full snapshot,
  `type: 11` incremental; wireframe'y po `type` string (`shape`, `text`,
  `image`, `placeholder`, `webview`, `embedded_content`), mutacje po
  `source` (0 mutation, 2 touch, 4 viewport, 9 pointer, 10 composition
  tree), modyfikatory warstw po `type`. Czyli dd-sdk-ios generuje z **tego
  samego repozytorium**, katalog `schemas/session-replay/mobile/` — nie z
  innego źródła. Rekordy wspólne (`common/`: meta, focus, view end, visual
  viewport, viewport resize, pointer) mają jeden typ Go dla obu platform.
- `SchemaRoots()`: `BrowserSegment` → browser, `MobileSegment` → mobile.

### Jak rozwiązano trudne miejsca

- **Unie zagnieżdżone → koperty.** `oneOf`/`anyOf` obiektów z nazwanymi
  wariantami i dyskryminatorem `const`/`enum` daje strukturę-kopertę: jeden
  wskaźnik na wariant + `Raw json.RawMessage` + `Variant()`. Nieznany typ
  rekordu/węzła/wireframe'u (SDK nowszy niż schemat) **nie wywala segmentu**
  — ląduje w `Raw` i wraca przy zapisie bajt w bajt. 8 kopert:
  `BrowserRecord`, `SerializedNodeWithId`, `BrowserIncrementalData`,
  `MobileRecord`, `Wireframe`, `WireframeUpdateMutation`,
  `CompositionLayerModifier`, `MobileIncrementalData`.
- **Dyskryminatory liczbowe i enumy.** Wartości to literały JSON
  (`json.Number("2")`, `"shape"`, `true`), porównywane wartościowo. `enum`
  jest brany tylko wtedy, gdy `const` nie rozróżnia wariantów, i tylko na
  ścieżce, gdzie inny wariant ma `const` (np. `MousemoveData.source ∈ {1, 6}`),
  żeby nowa wartość enumu w niepowiązanym polu nie odrzucała rekordu.
- **Rekurencja.** `SerializedNodeWithId = allOf [{id}, SerializedNode]`,
  a `SerializedNode` to `anyOf` sześciu węzłów, z których trzy mają
  `childNodes: [SerializedNodeWithId]`. Generator: placeholder w cache przy
  wejściu w plik (cykl `$ref` dostaje stabilny wskaźnik, wypełniany po
  powrocie), dystrybucja `allOf` przez unię (`{id} & (A|B)` →
  `(A&{id}) | (B&{id})`, warianty przemianowane na `DocumentNodeWithId`,
  `ElementNodeWithId`…), zbiory `visited` w każdym przebiegu (obniżanie unii,
  nazywanie, odcisk strukturalny z markerem cyklu). W Go domyka się przez
  `ChildNodes []SerializedNodeWithId` i wskaźniki `*T`.
- **Krotki** (`Change` = `[kod, ...payload]`, `anyOf` krotek, `additionalItems`)
  i `type: [string, null]` → `[]any`/`any` z opisem w komentarzu; liczby jako
  `json.Number`. Struktura tych ładunków jest w komentarzu pola, typowanie
  ich to osobna decyzja.
- **`additionalProperties: false`** (3 miejsca) — `AdditionalProperties`
  generowane mimo to, z adnotacją w doc-komentarzu. Próbki Datadoga same
  łamią własny schemat (`textStyle.type: "sans-serif"`,
  `has_full_snapshot_record` w segmencie mobilnym) i test sprawdza, że te
  klucze przeżywają.
- **Jawny `null` w polu wymaganym** (`AddedNodeMutation.nextId`,
  `TextMutation.value`: `anyOf [integer, null]`) — pola wymagane typu `any`
  nie mają `omitzero`, więc `null` wraca przy zapisie. Ta sama poprawka
  objęła `action.id` w RUM. Opcjonalne `null` (np. `previousId: null`)
  nadal zapada do „brak klucza" — semantycznie równoważne.
- Unie bez dyskryminatora (`InputState`: `text` vs `isChecked`,
  `MouseInteraction`: dwa nienazwane kształty) spłaszczone do jednej
  struktury z sumą pól, jak `usage` w telemetrii.

Wynik: **2 typy segmentów, 94 struktury, 8 kopert, ~4,6k linii** w
`browser_gen.go`, `common_gen.go`, `mobile_gen.go`, `segment_gen.go`.
`Decode([]byte) (Segment, error)`, `*UnknownSegmentError` dla nieznanego
`source`. Zero `float64` (test to sprawdza w obu pakietach).

### Jak odtworzyć

```
RUM_EVENTS_FORMAT=$PWD/rum-events-format go generate ./rumevents/...
```

(`./rumevents/` → preset `rum`, `./rumevents/replay/` → preset `replay`;
`TestGeneratedCodeIsUpToDate` w obu pakietach porównuje bajt w bajt).

### Testy: próbki upstream vs syntetyki

Upstream (`rum-events-format/samples/session-replay/`, skopiowane do
`rumevents/replay/testdata/samples/`): **4 rekordy browser** (full snapshot
v1, full snapshot bez `format`, full snapshot w formacie change, change
record — bez opakowania segmentu), **7 rekordów mobile** (full snapshot ×2,
incremental ×2, focus, meta, view end) i **1 segment mobile**. Każdy
dekodowany do właściwego wariantu i round-tripowany bez utraty.

**Syntetyczne** (nie ma ich upstream, mówię wprost): opakowanie segmentu
browser wokół rekordów upstream; rekordy browser typów 3 (wszystkie 9
`source`), 4, 6, 7, 8, 9; mobile typ 8 i `source` 2/4/9; drzewo DOM
12 poziomów z nieznanym kluczem i nieznanym typem węzła (`type: 42`) w
środku; rekord `type: 99`; `2^53+1` i `MaxInt64` w polach wymaganych,
nieznanych i w `Raw`. Test pokrycia sprawdza, że każdy wariant
`BrowserRecord` i `BrowserIncrementalData` został trafiony.

### Poza zakresem

`intake/` nietknięte. Router replay (multipart `segment` + `event`, zlib,
zasoby canvas) — osobna decyzja; ten pakiet dekoduje już rozpakowany JSON
segmentu.
