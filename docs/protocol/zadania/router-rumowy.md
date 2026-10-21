# Router RUM — co trzeba zrobić

**Do zrobienia. Rozpoznanie zamknięte, typy wygenerowane, kod routera przed nami.**

Zbiera w jedno ustalenia z trzech rozpoznań — `browser-sdk-rozpoznanie.md`,
`sdk-ios-rozpoznanie.md`, `sdk-android-rozpoznanie.md` — plus stan pakietu
`server/rumevents/`.

---

## Co już jest

`server/rumevents/` — typy Go wygenerowane ze schematów `DataDog/rum-events-format`,
commit `ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671`. 16 wariantów, 148 struktur.
Każda struktura ma `AdditionalProperties`, liczby przez `json.Number`,
`Decode([]byte) (Event, error)` wybiera wariant po dyskryminatorze.
Metoda `SchemaRoots()` mówi, które platformy mogą dany wariant wysłać.

**Trzy SDK generują modele z tego samego repozytorium schematów** — potwierdzone
w kodzie każdego z nich. Czyli te typy pokrywają przeglądarkę, iOS, Androida
oraz wrappery (Flutter, React Native, Unity, KMP, .NET MAUI, Roku, cpp, Electron).

---

## Czego NIE da się załatwić jednym handlerem

Kształt zdarzeń jest wspólny. **Transport nie jest.**

| | przeglądarka | iOS | Android |
|---|---|---|---|
| przekierowanie | `proxy` jako funkcja | `customEndpoint: URL?` | `useCustomEndpoint(url)` |
| klucz | query `?dd-api-key=` | nagłówek `DD-API-KEY` | nagłówek `DD-API-KEY` |
| kompresja | `?dd-evp-encoding=deflate` | `Content-Encoding: deflate` | `Content-Encoding: gzip` |
| logi: ramka | NDJSON | tablica JSON | tablica JSON |
| logi: czas | `date` liczba ms | `date` | `date` string ISO-8601 |
| CORS | wymagany | nie dotyczy | nie dotyczy |

Nasze `RequireAPIKey` i `Decompress()` **działają dla iOS i Androida bez zmian**.
Dla przeglądarki nie działa ani jedno, ani drugie.

---

## Pułapki, które cicho niszczą dane

**1. CORS (tylko przeglądarka).** Brak `Access-Control-Allow-Origin` w odpowiedzi
→ przeglądarka pokazuje SDK `status 0` → SDK ponawia `status 0` **tylko gdy jest
offline**, więc online uznaje to za sukces i **wyrzuca batch**. Nie dostaniemy nic
i nikt się nie dowie.

**2. Tylko 202 znaczy sukces (Android).** Kod 200 to dla niego `UnknownHttpError`
i **skasowanie batcha** z logiem ERROR (`DataOkHttpUploader.kt:206-237`).
Router RUM musi konsekwentnie odpowiadać 202 — odwrotnie niż `/api/v0.2/traces`,
gdzie agent wymaga 200 z `rate_by_service`.

**3. TLS (Android).** OkHttp z `ConnectionSpec.RESTRICTED_TLS`, wyłącznie HTTPS.
Cleartext tylko przez wewnętrzną flagę opisaną u nich jako „DO NOT USE".
**Intake musi mieć certyfikat z publicznego CA** — self-signed nie przejdzie.

**4. Czas w logach — trzy zapisy jednej rzeczy.** Liczba ms, string ISO-8601
i nasze `timestamp` w `HandleLogs`. Obsłużyć wszystkie trzy, pusty = czas
przyjęcia, nigdy 1970.

---

## Zakres routera

Host: `browser-intake.<nasza-domena>` albo osobny per platforma — do decyzji.

| ścieżka | co | typ |
|---|---|---|
| `/api/v2/rum` | zdarzenia RUM + telemetria | `rumevents.Decode` |
| `/api/v2/logs` | logi z SDK | `datadogV2.HTTPLogItem` |
| `/api/v2/replay` | Session Replay | multipart, `profParts` |
| `/api/v2/spans` | spany z mobile | **trzeci format spanów**, NDJSON kopert |
| `/api/v2/profile` | profiling | `profParts`, pprof (iOS) / perfetto (Android) |

`/api/v2/spans` to nowy kanał, nie ma nic wspólnego z `/api/v0.2/traces` od
agenta — koperta `{"spans":[…],"env":…}` per linia, span płaski z kluczami
`meta.*`/`metrics.*`, trace_id hex.

---

## Session Replay — co z tym robić

Nie parsujemy rekordów. Datadog sam ich nie parsuje na wejściu
(`segmentCollection.ts:20-26`: „stored without any processing from the intake").

Rozbieramy multipart, dekodujemy część `event` (metadane), segment rozpakowujemy
z zlib i **zapisujemy bajty**. Typy są wygenerowane, gdyby kiedyś były potrzebne.

Uwaga na różnicę: przeglądarka wysyła jedną część `segment`, mobile **N części**
(`file<i>`) i `event` jako tablicę. Rekordy mobilne to wireframe'y, nie DOM.

---

## Uwierzytelnianie

`clientToken` to **nie jest** klucz API agenta — jest publiczny, siedzi w kodzie
strony, tylko-do-zapisu. `applicationId` to nie poświadczenie, tylko wymiar danych.

Dla przeglądarki potrzebny osobny middleware czytający klucz **z query stringu**.
To siódmy schemat uwierzytelniania w tym serwerze i pierwszy, który w ogóle nie
idzie nagłówkiem — patrz `docs/zadania/` i komentarz w `apikey_mw.go`.

Ostrzeżenie: klucz w URL-u wyląduje w logach dostępowych każdego proxy przed
serwerem.

---

## Kształt danych do zapisu — uwagi dla warstwy storage

**RUM nie jest append-only.** Widok żyje, dopóki użytkownik jest na stronie,
i SDK wysyła go wielokrotnie z rosnącym `_dd.document_version` i aktualizowanymi
licznikami. Jeden `view.id` to seria aktualizacji jednego wiersza, nie seria
zdarzeń. Potrzebna semantyka „ostatnia wersja wygrywa".

Klucze: `application.id` → `session.id` (wymagane w każdym zdarzeniu) →
`view.id` (opcjonalne) → zdarzenie. `usr.id` identyfikuje osobę, `session.id`
tylko wizytę.

Nowy widok powstaje przy: wejściu (`initial_load`), nawigacji (`route_change`),
wygaśnięciu sesji (`session_renewal`) i powrocie z bfcache. Pole `loading_type`
to rozróżnia — mieszanie `initial_load` z `route_change` w jednej średniej
czasu ładowania daje bzdury.

Czego SDK **nie** wysyła, a co widać w UI Datadoga: `@session.frustration.count`.
To rollup liczony po ich stronie z `view.frustration.count` po `session.id`.

`SchemaRoots()` mówi, co skąd może przyjść: `timeseries/*` i `vital/app_launch`
tylko z mobile, `transition` tylko z przeglądarki.

---

## Dryf schematów

Snapshot schematów w dd-sdk-android jest **starszy niż master** (brak `transition`,
`vital-duration` i kilku innych). Telefony chodzą na starych wersjach SDK przez
miesiące. Generować z mastera i tolerować dryf — `AdditionalProperties` ratuje
w drugą stronę, gdy nowsze SDK doda pole.

---

## Otwarte decyzje

- jeden host dla wszystkich platform czy osobne
- czy wymagane skalary w `rumevents` mają być wskaźnikami (100% round-trip
  kosztem ergonomii) — patrz raport generatora
- czy `/api/v2/spans` idzie do routera RUM czy do trace'ów
- czy `/api/v0.2/stats` z Androida (3.14+, opt-in) zje istniejący
  `HandleAPMStats` — do sprawdzenia
