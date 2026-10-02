import { createAuthClient } from 'better-auth/svelte';

// Talks to Better Auth's endpoints under /api/auth on this origin.
export const authClient = createAuthClient();
