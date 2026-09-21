# Zdarzenia z agenta — do potwierdzenia empirycznego

**Stan:** `/api/v1/events` działa i jest przetestowany, ale **tylko dla klientów API**.
Nie wiemy, czy sam agent Datadoga ma na zdarzenia osobną drogę.

## Co już jest zrobione

Endpoint `POST /api/v1/events` przyjmuje payload w formacie `EventCreateRequest`
i zapisuje go do tabeli `ninjacat.events`. Sprawdzone end-to-end dwoma
oficjalnymi bibliotekami:

- `datadog-api-client-python 2.60.0` — `EventsApi.create_event()`
- `datadogpy 0.53.0` — `api.Event.create()`

Obie deserializują naszą odpowiedź do `EventCreateResponse`, czyli zwracany
kształt `{"status": "ok", "event": {...}}` jest zgodny z publicznym API.

## Czego NIE wiemy

Przeszukaliśmy kod handlerów i wszystkie przechwycone payloady z `captures/` —
**nigdzie nie ma ciągu `"events"`**. To znaczy tylko tyle, że nasz dotychczasowy
ruch testowy ich nie zawierał. Nie znaczy, że agent ich nie wysyła.

Konkretne podejrzenia do sprawdzenia:

1. **Czy agent wysyła zdarzenia przez `/intake/`?** Historycznie payload
   intake'u miał klucz `events` obok `gohai` i `meta`. Nasz `decodeIntake`
   go nie szuka. Jeśli tam są — cicho je gubimy.

2. **Czy monitory i checki agenta generują zdarzenia?** Np. `service_check`
   przechodzący w CRITICAL potrafi wyemitować zdarzenie. Jeśli tak, to jest
   kanał, którego nie słuchamy, a który jest istotny dla alertów.

3. **Czy DogStatsD ma pakiet zdarzenia?** Protokół statsd Datadoga ma format
   `_e{tytul.len,tekst.len}:tytul|tekst|...`. My nie mamy odbiornika statsd
   w ogóle, więc to osobny temat, ale warto zanotować, że taki kanał istnieje.

## Jak to sprawdzić

Tak samo jak resztę protokołu — empirycznie, nie z dokumentacji:

1. Postawić agenta z `docker-compose.datadog-lab.yml`.
2. Wymusić zdarzenie: restart usługi monitorowanej przez agenta albo check
   przechodzący w stan krytyczny.
3. Przejrzeć `captures/` pod kątem `events`, `_e{`, `event_type`.
4. Jeśli zdarzenia jadą przez intake — rozszerzyć `decodeIntake`
   i podpiąć pod istniejący `storage.EventsWriter`. Tabela i writer już są,
   więc to tylko parsowanie.

## Dlaczego to ma znaczenie

Zdarzenia to warstwa, która odpowiada na pytanie „coś się zmieniło o 14:30 —
co się wtedy stało". Bez niej wykresy pokazują, ŻE coś się stało, ale nigdy
DLACZEGO. Jeśli agent emituje je sam, a my ich nie zbieramy, to tracimy
najcenniejszą część tego strumienia — tę, której nikt nie musi wysyłać ręcznie.
