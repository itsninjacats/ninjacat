import { createHash } from 'node:crypto';
import { access } from 'node:fs/promises';
import { expect, test as setup, type APIRequestContext } from '@playwright/test';
import { betterAuth } from 'better-auth/minimal';
import { and, eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { authOptions } from '../../src/lib/server/auth-options';
import * as schema from '../../src/lib/server/db/schema';
import { routes } from './routes';
import { keyNames, password, people, stateFile, tenants, type PersonKey } from './people';

// The sign-ins need the seeded people.
setup.describe.configure({ mode: 'serial' });

// Touches only e2e- rows, and brings them in line with people.ts: a row is
// inserted when missing and updated when it differs. Nothing is deleted, so a
// run can never disturb what else lives in the development database.
setup('seed the e2e tenants, people and keys', async () => {
	const sql = postgres(process.env.DATABASE_URL!, { max: 1, onnotice: () => {} });
	const db = drizzle(sql, { schema });
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
	} finally {
		await sql.end();
	}
});

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
		// Before hydration the form has no handler and submits itself as a GET,
		// credentials in the query string.
		await page.waitForLoadState('networkidle');
		await page.getByLabel('E-mail address').fill(person.email);
		await page.getByLabel('Password').fill(password);
		await page.getByRole('button', { name: 'Sign in' }).click();
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
