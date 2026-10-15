import { createAuthClient } from 'better-auth/svelte';
import { adminClient, organizationClient } from 'better-auth/client/plugins';
import { goto } from '$app/navigation';
import { resolve } from '$app/paths';

// Talks to Better Auth's endpoints under /api/auth on this origin.
export const authClient = createAuthClient({
	plugins: [adminClient(), organizationClient()]
});

// Through Better Auth's own endpoint, as sign-in is, so it works on every page,
// including those reached without a tenant.
export async function signOut() {
	await authClient.signOut();
	await goto(resolve('/login'), { invalidateAll: true });
}
