# goja-sandbox

Spike: jak uruchamiać skrypty użytkownika w JS po stronie serwera.

Odpowiada na pytanie „jak działają te skrypty w JS" w narzędziach typu
Datadog on-call czy Workflow Automation — jak wstrzykuje się do nich dane
i jak przy tym nie dać się zabić.

```
go run .
```

## Co pokazuje

**Piaskownica.** `vm.Interrupt()` wołane z innej goroutine przerywa skrypt
po zadanym czasie — pętla nieskończona i żarcie pamięci giną po 200 ms.
Środowisko jest puste: nie ma `fetch`, `require`, `process` ani dostępu
do plików, dopóki czegoś nie wstawisz.

**Kontekst.** Cztery kroki:

1. `vm.Set("alert", alert)` — dane wejściowe jako globalna zmienna
2. `vm.Set("console", ...)` — funkcje gospodarza, czyli jedyne, co skrypt może zrobić
3. `RunString("(function(){ ... })()")` — opakowanie, żeby działało `return`
4. `v.Export()` → JSON → własna struktura — odczyt wyniku z powrotem do Go

Plus `SetFieldNameMapper(TagFieldNameMapper("json", true))`, żeby pola struktur
Go były widoczne w JS pod nazwami z tagów `json`, a nie jako `Host`, `Severity`.

## Ograniczenia, o których trzeba pamiętać

- `Interrupt` przerywa **JavaScript, nie kod Go**. Funkcja gospodarza, która
  blokuje na 5 sekund, przetrzyma limit — takie funkcje muszą same pilnować czasu.
- Jedna maszyna `goja` to jedna goroutine, nie jest bezpieczna współbieżnie.
  Robi się pulę albo tworzy nową na każde wywołanie (tworzenie jest tanie).
- Limit pamięci nie jest twardy — powyższe działa, bo alokacja jest wolniejsza
  niż timer. Przy prawdziwym multi-tenancie potrzeba mocniejszej izolacji
  (osobny proces, V8 isolates, WASM).

## Dlaczego goja

Interpreter ES w czystym Go, bez cgo. Tego używa k6 od Grafany do odpalania
skryptów testowych użytkowników, czyli dokładnie w tym zastosowaniu.

Alternatywy: `v8go` (szybszy, ale cgo), `wazero` + QuickJS w WASM
(mocniejsza izolacja, wolniej).
