# Ślady — korelacja, propagacja i dlaczego zegary kłamią

Notatki do przyszłej obsługi śladów w ninjacacie. Część ustaleń zweryfikowana
na binarkach agenta 7.83.2 i module `go.opentelemetry.io/proto/otlp`,
część to pomiary z naszych własnych zrzutów.

---

## 1. Czym jest ślad

Drzewo span'ów o wspólnym `trace_id`. Każdy span niesie:

```
trace_id     wspolny dla calego drzewa
span_id      identyfikator tego span'a
parent_id    identyfikator wolajacego
service      operation
start_time   duration
kind         tags
```

Drzewo **nie jest wnioskowane z czasu** — jest zapisane wprost w `parent_id`.
To ma znaczenie, patrz §4.

---

## 2. Propagacja — jak ślad przechodzi między usługami

### `traceparent` (W3C Trace Context)

```
traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01
             ▲  ▲                                ▲                ▲
             │  │                                │                └─ flagi
             │  │                                └─ span-id (8 B / 16 hex)
             │  └─ trace-id (16 B / 32 hex)
             └─ wersja (obecnie 00)
```

- **`trace-id`** — ten sam przez całe drzewo
- **`span-id`** — identyfikator **wołającego**; u odbiorcy staje się jego `parent_id`
- **flagi** — najmłodszy bit: `01` = próbkowany, `00` = do wyrzucenia

### `tracestate` — dane specyficzne dla dostawcy

```
tracestate: dd=s:1;o:rum,congo=t61rcWkgMzE
```

Lista `klucz=wartość` po przecinkach. Każdy dostawca dopisuje swój wpis
i nie rusza cudzych — dzięki temu ślad przechodzi przez usługi instrumentowane
różnymi bibliotekami, a każda odzyskuje swój kontekst.

### Formaty starsze, wciąż spotykane

```
Datadog:  x-datadog-trace-id, x-datadog-parent-id, x-datadog-sampling-priority
Zipkin:   X-B3-TraceId, X-B3-SpanId, X-B3-ParentSpanId
Jaeger:   uber-trace-id
```

**Zweryfikowane:** binarki agenta 7.83.2 zawierają ciągi `traceparent`,
`tracestate` oraz `B3` — agent rozumie propagację niezależnie od tego,
czyja biblioteka zaczęła ślad.

### Jak to wygląda w całości

```
usluga A:  generuje trace_id + span_id
           wysyla: traceparent: 00-<trace_id>-<span_id_A>-01
              ↓
usluga B:  czyta naglowek
           swoj span: trace_id = ten sam, parent_id = span_id_A
           wysyla dalej: 00-<trace_id>-<span_id_B>-01
              ↓
usluga C:  to samo
```

Span'y trafiają do backendu **niezależnie, każdy ze swojej usługi**.
Drzewo składa się dopiero przy odczycie, po wspólnym `trace_id`.

---

## 3. Sama korelacja nie jest trudna

Czytanie nagłówka i przekazywanie dalej. Korelacja z logami jest jeszcze
prostsza — biblioteka wstrzykuje `trace_id` do każdego wpisu, a Ty robisz
`WHERE trace_id = ...`. Zwykły join.

**Trudne jest co innego:**

**Składanie śladu.** Span'y przychodzą z różnych usług, nie po kolei,
z opóźnieniem. **Nie wiadomo, kiedy ślad jest kompletny** — usługa D może
odesłać swój span dziesięć sekund po korzeniu. Trzeba przyjąć okno czasowe
i pogodzić się z niekompletnymi drzewami.

**Tail-based sampling.** Head-based (decyzja przy korzeniu) jest łatwy.
Tail-based — „zachowaj ślad tylko jeśli ma błąd albo trwał ponad sekundę" —
wymaga buforowania **wszystkich** span'ów śladu do momentu decyzji.
To dlatego procesor `tailsampling` w kolektorze OTel zjada pamięć.
**W naszej skali: pomijamy, head-based wystarczy.**

**Niezgodność formatów ID.** Datadog historycznie używał 64-bitowych `trace_id`,
W3C i OTel używają 128-bitowych. Datadog dorobił obsługę 128 bitów, ale górne
bity trzyma w osobnym tagu `_dd.p.tid`. Konwersja w obie strony wymaga uwagi.

---

## 4. Zegary — struktura kontra oś czasu

To jest najważniejsze rozróżnienie w całym dokumencie.

| | Skąd | Wiarygodność |
|---|---|---|
| **struktura drzewa** | `parent_id` | **dokładna**, niezależna od zegarów |
| **czas trwania span'a** | zegar **monotoniczny** | **dokładny**, odporny na korekty NTP |
| **pozycja na osi czasu** | zegar ścienny maszyny | **niepewna** między maszynami |

Czyli psuje się **położenie**, nie **długość**. A przy szukaniu wąskiego gardła
zwykle interesuje Cię długość.

### Skala problemu — pomiar z naszej sesji

Agent sam raportuje swój rozjazd metryką `ntp.offset`. Z naszego zrzutu
`captures/20260919T202117.987_POST_api_v1_series_0031.txt`:

```json
"metric": "ntp.offset",
"points": [[1789842066, 0.852919907]]
```

**0,85 sekundy** — na kontenerze, na laptopie z działającym internetem.
Docker Desktop wstrzymuje i budzi swoją maszynę wirtualną, więc przypadek
skrajny, ale pokazuje skalę.

Realistyczne widełki:

```
to samo centrum danych, dobry NTP      ponizej 1 ms
chmura z lokalnym zrodlem czasu         mikrosekundy (AWS/GCP maja PTP)
przez publiczny internet                10–100 ms
laptop / VM po uspieniu                 setki ms do sekund
```

### Dlaczego NTP nie może być idealny

Fundamentalne ograniczenie: NTP szacuje przesunięcie **zakładając symetryczne
opóźnienie sieci** — mierzy podróż w obie strony i dzieli na pół. Przy trasie
asymetrycznej błąd wynosi połowę tej asymetrii i **protokół nie ma jak tego
wykryć**.

Do tego: zablokowany port 123 (częste w sieciach korporacyjnych, i nikt tego
nie zauważa), wstrzymywanie maszyn wirtualnych, sekundy przestępne
(Google rozmazuje na 24 h, inni robią skok — przez dobę maszyny się nie zgadzają),
dryf zegara sprzętowego rzędu kilkunastu ppm.

Dokładniejszy jest **PTP** (mikrosekundy), ale wymaga wsparcia sprzętowego
w przełącznikach i kartach sieciowych.

### Kiedy to boli

```
span 200 ms, rozjazd 3 ms      niewidoczne
span 5 ms, rozjazd 3 ms        wykres klamie
span 200 µs                    rozjazd DOMINUJE
```

Dla typowych żądań HTTP przez sieć NTP wystarcza. Dla śladów
wewnątrzprocesowych albo gRPC w tym samym klastrze — zaczyna przeszkadzać.

---

## 5. Korekcja rozjazdu i rola `span.kind`

Klasyczna heurystyka (Zipkin stosuje ją od lat):

> Dziecko nie może zacząć się przed rodzicem ani skończyć po nim.
> Jeśli tak wyszło — przesuń oś dziecka o minimalną wartość, która to naprawia.

**Ale nie wolno jej stosować wszędzie.** Od tego jest `span.kind`,
zweryfikowany w `go.opentelemetry.io/proto/otlp`:

```
SPAN_KIND_CLIENT / SPAN_KIND_SERVER       wywolanie synchroniczne
                                          -> heurystyka OBOWIAZUJE
SPAN_KIND_PRODUCER / SPAN_KIND_CONSUMER   kolejka komunikatow
                                          -> dziecko MOZE skonczyc pozniej
SPAN_KIND_INTERNAL                        w obrebie procesu, ten sam zegar
SPAN_KIND_UNSPECIFIED
```

Przy kolejce komunikatów „dziecko po rodzicu" jest **poprawne**, nie błędne —
producent wysyła i idzie dalej, konsument przetwarza pięć minut później.
Korekcja zepsułaby prawdziwy obraz.

Dlatego heurystykę nakłada się **wyłącznie na pary CLIENT/SERVER**.

---

## 6. Konsekwencja dla schematu ClickHouse

Dwa sprzeczne wzorce dostępu:

```sql
-- "pokaz mi ten jeden slad"
WHERE trace_id = '4bf92f35...'

-- "pokaz wolne endpointy w usludze"
WHERE service = 'api' AND timestamp > now() - INTERVAL 1 HOUR
```

Jeden `ORDER BY` ich nie obsłuży dobrze — to ta sama sytuacja, co przy
metrykach (patrz `ORDER BY (metric, host, timestamp)`).

Klasyczne rozwiązanie: tabela główna posortowana po `(service, timestamp)`
plus **osobna tabelka indeksowa** `trace_id → czas`, zasilana materialized view.
Dzięki temu pytanie o konkretny ślad nie skanuje całości.

**Decyzję trzeba podjąć przed napełnieniem tabeli** — zmiana `ORDER BY`
to przepisanie danych, nie `ALTER`.

---

## 7. Kolejność wdrażania

```
1. /v0.4/traces         najczesciej uzywana wersja formatu Datadoga
   /v1/traces           OTLP
2. /v0.6/stats          bez tego UI nie pokaze statystyk
3. skladanie drzewa     WHERE trace_id, sortowanie po parent_id
4. korekcja zegarow     heurystyka CLIENT/SERVER
5. flamegraph           to jest ta wlasciwa robota
```

Ślady dają wartość dopiero z dobrym interfejsem, a bez niego są kosztowne
w przechowywaniu i bezużyteczne w oglądaniu. Dlatego sensowna kolejność
całego projektu to **metryki → alerty → logi → ślady**.
