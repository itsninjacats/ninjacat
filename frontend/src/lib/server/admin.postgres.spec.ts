import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { asc, eq } from 'drizzle-orm';
import * as schema from './db/schema';
import { reachable, scratchDatabase } from './db/scratch';
import { changeMember, isUniqueViolation } from './admin';

// The rules that live in SQL: the tenant in every WHERE, the unique membership.
const { member, organization, user } = schema;

describe.skipIf(!(await reachable()))('changing a member, in Postgres', () => {
	const scratch = scratchDatabase();
	const db = scratch.db;
	beforeAll(scratch.create);
	afterAll(scratch.drop);

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
