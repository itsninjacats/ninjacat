/**
 * Create a panel account from the command line — the equivalent of Django's
 * `manage.py createsuperuser`.
 *
 * Public registration is disabled in src/lib/server/auth.ts, so this is the
 * only way in. The script builds its OWN better-auth instance, against the
 * same database and with the same adapter, but WITHOUT disableSignUp — which
 * is the whole point: the operator at a shell may create accounts, a stranger
 * posting at /sign-up/email may not.
 *
 * Going through better-auth rather than inserting rows directly matters. The
 * password hashing, the account/credential linking and the schema details are
 * better-auth's business, and a hand-rolled INSERT would drift from them the
 * first time the library changes either.
 *
 * Deliberately not importing anything from $lib or $env: those are SvelteKit
 * aliases and this runs as a plain process, exactly like scripts/migrate.ts.
 *
 *   bun run user:create -- someone@example.com 'a good password' 'Their Name'
 */
import { betterAuth } from 'better-auth/minimal';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import * as schema from '../src/lib/server/db/schema';

const [email, password, ...rest] = process.argv.slice(2);
const name = rest.join(' ').trim();

if (!email || !password) {
	console.error('usage: bun run user:create -- <email> <password> [name]');
	process.exit(1);
}
if (!email.includes('@')) {
	console.error(`create-user: ${email} does not look like an email address`);
	process.exit(1);
}
if (password.length < 8) {
	console.error('create-user: password must be at least 8 characters');
	process.exit(1);
}

const url = process.env.DATABASE_URL;
if (!url) {
	console.error('create-user: DATABASE_URL is not set');
	process.exit(1);
}

// max: 1 — this is a one-shot script, not a server.
const sql = postgres(url, { max: 1, onnotice: () => {} });
const db = drizzle(sql, { schema });

const auth = betterAuth({
	// Silences better-auth's "Base URL is not set" warning. Nothing here needs
	// a real one: no session is issued, nothing is redirected, no cookie is
	// signed — the script creates a row and exits.
	baseURL: process.env.ORIGIN || 'http://localhost',

	// A real secret is required for the instance to start, but nothing this
	// script does depends on its value: no session is issued and no cookie is
	// signed. A throwaway keeps the script runnable without the app's secret.
	secret: process.env.BETTER_AUTH_SECRET || 'create-user-cli-no-session-is-issued',
	database: drizzleAdapter(db, { provider: 'pg' }),
	emailAndPassword: { enabled: true }
});

try {
	await auth.api.signUpEmail({
		body: { email, password, name: name || email }
	});
	console.log(`create-user: created ${email}`);
} catch (e) {
	// better-auth reports a duplicate as a BAD_REQUEST rather than a conflict,
	// so say something the operator can act on instead of echoing its message.
	const message = e instanceof Error ? e.message : String(e);
	if (/exist/i.test(message)) {
		console.error(`create-user: ${email} already has an account`);
	} else {
		console.error(`create-user: failed — ${message}`);
	}
	process.exit(1);
} finally {
	await sql.end();
}
