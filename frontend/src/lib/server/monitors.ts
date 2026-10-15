import { and, asc, eq, isNull, sql } from 'drizzle-orm';
import type { DatadogMonitor } from '$lib/monitors/definition';
import type { Database } from './auth-options';
import { monitor } from './db/schema';

export type MonitorDefinition = DatadogMonitor & { editor: unknown };

export type MonitorVersion = { id: number; version: number };

export const monitorsChannel = 'ninjacat_monitors';

// Sent inside the transaction that writes the row, so it leaves only on commit. It can
// still be lost; the monitor service re-reads the table every 30 s anyway.
export function monitorChanged(tenantId: string, change: MonitorVersion) {
	return sql`select pg_notify(${monitorsChannel}, ${`${tenantId}/${change.id}/${change.version}`})`;
}

const returned = { id: monitor.id, version: monitor.version };

// A live monitor of the tenant; a deleted one is not changed again.
const live = (tenantId: string, id: number) =>
	and(eq(monitor.id, id), eq(monitor.tenantId, tenantId), isNull(monitor.deletedAt));

export function listMonitors(db: Database, tenantId: string) {
	return db
		.select({
			id: monitor.id,
			name: monitor.name,
			query: monitor.query,
			tags: monitor.tags,
			priority: monitor.priority,
			editorMethod: sql<string | null>`${monitor.editor}->>'method'`
		})
		.from(monitor)
		.where(and(eq(monitor.tenantId, tenantId), isNull(monitor.deletedAt)))
		.orderBy(asc(monitor.name), asc(monitor.id));
}

export async function findMonitor(db: Database, tenantId: string, id: number) {
	const [found] = await db.select().from(monitor).where(live(tenantId, id));
	return found;
}

/** A monitor id from a URL: a positive integer, as Postgres' bigint holds it. */
export function monitorId(text: string): number | null {
	return /^[1-9]\d{0,14}$/.test(text) ? Number(text) : null;
}

export function createMonitor(
	db: Database,
	tenantId: string,
	actorId: string | null,
	definition: MonitorDefinition
): Promise<MonitorVersion> {
	return db.transaction(async (tx) => {
		const [created] = await tx
			.insert(monitor)
			.values({ ...definition, tenantId, createdById: actorId, updatedById: actorId })
			.returning(returned);
		await tx.execute(monitorChanged(tenantId, created));
		return created;
	});
}

// Null when the tenant has no such live monitor.
export function updateMonitor(
	db: Database,
	tenantId: string,
	id: number,
	actorId: string | null,
	definition: MonitorDefinition
): Promise<MonitorVersion | null> {
	return db.transaction(async (tx) => {
		const [updated] = await tx
			.update(monitor)
			.set({
				...definition,
				version: sql`${monitor.version} + 1`,
				updatedById: actorId,
				updatedAt: sql`now()`
			})
			.where(live(tenantId, id))
			.returning(returned);
		if (!updated) {
			return null;
		}
		await tx.execute(monitorChanged(tenantId, updated));
		return updated;
	});
}

// Soft: the row stays, so the service can close the monitor's groups with
// the actor who deleted it. The version is bumped like any other write, so
// the deletion is newer than every definition the service has seen.
export function deleteMonitor(
	db: Database,
	tenantId: string,
	id: number,
	actorId: string | null
): Promise<MonitorVersion | null> {
	return db.transaction(async (tx) => {
		const [deleted] = await tx
			.update(monitor)
			.set({
				version: sql`${monitor.version} + 1`,
				deletedById: actorId,
				deletedAt: sql`now()`
			})
			.where(live(tenantId, id))
			.returning(returned);
		if (!deleted) {
			return null;
		}
		await tx.execute(monitorChanged(tenantId, deleted));
		return deleted;
	});
}
