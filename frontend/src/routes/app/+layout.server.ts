import { redirect } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';

// Straznik dla CALEJ sekcji /app. Dzieki temu podstrony nie musza go powtarzac,
// a dodanie nowej nie grozi zapomnieniem o sprawdzeniu sesji.
export const load: LayoutServerLoad = ({ locals }) => {
	if (!locals.user) {
		redirect(302, '/login');
	}

	return {
		user: {
			email: locals.user.email,
			name: locals.user.name
		}
	};
};
