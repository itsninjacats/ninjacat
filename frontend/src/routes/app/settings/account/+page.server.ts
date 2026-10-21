import { auth } from '$lib/server/auth';
import type { PageServerLoad } from './$types';

// The length rule Better Auth checks the new password against, so the form can say it first.
export const load: PageServerLoad = async () => {
	const { minPasswordLength, maxPasswordLength } = (await auth.$context).password.config;
	return { minPasswordLength, maxPasswordLength };
};
