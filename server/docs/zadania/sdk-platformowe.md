# SDK platformowe — odbiór z aplikacji, nie z agenta

**Na przyszłość. Nie robimy tego teraz.**

Wszystko, co dotąd odbieramy, przychodzi **z agenta**. Ten obszar jest inny:
dane wysyła sama aplikacja — przeglądarka, telefon, aplikacja desktopowa —
bez agenta w łańcuchu.

To zmienia trzy rzeczy naraz: uwierzytelnianie, CORS i to, że nadawcy są
publiczni i niezaufani.

---

## Jakie SDK istnieją

Datadog utrzymuje osobne SDK na platformę, wszystkie otwarte:

```
browser-sdk                   JS/TS — przegladarka          Apache 2.0
dd-sdk-android                Kotlin/Java
dd-sdk-ios                    Swift
dd-sdk-flutter                Dart
dd-sdk-reactnative            JS + mosty natywne
dd-sdk-kotlin-multiplatform
dd-sdk-unity                  gry
dd-sdk-roku                   telewizory
```

**Do sprawdzenia:** ta lista jest z pamięci, nie z kodu. Przed implementacją
trzeba ją potwierdzić — repozytoria są publiczne pod `github.com/DataDog/`.

`browser-sdk` jest już sklonowany w scratchpadzie sesji jako punkt wyjścia.

---

## Co wiemy na pewno

Adres intake'u RUM, z `pkg/opentelemetry-mapping-go/otlp/rum/rum.go` w agencie:

```
POST /api/v2/rum?batch_time=<ms>
                &ddtags=<tagi>
                &ddsource=browser        <- ROZNI SIE per platforma
                &dd-evp-origin=browser
                &dd-request-id=<losowe>
                &dd-api-key=<token>
```

`ddsource` i `dd-evp-origin` są parametrami, więc jedna ścieżka obsługuje
wszystkie platformy, a rozróżnia je wartość pola. To dobra wiadomość —
prawdopodobnie nie trzeba endpointu per SDK.

**Do sprawdzenia:** czy mobilne SDK faktycznie używają tej samej ścieżki,
czy mają własne. Sprawdzić w `dd-sdk-android` i `dd-sdk-ios`, jak składają URL.

---

## Trzy rzeczy, które trzeba zbudować przed czymkolwiek

### 1. Client tokeny

SDK w przeglądarce ma swój token **w kodzie strony**, widoczny dla każdego.
Dlatego to nie może być klucz API — musi być poświadczenie **tylko do zapisu**,
bez prawa odczytu czegokolwiek.

Dzisiejszy `apps/apikeys` zna jeden rodzaj klucza i daje pełny dostęp.
Potrzebny drugi typ albo kolumna z zakresem uprawnień.

### 2. Uwierzytelnianie z query stringu

`RequireAPIKey` czyta wyłącznie nagłówek `Dd-Api-Key`. RUM przysyła token
jako `dd-api-key` w query stringu — bo żądanie często wychodzi przez
`sendBeacon` przy zamykaniu karty, gdzie nagłówków ustawić się nie da.

To musi być **osobna ścieżka w middleware**, a nie rozluźnienie istniejącej:
klucz w URL-u trafia do logów serwera i historii przeglądarki, więc wolno na
to pozwolić tylko tam, gdzie nie ma wyboru, i tylko dla client tokenów.

### 3. CORS

Dziś nie odpowiadamy na preflight. Bez `Access-Control-Allow-Origin`
przeglądarka nie wyśle **nic** — i nie zobaczymy nawet próby, bo zatrzyma się
przed żądaniem właściwym.

Dochodzi decyzja, które originy wpuszczamy. Przy jednym kliencie wystarczy
lista z konfiguracji; przy wielu tenantach origin trzeba związać z tokenem.

---

## Czego się spodziewać poza RUM

Te SDK wysyłają więcej niż same zdarzenia RUM:

```
logi aplikacji        prawdopodobnie /api/v2/logs — czyli to, co juz mamy
slady z przegladarki  fetch/XHR z naglowkami propagacji
nagrania sesji        serializowany DOM + mutacje, setki kB na sesje
crash reporty         mobilne, ze stosami natywnymi
source mapy           /api/v2/srcmap — bez nich stosy JS sa zminifikowane
```

**Do sprawdzenia:** czy logi z `browser-logs` idą na `/api/v2/logs`, czy mają
własny track. Jeśli to samo — jeden endpoint mniej do napisania.

---

## Jak przestawić SDK na nas

`datadogRum.init()` przyjmuje `site`, a w nowszych wersjach `proxy`. Wygląda
na wystarczające, ale **nie sprawdziliśmy tego empirycznie** — a to jest
pierwsza rzecz do zweryfikowania, bo od niej zależy, czy w ogóle da się to
zrobić bez forkowania SDK.

Przy mobilnych to samo pytanie: czy konfiguracja pozwala podmienić adres,
czy jest zaszyty.

---

## Kolejność

```
1. Sprawdzic empirycznie, czy browser-sdk da sie przestawic bez forka
2. Client tokeny + uwierzytelnianie z query stringu + CORS
3. /api/v2/rum — jedna sciezka, rozroznienie po ddsource
4. Potwierdzic, czy mobilne SDK uzywaja tej samej sciezki
5. /api/v2/srcmap
6. Session Replay — osobna decyzja, duzy wolumen
```

Punkt 1 jest warunkiem wstępnym całej reszty. Jeśli okaże się, że SDK nie da
się przekierować bez forkowania, cały obszar wygląda inaczej.

Powiązane: `rum-i-monitorowanie-aplikacji.md` opisuje stronę serwerową tego
samego tematu — Error Tracking, profiling, debugger.
