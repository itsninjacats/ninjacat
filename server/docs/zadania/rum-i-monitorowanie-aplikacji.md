# RUM, monitorowanie aplikacji i błędy

Następny duży obszar po Kubernetesie. Trzy powiązane rzeczy, które warto
rozważać razem, bo dzielą źródło danych i połowę infrastruktury.

**Stan:** nic z tego nie jest zrobione.

**Kolejność:** zaraz po domknięciu routerów z agenta. Ustalone 2026-09-20 —
najpierw brakujące hosty ze skanu, potem endpointy RUM-owe. Zakres na teraz
taki sam jak wszędzie indziej: odebrać i odkodować, bez zapisu.

Warstwami zapisu i korelacji zajmuje się Michał osobiście.

---

## 1. RUM — Real User Monitoring

Co użytkownik faktycznie widzi w przeglądarce: czasy ładowania, Core Web
Vitals, kliknięcia, błędy JavaScriptu, żądania XHR/fetch, nagrania sesji.

### Skąd to leci

**Nie z agenta.** Z przeglądarki, przez SDK wpięty w stronę:

```js
datadogRum.init({
  applicationId: '...',
  clientToken: '...',
  site: 'datadoghq.com',       // <- to sklada adres intake'u
})
```

SDK jest na Apache 2.0 (`DataDog/browser-sdk`), więc da się go przestawić na
nas — sprawdzaliśmy to wcześniej w tej sesji.

### Endpoint

Z `pkg/opentelemetry-mapping-go/otlp/rum/rum.go`:

```
POST /api/v2/rum?batch_time=<ms>
                &ddtags=<tagi>
                &ddsource=browser
                &dd-evp-origin=browser
                &dd-request-id=<losowe>
                &dd-api-key=<klucz>
```

**Klucz API jedzie w QUERY STRINGU**, nie w nagłówku `Dd-Api-Key`. To jedyne
takie miejsce w całym protokole Datadoga i wymaga osobnej ścieżki w naszym
middleware — `RequireAPIKey` czyta wyłącznie nagłówek.

Powód jest prozaiczny: to żądanie wychodzi z przeglądarki, często jako
`sendBeacon` przy zamykaniu karty, gdzie nagłówków ustawić się nie da.

Konsekwencja bezpieczeństwa: w RUM używa się **client tokena**, nie klucza
API — bo ląduje w kodzie strony, widoczny dla każdego. Client token może
tylko wysyłać, nie czytać. Musimy mieć takie rozróżnienie w tabeli `api_key`,
albo osobną tabelę.

### Co trzeba zrobić

1. Endpoint `/api/v2/rum` z uwierzytelnianiem z query stringu
2. Osobny typ poświadczenia: client token (tylko zapis) obok klucza API
3. CORS — przeglądarka nie wyśle nic bez poprawnych nagłówków preflight
4. Tabela na zdarzenia RUM. Format to JSON, **jedno zdarzenie na linię**
   (NDJSON), nie tablica
5. Przestawienie `browser-sdk` — `site` albo `proxy` w `init()`

### Nagrania sesji

Session Replay to osobny, dużo grubszy strumień — serializowany DOM plus
mutacje, po kilkaset kilobajtów na sesję. Oddzielna decyzja; przy własnym
systemie prawdopodobnie do pominięcia na starcie.

---

## 2. Error Tracking

**Nie ma własnego intake'u.** To tryb APM:

```
apm_config.error_tracking_standalone.enabled
```

Włączony, trace-agent zatrzymuje wyłącznie spany z błędem i znaczy je
`_dd.error_tracking_standalone.error = "true"`
(`pkg/trace/agent/agent.go:1204`).

Czyli błędy przychodzą **zwykłym `/api/v0.2/traces`**, który już odbieramy.
Brakuje tylko rozpoznania i osobnego traktowania:

- grupowanie w „issues" po odcisku stosu wywołań, a nie po pojedynczym spanie
- zliczanie wystąpień zamiast trzymania każdego
- pierwsze/ostatnie wystąpienie, dotknięte wersje

Druga droga to błędy z RUM — JavaScriptowe, z przeglądarki, przez `/api/v2/rum`.
Docelowo obie powinny trafiać do jednego widoku.

**To jest najtańsza rzecz z całego dokumentu**, bo transport już mamy.

---

## 3. Monitorowanie aplikacji — czego brakuje wokół

Mamy już odbiór śladów i statystyk APM (`/api/v0.2/traces`, `/api/v0.2/stats`),
ale nic z nimi nie robimy. Do pełnego obrazu brakuje:

```
/api/v2/profile            intake.profile.<site>      profilowanie ciagle
/api/v2/debugger           debugger-intake.<site>     Dynamic Instrumentation
/api/v2/srcmap             sourcemap-intake.<site>    source mapy (pod RUM)
/api/v0.1/pipeline_stats   trace.agent.<site>         Data Streams
/api/v2/apmtelemetry       instrumentation-telemetry-intake
```

Wszystkie wypadły przy przeglądaniu `pkg/trace/config/endpoints.go`.
`/api/v2/srcmap` jest bezpośrednio związany z RUM — bez source map błędy
JavaScriptu pokazują zminifikowane stosy wywołań.

---

## Kolejność, którą bym zaproponował

```
1. Error Tracking na sladach       transport JUZ mamy, brakuje grupowania
2. /api/v2/rum + client tokeny     odblokowuje cala reszte RUM
3. /api/v2/srcmap                  bez tego bledy JS sa nieczytelne
4. Session Replay                  osobna decyzja, duzy wolumen
5. profiling, debugger             dopiero gdy slady beda uzywane
```

Punkt 1 jest tani i daje widoczny efekt od razu. Punkt 2 wymaga zmian
w uwierzytelnianiu, więc nie jest drobiazgiem.

---

## Do rozstrzygnięcia

- **Client token kontra klucz API.** Trzeba dorobić poświadczenie tylko do
  zapisu. Dzisiejszy `apikeys.Store` ma jeden rodzaj klucza.
- **CORS.** Nasz odbiornik nie odpowiada dziś na preflight. Przeglądarka bez
  tego nie wyśle nic.
- **Czy SDK da się przestawić bez forka.** `init()` przyjmuje `proxy`, co
  wygląda na wystarczające, ale nie sprawdziliśmy tego empirycznie.
