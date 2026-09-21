# Sesje i ochrona tras w SvelteKicie

Jak to działa w ninjacacie i gdzie są pułapki. Wszystko poniżej sprawdzone
empirycznie na działającej aplikacji, nie wzięte z dokumentacji.

---

## 1. Skąd serwer wie, kim jesteś

Serwer **nie pamięta nic** między żądaniami. Każde jest osobne i anonimowe,
dopóki przeglądarka nie dołączy ciastka.

```
przegladarka  --(better-auth.session_token, httpOnly)-->  serwer
                                                             |
                                                  hooks.server.ts
                                                             |
                                     czyta token, szuka w tabeli session,
                                     sprawdza czy nie wygasl
                                                             |
                                          event.locals.user = { ... }
```

Kod w `src/hooks.server.ts`:

```ts
const session = await auth.api.getSession({ headers: event.request.headers });
if (session) {
	event.locals.session = session.session;
	event.locals.user = session.user;
}
```

Stan siedzi w **dwóch miejscach**: ciastko w przeglądarce (sam token, nic więcej)
i wiersz w tabeli `session` w Postgresie (kto to jest, do kiedy ważne).

Dlatego wylogowanie to **skasowanie wiersza**, a nie „powiedzenie serwerowi,
żeby zapomniał".

Ciastko jest `httpOnly` — JavaScript w przeglądarce go nie odczyta. To dlatego
sesji nie trzyma się w `localStorage`.

---

## 2. Strażnik — gdzie mieszka i co to właściwie jest

To **jedna linijka w `load()`**, nie mechanizm frameworka:

```ts
// src/routes/app/+layout.server.ts
export const load: LayoutServerLoad = ({ locals }) => {
	if (!locals.user) {
		redirect(302, '/login');
	}
	return { uzytkownik: { email: locals.user.email, nazwa: locals.user.name } };
};
```

Jest w **layoucie**, a nie w pojedynczej stronie, bo `load` layoutu wykonuje się
przed każdą stroną potomną:

```
1. hooks.server.ts         KAZDE zadanie: ciastko -> locals.user
        ↓
2. app/+layout.server.ts   kazda trasa pod /app: brak sesji -> /login
        ↓
3. app/**/+page.server.ts  moze juz zalozyc, ze uzytkownik jest
```

Jedno sprawdzenie chroni `/app`, `/app/ustawienia/klucze` i każdą następną
stronę, którą dodasz. Bez tego trzeba by pamiętać o sprawdzeniu w każdej z osobna.

---

## 3. PUŁAPKA: layout nie chroni akcji formularzy

To najważniejsza rzecz w tym dokumencie.

Przy wysłaniu formularza SvelteKit wykonuje **najpierw akcję, potem `load`** —
odwrotnie niż przy zwykłym wejściu na stronę:

```
GET  /app/...    ->  load layoutu  ->  load strony  ->  render
POST ?/utworz    ->  AKCJA         ->  load layoutu  ->  load strony
                     ^^^^^^^^
                     straznik jeszcze sie NIE wykonal
```

Czyli **akcja bez własnego sprawdzenia byłaby otwarta na oścież**, mimo że
strony są chronione.

Dlatego każda akcja ma swoje:

```ts
utworz: async ({ request, locals }) => {
	if (!locals.user) {
		redirect(302, '/login');
	}
	...
}
```

To wygląda na powtórzenie tego, co jest w layoucie. **Nie jest.**

### Zweryfikowane pomiarem

```
GET  /app                    bez ciastka  ->  302 na /login
GET  /app/ustawienia/klucze  bez ciastka  ->  302 na /login
POST ?/utworz                bez ciastka  ->  kluczy przed: 1, po: 1  (odrzucone)
```

Akcję odrzuciło **jej własne sprawdzenie**, nie layout.

### Zasada

```
straznik w +layout.server.ts   ->  chroni STRONY pod ta galezia
sprawdzenie w kazdej akcji      ->  chroni ZAPIS
sprawdzenie w +server.ts        ->  chroni endpointy API
```

`+server.ts` pod `/app` też **nie przechodzi** przez `load` layoutu — wymaga
własnego sprawdzenia.

---

## 4. Akcje mogą żyć tylko w `+page.server.ts`

Nie w layoucie. Dlatego formularz wylogowania w `app/+layout.svelte` celuje
w akcję strony po pełnej ścieżce:

```svelte
<form method="POST" action="/app?/wyloguj" use:enhance>
```

a sama akcja siedzi w `app/+page.server.ts`. To działa — SvelteKit pozwala
wysłać formularz do akcji innej trasy.

---

## 5. Czego nie wolno eksportować z `+page.server.ts`

Wyłącznie: `load`, `actions`, `prerender`, `csr`, `ssr`, `trailingSlash`,
`config`, `entries` — oraz cokolwiek z prefiksem `_`.

Próba wyeksportowania funkcji pomocniczej kończy się błędem 500:

```
Error: Invalid export 'skrot' in src/routes/.../+page.server.ts
```

Logika współdzielona idzie do `$lib/server/` — tak powstał
`src/lib/server/api-keys.ts`. Dobre ograniczenie: wymusza porządek zamiast
pozwalać na rozlewanie funkcji po plikach tras.

---

## 6. Gdzie co siedzi w ninjacacie

```
src/hooks.server.ts                       ciastko -> locals.user (kazde zadanie)
src/lib/server/auth.ts                    konfiguracja better-auth
src/lib/server/db/auth.schema.ts          tabele user, session, account, verification
src/routes/login/+page.server.ts          akcja `zaloguj` (rejestracji NIE MA)
src/routes/app/+layout.server.ts          straznik dla calej sekcji /app
src/routes/app/+layout.svelte             naglowek i nawigacja
src/routes/app/+page.server.ts            akcja `wyloguj`
src/routes/app/ustawienia/klucze/         klucze API: load + akcje utworz/usun
src/lib/server/api-keys.ts                generowanie i skrot kluczy
```

---

## 7. Warte rozważenia później

**Cache sesji w ciastku.** Domyślnie **każde** żądanie idzie do bazy po sesję.
Przy dashboardzie z wieloma zapytaniami to się sumuje:

```ts
export const auth = betterAuth({
	session: { cookieCache: { enabled: true, maxAge: 300 } },
	...
});
```

Sesja trafia wtedy do podpisanego ciastka i baza jest odpytywana raz na
5 minut. Kosztem jest opóźnienie wylogowania do 5 minut — przy aplikacji,
gdzie odcięcie dostępu musi być natychmiastowe, zostaw wyłączone.

**Wiele instancji serwera.** Układ jest bezstanowy, więc skaluje się poziomo
bez zmian — nie trzeba sticky sessions. Ale:

- `BETTER_AUTH_SECRET` **musi być identyczny** na wszystkich instancjach
  (podpisuje ciastka; rozjazd = losowe wylogowania)
- rate limiting better-auth domyślnie żyje **w pamięci procesu** — przy pięciu
  instancjach masz pięć niezależnych liczników. Od tego jest `secondaryStorage`
  (interfejs z metodami `get`, `set`, `delete`, `getAndDelete`, `increment`;
  `increment` jest **wymagany** przy `rateLimit.storage = 'secondary-storage'`)
- stan na poziomie modułu (`const cache = new Map()`) jest **per proces** —
  klasyczna pułapka; cokolwiek wspólne idzie do Postgresa albo Redisa

**Zakładanie kont.** Rejestracji świadomie nie ma. Trzeba zdecydować jak:
polecenie CLI, zaproszenia, albo pierwszy użytkownik ze zmiennej środowiskowej.
