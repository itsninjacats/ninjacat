-- Schemat ninjacata dla danych z Datadog Agenta.
--
-- Zasada nadrzedna: JEDNA tabela na typ payloadu, nie jedna na metryke.
-- Tabela per metryka to antywzorzec w ClickHouse - kazda tabela to osobne
-- katalogi, osobne merge i osobny narzut; przy tysiacach metryk serwer padnie
-- na samej liczbie plikow. ClickHouse jest kolumnowy i LowCardinality(String)
-- trzyma nazwe metryki jako kilkubajtowy identyfikator ze slownika, wiec
-- miliard wierszy z ta sama nazwa kosztuje tyle, co slownik plus indeksy.

CREATE DATABASE IF NOT EXISTS ninjacat;

-- ---------------------------------------------------------------------------
-- Metryki punktowe: /api/v1/series i /api/v2/series
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.metrics
(
    -- Wielodostepnosc. Kolumna jest TANIA (LowCardinality, przy jednym
    -- kliencie to kilka bajtow na cala tabele), ale jej pozycja na poczatku
    -- ORDER BY jest DROGA do zmiany pozniej - wymagalaby przepisania tabeli.
    -- Dlatego wchodzi od razu, nawet gdy klient jest jeden.
    --
    -- Zrodlo: klucz API, z ktorym przyszlo zadanie (api_key.tenant_id
    -- w Postgresie -> apikeys.Key.TenantID w Go).
    tenant_id    LowCardinality(String),

    -- DoubleDelta swietnie sciska rosnace timestampy o staly krok
    -- (agent wysyla co 15 s), ZSTD dobija reszte.
    timestamp    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    metric       LowCardinality(String),
    host         LowCardinality(String),
    metric_type  LowCardinality(String),     -- GAUGE / COUNT / RATE
    source_type  LowCardinality(String),
    unit         LowCardinality(String),
    interval     UInt32 CODEC(DoubleDelta, ZSTD(1)),

    -- Gorilla to kodek zaprojektowany dla szeregow czasowych: XOR-uje kolejne
    -- wartosci float i zapisuje tylko rozniace sie bity. Przy metrykach, ktore
    -- zmieniaja sie powoli, daje kilkukrotnie lepsza kompresje niz sam ZSTD.
    value        Float64 CODEC(Gorilla, ZSTD(1)),

    -- Tagi "goroace" wyciagniete do kolumn - po nich filtruje sie najczesciej,
    -- a kolumna jest duzo szybsza niz wyciaganie klucza z mapy.
    env          LowCardinality(String) MATERIALIZED tags['env'],
    service      LowCardinality(String) MATERIALIZED tags['service'],
    kube_namespace  LowCardinality(String) MATERIALIZED tags['kube_namespace'],
    kube_deployment LowCardinality(String) MATERIALIZED tags['kube_deployment'],
    pod_name     String MATERIALIZED tags['pod_name'],

    -- Reszta tagow zostaje w mapie - nie trzeba znac ich z gory.
    tags         Map(LowCardinality(String), String),

    -- Identyfikator szeregu czasowego: te same metryka+host+tagi = ten sam
    -- szereg. Przydaje sie do rollupow i do laczenia z metadanymi.
    series_id    UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, tags),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values mapValues(tags) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
-- Partycjonowanie dzienne: przy 30-dniowej retencji daje 30 partycji, czyli
-- tyle, ile trzeba. Usuwanie starych danych to wtedy DROP PARTITION - operacja
-- na metadanych, nie kasowanie wiersz po wierszu.
-- Przy retencji liczonej w miesiacach lub latach zmien na toYYYYMM(timestamp),
-- bo kilkaset partycji spowalnia planowanie zapytan.
PARTITION BY toDate(timestamp)
-- Kolejnosc sortowania to NAJWAZNIEJSZA decyzja w tej tabeli. Decyduje
-- jednoczesnie o kompresji (sasiadujace wiersze sa podobne) i o szybkosci
-- (ClickHouse pomija cale granule po kluczu). Uklad "metryka, host, czas"
-- pasuje do zapytania, ktore pada najczesciej: jedna metryka, jeden host,
-- zakres czasu.
-- tenant_id NA POCZATKU: zapytanie jednego klienta pomija cudze dane
-- na poziomie granul, zanim cokolwiek przeczyta.
ORDER BY (tenant_id, metric, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY
SETTINGS index_granularity = 8192;

-- ---------------------------------------------------------------------------
-- Rollup 1-minutowy. Surowe punkty trzymamy 30 dni, agregaty duzo dluzej.
--
-- AggregatingMergeTree przechowuje stany funkcji agregujacych, nie gotowe
-- liczby. Dzieki temu da sie je potem laczyc (np. policzyc srednie dzienne
-- ze srednich minutowych) bez bledu, ktory powstalby przy usrednianiu srednich.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.metrics_1m
(
    tenant_id   LowCardinality(String),
    bucket      DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric      LowCardinality(String),
    host        LowCardinality(String),
    series_id   UInt64,
    points      AggregateFunction(count, Float64),
    value_avg   AggregateFunction(avg, Float64),
    value_min   AggregateFunction(min, Float64),
    value_max   AggregateFunction(max, Float64),
    value_sum   AggregateFunction(sum, Float64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tenant_id, metric, host, series_id, bucket)
TTL bucket + INTERVAL 400 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS ninjacat.metrics_1m_mv
TO ninjacat.metrics_1m AS
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS bucket,
    metric,
    host,
    series_id,
    countState(value) AS points,
    avgState(value)   AS value_avg,
    minState(value)   AS value_min,
    maxState(value)   AS value_max,
    sumState(value)   AS value_sum
FROM ninjacat.metrics
GROUP BY tenant_id, bucket, metric, host, series_id;

-- ---------------------------------------------------------------------------
-- Szkice DDSketch: /api/beta/sketches
-- Osobna tabela, bo to inny ksztalt danych - nie pojedyncza wartosc, tylko
-- histogram w postaci dwoch rownoleglych tablic.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.sketches
(
    tenant_id     LowCardinality(String),
    timestamp     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric        LowCardinality(String),
    host          LowCardinality(String),
    tags          Map(LowCardinality(String), String),

    count         UInt64 CODEC(T64, ZSTD(1)),
    min           Float64 CODEC(Gorilla, ZSTD(1)),
    max           Float64 CODEC(Gorilla, ZSTD(1)),
    avg           Float64 CODEC(Gorilla, ZSTD(1)),
    sum           Float64 CODEC(Gorilla, ZSTD(1)),

    -- Kubelki DDSketcha: k to indeksy (logarytmiczne), n to liczebnosci.
    -- Trzymane jako tablice, bo tylko w komplecie maja sens i tylko w komplecie
    -- da sie je scalac miedzy hostami.
    bucket_keys   Array(Int32) CODEC(ZSTD(1)),
    bucket_counts Array(UInt32) CODEC(ZSTD(1)),

    series_id     UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, tags)
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, metric, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Statusy checkow: /api/v1/check_run
-- Malo wierszy, inny wzorzec zapytan (ostatni status per check), wiec osobno.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.check_runs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    check_name LowCardinality(String),
    host       LowCardinality(String),
    status     Enum8('OK' = 0, 'WARNING' = 1, 'CRITICAL' = 2, 'UNKNOWN' = 3),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, check_name, host, timestamp)
TTL timestamp + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Logi: /api/v2/logs
-- Zupelnie inny profil - duze pole tekstowe, filtrowanie po serwisie i czasie,
-- szukanie po tresci. Stad inna kolejnosc sortowania i mocniejszy ZSTD.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.logs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host       LowCardinality(String),
    service    LowCardinality(String),
    source     LowCardinality(String),
    status     LowCardinality(String),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), String),

    -- Indeks tokenowy pozwala szybko odsiac granule przy szukaniu frazy,
    -- zanim ClickHouse zacznie czytac samo pole message.
    INDEX idx_message message TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, service, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 14 DAY;

-- ---------------------------------------------------------------------------
-- Metadane hostow z /intake/ (gohai). Jeden aktualny wiersz na host.
-- ReplacingMergeTree nadpisuje starsze wersje przy merge'u.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.hosts
(
    tenant_id     LowCardinality(String),
    host          String,
    seen_at       DateTime('UTC'),
    agent_version LowCardinality(String),
    os            LowCardinality(String),
    platform      Map(String, String),
    cpu           Map(String, String),
    memory        Map(String, String),
    tags          Array(String)
)
ENGINE = ReplacingMergeTree(seen_at)
ORDER BY (tenant_id, host);

-- ---------------------------------------------------------------------------
-- Inwentarz procesow: /api/v1/collector (process-agent)
--
-- To jedyny endpoint z wlasna 16-bajtowa ramka i legacy zstd. Ta sama ramka
-- niesie 36 roznych typow - tu zapisujemy typ 12 (CollectorProc). Przy
-- Kubernetesie przyjda nia tez manifesty zasobow (pody, deploymenty),
-- ale to osobna tabela i osobny temat.
--
-- Migawki sa GRUBE: kilkaset procesow na host co ~10 s. Stad krotsze TTL
-- niz przy metrykach - inwentarz procesow sprzed tygodnia nikogo nie
-- interesuje, a zajmuje wiecej miejsca niz metryki z miesiaca.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.processes
(
    tenant_id     LowCardinality(String),
    timestamp     DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    host          LowCardinality(String),

    pid           Int32 CODEC(T64, ZSTD(1)),
    ppid          Int32 CODEC(T64, ZSTD(1)),
    user          LowCardinality(String),

    -- Nazwa pliku wykonywalnego powtarza sie w kazdej migawce, wiec slownik.
    -- Pelna linia polecen jest zmienna, wiec zwykly String z mocnym ZSTD.
    comm          LowCardinality(String),
    exe           String CODEC(ZSTD(1)),
    cmdline       String CODEC(ZSTD(3)),

    rss           UInt64 CODEC(T64, ZSTD(1)),
    vms           UInt64 CODEC(T64, ZSTD(1)),
    cpu_pct       Float32 CODEC(Gorilla, ZSTD(1)),
    threads       Int32 CODEC(T64, ZSTD(1)),
    open_fds      Int32 CODEC(T64, ZSTD(1)),

    state         LowCardinality(String),
    create_time   DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    container_id  String CODEC(ZSTD(1)),

    tags          Map(LowCardinality(String), String)
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
-- "co chodzilo na tym hoscie w tym czasie" - to jest pytanie, ktore padnie.
ORDER BY (tenant_id, host, timestamp, pid)
TTL timestamp + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- Zdarzenia: /api/v1/events
--
-- Deploye, restarty, zmiany configu, alerty z monitorow. Rzecz, ktora
-- nakladasz na wykres jako pionowa kreske, zeby odpowiedziec na pytanie
-- "cos sie zmienilo o 14:30 - co sie wtedy stalo".
--
-- ORDER BY jest tu INNY niz przy metrykach i to jest swiadome. Przy metryce
-- pytasz o konkretna nazwe, wiec metric idzie przed czasem. Zdarzen nie
-- filtrujesz po nazwie - chcesz WSZYSTKIE z danego okna, zeby narysowac je
-- na osi. Dlatego czas stoi zaraz po tenant_id.
--
-- Wolumen jest tu o rzedy wielkosci mniejszy niz przy metrykach: to skala
-- ludzka i systemowa (dziesiatki, setki dziennie), nie maszynowa. Stad
-- dluzsze TTL i brak rollupu - nie ma czego agregowac.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ninjacat.events
(
    tenant_id        LowCardinality(String),
    timestamp        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Nasz identyfikator. Datadog zwraca klientowi liczbowe id zdarzenia,
    -- wiec trzymamy tez jego postac liczbowa - patrz event_id_num.
    event_id         UUID,
    event_id_num     UInt64 CODEC(T64, ZSTD(1)),

    title            String CODEC(ZSTD(1)),
    text             String CODEC(ZSTD(3)),
    host             LowCardinality(String),

    -- error / warning / info / success / user_update / recommendation / snapshot
    alert_type       LowCardinality(String),
    -- normal / low
    priority         LowCardinality(String),

    -- Klucz grupowania: zdarzenia o tym samym kluczu Datadog sklada w jeden
    -- watek (np. kolejne restarty tej samej uslugi).
    aggregation_key  String CODEC(ZSTD(1)),
    source_type_name LowCardinality(String),
    device_name      LowCardinality(String),

    env              LowCardinality(String) MATERIALIZED tags['env'],
    service          LowCardinality(String) MATERIALIZED tags['service'],
    kube_namespace   LowCardinality(String) MATERIALIZED tags['kube_namespace'],

    tags             Map(LowCardinality(String), String),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values mapValues(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_aggkey     aggregation_key TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;
