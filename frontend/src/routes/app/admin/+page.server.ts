import { redirect } from '@sveltejs/kit';
import { requireInstanceAdmin } from '$lib/server/admin';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	requireInstanceAdmin(locals);
	redirect(307, '/app/admin/tenants');
};
