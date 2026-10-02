# Silnik kwerend — dokument projektowy

Stan na dziś: `apps/query` umie zadać jedno pytanie — „jedna metryka, opcjonalnie
zawężona do hostów, w oknie czasu". `QuerySeries` jest płaską strukturą, a SQL
powstaje w `worker.go` przez sklejenie kilku kawałków. To wystarczyło, żeby
zobaczyć wykres, i nie wystarczy na nic więcej.

Ten dokument opisuje, w co to ma urosnąć. **Nie jest planem na jutro** — jest
opisem kierunku, żeby kolejne zmiany szły w jedną stronę zamiast dokładać
kolejne pola do `QuerySeries`.

## Zasada

Nie zamieniamy surowego SQL-a na ORM. ORM jest zbudowany wokół encji i relacji;
my mamy strumień punktów i pytania o agregaty w oknach. Żaden ORM nie wyrazi
`toStartOfInterval`, `avgMerge` ani `PREWHERE`, więc pisałoby się surowy SQL
przez dodatkową warstwę.

Robimy to, co robią wszyscy w tej branży:

```
tekst zapytania          "avg:system.cpu.user{env:prod} by {host}"
      │
      ▼  parser
drzewo (AST)             typowana struktura w Go
      │
      ▼  kompilator
SQL z bindowanymi parametrami
```

Zysk jest jeden i konkretny: **SQL powstaje w jednym miejscu**. Walidacja,
limity kardynalności, wybór tabeli, podmiana funkcji agregującej — to wszystko
są decyzje kompilatora, nie handlera HTTP i nie frontendu.

Parser jest opcjonalny i późny. Panel może budować AST bezpośrednio z kontrolek.
Tekstowe pole zapytań to wygoda, nie fundament.

## Drzewo

Docelowy kształt, w kolejności od tego, co już mamy:

```go
type Query struct {
    TenantID string
    Metric   string

    // Filtry po dowolnych tagach, nie tylko po hoscie.
    // {env:prod, service:api} -> tags['env']='prod' AND tags['service']='api'
    Filters []Filter

    // Po czym rozbić na osobne linie. Puste = jedna linia zbiorcza.
    // by {host} -> GROUP BY host
    GroupBy []string

    // Agregacja WEWNĄTRZ kubełka czasu (co robimy z próbkami w tej samej
    // minucie) i MIĘDZY szeregami (co robimy, gdy GroupBy nie zawiera tagu,
    // który rozróżnia dwa szeregi). To są dwie różne rzeczy i mylenie ich
    // jest najczęstszym błędem w tego typu systemach.
    TimeAgg   Aggregation
    SpaceAgg  Aggregation

    From, To time.Time
    Step     time.Duration

    // Funkcje nakładane na gotowy szereg: rate(), derivative(), moving_avg().
    Functions []Function
}

type Filter struct {
    Key    string
    Op     FilterOp  // eq, neq, in, not_in, matches
    Values []string
}
```

Kolejność implementacji, od najbardziej potrzebnego:

1. **`Filters`** — bez tego nie da się zrobić nic poza „cała metryka".
2. **`GroupBy`** — bez tego nie zobaczysz rozbicia po `kube_namespace`.
3. **`SpaceAgg`** — potrzebne w momencie, gdy `GroupBy` jest węższe niż
   kardynalność danych.
4. **`Functions`** — `rate()` przy licznikach.
5. Arytmetyka między zapytaniami (`a / b`) — najpóźniej, bo wymaga wyrównania
   osi czasu dwóch szeregów.

## Typ metryki decyduje o agregacji

To jest reguła, którą wymusił konkretny błąd, i dlatego jest tu opisana
z przykładem.

Kolumna `metric_type` jest w tabeli od początku (GAUGE / COUNT / RATE), ale
`worker.go` jej nie czyta. Efekt: panel pozwala policzyć `sum` z `uptime`.

Co się wtedy dzieje — zmierzone, nie wydumane. Próbki co 10 s, kubełek 12 s
(bo `resolveStep` celuje w ~300 punktów, a 3600/300 = 12):

```
kubelek     probek   suma   srednia
02:04:24      1       940     940
02:04:12      2      1850     925     <- co piaty kubelek lapie dwie probki
02:04:00      1       910     910
02:03:12      2      1730     865
```

Na wykresie to piła: linia rośnie liniowo i co jakiś czas strzela na dwukrotność.
Wygląda jak wyścig albo zepsute timestampy. Nie jest ani jednym, ani drugim —
`sum` z gauge'a jest po prostu wielkością bez znaczenia, a 12 i 10 nie dzielą
się równo.

**Reguła:** kompilator dobiera dozwolone i domyślne agregacje z typu metryki.

| typ    | TimeAgg domyślnie | dozwolone            | zabronione |
|--------|-------------------|----------------------|------------|
| GAUGE  | `avg`             | avg, min, max, last   | **sum**    |
| COUNT  | `sum`             | sum, min, max         | avg        |
| RATE   | `avg`             | avg, min, max         | sum        |

Panel nie powinien pokazywać przycisku, którego kompilator i tak odrzuci. Czyli
typ metryki musi wyjeżdżać razem z listą metryk — `ListMetrics` powinno zwracać
`[]MetricInfo{Name, Type, Unit}`, nie `[]string`.

To jest też odpowiedź na „czemu w ogóle przechowujemy `metric_type`". Nie dla
ozdoby w UI — po to, żeby silnik nie pozwolił zadać bezsensownego pytania.

### UNSPECIFIED to nie przypadek brzegowy

Typ 0 (`UNSPECIFIED`) wygląda na rzadkość, ale **oficjalny przykład z dokumentacji
Datadoga wysyła właśnie jego**:

```python
MetricSeries(metric="system.load.1", type=MetricIntakeType.UNSPECIFIED, ...)
```

Sprawdzone przeciwko naszemu serwerowi — ląduje w tabeli jako `UNSPECIFIED`.
Czyli wszystko, co ktoś skopiuje z ich docsów, przyjdzie bez typu.

Ich semantyka to nie „typ nieznany", tylko „nie deklaruję typu": backend bierze
typ z metadanych metryki, a przy ich braku traktuje ją jak gauge.

**Reguła:** przy braku typu zachowuj się jak GAUGE.

Gauge jest jedynym typem, którego zbiór dozwolonych agregacji wyklucza `sum` —
czyli przy niepewności padamy na wariant, który najmniej pozwala. Domyślanie się
COUNT-a otworzyłoby z powrotem błąd z piłą, i to akurat na metrykach
z przykładów Datadoga.

**Rozwiązywać przy ODCZYCIE, nie przy zapisie.** W tabeli zostaje to, co
powiedział nadawca. Zapisanie `GAUGE` byłoby zmyślaniem danych i zamknęłoby
drogę do rozróżnienia „to jest gauge" od „nikt nie powiedział".

### Katalog metryk

Z powyższego wynika osobna tabela: nazwa, typ, jednostka, ostatnio widziana.

```
metryka przyszla jako GAUGE          -> katalog zapamietuje GAUGE
ta sama metryka jako UNSPECIFIED     -> katalog nadal mowi GAUGE
metryka znana tylko jako UNSPECIFIED -> traktuj jak GAUGE
```

Ta sama tabela obsługuje trzy pozornie niezależne rzeczy:

1. `ListMetrics` zwracające `{Name, Type, Unit}` zamiast gołych nazw
2. `GET /api/v1/metrics` — lista aktywnych metryk (dziś 404)
3. `GET /api/v1/search` — wyszukiwarka metryk (dziś 404)

Dziś typ wyciągamy przez `DISTINCT` z tabeli `metrics`, co działa przy trzydziestu
metrykach i przestanie działać przy trzydziestu tysiącach.

## Wybór tabeli

Dziś `worker.go` czyta zawsze `metrics`. To jest świadome i na razie słuszne:
jedno źródło, jedna ścieżka kodu, zero ryzyka, że rollup i surowe dane pokażą
co innego.

Kompilator jest właściwym miejscem, żeby to zmienić, bo wybór zależy od pytania:

- okno starsze niż TTL `metrics` → musi iść do `metrics_1m`
- `Step` grubszy niż minuta i zakres szerszy niż kilka godzin → `metrics_1m`
  jest tańszy
- `Step` drobniejszy niż minuta → tylko `metrics`, rollup nie ma tej rozdzielczości

Przy `metrics_1m` zmienia się też składnia agregacji: `avg(value)` staje się
`avgMerge(value_avg)`. To kolejny powód, żeby generować SQL w jednym miejscu —
przy dwóch źródłach o różnej składni ręczne sklejanie przestaje być wykonalne.

## Ograniczenia, które kompilator musi wymuszać

Nie jako dodatek — jako część kontraktu.

**Liczba punktów.** Już jest (`maxPoints` w `panelapi`), ale siedzi w złej
warstwie. Należy do kompilatora, bo to on wie, ile kubełków wyjdzie.

**Kardynalność.** `GROUP BY tags['pod_name']` na klastrze z autoskalowaniem
potrafi zwrócić dziesiątki tysięcy szeregów. Kompilator musi dokładać `LIMIT`
i zwracać informację, że wynik jest ucięty — cicha obcinka jest gorsza niż błąd.

**Zakres czasu.** Zapytanie bez `From` to skan całej tabeli. Domyślne okno
jest obowiązkowe.

**Białe listy.** Nazwy funkcji agregujących i kolumn do `GROUP BY` nie mogą
pochodzić wprost z requestu. Wartości zawsze bindowane, nazwy zawsze z mapy —
tak jak dziś działa `aggregations` w `worker.go`.

## Czego NIE budować na start

Żeby nie rozdąć tego ponad potrzebę:

- **Parsera tekstowego.** Panel buduje AST z kontrolek. Parser dopiero wtedy,
  gdy ktoś poprosi o pole tekstowe.
- **Arytmetyki między szeregami.** Wymaga wyrównywania osi czasu i decyzji,
  co zrobić z dziurami. Osobny temat.
- **Cache'u wyników.** Najpierw zmierzyć, czy jest potrzebny. ClickHouse na
  tych wolumenach odpowiada w milisekundach.
- **Własnego języka.** Jeśli kiedykolwiek, to zgodnego z Datadogiem — agenci
  i tak są ich, więc ich składnia jest darmową kompatybilnością.

## Co zrobić najpierw

Kolejność, w której każdy krok zostawia działający system:

1. `ListMetrics` zwraca typ i jednostkę obok nazwy.
2. Panel chowa agregacje niedozwolone dla typu metryki. **To zamyka błąd
   z `sum` na uptime.**
3. `QuerySeries` dostaje `Filters` i `GroupBy`; `worker.go` rozdziela się na
   `compile(Query) (string, []any, error)` i wykonanie.
4. Testy kompilatora — na tym etapie da się je pisać bez bazy, bo kompilator
   to czysta funkcja z AST na tekst i argumenty.
5. Wybór `metrics` vs `metrics_1m` w kompilatorze.

Krok 3 jest tym, który naprawdę zmienia architekturę. Reszta to obudowa.
