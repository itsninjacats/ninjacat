import { fail } from '@sveltejs/kit';
import { asc, eq } from 'drizzle-orm';
import { auth } from '$lib/server/auth';
import { isInstanceAdmin } from '$lib/server/auth-options';
import { db } from '$lib/server/db';
import { member, organization, user } from '$lib/server/db/schema';
import { authErrorCode, requireInstanceAdmin } from '$lib/server/admin';
import { formText } from '$lib/server/form';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	requireInstanceAdmin(locals);

	const users = await db
		.select({
			id: user.id,
			email: user.email,
			name: user.name,
			role: user.role,
			createdAt: user.createdAt
		})
		.from(user)
		.orderBy(asc(user.email));

	const memberships = await db
		.select({
			userId: member.userId,
			id: organization.id,
			name: organization.name,
			role: member.role
		})
		.from(member)
		.innerJoin(organization, eq(organization.id, member.organizationId))
		.orderBy(asc(organization.name));

	return {
		users: users.map(({ role, ...u }) => ({
			...u,
			instanceAdmin: isInstanceAdmin(role),
			tenants: memberships.filter((t) => t.userId === u.id)
		}))
	};
};

export const actions: Actions = {
	create: async ({ request, locals }) => {
		requireInstanceAdmin(locals);

		const form = await request.formData();
		const email = formText(form, 'email');
		const name = formText(form, 'name');
		const password = form.get('password')?.toString() ?? '';
		const admin = form.get('admin') === 'on';
		// Never the password: what a failed action returns is put back in the form.
		const values = { email, name, admin };

		// createUser hashes whatever it is given; the length rule sign-up and
		// password changes follow is checked here.
		const { minPasswordLength: min, maxPasswordLength: max } = (await auth.$context).password
			.config;
		if (password.length < min || password.length > max) {
			return fail(400, { ...values, error: m.admin_user_password_length({ min, max }) });
		}

		try {
			// With the caller's headers, so the admin plugin checks the caller's
			// own permission to create users and set their role as well.
			await auth.api.createUser({
				headers: request.headers,
				body: { email, password, name: name || email, role: admin ? 'admin' : 'user' }
			});
		} catch (e) {
			switch (authErrorCode(e)) {
				case 'USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL':
					return fail(409, { ...values, error: m.admin_user_exists() });
				case 'INVALID_EMAIL':
					return fail(400, { ...values, error: m.admin_user_email_invalid() });
			}
			throw e;
		}
		return { created: email.toLowerCase() };
	}
};
