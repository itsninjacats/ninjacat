# DSM — dwa endpointy bez znanego kształtu

**Oflagowane 2026-09-20. Odebrane i zalogowane, nieotypowane.**

Na hoście `trace.agent.<site>` mamy dwie ścieżki, których payloadu **nie da się
odtworzyć z kodu Datadoga**. Obie dotyczą Data Streams Monitoring.

---

## `/api/v0.1/pipeline_stats`

trace-agent jest tu czystym `httputil.ReverseProxy`
(`pkg/trace/api/pipeline_stats.go`). Podmienia lokalną ścieżkę na tę, dokleja
`Via`, `X-Datadog-Additional-Tags`, `X-Datadog-Container-Tags` i przepuszcza
ciało bajt w bajt. **Nie parsuje go**, więc nie ma w repozytorium typu Go.

Format ustala tracer. Na tym hoście msgpack jest normą, a protobuf wyjątkiem
(patrz `spis-endpointow-datadoga.md` §5.4), ale to przypuszczenie, nie fakt.

## `/api/v2/data_streams_messages`

Pipeline Event Platform, `eventType: "data-streams-message"`,
`contentType: application/json`, host przez `hostnameEndpointPrefix`.

**W całym otwartym repozytorium agenta nie ma producenta tego zdarzenia.**
Stała występuje w pięciu miejscach: deklaracja pipeline'u, jeden `if`
doklejający nagłówek w `epforwarder.go` i trzy wpisy w changelogu. Nikt nic
tam nie wysyła.

Czyli pipeline ma host, ścieżkę i content-type, ale to, co go karmi, jest poza
tym repozytorium.

---

## Co jest teraz

- `pipeline_stats` — `describe()`: rozmiar, Content-Type, Content-Encoding,
  nagłówki proxy, pierwsze bajty. Zero dekodowania.
- `data_streams_messages` — `[]json.RawMessage`, log liczby wiadomości.

To nie jest niedoróbka, tylko jedyna uczciwa odpowiedź: nie ma czego
odwzorować.

## Jak to domknąć

Złapać prawdziwy payload. Postawić aplikację z tracerem i włączonym DSM
(`DD_DATA_STREAMS_ENABLED=true`), skierować na nasz endpoint, zapisać ciało
i dopiero wtedy decydować o dekoderze. Do zrzutu nadaje się `server/capture/`.

Dopóki tego nie ma, każdy dekoder byłby zgadywaniem.

**Uwaga:** ten sam problem dotyczy `/api/v2/profile` — tam też agent jest
proxy — ale format profili jest publicznie udokumentowany (multipart:
`event` JSON + załączniki pprof), więc tamten da się otypować bez łapania
ruchu. Patrz [[profiling]] w `spis-endpointow-datadoga.md` §4.
