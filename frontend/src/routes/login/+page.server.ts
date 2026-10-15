import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

// There is no action here: the form signs in through Better Auth's own
// endpoint (/api/auth/sign-in/email), where its rate limiter counts the
// attempts and the response sets the session cookie.
export const load: PageServerLoad = ({ locals }) => {
	if (locals.user) {
		redirect(302, '/app');
	}
	return {};
};
