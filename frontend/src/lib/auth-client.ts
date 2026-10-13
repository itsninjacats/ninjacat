import { createAuthClient } from 'better-auth/svelte';
import { adminClient, organizationClient } from 'better-auth/client/plugins';

// Talks to Better Auth's endpoints under /api/auth on this origin.
export const authClient = createAuthClient({
	plugins: [adminClient(), organizationClient()]
});
