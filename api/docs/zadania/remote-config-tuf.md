# Zadanie: remote config przez własny TUF

**Stan:** rozpoznane, niezaimplementowane
**Priorytet:** niski — wisienka na torcie, po metrykach / panelu / logach / monitorach
**Blokuje:** kubeactions (remediacja), autoskalowanie, zdalna zmiana konfiguracji agenta

---

## Po co to w ogóle

Remote config to **rura**, nie funkcja. Agent odpytuje serwer i dostaje podpisany
pakiet. Tą rurą płynie wszystko, co idzie **w dół**, do infrastruktury:

```
remote config
  ├── konfiguracja agenta      sampling, poziom logow, wlaczanie funkcji
  ├── kubeactions              delete_pod, restart_deployment, rollback_deployment
  ├── AUTOSKALOWANIE           targetReplicas, min/maxReplicas
  └── reguly bezpieczenstwa    CWS, ASM
```

Bez rury nie ma żadnego z tych ładunków. Nie da się zaimplementować
autoskalowania „obok" remote configu.

Dowód, że akcje jadą tym kanałem — komentarz w `agent-payload/proto/kubeactions/kubeactions.proto`:

> `ActionID` ... is different from the **RC metadata.id** which is tied to the
> **RC config lifecycle**

A przy autoskalowaniu: w całym `agent-payload` są **dokładnie cztery** pliki
JSON Schema i wszystkie są od autoskalowania
(`WorkloadRecommendationsRequest/Reply`, `WorkloadValuesList`,
`ClusterAutoscalingValuesList`). Reszta protokołu to protobuf przez intake'y —
te cztery jadą JSON-em przez remote config i dlatego są walidowane schematem.

---

## Co już ustaliliśmy empirycznie

### Remote config jest podpisany przez TUF

W binarce agenta 7.83.2:

```
TUF   root.json   targets.json   snapshot.json   timestamp.json
```

TUF (The Update Framework) podpisuje **treść konfiguracji, nie binarkę agenta**.
Agent trzyma klucze publiczne i odrzuca pakiety, których nie da się do nich
wywieść.

Cztery role, każda z własnym kluczem:

```
root        potwierdza klucze pozostalych rol, sluzy do ich rotacji
targets     podpisuje FAKTYCZNA tresc konfiguracji
snapshot    zapobiega mieszaniu wersji z roznych momentow
timestamp   zapobiega podsunieciu starej konfiguracji (freeze attack)
```

### Korzeń zaufania DA SIĘ nadpisać — potwierdzone

Klucze konfiguracyjne (**nieudokumentowane**, istnieją tylko w binarce):

```
remote_configuration.director_root
remote_configuration.director_root_version
remote_configuration.config_root
```

Test: uruchomiony agent z `DD_REMOTE_CONFIGURATION_DIRECTOR_ROOT="MOJ-WLASNY-ROOT"`
i `DD_REMOTE_CONFIGURATION_ENABLED=true`.

Wynik — agent **spanikował i zakończył pracę**:

```
pkg/config/remote/meta/meta.go:98 in parseRootVersion
  Corrupted root metadata: invalid character 'M' looking for beginning of value
panic: invalid character 'M' looking for beginning of value
```

**Panika jest dobrą wiadomością.** Znaczy, że agent traktuje podaną wartość jako
**jedyne** źródło zaufania. Gdyby po błędzie cicho wracał do wbudowanego korzenia
Datadoga, nadpisanie byłoby bezużyteczne.

Z komunikatu wiemy też:

- wartość to **surowy JSON**, nie base64 — parser od razu oczekiwał `{`
- to jest `root.json` z TUF, a funkcja czyta z niego numer wersji
- obecność `director_root_version` pasuje do mechanizmu rotacji kluczy w TUF

**Wniosek: blokady kryptograficznej nie ma.** Agent zostaje niezmieniony,
oficjalny obraz — zmienia się tylko to, komu ufa.

---

## Czego NIE wiemy

Do sprawdzenia przy implementacji:

1. **Czy poza korzeniem nie ma dodatkowego przypięcia** do kluczy Datadoga
   gdzie indziej w kodzie.
2. **Pełny protokół wymiany** — jak wygląda żądanie agenta i kształt odpowiedzi.
   Nigdy tego nie przechwyciliśmy, bo w naszych przebiegach remote config był
   wyłączony albo host zablokowany.
3. **Różnica między `director_root` a `config_root`** — w TUF bywa osobne
   repozytorium „director" (kto co ma dostać) i „config" (treść). Trzeba
   ustalić, które jest do czego.
4. **Czy da się to zrobić bez Cluster Agenta.** Autoskalowanie stosuje
   **Cluster Agent** (`datadog/cluster-agent`, osobny obraz, którego nie
   badaliśmy) — to on ma uprawnienia RBAC do zmiany liczby replik.
   Node'owy agent prawdopodobnie nie wystarczy.

---

## Jak to przetestować

### Krok 1 — przechwycić prawdziwy protokół

```bash
DD_SITE=ninjacat.local            # albo DD_PROXY_HTTPS na wlasny odbiornik
DD_REMOTE_CONFIGURATION_ENABLED=true
```

i zapisać, co agent wysyła na `config.<site>`: metodę, ścieżkę, nagłówki, ciało.
Bez tego nie ma po co generować kluczy.

Uwaga: przy `DD_SITE` wskazującym na nas potrzebny certyfikat z dwoma wpisami SAN
(`*.ninjacat.local` i `*.agent.ninjacat.local`) — patrz `docs/datadog-agent.md` §2.

### Krok 2 — wygenerować własne metadane TUF

Czterema kluczami (root, targets, snapshot, timestamp), na przykład biblioteką
`theupdateframework/go-tuf`.

### Krok 3 — podać agentowi własny korzeń

```bash
DD_REMOTE_CONFIGURATION_DIRECTOR_ROOT='{"signed":{...},"signatures":[...]}'
```

Kryterium sukcesu: agent **nie panikuje** i loguje pobranie konfiguracji.

### Krok 4 — odesłać podpisany pakiet

Najprostszy możliwy — na przykład zmiana poziomu logowania. Jeśli agent go
zastosuje, rura działa i reszta (kubeactions, autoskalowanie) to już tylko
kwestia ładunku.

---

## Kolejność w projekcie

```
1. metryki + zapis do ClickHouse     <- fundament
2. pokazywanie metryk w panelu
3. przegladarka logow
4. monitory
...
n. remote config + kubeactions       <- TO ZADANIE
```

Do tamtej pory protokół remote configu i tak zdąży się zmienić. Lepiej mieć
działający produkt niż najtrudniejszą funkcję bez reszty.
