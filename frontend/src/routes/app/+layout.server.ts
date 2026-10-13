import { redirect } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals }) => {
	// The guard has already sent a visitor to /login; this narrows the type.
	if (!locals.user) {
		redirect(302, '/login');
	}

	return {
		user: {
			email: locals.user.email,
			name: locals.user.name
		},
		tenant: locals.tenant,
		tenants: locals.tenants
	};
};
