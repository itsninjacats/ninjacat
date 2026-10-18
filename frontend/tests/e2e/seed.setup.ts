import { createHash } from 'node:crypto';
import { access } from 'node:fs/promises';
import { expect, test as setup, type APIRequestContext } from '@playwright/test';
import { betterAuth } from 'better-auth/minimal';
import { and, eq, sql } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { authOptions } from '../../src/lib/server/auth-options';
import * as schema from '../../src/lib/server/db/schema';
import { routes } from './routes';
import { keyNames, password, people, stateFile, tenants, type PersonKey } from './people';
import { canonicalJson } from '../../src/lib/monitors/definition';
import { checkRunsOf, definitionOf, historyOf, monitors } from './monitors';

// The sign-ins need the seeded people.
setup.describe.configure({ mode: 'serial' });

// Touches only e2e- rows, and brings them in line with people.ts: a row is
// inserted when missing and updated when it differs. Nothing is deleted, so a
// run can never disturb what else lives in the development database.
setup('seed the e2e tenants, people and keys', async () => {
	const client = postgres(process.env.DATABASE_URL!, { max: 1, onnotice: () => {} });
	const db = drizzle(client, { schema });
	// The panel's own options, as scripts/create-user.ts uses them: passwords are
	// hashed exactly as the panel hashes them.
	const auth = betterAuth(authOptions(db, process.env));

	try {
		const created = new Date('2026-01-01T00:00:00Z');
		for (const tenant of Object.values(tenants)) {
			await db
				.insert(schema.organization)
				.values({ ...tenant, createdAt: created })
				.onConflictDoUpdate({
					target: schema.organization.id,
					set: { name: tenant.name, slug: tenant.slug }
				});
		}

		const userIds = {} as Record<PersonKey, string>;
		for (const [key, person] of Object.entries(people) as [
			PersonKey,
			(typeof people)[PersonKey]
		][]) {
			const [existing] = await db
				.select({ id: schema.user.id })
				.from(schema.user)
				.where(eq(schema.user.email, person.email));
			const userRole = person.admin ? 'admin' : 'user';
			if (existing) {
				userIds[key] = existing.id;
				await db
					.update(schema.user)
					.set({ name: person.name, role: userRole })
					.where(eq(schema.user.id, existing.id));
			} else {
				userIds[key] = (
					await auth.api.createUser({
						body: { email: person.email, password, name: person.name, role: userRole }
					})
				).user.id;
			}

			for (const [order, { tenant, role }] of person.tenants.entries()) {
				const organizationId = tenants[tenant].id;
				const [member] = await db
					.select({ id: schema.member.id })
					.from(schema.member)
					.where(
						and(
							eq(schema.member.userId, userIds[key]),
							eq(schema.member.organizationId, organizationId)
						)
					);
				if (member) {
					await db.update(schema.member).set({ role }).where(eq(schema.member.id, member.id));
				} else {
					await db.insert(schema.member).values({
						id: `e2e-${organizationId}-${userIds[key]}`,
						organizationId,
						userId: userIds[key],
						role,
						// The first listed is the tenant a new session starts in.
						createdAt: new Date(created.getTime() + order * 1000)
					});
				}
			}
		}

		for (const tenant of ['a', 'b'] as const) {
			for (const name of keyNames[tenant]) {
				// Derived from the name, so a second run finds the same hash and
				// inserts nothing. Stored as the panel stores a key: its SHA-256 and
				// its first 8 characters (src/lib/server/api-keys.ts).
				const key = sha256(name).slice(0, 32);
				await db
					.insert(schema.apiKey)
					.values({
						name,
						tenantId: tenants[tenant].id,
						keyHash: sha256(key),
						prefix: key.slice(0, 8),
						createdById: userIds.admin
					})
					.onConflictDoNothing();
			}
		}

		// A monitor is written as the panel writes it, but by its fixed id. A
		// changed definition bumps the version, so the monitor service reloads it.
		for (const monitor of Object.values(monitors)) {
			const definition = definitionOf(monitor);
			const [existing] = await db
				.select({
					name: schema.monitor.name,
					type: schema.monitor.type,
					query: schema.monitor.query,
					message: schema.monitor.message,
					tags: schema.monitor.tags,
					priority: schema.monitor.priority,
					options: schema.monitor.options,
					editor: schema.monitor.editor,
					deletedAt: schema.monitor.deletedAt
				})
				.from(schema.monitor)
				.where(eq(schema.monitor.id, monitor.id));
			if (!existing) {
				await db
					.insert(schema.monitor)
					.overridingSystemValue()
					.values({
						...definition,
						id: monitor.id,
						tenantId: tenants[monitor.tenant].id,
						createdById: userIds.admin,
						updatedById: userIds.admin
					});
			} else if (canonicalJson(existing) !== canonicalJson({ ...definition, deletedAt: null })) {
				await db
					.update(schema.monitor)
					.set({
						...definition,
						version: sql`${schema.monitor.version} + 1`,
						deletedAt: null,
						deletedById: null,
						updatedAt: sql`now()`
					})
					.where(eq(schema.monitor.id, monitor.id));
			}
		}
	} finally {
		await client.end();
	}
});

// What the monitors went through, in ClickHouse, as the monitor service stores
// it: the status pages read it. Aligned to six hours, so runs within them
// write the same transitions, which ClickHouse collapses.
setup('seed the e2e monitors history', async () => {
	const base = Math.floor(Date.now() / (6 * 3_600_000)) * 6 * 3_600_000;
	for (const key of Object.keys(monitors) as (keyof typeof monitors)[]) {
		const { transitions, evaluations } = historyOf(key, tenants[monitors[key].tenant].id, base);
		await insertRows('monitor_transitions', transitions);
		await insertRows('monitor_evaluations', evaluations);
	}
	// Runs are not collapsed, so the check monitor's are replaced rather than added to.
	const tenant = tenants[monitors.check.tenant].id;
	await clickhouse('DELETE FROM check_runs WHERE tenant_id = {tenant:String}', '', { tenant });
	await insertRows('check_runs', checkRunsOf(tenant, base));
});

async function insertRows(table: string, rows: Record<string, unknown>[]) {
	if (rows.length === 0) return;
	await clickhouse(
		`INSERT INTO ${table} FORMAT JSONEachRow`,
		rows.map((row) => JSON.stringify(row)).join('\n')
	);
}

async function clickhouse(query: string, body: string, params: Record<string, string> = {}) {
	const address = process.env.CLICKHOUSE_HTTP_ADDR ?? 'localhost:8123';
	const user = process.env.CLICKHOUSE_USER ?? 'ninjacat';
	const password = process.env.CLICKHOUSE_PASSWORD ?? 'ninjacat';
	const url = new URL(`http://${address}/`);
	url.searchParams.set('database', process.env.CLICKHOUSE_DB ?? 'ninjacat');
	url.searchParams.set('query', query);
	url.searchParams.set('date_time_input_format', 'best_effort');
	for (const [name, value] of Object.entries(params)) url.searchParams.set(`param_${name}`, value);
	const response = await fetch(url, {
		method: 'POST',
		headers: { authorization: `Basic ${btoa(`${user}:${password}`)}` },
		body
	});
	expect(response.status, await response.text()).toBe(200);
}

// A session that still works, for the same person, is kept: each sign-in adds a session row, and the
// limiter allows five a minute.
async function stillSignedIn(request: APIRequestContext, email: string): Promise<boolean> {
	const response = await request.get('/api/auth/get-session');
	return response.ok() && (await response.json())?.user?.email === email;
}

setup('sign in the admin through the form', async ({ browser }) => {
	const person = people.admin;
	const context = await browser.newContext({
		storageState: await existingState('admin'),
		extraHTTPHeaders: { 'x-forwarded-for': person.address }
	});
	if (!(await stillSignedIn(context.request, person.email))) {
		const page = await context.newPage();
		await page.goto('/login');
		// Enabled once the page has hydrated, so what is typed is not reset.
		const signIn = page.getByRole('button', { name: 'Sign in' });
		await expect(signIn).toBeEnabled();
		await page.getByLabel('E-mail address').fill(person.email);
		await page.getByLabel('Password').fill(password);
		await signIn.click();
		await page.waitForURL('**/app');
	}
	await context.storageState({ path: stateFile('admin') });
	await context.close();
});

for (const key of ['memberA', 'memberB', 'longEmail', 'noTenant'] as const) {
	setup(`sign in ${key} through Better Auth's endpoint`, async ({ playwright, baseURL }) => {
		const person = people[key];
		const request = await playwright.request.newContext({
			baseURL,
			storageState: await existingState(key),
			extraHTTPHeaders: { 'x-forwarded-for': person.address, origin: baseURL! }
		});
		if (!(await stillSignedIn(request, person.email))) {
			const response = await request.post('/api/auth/sign-in/email', {
				data: { email: person.email, password }
			});
			expect(response.status(), await response.text()).toBe(200);
		}
		await request.storageState({ path: stateFile(key) });
		await request.dispose();
	});
}

// Vite compiles a page, and finds the dependencies it has not optimised yet, on
// the first request for it; a new dependency reloads every open page. Opening
// each page once here keeps that from happening under the view tests.
setup('warm up the dev server', async ({ browser }) => {
	setup.setTimeout(180_000);
	for (const route of routes) {
		const context = await browser.newContext({
			storageState: route.as ? stateFile(route.as) : undefined
		});
		const page = await context.newPage();
		await page.goto(route.path);
		await page.waitForLoadState('networkidle');
		await context.close();
	}
});

function sha256(text: string): string {
	return createHash('sha256').update(text).digest('hex');
}

async function existingState(person: PersonKey): Promise<string | undefined> {
	try {
		await access(stateFile(person));
		return stateFile(person);
	} catch {
		return undefined;
	}
}
