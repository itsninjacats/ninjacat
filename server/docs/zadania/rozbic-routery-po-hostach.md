# Rozbić routery — jeden host, jeden plik

**Do zrobienia. Czysto porządkowe, nie zmienia zachowania.**

W `server/intake/` część plików grupuje kilka niezwiązanych hostów. To nie
jest decyzja projektowa — tak wyszło z podziału pracy między agentów podczas
pisania (jeden agent dostawał „temat", np. security, i wszystko lądowało
w jednym pliku).

Routing jest tym nietknięty: każdy host ma własny `case` w switchu w
`routes.go` i własny silnik Gina, więc izolacja działa. To wyłącznie kwestia
tego, w którym pliku leży kod.

---

## Co rozbić

**`router_install.go`** — najgorszy przypadek, trzy zupełnie niezwiązane rzeczy:

| funkcja | host | co to |
|---|---|---|
| `routeInstall` | `install.datadoghq.com` | rejestr OCI, pobieranie pakietów |
| `routeAIUsage` | `eudm-intake.<site>` | zużycie AI, przez lokalny evp_proxy |
| `routeLLMObs` | `llmobs-intake.<site>` | sonda diagnostyczna |

Rejestr OCI to w ogóle inny kierunek ruchu — agent **pobiera** od nas pliki,
a nie wysyła dane. Zasługuje na własny plik choćby z tego powodu.

**`router_misc.go`** — dwa niezwiązane hosty:

| funkcja | host |
|---|---|
| `routeResources` | `resources-intake.<site>` |
| `routeTelemetry` | `instrumentation-telemetry-intake.<site>` |

**`router_security.go`** — pięć hostów, pięć funkcji route. Tu grupowanie
częściowo się broni (wszystko to bezpieczeństwo), ale `sbom-intake` jest
bliżej inwentarza niż CWS-a. Do rozważenia, nie pilne.

**`router_evp.go`** — sześć hostów, sześć funkcji route. Łączy je tylko to,
że jadą przez Event Platform, co jest cechą transportu, nie domeny.
`agenthealth` w ogóle **nie jest** trackiem EvP — ma własny forwarder.

---

## Czego NIE ruszać

Te grupowania są domenowe i mają zostać — jedna funkcja route obsługuje
kilka nazw hostów, bo to jeden produkt:

- `routeNDM` — `ndm-intake`, `snmp-traps-intake`, `ndmflow-intake`,
  `netpath-intake`. Jeden produkt (Network Device Monitoring), jedna rodzina
  dekoderów.
- `routeContainers` — `contlcycle-intake` + `contimage-intake`, ten sam
  komponent agenta.
- `routeLogs` — `http-intake.logs` + `agent-http-intake.logs`, identyczny
  payload.
- `routeKubeops` — `kubeops-intake` + `orchestrator`.
- `routeDBM` — jeden host, sześć tracków.

---

## Uwaga przy rozbijaniu — nieaktualna, zamknięte 2026-09-20

Była tu ostrzeżenie o `routeSharedInput`, który rozstrzygał kolizję ścieżki
`/v1/input` między logami a profilerem na hoście łączonym. Tryb łączony
(silnik `all`) został usunięty — serwer stoi zawsze za wieloma ingressami,
każdy intake ma własny `Host`, a nierozpoznany host dostaje 404 z nazwą.
`routeSharedInput`, `routeProfileOnly` i `routeLogsOnly` już nie istnieją.
`routeLogs` i `routeProfile` rejestrują `/v1/input` każde na swoim hoście
i to jest poprawne. Routing po hoście pokrywa `intake/routes_test.go`.
