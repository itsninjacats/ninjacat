# DDSketch agenta — parametry i mapowanie

Ustalenia empiryczne i ze źródeł agenta. Potrzebne wszędzie, gdzie czytamy
percentyle albo budujemy szkice sami.

Źródło: `DataDog/datadog-agent`, plik `pkg/util/quantile/config.go`.

## Stałe

```go
const (
	defaultBinLimit = 4096
	defaultEps      = 1.0 / 128.0   // 0.0078125
	defaultMin      = 1e-9
)
```

**Nie są konfigurowalne per metryka.** Payload (`SketchPayload_Sketch_Dogsketch`)
nie zawiera żadnego pola o dokładności, gammie ani mapowaniu — sprawdzone,
zero trafień w całym protobufie. Gdyby zależały od metryki, musiałyby jechać
razem z danymi, bo inaczej odbiorca nie zinterpretowałby `K`. Czyli to jest
jedna stała zaszyta po obu stronach.

## Mapowanie

```go
eps *= 2                      // 0.015625
gamma.v  = 1 + eps            // 1.015625
gamma.ln = math.Log1p(eps)    // 0.0155041865

emin      = floor(log(defaultMin) / gamma.ln)   // -1337
norm.bias = -emin + 1                           //  1338

key(v)  = RoundToEven( ln(v) / gamma.ln ) + bias
f64(k)  = gamma.v ^ (k - bias)
```

Wartości kontrolne:

| wartość | klucz |
|---|---|
| 11.9 ms | 1498 |
| 47.3 ms | 1587 |
| 890 ms | 1776 |
| 1150 ms | 1793 |

## Pułapka: to jest zaokrąglenie do NAJBLIŻSZEGO

Komentarz w źródle agenta mówi:

```go
// key returns a value k such that:
//	γ^k <= v < γ^(k+1)
```

**Ten komentarz jest nieprawdziwy dla tego kodu.** Opisuje konwencję `floor`,
a implementacja używa `RoundToEven`. Sprawdzone: `key(1150) = 1793`,
ale `γ^(1793-1338) = 1157.95`, czyli WIĘCEJ niż 1150.

Prawdziwa konwencja:

```
γ^(k-bias)  to SRODEK kubelka, nie dolna granica

kubelek k obejmuje:  γ^(k-bias-0.5)  ..  γ^(k-bias+0.5)
```

Zweryfikowane na zakresie od 0.5 ms do 3600 s — trzyma się wszędzie.

**Przy odczycie percentyla podawaj `γ^(k-bias)`**, czyli reprezentanta, a nie
przedział. Błąd jest wtedy w granicach eps:

```
|1157.95 - 1150| / 1150 = 0.69%  <  0.78% = eps
```

Podanie `γ^(k-bias) .. γ^(k+1-bias)` przesuwa odpowiedź o pół kubełka w górę.

## NIE używać sketches-go

`github.com/DataDog/sketches-go` liczy gammę inaczej — wzorem z pracy o
DDSketchu, `(1+α)/(1-α)` — i nie ma przesunięcia `bias`. Te same wartości
dostają tam klucze rzędu 120–350 zamiast 1500–1800.

Szkice zbudowane tą biblioteką **nie scalą się** ze szkicami z agenta. Zamiast
niej przepisać kilkanaście linijek z `pkg/util/quantile` — to jest cały kod
widoczny wyżej.

## Co już działa

Ścieżka agenta jest sprawdzona end-to-end (protobuf → `sketches`):

```
POST /api/beta/sketches -> 202 Accepted (140 bajtow protobufa)

bucket_keys:   [1498,1499,1586,1587,1588,1592,1776,1793]
bucket_counts: [2,2,2,1,1,1,2,1]
count: 12  min: 11.9  max: 1150  avg: 268.625  sum: 3223.5
```

`parseSketchesProtobuf` czyta `Dogsketches` i mapuje `K`/`N` poprawnie.

## Pole `Distributions` — sprawdzone, agent go nie używa

`SketchPayload_Sketch` ma dwa pola na szkice:

```
Dogsketches   []Dogsketch      -> Ts Cnt Min Max Avg Sum  K  N       <- czytamy
Distributions []Distribution   -> Ts Cnt Min Max Avg Sum  V G Delta Buf
```

Numery pól w `.proto` opowiadają historię formatu:

```proto
repeated Distribution distributions = 3;
reserved 5, 6;
reserved "distributionsK", "distributionsC";   // porzucone proby
repeated Dogsketch   dogsketches   = 7;
```

**Agent nigdy nie wypełnia `distributions`.** Potwierdzone w jego serializerze
(`pkg/serializer/internal/metrics/sketch_series_list.go`), gdzie pole jest
wykreślone:

```go
// Unused fields are commented out
const sketchHost        = 2
// const sketchDistributions = 3
const sketchTags        = 4
const sketchDogsketches = 7
```

Czym jest ten format: trzyma próbkę rzeczywistych wartości (`V`) z informacją,
ile pominięto między nimi (`G`) i jaki jest margines błędu (`Delta`). Budowa
odpowiada rodzinie Greenwalda-Khanny — gwarantuje błąd RANGI, nie błąd
wartości, i nie scala się czysto między hostami, bo margines narasta przy
każdym łączeniu. Stąd przejście na DDSketch.

Pomijanie go jest więc bezpieczne, a nie ryzykowne.
