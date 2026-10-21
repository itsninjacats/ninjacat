# OTLP — osobny router, osobny host

**Na przyszłość. Nie robimy tego teraz.**

`otlp.ninjacat.<<domena>>` — własny host w routerze, obok `api.`, `trace.agent.`
i reszty. **Osobno**, nie jako kolejna ścieżka w `router_api.go`.

---

## Dlaczego osobno, a nie w istniejącym routerze

To nie jest protokół Datadoga. Wszystko, co dotąd odbieramy, to ich wymysł —
ich ścieżki, ich protobufy, ich nagłówki. OTLP to standard CNCF, który ma
własną specyfikację i własne typy (`go.opentelemetry.io/collector/pdata`).

Konkretnie różni się w czterech miejscach:

| | Datadog intake | OTLP |
|---|---|---|
| uwierzytelnianie | `Dd-Api-Key` | dowolny nagłówek, konwencja `Authorization` |
| transport | tylko HTTP | gRPC **i** HTTP |
| kodowanie | protobuf albo JSON, zależnie od ścieżki | protobuf albo JSON, wybór klienta |
| typy | `agent-payload/gogen` | `pdata` |

Gdyby to wsadzić do `router_api.go`, trzeba by tam wpuścić drugi system
uwierzytelniania i drugi zestaw typów. To jest dokładnie ten rodzaj mieszania,
którego chcemy uniknąć — jeden plik, jeden host, jedna konwencja.

---

## Ścieżki

Standard OTLP/HTTP (specyfikacja OTel, nie kod agenta):

```
POST /v1/traces
POST /v1/metrics
POST /v1/logs
POST /v1/profiles     (jeszcze niestabilne w specyfikacji)
```

Domyślne porty, potwierdzone w `pkg/config/example/datadog-agent_linux.yaml.example`:

```
DD_OTLP_CONFIG_RECEIVER_PROTOCOLS_GRPC_ENDPOINT   localhost:4317
DD_OTLP_CONFIG_RECEIVER_PROTOCOLS_HTTP_ENDPOINT   localhost:4318
```

**Do sprawdzenia:** ścieżki `/v1/*` są ze specyfikacji OTLP, nie ze sklonowanego
kodu — odbiornik w agencie pochodzi z upstreamowego collectora i jest zależnością
modułu, więc nie ma go w drzewie. Przed implementacją potwierdzić w
`go.opentelemetry.io/collector/receiver/otlpreceiver`.

---

## gRPC — decyzja do podjęcia

Większość SDK OTel domyślnie mówi po gRPC, na 4317. Nasz serwer to Gin po HTTP.
Do wyboru:

1. **tylko HTTP/protobuf** — wystarcza, bo każde SDK umie `OTLP_PROTOCOL=http/protobuf`,
   ale wymaga od użytkownika zmiany konfiguracji
2. **gRPC obok** — osobny listener, osobny meta-proces w drzewie Ergo

Zacząć od HTTP. gRPC dołożyć wtedy, gdy będzie realny użytkownik, który nie może
przestawić protokołu.

---

## Co Datadog robi z OTLP i co z tego bierzemy

W agencie jest gotowe tłumaczenie OTLP → model Datadoga:
`pkg/opentelemetry-mapping-go/otlp/` — podpakiety `attributes`, `metrics`,
`logs`, `rum`. Apache 2.0.

Najciekawszy jest `metrics/exponential_histograms_translator.go`: histogram
wykładniczy OTLP wpada wprost do DDSketcha, bo obie struktury są logarytmiczne.

```go
store := store.NewDenseStore()
for j := 0; j < bucketCounts.Len(); j++ {
    index := j + int(offset)
    store.AddWithCount(index, float64(bucketCounts.At(j)))
}
```

Indeks kubełka OTLP staje się indeksem kubełka DDSketcha bez przeliczania
wartości. **Uwaga:** to `sketches-go`, a nie `pkg/util/quantile` — inna baza,
inne parametry. Nasz `server/sketch/ddsketch.go` odwzorowuje ten drugi
(`gamma = 1.015625`, `bias = 1338`), więc **nie wolno** tych dwóch mieszać
w jednej kolumnie. Patrz `docs/ddsketch-agenta.md`.

Czy w ogóle tłumaczymy OTLP na model Datadoga, czy trzymamy go osobno — to
decyzja na później, razem z kształtem tabel. Na start: odebrać i odkodować,
tak jak przy kubeopsach.

---

## Uwierzytelnianie

OTLP nie ma `Dd-Api-Key`. Kolektory wysyłają nagłówki z konfiguracji:

```yaml
exporters:
  otlphttp:
    endpoint: https://otlp.ninjacat.example
    headers:
      authorization: Bearer <klucz>
```

Czyli ten sam `apikeys.Store`, ale inny nagłówek i inny middleware —
`RequireAPIKey` czyta konkretnie `Dd-Api-Key`. Do rozstrzygnięcia: przyjmować
oba naraz czy tylko `Authorization`.

---

## Zakres zadania

1. `server/intake/router_otlp.go` — host `otlp.`, cztery ścieżki, dekodowanie do `pdata`
2. middleware uwierzytelniający po `Authorization`
3. decyzja: gRPC teraz czy później
4. decyzja: tłumaczyć na model Datadoga czy trzymać OTLP osobno

Punkty 1–2 są mechaniczne. 3 i 4 wymagają rozmowy.
