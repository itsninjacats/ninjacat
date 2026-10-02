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
 *   bun run user:create -- someone@example.com --name 'Their Name'
 *
 * The password is asked for, twice and without echo, as createsuperuser does.
 * It is never an argument: an argument stays in the shell's history, in the
 * process list and in a Job's spec. Where nobody is there to type (a Job, a
 * script), it is read from NINJACAT_USER_PASSWORD instead.
 */
import { parseArgs } from 'node:util';
import { betterAuth } from 'better-auth/minimal';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import * as schema from '../src/lib/server/db/schema';

const usage = 'usage: bun run user:create -- <email> [--name "Their Name"]';

function fail(message: string): never {
	console.error(`create-user: ${message}`);
	process.exit(1);
}

// What was typed past the end of a line: both passwords pasted at once
// arrive together.
let typedAhead = '';

/** One line typed at the terminal, not echoed. */
function readHidden(prompt: string): Promise<string> {
	return new Promise((resolve) => {
		const stdin = process.stdin;
		let typed = '';

		const finish = () => {
			stdin.setRawMode(false);
			stdin.pause();
			stdin.off('data', onData);
			process.stdout.write('\n');
		};

		const onData = (chunk: string) => {
			const keys = [...chunk];

			for (const [at, key] of keys.entries()) {
				if (key === '\r' || key === '\n') {
					typedAhead = keys.slice(at + 1).join('');
					finish();
					resolve(typed);
					return;
				}
				if (key === '\u0003') {
					// Ctrl-C: raw mode hands it to us instead of ending the process.
					finish();
					process.exit(130);
				}
				if (key === '\u007f' || key === '\b') {
					typed = typed.slice(0, -1);
				} else {
					typed += key;
				}
			}
		};

		process.stdout.write(prompt);
		stdin.setRawMode(true);
		stdin.setEncoding('utf8');
		stdin.resume();
		stdin.on('data', onData);

		if (typedAhead !== '') {
			const ahead = typedAhead;
			typedAhead = '';
			onData(ahead);
		}
	});
}

async function askPassword(): Promise<string> {
	const fromEnvironment = process.env.NINJACAT_USER_PASSWORD;
	if (fromEnvironment) {
		return fromEnvironment;
	}
	if (!process.stdin.isTTY) {
		fail('no terminal to ask for the password at; set NINJACAT_USER_PASSWORD');
	}

	const password = await readHidden('Password: ');
	const again = await readHidden('Password (again): ');
	if (password !== again) {
		fail('the two passwords are not the same');
	}
	return password;
}

const { values, positionals } = parseArgs({
	args: process.argv.slice(2),
	options: { name: { type: 'string' } },
	allowPositionals: true
});
const [email, ...unexpected] = positionals;

if (!email) {
	fail(usage);
}
if (unexpected.length > 0) {
	fail(`the password is not an argument, and the name goes after --name\n${usage}`);
}
if (!email.includes('@')) {
	fail(`${email} does not look like an email address`);
}

const url = process.env.DATABASE_URL;
if (!url) {
	fail('DATABASE_URL is not set');
}

const password = await askPassword();
if (password.length < 8) {
	fail('the password must be at least 8 characters');
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
		body: { email, password, name: values.name?.trim() || email }
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
