# Zadanie: ustalić, co naprawdę leci na `/api/v1/metadata`

**Stan:** nieznane
**Priorytet:** niski — nie blokuje niczego
**Skutek teraz:** endpoint odpowiada `202` i nic nie zapisuje

---

## Problem

Przez całą sesję przechwyciliśmy **cztery** żądania na `/api/v1/metadata`
i **wszystkie cztery były puste**:

```
2B surowe -> 2B   {}
2B surowe -> 2B   {}
2B surowe -> 2B   {}
2B surowe -> 2B   {}
```

Wszystkie pochodziły z **przemiatania diagnostycznego** agenta przy starcie
(nagłówek `X-Requested-With: datadog-agent-diagnose`), nie z realnego wysyłania
danych.

Czyli: **nie wiemy, co ten endpoint przyjmuje.**

To, co obecnie stoi w `docs/datadog-agent.md` („metadane checków: które
integracje są włączone, w jakich wersjach, czy się załadowały"), jest
**niezweryfikowane** — pochodzi z wiedzy ogólnej, nie z obserwacji.
Do poprawienia przy okazji.

---

## Dlaczego payload był pusty

Agent w naszych przebiegach **nie miał włączonej ani jednej integracji**.
Bez Postgresa, Redisa czy nginxa nie ma czego raportować — stąd `{}`.

---

## Jak to ustalić

### Droga A — obserwacja (szybsza)

Uruchomić agenta z jakąkolwiek działającą integracją i poczekać.
Najprościej Postgres, bo już go mamy na 5433:

```bash
# conf.d/postgres.d/conf.yaml w kontenerze agenta
init_config:
instances:
  - host: host.docker.internal
    port: 5433
    username: root
    password: mysecretpassword
    dbname: local
```

Potem przechwycić ruch tak jak przez całą sesję (`DEBUG=true` + `captures/`)
i zobaczyć, czy `/api/v1/metadata` przestanie być puste.

### Droga B — kod agenta (pewniejsza)

Integracje są w obrazie jako **czysty kod Pythona**, więc da się przeczytać,
co i kiedy tam wysyłają:

```bash
docker run --rm --entrypoint sh datadog/agent:7 -c '
P=$(ls -d /opt/datadog-agent/embedded/lib/python3.*/site-packages/datadog_checks | head -1)
grep -rn "v1/metadata\|set_metadata\|metadata_manager" "$P/base/" | head
'
```

Punkt zaczepienia: w `datadog_checks/base` jest mechanizm `set_metadata`,
którym checki raportują swoje wersje. Warto sprawdzić, czy to właśnie leci
tędy, czy dokłada się do `/intake/`.

Ta sama metoda, którą ustaliliśmy format DBM — patrz `docs/datadog-agent.md`,
sekcja o `database_monitoring_query_sample`.

---

## Co zrobić po ustaleniu

1. **Odpiąć od `decodeIntake`.** Obecnie `HandleMetadata` woła tę samą funkcję
   co `/intake/`, czyli udaje, że rozumie payload. Działa bez szkody tylko
   dlatego, że zapis do `hosts` jest strzeżony warunkiem `hostname != ""`,
   a przy `{}` hostname jest pusty.

2. **Zdecydować, czy warto zapisywać.** Jeśli to faktycznie spis integracji
   i ich wersji, to materiał na kolumnę w tabeli `hosts` albo na osobną
   tabelkę `integrations` — ale dopiero gdy będzie wiadomo, co tam jest.

3. **Poprawić `docs/datadog-agent.md`**, zastępując domysł obserwacją.

---

## Zasada, która się za tym kryje

Dokumentacja opisuje to, co **zmierzyliśmy**. Gdy czegoś nie zmierzyliśmy,
trzeba to oznaczyć, a nie uzupełniać z pamięci. Ten endpoint jest przykładem,
gdzie wiedza ogólna wślizgnęła się do dokumentu jako fakt.
