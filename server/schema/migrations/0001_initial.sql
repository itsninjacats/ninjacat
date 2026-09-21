-- Schemat ninjacata dla danych z Datadog Agenta.
--
-- Zasada nadrzedna: JEDNA tabela na typ payloadu, nie jedna na metryke.
-- Tabela per metryka to antywzorzec w ClickHouse - kazda tabela to osobne
-- katalogi, osobne merge i osobny narzut; przy tysiacach metryk serwer padnie
-- na samej liczbie plikow. ClickHouse jest kolumnowy i LowCardinality(String)
-- trzyma nazwe metryki jako kilkubajtowy identyfikator ze slownika, wiec
-- miliard wierszy z ta sama nazwa kosztuje tyle, co slownik plus indeksy.

-- No CREATE DATABASE and no database qualifiers here: the migration runner
-- (server/schema) applies every statement over a connection already opened
-- against the target database (CLICKHOUSE_DB), and the database itself is
-- created by whoever provisions ClickHouse — the container entrypoint reads
-- CLICKHOUSE_DB in every deployment we have. Qualifying the names would pin
-- the schema to one database name and break running against a scratch one.

-- TAG columns are Map(LowCardinality(String), Array(LowCardinality(String))).
--
-- A Datadog tag is a label in a SET, not a key-value property: nothing forbids
-- two tags sharing the part before the colon, and real clusters produce them
-- constantly — a pod behind two services carries kube_service:a AND
-- kube_service:b. A plain Map(key, value) kept whichever won the race and
-- silently dropped the rest, so the value side is an ARRAY: the map key is the
-- tag key, the array holds every value that arrived, in order. Fidelity is
-- free on disk (15.23 MiB vs 15.22 for the plain map over the same 800k-row
-- slice) and the ~60% penalty on map lookups lands on cold paths only — the
-- hot filters go through the materialised columns, which read tags['k'][1].
-- Queries move from equality to membership: has(tags['k'], 'v').
-- Full measurements and the rejected alternatives:
-- docs/decisions/0001-tags-are-a-multiset.md.
--
-- Values stay LowCardinality, plus ZSTD(3). Measured on a real Kubernetes
-- cluster's telemetry, 704k metric points, before the array change:
--
--   Map(LowCardinality, String) + LZ4   96.5 bytes/point   (the old shape)
--   Map(LowCardinality, String) + ZSTD1 43.5 bytes/point
--   Map(LowCardinality, LowCardinality) 17.6 bytes/point   <- this
--
-- 5.5x smaller, and it is not a trade: inserting was marginally FASTER than
-- LZ4 (1.34s vs 1.44s for the same batch) and scanning the map was twice as
-- fast, because comparisons run against dictionary ids instead of strings.
--
-- Kubernetes tags and labels are bounded and repetitive by nature — a
-- namespace, a deployment, an image — so the dictionary stays small.
--
-- Kubernetes labels and annotations, and the gohai host metadata maps, are
-- NOT tags and stay ordinary maps: their APIs forbid duplicate keys, so the
-- array shape would pay the lookup penalty for a problem they cannot have.
--
-- ANNOTATIONS ARE THE EXCEPTION and keep String values: the Kubernetes API
-- caps a label value at 63 characters but places no such limit on an
-- annotation, and kubectl.kubernetes.io/last-applied-configuration routinely
-- holds kilobytes of JSON. A dictionary of those would be worse than useless.


-- ---------------------------------------------------------------------------
-- Metryki punktowe: /api/v1/series i /api/v2/series
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS metrics
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
    env          LowCardinality(String) MATERIALIZED tags['env'][1],
    service      LowCardinality(String) MATERIALIZED tags['service'][1],
    kube_namespace  LowCardinality(String) MATERIALIZED tags['kube_namespace'][1],
    kube_deployment LowCardinality(String) MATERIALIZED tags['kube_deployment'][1],
    pod_name     String MATERIALIZED tags['pod_name'][1],

    -- Reszta tagow zostaje w mapie - nie trzeba znac ich z gory.
    tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Identyfikator szeregu czasowego: te same metryka+host+tagi = ten sam
    -- szereg. Przydaje sie do rollupow i do laczenia z metadanymi.
    series_id    UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, tags),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    -- mapValues on an array-valued map yields Array(Array(...)), which
    -- bloom_filter rejects — flatten to one level so the index still sees
    -- every tag value.
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
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
CREATE TABLE IF NOT EXISTS metrics_1m
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

CREATE MATERIALIZED VIEW IF NOT EXISTS metrics_1m_mv
TO metrics_1m AS
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
FROM metrics
GROUP BY tenant_id, bucket, metric, host, series_id;

-- ---------------------------------------------------------------------------
-- Szkice DDSketch: /api/beta/sketches
-- Osobna tabela, bo to inny ksztalt danych - nie pojedyncza wartosc, tylko
-- histogram w postaci dwoch rownoleglych tablic.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sketches
(
    tenant_id     LowCardinality(String),
    timestamp     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric        LowCardinality(String),
    host          LowCardinality(String),
    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

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
CREATE TABLE IF NOT EXISTS check_runs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    check_name LowCardinality(String),
    host       LowCardinality(String),
    status     Enum8('OK' = 0, 'WARNING' = 1, 'CRITICAL' = 2, 'UNKNOWN' = 3),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3))
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
CREATE TABLE IF NOT EXISTS logs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host       LowCardinality(String),
    service    LowCardinality(String),
    source     LowCardinality(String),
    status     LowCardinality(String),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

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
CREATE TABLE IF NOT EXISTS hosts
(
    tenant_id     LowCardinality(String),
    host          String,
    seen_at       DateTime('UTC'),
    agent_version LowCardinality(String),
    os            LowCardinality(String),
    platform      Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    cpu           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    memory        Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3))
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
CREATE TABLE IF NOT EXISTS processes
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

    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3))
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
CREATE TABLE IF NOT EXISTS events
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

    env              LowCardinality(String) MATERIALIZED tags['env'][1],
    service          LowCardinality(String) MATERIALIZED tags['service'][1],
    kube_namespace   LowCardinality(String) MATERIALIZED tags['kube_namespace'][1],

    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    -- mapValues on an array-valued map yields Array(Array(...)), which
    -- bloom_filter rejects — flatten to one level so the index still sees
    -- every tag value.
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_aggkey     aggregation_key TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Kubernetes resources: /api/v2/orch (orchestrator collectors, types 41..88)
--
-- One GENERIC table for all ~30 kinds, not one per kind — the same principle
-- as the header of this file: a table per kind multiplies directories and
-- merges, while `kind` compresses to a dictionary entry. The columns every
-- kind shares (identity, owner, phase, the ready/desired pair) are real
-- columns; the numbers that differ per kind (restarts, updated replicas,
-- port counts...) live in `counts`, so a new kind needs no schema change.
--
-- Append-only HISTORY: one row per object per collection pass, because the
-- question "when did this deployment lose its replicas" needs the passes that
-- are gone. "What is in the cluster right now" is a different question with a
-- different engine — the ReplacingMergeTree below, fed by a materialized view
-- so the writer inserts once and both tables stay in step.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_resources
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    kind             LowCardinality(String),   -- Pod / Node / Deployment / ...
    namespace        LowCardinality(String),
    name             String,
    uid              String,
    resource_version String,

    -- First owner reference only: enough to walk Pod -> ReplicaSet ->
    -- Deployment, which is the join that actually gets asked for.
    owner_kind       LowCardinality(String),
    owner_name       String,

    node_name        LowCardinality(String),
    phase            LowCardinality(String),
    status           LowCardinality(String),

    -- The numbers nearly every controller reports, promoted to columns
    -- because "ready vs desired" is THE dashboard query for workloads.
    ready            Int32 CODEC(T64, ZSTD(1)),
    desired          Int32 CODEC(T64, ZSTD(1)),
    available        Int32 CODEC(T64, ZSTD(1)),

    -- Kind-specific numbers (restarts, updated replicas, taints...) keyed by
    -- their own vocabulary, so no kind forces a column on all the others.
    counts           Map(String, Int64),

    labels           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    annotations      Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    agent_version    LowCardinality(String),
    -- The agent splits one collection pass into GroupSize frames sharing a
    -- GroupId. Kept so a pass can be reassembled or spotted as partial.
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),

    -- Labels are the tags of the Kubernetes world — the filter people reach
    -- for first — so they get the same bloom filters tags get elsewhere.
    INDEX idx_label_keys   mapKeys(labels)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_label_values mapValues(labels) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, kind, namespace, name, collected_at)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;

-- The "right now" view: one row per live object, newest collection wins.
-- Keyed on identity and versioned by collected_at, so repeated passes
-- collapse at merge time (readers still use FINAL or argMax for exactness,
-- like the hosts table). No TTL on purpose: a live object must never age out
-- of the current view — rows leave only by being replaced.
CREATE TABLE IF NOT EXISTS k8s_resources_current
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    kind             LowCardinality(String),
    namespace        LowCardinality(String),
    name             String,
    uid              String,
    resource_version String,

    owner_kind       LowCardinality(String),
    owner_name       String,

    node_name        LowCardinality(String),
    phase            LowCardinality(String),
    status           LowCardinality(String),

    ready            Int32 CODEC(T64, ZSTD(1)),
    desired          Int32 CODEC(T64, ZSTD(1)),
    available        Int32 CODEC(T64, ZSTD(1)),

    counts           Map(String, Int64),

    labels           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    annotations      Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    agent_version    LowCardinality(String),
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),

    INDEX idx_label_keys   mapKeys(labels)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_label_values mapValues(labels) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(collected_at)
ORDER BY (tenant_id, cluster_id, kind, namespace, name);

CREATE MATERIALIZED VIEW IF NOT EXISTS k8s_resources_current_mv
TO k8s_resources_current AS
SELECT
    tenant_id,
    collected_at,
    cluster_id,
    cluster_name,
    kind,
    namespace,
    name,
    uid,
    resource_version,
    owner_kind,
    owner_name,
    node_name,
    phase,
    status,
    ready,
    desired,
    available,
    counts,
    labels,
    annotations,
    tags,
    agent_version,
    group_id,
    group_size
FROM k8s_resources;

-- ---------------------------------------------------------------------------
-- Raw Kubernetes manifests: /api/v2/orchmanif (CollectorManifest, types 80-82)
--
-- Content is the object's own YAML or JSON — "exactly what kubectl would
-- show". Big and rarely read: it exists for the moment someone needs the full
-- spec of the pod that died, not for dashboards. Hence ZSTD(3) on the body
-- and a TTL deliberately SHORTER than k8s_resources — a week of full YAML
-- outweighs a month of the structured rows next door.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_manifests
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    uid              String,
    kind             LowCardinality(String),
    api_version      LowCardinality(String),
    resource_version String,

    content          String CODEC(ZSTD(3)),
    content_type     LowCardinality(String),
    is_terminated    UInt8
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, kind, uid, collected_at)
TTL toDateTime(collected_at) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- Cluster summary: CollectorCluster (type 46), one row per collection pass.
--
-- The capacity-over-time table: node count, allocatable vs capacity, and the
-- version spread of kubelets and apiservers (a map of version -> node count,
-- which is how you watch an upgrade roll through). Tiny volume — one row per
-- cluster every pass — so monthly partitions and a long TTL cost nothing.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_cluster
(
    tenant_id          LowCardinality(String),
    collected_at       DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id         LowCardinality(String),
    cluster_name       LowCardinality(String),

    node_count         UInt32 CODEC(T64, ZSTD(1)),
    pod_capacity       UInt32 CODEC(T64, ZSTD(1)),
    pod_allocatable    UInt32 CODEC(T64, ZSTD(1)),
    cpu_capacity       UInt64 CODEC(T64, ZSTD(1)),
    cpu_allocatable    UInt64 CODEC(T64, ZSTD(1)),
    memory_capacity    UInt64 CODEC(T64, ZSTD(1)),
    memory_allocatable UInt64 CODEC(T64, ZSTD(1)),

    kubelet_versions   Map(String, UInt32),
    apiserver_versions Map(String, UInt32)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(collected_at)
ORDER BY (tenant_id, cluster_id, collected_at)
TTL toDateTime(collected_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Kubernetes action results: /api/v2/kubeactions
--
-- The audit half of remote configuration: who asked for what, and how it
-- went. Human scale like the events table — tens a day — and audited like
-- one, so the same monthly partitions and 90-day TTL. extra_keys records
-- which undeclared JSON keys an event carried (the agent side of this is
-- young), so a newer agent's additions are at least visible.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_actions
(
    tenant_id          LowCardinality(String),
    timestamp          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    action_id          String,
    org_id             Int64 CODEC(T64, ZSTD(1)),
    event_type         LowCardinality(String),  -- action_received / action_progress / action_executed
    status             LowCardinality(String),  -- success / failed / skipped / expired / claimed / in_progress
    action_type        LowCardinality(String),

    cluster_id         LowCardinality(String),
    cluster_name       LowCardinality(String),
    resource_id        String,
    resource_kind      LowCardinality(String),
    resource_name      String,
    resource_namespace LowCardinality(String),

    requested_by       LowCardinality(String),
    message            String CODEC(ZSTD(3)),
    extra_keys         Array(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, cluster_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Container lifecycle events: /api/v2/contlcycle
--
-- The restart/OOM history, so append-only — a ReplacingMergeTree here would
-- eat the very rows the table exists for. exit_code is Nullable because the
-- wire makes a three-way distinction the column must keep: exited 0, exited
-- nonzero, and "the runtime never reported a code". Collapsing the last into
-- 0 would invent clean exits. Same rule for the timestamps: absent is
-- absent, never 1970.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS container_events
(
    tenant_id      LowCardinality(String),
    timestamp      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host           LowCardinality(String),
    cluster_id     LowCardinality(String),
    object_kind    LowCardinality(String),  -- container / pod / task
    event_type     LowCardinality(String),  -- start / delete / ...

    container_id   String,
    container_name LowCardinality(String),
    pod_uid        String,
    task_arn       String,
    source         LowCardinality(String),

    exit_code      Nullable(Int32),
    created_at     Nullable(DateTime64(3, 'UTC')),
    exited_at      Nullable(DateTime64(3, 'UTC')),

    owner_type     LowCardinality(String),
    owner_uid      String,

    old_state      LowCardinality(String),
    new_state      LowCardinality(String),
    transition_at  Nullable(DateTime64(3, 'UTC'))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, cluster_id, container_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Container image inventory: /api/v2/contimage
--
-- Keyed by (digest, host). The digest is the immutable identity: tags move
-- between builds, the digest never does, and it is what an SBOM will
-- reference — the join key between the two tracks. host says where that
-- content actually sits, and it MUST stay in the key: without it a 50-node
-- rollout collapses into one row holding whichever node reported last, and
-- placement is silently destroyed. image_id stays a plain column — it is the
-- runtime-local id, not an identity worth keying on.
--
-- The TTL makes the inventory self-healing. While an image is still on a
-- node, every collection refreshes collected_at and the row never expires;
-- once the image is gone, nothing refreshes it and it ages out on its own.
-- ReplacingMergeTree has no notion of deletion, so without the TTL stale
-- placements would sit here forever.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS container_images
(
    tenant_id    LowCardinality(String),
    collected_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host         LowCardinality(String),

    -- The identity this table replaces on. Prefer the registry digest; fall
    -- back to the image id when the agent has no digest to give.
    --
    -- A missing digest is NORMAL, not a defect: an image built locally and
    -- never pushed, loaded from a tarball, or freshly pulled before the
    -- runtime resolved RepoDigests simply has none. Keying on digest alone
    -- would collapse every such image on a host into one row, so those images
    -- used to be dropped — which quietly made the inventory lie.
    --
    -- image_id is the image CONFIG digest: also content-addressed, just
    -- computed locally rather than assigned by a registry. Weaker across
    -- heterogeneous runtimes, which is why it is the fallback and not an equal.
    image_key    String,

    -- Which of the two fed image_key: 'digest' or 'image_id'. Makes "show me
    -- images with no registry digest" a query rather than a guess — and those
    -- are exactly the images an SBOM from sbom-intake will never join to.
    identity_source LowCardinality(String),

    image_id     String,
    digest       String,
    name         String,
    short_name   LowCardinality(String),
    registry     LowCardinality(String),

    repo_tags    Array(String),
    repo_digests Array(String),

    size_bytes   UInt64 CODEC(T64, ZSTD(1)),
    os_name      LowCardinality(String),
    os_version   LowCardinality(String),
    architecture LowCardinality(String),
    layer_count  UInt32 CODEC(T64, ZSTD(1)),
    layer_bytes  UInt64 CODEC(T64, ZSTD(1)),

    -- BuiltAt comes from layer history, PublishedAt often not at all — the
    -- agent frequently has neither, and a missing date must not become 1970.
    built_at     Nullable(DateTime64(3, 'UTC')),
    published_at Nullable(DateTime64(3, 'UTC')),

    dd_tags      Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3))
)
ENGINE = ReplacingMergeTree(collected_at)
PARTITION BY toYYYYMM(collected_at)
ORDER BY (tenant_id, image_key, host)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;
