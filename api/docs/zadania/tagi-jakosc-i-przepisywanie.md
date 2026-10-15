# Jakość tagów: wykrywanie i przepisywanie

**Na świętego nigdy.** Zapisane, żeby pomysł nie przepadł — nie żeby to robić.

---

## Problem

Tagi tożsamości bywają bezwartościowe, a mimo to wyglądają poprawnie. Realne
przykłady z produkcji:

```
service:api          jedna nazwa na kilkadziesiąt różnych aplikacji
service:service      szablon Helma, który się nie rozwinął
service:deployment   j.w.
```

`service:service` powstaje, gdy ktoś napisze `tags.datadoghq.com/service:
{{ .Values.service }}`, wartość jest pusta i zostaje nazwa klucza. W Datadogu
nic nie krzyczy: tag jest, dashboardy się rysują, monitory działają. Tylko że
grupują po czymś, co nic nie znaczy.

Tagi ustala klaster (patrz `router-rumowy.md` i Unified Service Tagging), więc
do nas przychodzą już gotowe. Nie możemy ich poprawić u źródła, jeśli nie mamy
dostępu do klastra.

---

## Dwie rzeczy, nie jedna

### 1. Wykrywanie — tańsze i ciekawsze

Mamy komplet sygnałów z jednego agenta i te same wymiary we wszystkich
tabelach. Da się więc **policzyć**, że tag nie niesie informacji:

- ten sam `service` w wielu `kube_deployment` / namespace'ach
- wartość tagu identyczna z nazwą klucza (`service:service`)
- wartość z krótkiej listy generyków (`api`, `app`, `web`, `backend`)
- kardynalność bliska jedynki tam, gdzie powinna być większa

To jest zapytanie po istniejących tabelach, nie nowy podsystem. I nikt z
konkurencji tego nie robi, bo wymaga korelacji między sygnałami.

### 2. Przepisywanie — droższe i z pułapką

Reguły na wejściu: „jeśli `service` jest podejrzany, wyprowadź go z
`kube_deployment`".

**Pułapka:** to jest zmienianie danych przed zapisem, czyli dokładnie to, czego
unikamy w całym `intake/` — tam zasadą jest, że payload dociera nieskonwertowany
i nic nie ginie. Przepisany tag musiałby lądować **obok** oryginału, nigdy
zamiast. Inaczej za rok nikt nie odtworzy, co naprawdę przyszło.

---

## Dlaczego „na świętego nigdy"

Przepisywanie tagów to szczególny przypadek **pipeline'ów przetwarzania** —
osobnego podsystemu, którego nie mamy i który jest dużo większy niż ten jeden
przypadek: parsowanie, remapowanie, wzbogacanie, reguły warunkowe, kolejność
wykonania, podgląd efektu przed wdrożeniem.

Zbudowanie samego przepisywania tagów byłoby zbudowaniem jednej rury z
przyszłego systemu, w miejscu, które potem trzeba będzie rozebrać. Jeśli to
robić, to razem z pipeline'ami, nie przed nimi.

**Wykrywanie** tej zależności nie ma — to zapytanie, nie przetwarzanie. Gdyby
kiedyś wracać do tematu, zacząć od niego.

---

## Kolejność w projekcie

Przed tym: metryki, logi, trace'y i monitory zrobione porządnie
(patrz `silnik-kwerend.md`). Potem RUM (`router-rumowy.md`). Session Replay na
samym końcu. To jest dopiero za tym wszystkim.
