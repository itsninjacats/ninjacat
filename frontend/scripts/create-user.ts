/**
 * Create a panel account from the command line — the equivalent of Django's
 * `manage.py createsuperuser`.
 *
 * Public registration is disabled (src/lib/server/auth-options.ts), so on a
 * fresh install this is the only way in. The account is made by the admin
 * plugin's createUser, called as a system action: no session, so no
 * permission check, and disableSignUp does not apply to it. The script uses
 * the panel's own options, so it hashes and stores exactly as the panel does.
 *
 * Deliberately not importing anything from $lib or $env: those are SvelteKit
 * aliases and this runs as a plain process, exactly like scripts/migrate.ts.
 *
 *   bun run user:create -- someone@example.com --name 'Their Name'
 *       [--admin] [--tenant <id|slug>]... [--role owner|admin|member]
 *
 * --admin makes an instance admin, who creates tenants and accounts.
 * --tenant adds the account to an existing tenant, with --role (member by
 * default). The first account of an empty database is always an admin and
 * the owner of the tenant `default`, which it creates.
 *
 * The password is asked for, twice and without echo, as createsuperuser does.
 * It is never an argument: an argument stays in the shell's history, in the
 * process list and in a Job's spec. Where nobody is there to type (a Job, a
 * script), it is read from NINJACAT_USER_PASSWORD instead.
 */
import { parseArgs } from 'node:util';
import { betterAuth } from 'better-auth/minimal';
import { eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { authOptions } from '../src/lib/server/auth-options';
import * as schema from '../src/lib/server/db/schema';

const usage =
	'usage: bun run user:create -- <email> [--name "Their Name"] [--admin] [--tenant <id|slug>]... [--role owner|admin|member]';
const roles = ['owner', 'admin', 'member'] as const;

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
	options: {
		name: { type: 'string' },
		admin: { type: 'boolean', default: false },
		tenant: { type: 'string', multiple: true, default: [] },
		role: { type: 'string' }
	},
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
const askedRole = roles.find((role) => role === (values.role ?? 'member'));
if (!askedRole) {
	fail(`--role is one of ${roles.join(', ')}\n${usage}`);
}

const url = process.env.DATABASE_URL;
if (!url) {
	fail('DATABASE_URL is not set');
}

// max: 1 — this is a one-shot script, not a server.
const sql = postgres(url, { max: 1, onnotice: () => {} });
const db = drizzle(sql, { schema });

const auth = betterAuth(
	authOptions(db, {
		...process.env,
		// Silences better-auth's "Base URL is not set" warning. Nothing here needs
		// a real one: no session is issued, nothing is redirected, no cookie is
		// signed — the script creates rows and exits.
		ORIGIN: process.env.ORIGIN || 'http://localhost',
		// A real secret is required for the instance to start, but nothing this
		// script does depends on its value. A throwaway keeps the script runnable
		// without the app's secret.
		BETTER_AUTH_SECRET: process.env.BETTER_AUTH_SECRET || 'create-user-cli-no-session-is-issued'
	})
);

// An id wins over a slug: an owner can set any slug, another tenant's id
// included.
async function tenantId(tenant: string): Promise<string> {
	for (const column of [schema.organization.id, schema.organization.slug]) {
		const [found] = await db
			.select({ id: schema.organization.id })
			.from(schema.organization)
			.where(eq(column, tenant));
		if (found) {
			return found.id;
		}
	}
	fail(`there is no tenant ${tenant}`);
}

try {
	const [anyone] = await db.select({ id: schema.user.id }).from(schema.user).limit(1);
	const first = !anyone;
	if (first && (values.tenant.length > 0 || values.role !== undefined)) {
		fail(
			'the first account is always the admin and owner of the tenant default; drop --tenant and --role'
		);
	}
	const admin = first || values.admin;
	const role = first ? 'owner' : askedRole;
	const tenants = first
		? ['default']
		: [...new Set(await Promise.all(values.tenant.map(tenantId)))];

	const password = await askPassword();
	if (password.length < 8) {
		fail('the password must be at least 8 characters');
	}

	if (first) {
		// Better Auth's createOrganization chooses the id itself, and this one
		// has to be 'default': the server's self-monitoring writes to it.
		await db
			.insert(schema.organization)
			.values({ id: 'default', name: 'Default', slug: 'default', createdAt: new Date() })
			.onConflictDoNothing();
	}

	const { user } = await auth.api.createUser({
		body: { email, password, name: values.name?.trim() || email, role: admin ? 'admin' : 'user' }
	});
	try {
		for (const organizationId of tenants) {
			await auth.api.addMember({ body: { userId: user.id, organizationId, role } });
		}
	} catch (e) {
		// Running this again could not mend an account left without its
		// tenants: the e-mail is taken, and after the first account the user
		// table is no longer empty.
		await (await auth.$context).internalAdapter.deleteUser(user.id);
		throw e;
	}

	console.log(`create-user: created ${email}${admin ? ', an instance admin' : ''}`);
	for (const organizationId of tenants) {
		console.log(`create-user: ${role} of the tenant ${organizationId}`);
	}
} catch (e) {
	// better-auth reports a duplicate as a BAD_REQUEST rather than a conflict,
	// so say something the operator can act on instead of echoing its message.
	const message = e instanceof Error ? e.message : String(e);
	if (/exist/i.test(message)) {
		console.error(`create-user: ${email} already has an account`);
	} else {
		console.error(`create-user: failed — ${message}`);
	}
	process.exitCode = 1;
} finally {
	await sql.end();
}
