import { fail, redirect } from '@sveltejs/kit';
import { and, desc, eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { apiKey, member } from '$lib/server/db/schema';
import { nowyKlucz, skrotKlucza, prefiks, refreshApiKeys } from '$lib/server/api-keys';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

// Until the hook resolves the tenant on every request, this page checks it
// here: a session keeps its active organization after its user is removed
// from it, and a key made then would still write into that tenant.
async function activeTenant(locals: App.Locals): Promise<string | null> {
	const tenantId = locals.session?.activeOrganizationId;
	if (!locals.user || !tenantId) {
		return null;
	}
	const [membership] = await db
		.select({ id: member.id })
		.from(member)
		.where(and(eq(member.userId, locals.user.id), eq(member.organizationId, tenantId)))
		.limit(1);
	return membership ? tenantId : null;
}

export const load: PageServerLoad = async ({ locals }) => {
	if (!locals.user) {
		redirect(302, '/login');
	}

	const tenantId = await activeTenant(locals);
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

		const tenantId = await activeTenant(locals);
		if (!tenantId) {
			return fail(403, { blad: m.no_tenant(), nazwa });
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

		const tenantId = await activeTenant(locals);
		if (!tenantId) {
			return fail(403, { blad: m.no_tenant() });
		}

		await db.delete(apiKey).where(and(eq(apiKey.id, id), eq(apiKey.tenantId, tenantId)));

		// Przy usuwaniu odswiezenie jest WAZNIEJSZE niz przy tworzeniu:
		// dopoki keeper trzyma stary obraz, skasowany klucz nadal dziala.
		const odswiezenie = await refreshApiKeys();

		return { usuniety: true, odcietyOdRazu: odswiezenie.ok };
	}
};
