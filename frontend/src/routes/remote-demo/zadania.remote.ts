// Plik *.remote.ts wykonuje sie WYLACZNIE na serwerze - tak samo jak
// +page.server.ts. Roznica polega na tym, ze eksportowane funkcje mozna
// zaimportowac i zawolac wprost z komponentu, jakby byly lokalne.
//
// SvelteKit podmienia je pod spodem na wywolanie HTTP. Ty tego nie widzisz.

import { query, command } from '$app/server';
import { eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { task } from '$lib/server/db/schema';

// query = odczyt. Wynik jest cache'owany i da sie go odswiezyc.
export const pobierzZadania = query(async () => {
	return db.select().from(task).orderBy(task.priority);
});

// command = zapis. 'unchecked' znaczy "nie waliduj argumentu schematem" -
// w prawdziwym kodzie wstawia sie tu schemat (valibot, zod), bo argument
// przychodzi z przegladarki i nie wolno mu ufac.
export const dodajZadanie = command('unchecked', async (tytul: string) => {
	if (!tytul.trim()) {
		throw new Error('Tytul nie moze byc pusty');
	}
	await db.insert(task).values({ title: tytul, priority: 1 });

	// Mowi klientowi: te dane sa juz nieaktualne, pobierz na nowo.
	await pobierzZadania().refresh();
});

export const usunZadanie = command('unchecked', async (id: number) => {
	await db.delete(task).where(eq(task.id, id));
	await pobierzZadania().refresh();
});
