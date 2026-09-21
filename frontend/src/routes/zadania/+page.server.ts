// +page.server.ts wykonuje sie WYLACZNIE na serwerze.
// Nigdy nie trafia do przegladarki, wiec mozna tu bezpiecznie dotykac
// bazy danych, sekretow i czegokolwiek z $env/dynamic/private.

import { fail } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { task } from '$lib/server/db/schema';
import type { PageServerLoad, Actions } from './$types';

// load() odpala sie przy kazdym wejsciu na strone.
// To, co zwroci, laduje w `data` w komponencie.
export const load: PageServerLoad = async ({ locals, url }) => {
	// url to standardowy obiekt URL. Query string czytasz przez searchParams.
	// Nic nie trzeba deklarowac w nazwie katalogu - to nie jest czesc trasy.
	const filtr = url.searchParams.get('priorytet');

	const zapytanie = db.select().from(task);
	const zadania = filtr
		? await zapytanie.where(eq(task.priority, Number(filtr))).orderBy(task.priority)
		: await zapytanie.orderBy(task.priority);

	return {
		zadania,
		filtr,
		// locals.user ustawia hooks.server.ts przy kazdym requescie.
		// Dzieki temu nie trzeba sprawdzac sesji w kazdym load() osobno.
		uzytkownik: locals.user?.email ?? null
	};
};

// actions obsluguja POST z formularzy na tej stronie.
// Kazda akcja to osobny klucz; w HTML wskazujesz ja przez action="?/nazwa".
export const actions: Actions = {
	dodaj: async ({ request }) => {
		const dane = await request.formData();
		const tytul = dane.get('tytul')?.toString().trim() ?? '';
		const priorytet = Number(dane.get('priorytet') ?? 1);

		// fail() zwraca blad walidacji BEZ przekierowania - dane wracaja
		// do komponentu w zmiennej `form`, formularz zostaje wypelniony.
		if (!tytul) {
			return fail(400, { blad: 'Tytul nie moze byc pusty', tytul });
		}

		await db.insert(task).values({ title: tytul, priority: priorytet });

		// Po zakonczeniu akcji SvelteKit sam odpala load() jeszcze raz,
		// wiec lista odswiezy sie bez naszego udzialu.
		return { sukces: true };
	},

	usun: async ({ request }) => {
		const dane = await request.formData();
		const id = Number(dane.get('id'));

		if (!id) {
			return fail(400, { blad: 'Brak id' });
		}

		await db.delete(task).where(eq(task.id, id));
		return { sukces: true };
	}
};
