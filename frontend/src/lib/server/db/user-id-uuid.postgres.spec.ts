import { randomBytes } from 'node:crypto';
import { cpSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { betterAuth } from 'better-auth/minimal';
import { generateRandomString, hashPassword } from 'better-auth/crypto';
import { eq } from 'drizzle-orm';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import { authOptions } from '../auth-options';
import * as schema from './schema';
import { migrations, reachable, scratchDatabase } from './scratch';

// 0003_user_id_uuid on the rows an install of the release before it holds:
// the database is built from the migrations up to 0002, filled, then migrated.
const { account, apiKey, invitation, member, monitor, organization, session, user } = schema;

const uuidV4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const betterAuthId = () => generateRandomString(32, 'a-z', 'A-Z', '0-9');

describe.skipIf(!(await reachable()))('user ids become UUIDs, in Postgres', () => {
	const throughMonitor = join(tmpdir(), `ninjacat_migrations_${randomBytes(6).toString('hex')}`);
	const scratch = scratchDatabase(throughMonitor);
	const { db, sql } = scratch;

	// Two users from Better Auth's default generator, one already a UUID.
	const old = { alice: betterAuthId(), bob: betterAuthId(), carol: crypto.randomUUID() };
	const alicePassword = 'alice-password-from-before';

	const userReferences = () => sql<{ name: string; table: string; definition: string }[]>`
		select conname as name, conrelid::regclass::text as table,
			pg_get_constraintdef(oid) as definition
		from pg_constraint
		where contype = 'f' and confrelid = 'public.user'::regclass
		order by conname`;

	const tables = ['user', 'account', 'session', 'member', 'invitation', 'api_key', 'monitor'];
	const counts = async () =>
		Object.fromEntries(
			await Promise.all(
				tables.map(async (table) => {
					const [{ count }] = await sql<{ count: number }[]>`
						select count(*)::int as count from ${sql(table)}`;
					return [table, count];
				})
			)
		);

	let referencesBefore: Awaited<ReturnType<typeof userReferences>>;
	let countsBefore: Record<string, number>;
	let ids: Record<keyof typeof old, string>;

	beforeAll(async () => {
		cpSync(migrations, throughMonitor, { recursive: true });
		const journalFile = join(throughMonitor, 'meta', '_journal.json');
		const journal = JSON.parse(readFileSync(journalFile, 'utf8'));
		journal.entries = journal.entries.filter((entry: { idx: number }) => entry.idx <= 2);
		writeFileSync(journalFile, JSON.stringify(journal));
		await scratch.create();

		const now = new Date();
		const expiresAt = new Date(now.getTime() + 3_600_000);
		await db
			.insert(organization)
			.values({ id: 'default', name: 'Default', slug: 'default', createdAt: now });
		await db.insert(user).values(
			Object.entries(old).map(([name, id]) => ({
				id,
				name,
				email: `${name}@example.com`,
				role: name === 'alice' ? 'admin' : 'user'
			}))
		);
		const aliceHash = await hashPassword(alicePassword);
		await db.insert(account).values([
			...Object.entries(old).map(([name, id]) => ({
				id: betterAuthId(),
				accountId: id,
				providerId: 'credential',
				userId: id,
				password: name === 'alice' ? aliceHash : null,
				updatedAt: now
			})),
			// Not a credential account: its account_id is the provider's, not ours.
			{
				id: betterAuthId(),
				accountId: '12345',
				providerId: 'github',
				userId: old.bob,
				updatedAt: now
			}
		]);
		await db.insert(session).values([
			{ id: 'alice-session', token: betterAuthId(), userId: old.alice, expiresAt, updatedAt: now },
			{
				id: 'bob-impersonated',
				token: betterAuthId(),
				userId: old.bob,
				impersonatedBy: old.alice,
				expiresAt,
				updatedAt: now
			}
		]);
		await db.insert(member).values([
			{
				id: 'alice-owner',
				organizationId: 'default',
				userId: old.alice,
				role: 'owner',
				createdAt: now
			},
			{ id: 'bob-member', organizationId: 'default', userId: old.bob, createdAt: now },
			{ id: 'carol-member', organizationId: 'default', userId: old.carol, createdAt: now }
		]);
		await db.insert(invitation).values({
			id: 'invitation',
			organizationId: 'default',
			email: 'dave@example.com',
			inviterId: old.alice,
			expiresAt
		});
		await db.insert(apiKey).values([
			{
				name: 'bob',
				tenantId: 'default',
				keyHash: 'hash-bob',
				prefix: 'aaaaaaaa',
				createdById: old.bob
			},
			{
				name: 'carol',
				tenantId: 'default',
				keyHash: 'hash-carol',
				prefix: 'bbbbbbbb',
				createdById: old.carol
			}
		]);
		await db.insert(monitor).values([
			{
				tenantId: 'default',
				name: 'changed hands',
				type: 'query alert',
				query: 'avg(last_5m):avg:system.load.1{*} > 1',
				createdById: old.alice,
				updatedById: old.bob,
				deletedById: old.carol,
				deletedAt: now
			},
			{
				tenantId: 'default',
				name: 'nobody',
				type: 'query alert',
				query: 'avg(last_5m):avg:system.load.1{*} > 2'
			}
		]);

		referencesBefore = await userReferences();
		countsBefore = await counts();

		await migrate(db, { migrationsFolder: migrations });

		const users = await db.select({ id: user.id, name: user.name }).from(user);
		ids = Object.fromEntries(users.map((row) => [row.name, row.id])) as typeof ids;
	});

	afterAll(async () => {
		await scratch.drop();
		rmSync(throughMonitor, { recursive: true, force: true });
	});

	it('gives every user that had no UUID a new one, and leaves a UUID alone', () => {
		expect(ids.alice).toMatch(uuidV4);
		expect(ids.bob).toMatch(uuidV4);
		expect(ids.alice).not.toBe(ids.bob);
		expect(ids.carol).toBe(old.carol);
	});

	it('moves every column that holds a user id along with it', async () => {
		const accounts = await db
			.select({
				userId: account.userId,
				accountId: account.accountId,
				provider: account.providerId
			})
			.from(account);
		expect(accounts).toEqual(
			expect.arrayContaining([
				{ userId: ids.alice, accountId: ids.alice, provider: 'credential' },
				{ userId: ids.bob, accountId: ids.bob, provider: 'credential' },
				{ userId: ids.carol, accountId: ids.carol, provider: 'credential' },
				{ userId: ids.bob, accountId: '12345', provider: 'github' }
			])
		);

		const sessions = await db
			.select({ id: session.id, userId: session.userId, impersonatedBy: session.impersonatedBy })
			.from(session);
		expect(sessions).toEqual(
			expect.arrayContaining([
				{ id: 'alice-session', userId: ids.alice, impersonatedBy: null },
				{ id: 'bob-impersonated', userId: ids.bob, impersonatedBy: ids.alice }
			])
		);

		const members = await db.select({ id: member.id, userId: member.userId }).from(member);
		expect(Object.fromEntries(members.map((row) => [row.id, row.userId]))).toEqual({
			'alice-owner': ids.alice,
			'bob-member': ids.bob,
			'carol-member': ids.carol
		});

		const [invited] = await db.select({ inviterId: invitation.inviterId }).from(invitation);
		expect(invited.inviterId).toBe(ids.alice);

		const keys = await db.select({ name: apiKey.name, by: apiKey.createdById }).from(apiKey);
		expect(Object.fromEntries(keys.map((row) => [row.name, row.by]))).toEqual({
			bob: ids.bob,
			carol: ids.carol
		});

		const monitors = await db
			.select({
				name: monitor.name,
				created: monitor.createdById,
				updated: monitor.updatedById,
				deleted: monitor.deletedById
			})
			.from(monitor);
		expect(monitors).toEqual(
			expect.arrayContaining([
				{ name: 'changed hands', created: ids.alice, updated: ids.bob, deleted: ids.carol },
				{ name: 'nobody', created: null, updated: null, deleted: null }
			])
		);

		expect(await db.select({ id: organization.id }).from(organization)).toEqual([
			{ id: 'default' }
		]);
	});

	it('leaves no row pointing at a user that is gone, and loses no row', async () => {
		const [{ orphans }] = await sql<{ orphans: number }[]>`
			select count(*)::int as orphans from (
				select user_id as id from account
				union all select user_id from session
				union all select impersonated_by from session
				union all select user_id from member
				union all select inviter_id from invitation
				union all select created_by_id from api_key
				union all select created_by_id from monitor
				union all select updated_by_id from monitor
				union all select deleted_by_id from monitor
			) referenced
			where id is not null and id not in (select id from "user")`;
		expect(orphans).toBe(0);
		expect(await counts()).toEqual(countsBefore);
	});

	it('puts back the foreign keys exactly as they were', async () => {
		expect(referencesBefore).toHaveLength(8);
		expect(await userReferences()).toEqual(referencesBefore);
	});

	it('signs in a user from before with the same password, under the new id', async () => {
		const auth = betterAuth(
			authOptions(db, {
				ORIGIN: 'http://panel.test',
				BETTER_AUTH_SECRET: 'a-test-secret-that-is-long-enough-to-pass'
			})
		);
		const signedIn = await auth.api.signInEmail({
			body: { email: 'alice@example.com', password: alicePassword }
		});
		expect(signedIn.user.id).toBe(ids.alice);
	});

	it('gives a user created afterwards a UUID, and other rows Better Auth ids', async () => {
		const auth = betterAuth(
			authOptions(db, {
				ORIGIN: 'http://panel.test',
				BETTER_AUTH_SECRET: 'a-test-secret-that-is-long-enough-to-pass'
			})
		);
		const { user: created } = await auth.api.createUser({
			body: { email: 'erin@example.com', password: 'erin-password', name: 'Erin', role: 'user' }
		});
		expect(created.id).toMatch(uuidV4);
		await auth.api.addMember({
			body: { userId: created.id, organizationId: 'default', role: 'member' }
		});

		const signedIn = await auth.api.signInEmail({
			body: { email: 'erin@example.com', password: 'erin-password' }
		});
		expect(signedIn.user.id).toBe(created.id);

		const [credential] = await db
			.select({ id: account.id, accountId: account.accountId })
			.from(account)
			.where(eq(account.userId, created.id));
		expect(credential.accountId).toBe(created.id);
		const [membership] = await db
			.select({ id: member.id })
			.from(member)
			.where(eq(member.userId, created.id));
		const [opened] = await db
			.select({ id: session.id })
			.from(session)
			.where(eq(session.userId, created.id));
		for (const id of [credential.id, membership.id, opened.id]) {
			expect(id).toMatch(/^[A-Za-z0-9]{32}$/);
		}
	});
});
