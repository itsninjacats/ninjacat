import { redirect } from '@sveltejs/kit';
import { isInstanceAdmin } from '$lib/server/auth-options';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	if (locals.tenant) {
		redirect(302, '/app');
	}
	return {
		email: locals.user?.email ?? '',
		instanceAdmin: isInstanceAdmin(locals.user?.role)
	};
};
