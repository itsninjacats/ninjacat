import { env } from '$env/dynamic/private';
import { betterAuth } from 'better-auth/minimal';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { sveltekitCookies } from 'better-auth/svelte-kit';
import { getRequestEvent } from '$app/server';
import { db } from '$lib/server/db';

export const auth = betterAuth({
	baseURL: env.ORIGIN,
	secret: env.BETTER_AUTH_SECRET,
	database: drizzleAdapter(db, { provider: 'pg' }),
	emailAndPassword: {
		enabled: true,

		// Registration is closed. NinjaCat is a self-hosted observability backend,
		// not a service anyone should be able to sign themselves up to — an open
		// /sign-up/email on a panel that can read every metric in the cluster is a
		// hole, not a feature.
		//
		// This is enforced HERE rather than by hiding a button: better-auth's
		// sign-up route checks this flag and answers BAD_REQUEST with
		// EMAIL_PASSWORD_SIGN_UP_DISABLED, so the endpoint is shut whatever the UI
		// does and whatever anyone posts at it directly.
		//
		// Accounts are created from the command line instead:
		//     bun run user:create -- <email> <password> [name]
		// See scripts/create-user.ts, which builds its own better-auth instance
		// with sign-up enabled against this same database — the CLI is allowed to
		// do what the public endpoint is not.
		disableSignUp: true
	},
	plugins: [
		sveltekitCookies(getRequestEvent) // make sure this is the last plugin in the array
	]
});
