/**
 * Create a panel account. Sign-up is closed, so on a fresh install this is the
 * only way in. A plain process, outside SvelteKit: no $lib or $env imports.
 *
 *   bun run user:create -- someone@example.com --name 'Their Name'
 *       [--admin] [--tenant <id|slug>]... [--role owner|admin|member]
 *
 * --admin makes an instance admin. --tenant adds the account to an existing
 * tenant, with --role (member by default). The first account of an empty
 * database is always an admin and the owner of the tenant `default`.
 *
 * The password is asked for twice, without echo, and is never an argument (it
 * would stay in the shell history and the process list); where nobody types
 * (a Job), it comes from NINJACAT_USER_PASSWORD.
 */
import { parseArgs } from 'node:util';
import { betterAuth } from 'better-auth/minimal';
import { eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { authOptions } from '../src/lib/server/auth-options';
import * as schema from '../src/lib/server/db/schema';
import { tenantRoles } from '../src/lib/tenant';

const usage =
	'usage: bun run user:create -- <email> [--name "Their Name"] [--admin] [--tenant <id|slug>]... [--role owner|admin|member]';

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
const askedRole = tenantRoles.find((role) => role === (values.role ?? 'member'));
if (!askedRole) {
	fail(`--role is one of ${tenantRoles.join(', ')}\n${usage}`);
}

const url = process.env.DATABASE_URL;
if (!url) {
	fail('DATABASE_URL is not set');
}

const sql = postgres(url, { max: 1, onnotice: () => {} });
const db = drizzle(sql, { schema });

const auth = betterAuth(
	authOptions(db, {
		...process.env,
		// Placeholders: the script issues no session and signs no cookie.
		ORIGIN: process.env.ORIGIN || 'http://localhost',
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
