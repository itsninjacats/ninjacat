import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { eq } from 'drizzle-orm';
import { PgDialect } from 'drizzle-orm/pg-core';
import { drizzle } from 'drizzle-orm/postgres-js';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import postgres from 'postgres';
import * as schema from './db/schema';
import type { Database } from './auth-options';
import {
	createMonitor,
	deleteMonitor,
	monitorChanged,
	monitorsChannel,
	updateMonitor,
	type MonitorDefinition
} from './monitors';

describe('the monitor notification', () => {
	it('names the tenant, the monitor and its version, as a parameter', () => {
		const query = new PgDialect().sqlToQuery(monitorChanged('org-1', { id: 7, version: 3 }));
		expect(query.sql).toBe('select pg_notify($1, $2)');
		expect(query.params).toEqual(['ninjacat_monitors', 'org-1/7/3']);
	});
});

// Against a scratch database built from the Drizzle migrations, as in
// admin.postgres.spec.ts. Skipped when Postgres is not reachable.
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

const { monitor, organization, user } = schema;

const definition: MonitorDefinition = {
	name: 'CPU high',
	type: 'query alert',
	query: 'avg(last_5m):avg:system.cpu.user{*} > 90',
	message: 'CPU is high',
	options: { thresholds: { critical: 90 } },
	tags: ['team:infra'],
	priority: 2,
	editor: { kind: 'threshold' }
};

describe.skipIf(!(await reachable()))('writing monitors, in Postgres', () => {
	const name = `ninjacat_test_${randomBytes(6).toString('hex')}`;
	const admin = postgres(url, { max: 1, onnotice: () => {} });
	let sql: postgres.Sql;
	let db: Database;
	const notified: string[] = [];

	beforeAll(async () => {
		await admin.unsafe(`create database ${name}`);
		const scratch = new URL(url);
		scratch.pathname = `/${name}`;
		sql = postgres(scratch.toString(), { onnotice: () => {} });
		db = drizzle(sql, { schema });
		await migrate(db, {
			migrationsFolder: fileURLToPath(new URL('../../../drizzle', import.meta.url))
		});
		await sql.listen(monitorsChannel, (payload) => notified.push(payload));
	});

	afterAll(async () => {
		await sql?.end();
		await admin.unsafe(`drop database if exists ${name} with (force)`);
		await admin.end();
	});

	beforeEach(async () => {
		await db.delete(monitor);
		await db.delete(organization);
		await db.delete(user);
		await db.insert(user).values({ id: 'u1', name: 'u1', email: 'u1@example.com' });
		const createdAt = new Date();
		await db.insert(organization).values([
			{ id: 'a', name: 'A', slug: 'a', createdAt },
			{ id: 'b', name: 'B', slug: 'b', createdAt }
		]);
		notified.length = 0;
	});

	// Notifications arrive in commit order, so once `expected` is here, any
	// one sent before it is too.
	const notifiedUpTo = async (expected: string) => {
		await vi.waitFor(() => expect(notified).toContain(expected));
		return [...notified];
	};

	const row = async (id: number) => (await db.select().from(monitor).where(eq(monitor.id, id)))[0];

	it('creates a monitor for the tenant and says so', async () => {
		const created = await createMonitor(db, 'a', 'u1', definition);
		expect(created.version).toBe(1);
		expect(await notifiedUpTo(`a/${created.id}/1`)).toEqual([`a/${created.id}/1`]);

		const stored = await row(created.id);
		expect(stored).toMatchObject({
			...definition,
			tenantId: 'a',
			createdById: 'u1',
			updatedById: 'u1'
		});
		expect(stored.deletedAt).toBeNull();
	});

	it('bumps the version on every update, within the tenant only', async () => {
		const { id } = await createMonitor(db, 'a', 'u1', definition);
		expect(await updateMonitor(db, 'b', id, 'u1', { ...definition, name: 'stolen' })).toBeNull();
		expect(
			await updateMonitor(db, 'a', id, null, { ...definition, name: 'CPU very high' })
		).toEqual({
			id,
			version: 2
		});
		expect(await updateMonitor(db, 'a', id, 'u1', definition)).toEqual({ id, version: 3 });

		expect(await notifiedUpTo(`a/${id}/3`)).toEqual([`a/${id}/1`, `a/${id}/2`, `a/${id}/3`]);
		expect(await row(id)).toMatchObject({ name: 'CPU high', version: 3, updatedById: 'u1' });
	});

	it('deletes softly, once, within the tenant only', async () => {
		const { id } = await createMonitor(db, 'a', 'u1', definition);
		expect(await deleteMonitor(db, 'b', id, 'u1')).toBeNull();
		expect(await deleteMonitor(db, 'a', id, 'u1')).toEqual({ id, version: 2 });
		expect(await deleteMonitor(db, 'a', id, 'u1')).toBeNull();
		expect(await updateMonitor(db, 'a', id, 'u1', definition)).toBeNull();

		expect(await notifiedUpTo(`a/${id}/2`)).toEqual([`a/${id}/1`, `a/${id}/2`]);
		const stored = await row(id);
		expect(stored).toMatchObject({ version: 2, deletedById: 'u1' });
		expect(stored.deletedAt).toBeInstanceOf(Date);
	});

	it('notifies nothing when the write fails', async () => {
		await expect(createMonitor(db, 'a', 'u1', { ...definition, priority: 9 })).rejects.toThrow();
		await expect(createMonitor(db, 'nobody', 'u1', definition)).rejects.toThrow();

		const { id } = await createMonitor(db, 'b', 'u1', definition);
		expect(await notifiedUpTo(`b/${id}/1`)).toEqual([`b/${id}/1`]);
	});

	it('keeps a tenant that still has monitors', async () => {
		await createMonitor(db, 'a', 'u1', definition);
		await expect(db.delete(organization).where(eq(organization.id, 'a'))).rejects.toThrow();
	});
});
