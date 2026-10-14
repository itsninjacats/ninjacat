import { fail, redirect } from '@sveltejs/kit';
import { and, desc, eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { apiKey } from '$lib/server/db/schema';
import { newKey, keyHash, keyPrefix, refreshApiKeys } from '$lib/server/api-keys';
import { formText } from '$lib/server/form';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export const load: PageServerLoad = async ({ locals }) => {
	const tenantId = locals.tenant?.id;
	if (!tenantId) {
		return { keys: [] };
	}

	const keys = await db
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

	return { keys };
};

// Every form names the tenant its page was shown for. The session's tenant can
// change in another tab, and a key made for the wrong one sends an agent's data
// there.
export const actions: Actions = {
	create: async ({ request, locals }) => {
		if (!locals.user) {
			redirect(302, '/login');
		}

		const form = await request.formData();
		const name = formText(form, 'name');

		if (!name || name.length > 100) {
			return fail(400, { error: m.api_key_name_invalid(), name });
		}

		const tenantId = locals.tenant?.id;
		if (form.get('tenant') !== tenantId) {
			return fail(409, { error: m.tenant_changed(), name });
		}

		const key = newKey();

		await db.insert(apiKey).values({
			name,
			tenantId,
			keyHash: keyHash(key),
			prefix: keyPrefix(key),
			createdById: locals.user.id
		});

		// Now, not at the intake's next refresh: the key is about to go into an agent's config.
		return { created: key, name, activeNow: await refreshApiKeys() };
	},

	delete: async ({ request, locals }) => {
		const form = await request.formData();
		const id = formText(form, 'id');

		// Postgres refuses anything else in a uuid column with an error, not a
		// row that does not match.
		if (!uuid.test(id)) {
			return fail(400, { deleteError: m.api_key_id_invalid() });
		}

		const tenantId = locals.tenant?.id;
		if (form.get('tenant') !== tenantId) {
			return fail(409, { deleteError: m.tenant_changed() });
		}

		const deleted = await db
			.delete(apiKey)
			.where(and(eq(apiKey.id, id), eq(apiKey.tenantId, tenantId)))
			.returning({ id: apiKey.id });
		if (deleted.length === 0) {
			return fail(404, { deleteError: m.api_key_not_found() });
		}

		// Until the intake re-reads its keys, the deleted one is still accepted.
		return { deleted: true, revokedNow: await refreshApiKeys() };
	}
};
