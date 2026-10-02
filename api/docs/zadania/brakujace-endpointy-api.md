# Brakujące endpointy publicznego API

Cztery ścieżki, które `datadog-api-client` wywołuje, a my odpowiadamy 404.
Wykryte przez `HandleUnknown` — ten sam mechanizm, którym odkryliśmy protokół
agenta.

```
POST /api/v1/distribution_points
GET  /api/v1/metrics
GET  /api/v1/search
GET  /api/v1/query
```

Nie są równie pilne i nie są tej samej wagi. Poniżej w kolejności, w jakiej
mają sens.

---

## 1. GET /api/v1/metrics i GET /api/v1/search

**Wyjdą same przy katalogu metryk.** Opisany w `silnik-kwerend.md` — tabela
nazwa/typ/jednostka/ostatnio-widziana, potrzebna niezależnie, bo bez niej
`ListMetrics` nie ma skąd wziąć typu metryki.

Gdy katalog powstanie, te dwa endpointy to cienka warstwa nad nim. Nic do
przemyślenia, sama robota.

---

## 2. POST /api/v1/distribution_points

Tu jest realna decyzja projektowa, więc opisana szerzej.

### Pola

Payload z ich dokumentacji wygląda ubogo, bo przykład wypełnia minimum:

```json
{
  "series": [
    { "metric": "system.load.1.dist", "points": [ [1636629071, [1.0, 2.0]] ] }
  ]
}
```

Ale schemat ma pięć pól. Potwierdzone dwiema drogami — introspekcją modeli
`datadog-api-client` i ich dokumentacją API, zgodne co do joty:

| pole | typ | uwagi |
|---|---|---|
| `metric` | string | wymagane |
| `points` | `[[ts, [v1, v2, …]], …]` | wymagane, surowe wartości, POSIX w sekundach |
| `host` | string | opcjonalne |
| `tags` | `[string]` | opcjonalne |
| `type` | enum | jedyna wartość `"distribution"`, domyślna |

`type` jest stałą i nie niesie żadnej informacji — inaczej niż przy metrykach,
gdzie ten sam parametr rozstrzyga o dozwolonych agregacjach.

### Dwie pułapki, obie już rozpoznane

**Timestamp jest floatem, nie intem.** Dokumentacja mówi „POSIX time in
seconds", ale ich własny przykład robi `datetime.now().timestamp()`, co w
Pythonie zwraca float. W JSON-ie przyjdzie `1636629071.234567`, a parsowanie
tego wprost do `int64` wywali się na `cannot unmarshal number`.

Ten sam kształt mamy już rozwiązany w `parseSeriesJSONv1`: punkt czytamy jako
`[2]float64`, a dopiero potem `wireTime(int64(p[0]))`.

**Ciało jest skompresowane.** `content_encoding` ma jedyną dozwoloną wartość
`deflate`, więc ten endpoint praktycznie zawsze przyjdzie spakowany.

Sprawdzone empirycznie przeciwko `/api/v2/series` — i gołe deflate, i zlib
przechodzą przez `capture.go` i lądują w bazie poprawnie. Czyli po stronie
dekompresji nie ma nic do zrobienia.

### To co innego niż wysyła agent

Distribution przychodzi do nas **dwiema drogami o różnych formatach**:

| kto | ścieżka | co niesie |
|---|---|---|
| agent | `/api/beta/sketches` | DDSketch w protobufie — **gotowe kubełki** |
| klient API | `/api/v1/distribution_points` | JSON — **surowe wartości** |

Agent zrobił już całą robotę: zbudował szkic o logarytmicznych kubełkach,
policzył count/min/max/sum. My tylko zapisujemy.

Klient API nie robi nic. Daje listę liczb.

### Decyzja: budować szkic u nas

Trzy możliwości:

1. **Zbudować DDSketch po naszej stronie** i zapisać do `sketches` — tej samej
   tabeli, co dane z agenta.
2. Zapisać surowe wartości do osobnej tabeli.
3. Policzyć count/min/max/avg/sum i zapisać jako zwykłe metryki.

**Wybieramy (1).** Powód: obie drogi mają się zbiegać na jednym formacie
przechowywania. Inaczej zapytanie o percentyl musiałoby wiedzieć, którymi
drzwiami dane weszły — a to jest dokładnie ten rodzaj rozgałęzienia, który
potem żyje w każdym zapytaniu.

(3) odpada, bo traci cały sens distribution: percentyli nie da się odtworzyć
ze średniej. (2) odpada, bo mnoży tabele i przerzuca problem na odczyt.

Do zbudowania szkica służy `github.com/DataDog/sketches-go` — oficjalna
implementacja DDSketch, ta sama, której używa agent. Parametry (dokładność
względna, limit kubełków) muszą odpowiadać temu, co przysyła agent, inaczej
scalanie szkiców z obu źródeł da śmieci.

### Czego brakuje w payloadzie

Brak hosta i tagów oznacza, że wiersz w `sketches` trzeba wypełnić czymś
sensownym. Host pusty, nie zmyślony — tak jak przy `UNSPECIFIED` w metrykach:
zapisujemy to, co powiedział nadawca, a domyślanie się zostawiamy na odczyt.

---

## 3. GET /api/v1/query

Najmniej pilny i **strategicznie najważniejszy**.

To jest publiczne API zapytań Datadoga, z ich składnią:

```
GET /api/v1/query?from=...&to=...&query=avg:ninjacat.node.goroutines{*}
```

Implementacja oznacza, że **datasource Datadoga w Grafanie działa przeciwko
ninjacatowi**. Bez pisania wtyczki — wystarczy przestawić adres, dokładnie tak
jak przestawiliśmy agenta i klienty Pythona.

To samo dotyczy ich CLI, providera Terraform i każdego narzędzia, które umie
gadać z Datadogiem.

### Dlaczego dopiero po silniku kwerend

Ten endpoint to **parser ich składni plus nasz kompilator**. Bez kompilatora
AST→SQL nie ma do czego parsować, a pisanie go osobno pod ten jeden endpoint
oznaczałoby drugą implementację obok tej, której używa panel.

Kolejność jest więc wymuszona:

```
silnik kwerend (AST + kompilator)
        │
        ├─> panel        (buduje AST z klikania)
        └─> /api/v1/query (parsuje tekst do AST)
```

Ich składnia jest przy okazji odpowiedzią na „jaki język zapytań" — skoro i tak
budujemy kompilator, ich gramatyka jest darmową kompatybilnością z całym
ekosystemem zamiast wymyślania własnej.

---

## Kolejność

1. Katalog metryk → `/api/v1/metrics`, `/api/v1/search` przy okazji
2. `/api/v1/distribution_points` — samodzielne, nie blokuje niczego
3. Silnik kwerend
4. `/api/v1/query` — dopiero gdy istnieje kompilator
