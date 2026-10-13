import { fail, redirect } from '@sveltejs/kit';
import { and, desc, eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { apiKey } from '$lib/server/db/schema';
import { nowyKlucz, skrotKlucza, prefiks, refreshApiKeys } from '$lib/server/api-keys';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	if (!locals.user) {
		redirect(302, '/login');
	}

	const tenantId = locals.tenant?.id;
	if (!tenantId) {
		return { klucze: [] };
	}

	const klucze = await db
		.select({
			id: apiKey.id,
			name: apiKey.name,
			prefix: apiKey.prefix,
			createdAt: apiKey.createdAt,
			lastUsedAt: apiKey.lastUsedAt
		})
		.from(apiKey)
		.where(eq(apiKey.tenantId, tenantId))
		.orderBy(desc(apiKey.createdAt));

	return { klucze };
};

// Every form names the tenant its page was shown for. The session's tenant can
// change in another tab, and a key made for the wrong one sends an agent's data
// there.
export const actions: Actions = {
	utworz: async ({ request, locals }) => {
		if (!locals.user) {
			redirect(302, '/login');
		}

		const dane = await request.formData();
		const nazwa = dane.get('nazwa')?.toString().trim() ?? '';

		if (!nazwa) {
			return fail(400, { blad: 'Podaj nazwę klucza' });
		}
		if (nazwa.length > 100) {
			return fail(400, { blad: 'Nazwa może mieć najwyżej 100 znaków', nazwa });
		}

		const tenantId = locals.tenant?.id;
		if (!tenantId) {
			return fail(403, { blad: m.no_tenant(), nazwa });
		}
		if (dane.get('tenant') !== tenantId) {
			return fail(409, { blad: m.tenant_changed(), nazwa });
		}

		const klucz = nowyKlucz();

		await db.insert(apiKey).values({
			name: nazwa,
			tenantId,
			keyHash: skrotKlucza(klucz),
			prefix: prefiks(klucz),
			createdById: locals.user.id
		});

		// Mowimy serwerowi Go, zeby przeczytal klucze od razu. Bez tego
		// nowy klucz zaczalby dzialac dopiero po cyklicznym odswiezeniu
		// (do 30 s) — a uzytkownik wlasnie go skopiowal i chce wkleic
		// do konfiguracji agenta.
		const odswiezenie = await refreshApiKeys();

		// Jedyny moment, w ktorym klucz w postaci jawnej opuszcza serwer.
		// Potem zostaje tylko skrot i nie da sie go odtworzyc.
		return { utworzony: klucz, nazwa, aktywnyOdRazu: odswiezenie.ok };
	},

	usun: async ({ request, locals }) => {
		if (!locals.user) {
			redirect(302, '/login');
		}

		const dane = await request.formData();
		const id = dane.get('id')?.toString() ?? '';

		if (!id) {
			return fail(400, { blad: 'Brak identyfikatora klucza' });
		}

		const tenantId = locals.tenant?.id;
		if (!tenantId) {
			return fail(403, { blad: m.no_tenant() });
		}
		if (dane.get('tenant') !== tenantId) {
			return fail(409, { blad: m.tenant_changed() });
		}

		await db.delete(apiKey).where(and(eq(apiKey.id, id), eq(apiKey.tenantId, tenantId)));

		// Przy usuwaniu odswiezenie jest WAZNIEJSZE niz przy tworzeniu:
		// dopoki keeper trzyma stary obraz, skasowany klucz nadal dziala.
		const odswiezenie = await refreshApiKeys();

		return { usuniety: true, odcietyOdRazu: odswiezenie.ok };
	}
};
