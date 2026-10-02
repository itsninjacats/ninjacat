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
		//     bun run user:create -- <email> [--name "Their Name"]
		// See scripts/create-user.ts, which builds its own better-auth instance
		// with sign-up enabled against this same database — the CLI is allowed to
		// do what the public endpoint is not.
		disableSignUp: true
	},
	// Sign-in attempts are counted per client address. Better Auth's limiter is
	// on only in production unless told otherwise, and its default for sign-in
	// (3 in 10 seconds) still allows eighteen guesses a minute.
	//
	// The counters live in this process's memory: enough for one panel pod.
	// With several, switch `storage` to "database" (it needs a rateLimit table).
	rateLimit: {
		enabled: true,
		customRules: { '/sign-in/email': { window: 60, max: 5 } }
	},
	// The client's address is read from X-Forwarded-For. Without this list
	// only a header holding a single address is believed; behind a proxy that
	// appends to the header nothing is, and every visitor shares one counter.
	// The same setting, with the same meaning, as the intake's.
	advanced: {
		ipAddress: {
			trustedProxies: (env.NINJACAT_TRUSTED_PROXIES ?? '')
				.split(',')
				.map((proxy) => proxy.trim())
				.filter((proxy) => proxy !== '')
		}
	},
	plugins: [
		sveltekitCookies(getRequestEvent) // make sure this is the last plugin in the array
	]
});
