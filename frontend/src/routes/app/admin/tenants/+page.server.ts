import { fail, redirect } from '@sveltejs/kit';
import { asc, count, eq } from 'drizzle-orm';
import { auth } from '$lib/server/auth';
import { db } from '$lib/server/db';
import { member, organization, user } from '$lib/server/db/schema';
import {
	authErrorCode,
	formText,
	isUniqueViolation,
	requireInstanceAdmin
} from '$lib/server/admin';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const admin = requireInstanceAdmin(locals);

	const tenants = await db
		.select({
			id: organization.id,
			name: organization.name,
			slug: organization.slug,
			createdAt: organization.createdAt,
			members: count(member.id)
		})
		.from(organization)
		.leftJoin(member, eq(member.organizationId, organization.id))
		.groupBy(organization.id)
		.orderBy(asc(organization.name));

	const users = await db
		.select({ id: user.id, email: user.email, name: user.name })
		.from(user)
		.orderBy(asc(user.email));

	return { tenants, users, adminId: admin.id };
};

// The plugin takes any string as a slug; this keeps it to what can be typed
// after create-user.ts --tenant without quoting.
const slugPattern = /^[a-z0-9]+(-[a-z0-9]+)*$/;

export const actions: Actions = {
	create: async ({ request, locals }) => {
		requireInstanceAdmin(locals);

		const form = await request.formData();
		const name = formText(form, 'name');
		const slug = formText(form, 'slug').toLowerCase();
		const owner = formText(form, 'owner');
		const values = { name, slug, owner };

		if (name === '' || name.length > 100) {
			return fail(400, { ...values, error: m.admin_tenant_name_invalid() });
		}
		if (!slugPattern.test(slug) || slug.length > 63) {
			return fail(400, { ...values, error: m.admin_tenant_slug_invalid() });
		}
		const [found] = await db.select({ id: user.id }).from(user).where(eq(user.id, owner));
		if (!found) {
			return fail(400, { ...values, error: m.admin_tenant_owner_missing() });
		}

		let created;
		try {
			// Without the caller's headers this is a system action: the plugin
			// makes `userId` the owner, not the caller, who need not be a member,
			// and leaves the caller's active tenant alone.
			created = await auth.api.createOrganization({ body: { name, slug, userId: owner } });
		} catch (e) {
			if (authErrorCode(e) === 'ORGANIZATION_ALREADY_EXISTS' || isUniqueViolation(e)) {
				return fail(409, { ...values, error: m.admin_tenant_slug_taken() });
			}
			throw e;
		}

		redirect(303, `/app/admin/tenants/${created.id}`);
	}
};
