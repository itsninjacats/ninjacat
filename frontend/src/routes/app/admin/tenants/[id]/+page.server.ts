import { error, fail } from '@sveltejs/kit';
import { isAPIError } from 'better-auth/api';
import { asc, eq } from 'drizzle-orm';
import { auth } from '$lib/server/auth';
import { db } from '$lib/server/db';
import { member, organization, user } from '$lib/server/db/schema';
import { changeMember, isUniqueViolation, requireInstanceAdmin } from '$lib/server/admin';
import { formText } from '$lib/server/form';
import { isTenantRole, type TenantRole } from '$lib/tenant';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	requireInstanceAdmin(locals);

	const [tenant] = await db
		.select({
			id: organization.id,
			name: organization.name,
			slug: organization.slug,
			createdAt: organization.createdAt
		})
		.from(organization)
		.where(eq(organization.id, params.id));
	if (!tenant) {
		error(404, 'no such tenant');
	}

	const members = await db
		.select({
			id: member.id,
			userId: user.id,
			email: user.email,
			name: user.name,
			role: member.role,
			createdAt: member.createdAt
		})
		.from(member)
		.innerJoin(user, eq(user.id, member.userId))
		.where(eq(member.organizationId, tenant.id))
		.orderBy(asc(user.email));

	const memberIds = new Set(members.map((m) => m.userId));
	const users = await db
		.select({ id: user.id, email: user.email, name: user.name })
		.from(user)
		.orderBy(asc(user.email));

	return { tenant, members, candidates: users.filter((u) => !memberIds.has(u.id)) };
};

async function changed(tenantId: string, memberId: string, role: TenantRole | null) {
	const outcome = await changeMember(db, tenantId, memberId, role);
	if (outcome === 'not-found') {
		return fail(404, { error: m.admin_member_not_found() });
	}
	if (outcome === 'last-owner') {
		return fail(409, { error: m.admin_member_last_owner() });
	}
	return { changed: true };
}

export const actions: Actions = {
	add: async ({ request, locals, params }) => {
		requireInstanceAdmin(locals);

		const form = await request.formData();
		const userId = formText(form, 'user');
		const role = formText(form, 'role');
		if (userId === '' || !isTenantRole(role)) {
			return fail(400, { error: m.admin_member_invalid() });
		}

		try {
			// addMember checks no permission of its own: requireInstanceAdmin
			// above is the only check.
			await auth.api.addMember({ body: { userId, organizationId: params.id, role } });
		} catch (e) {
			if (isUniqueViolation(e)) {
				return fail(409, { error: m.admin_member_exists() });
			}
			switch (isAPIError(e) ? e.body?.code : undefined) {
				case 'USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION':
					return fail(409, { error: m.admin_member_exists() });
				case 'USER_NOT_FOUND':
				case 'ORGANIZATION_NOT_FOUND':
					return fail(404, { error: m.admin_member_no_user() });
			}
			throw e;
		}
		return { added: true };
	},

	role: async ({ request, locals, params }) => {
		requireInstanceAdmin(locals);
		const form = await request.formData();
		const memberId = formText(form, 'member');
		const role = formText(form, 'role');
		if (memberId === '' || !isTenantRole(role)) {
			return fail(400, { error: m.admin_member_invalid() });
		}
		return changed(params.id, memberId, role);
	},

	remove: async ({ request, locals, params }) => {
		requireInstanceAdmin(locals);
		const form = await request.formData();
		const memberId = formText(form, 'member');
		if (memberId === '') {
			return fail(400, { error: m.admin_member_invalid() });
		}
		return changed(params.id, memberId, null);
	}
};
