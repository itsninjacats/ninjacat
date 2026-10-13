import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { asc, eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/postgres-js';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import postgres from 'postgres';
import * as schema from './db/schema';
import { changeMember, isUniqueViolation } from './admin';
import type { Database } from './auth-options';

// Against a scratch database built from the Drizzle migrations, so the rules
// that live in SQL (the tenant in every WHERE, the unique membership) are what
// is tested. Skipped when Postgres is not reachable; the dev stack's by default.
const url = process.env.DATABASE_URL ?? 'postgres://root:mysecretpassword@localhost:5433/local';

async function reachable(): Promise<boolean> {
	const probe = postgres(url, { max: 1, connect_timeout: 2, onnotice: () => {} });
	try {
		await probe`select 1`;
		return true;
	} catch {
		return false;
	} finally {
		await probe.end();
	}
}

const { member, organization, user } = schema;

describe.skipIf(!(await reachable()))('changing a member, in Postgres', () => {
	const name = `ninjacat_test_${randomBytes(6).toString('hex')}`;
	const admin = postgres(url, { max: 1, onnotice: () => {} });
	let sql: postgres.Sql;
	let db: Database;

	beforeAll(async () => {
		await admin.unsafe(`create database ${name}`);
		const scratch = new URL(url);
		scratch.pathname = `/${name}`;
		sql = postgres(scratch.toString(), { onnotice: () => {} });
		db = drizzle(sql, { schema });
		await migrate(db, {
			migrationsFolder: fileURLToPath(new URL('../../../drizzle', import.meta.url))
		});
	});

	afterAll(async () => {
		await sql?.end();
		await admin.unsafe(`drop database if exists ${name} with (force)`);
		await admin.end();
	});

	beforeEach(async () => {
		await db.delete(member);
		await db.delete(organization);
		await db.delete(user);
		await db
			.insert(user)
			.values(['u1', 'u2', 'u3'].map((id) => ({ id, name: id, email: `${id}@example.com` })));
		const createdAt = new Date();
		await db.insert(organization).values([
			{ id: 'a', name: 'A', slug: 'a', createdAt },
			{ id: 'b', name: 'B', slug: 'b', createdAt }
		]);
		await db.insert(member).values([
			{ id: 'a-owner', organizationId: 'a', userId: 'u1', role: 'owner', createdAt },
			{ id: 'a-member', organizationId: 'a', userId: 'u2', role: 'member', createdAt },
			{ id: 'b-owner', organizationId: 'b', userId: 'u1', role: 'owner', createdAt },
			{ id: 'b-member', organizationId: 'b', userId: 'u3', role: 'member', createdAt }
		]);
	});

	const roles = async () =>
		Object.fromEntries(
			(
				await db.select({ id: member.id, role: member.role }).from(member).orderBy(asc(member.id))
			).map((row) => [row.id, row.role])
		);

	it('does not reach a member of another tenant', async () => {
		const before = await roles();
		expect(await changeMember(db, 'a', 'b-member', 'owner')).toBe('not-found');
		expect(await changeMember(db, 'a', 'b-owner', null)).toBe('not-found');
		expect(await roles()).toEqual(before);
	});

	it('keeps the last owner', async () => {
		expect(await changeMember(db, 'a', 'a-owner', null)).toBe('last-owner');
		expect(await changeMember(db, 'a', 'a-owner', 'member')).toBe('last-owner');
		expect((await roles())['a-owner']).toBe('owner');
	});

	it('changes and removes members of the tenant only', async () => {
		expect(await changeMember(db, 'a', 'a-member', 'owner')).toBe('done');
		expect(await changeMember(db, 'a', 'a-owner', 'admin')).toBe('done');
		expect(await changeMember(db, 'a', 'a-owner', null)).toBe('done');
		expect(await roles()).toEqual({
			'a-member': 'owner',
			'b-member': 'member',
			'b-owner': 'owner'
		});
	});

	it('holds one membership per user and tenant', async () => {
		const again = db
			.insert(member)
			.values({ id: 'a-again', organizationId: 'a', userId: 'u2', createdAt: new Date() });
		const thrown = await again.then(
			() => null,
			(e: unknown) => e
		);
		expect(isUniqueViolation(thrown)).toBe(true);
		expect(await db.select().from(member).where(eq(member.userId, 'u2'))).toHaveLength(1);
	});
});
