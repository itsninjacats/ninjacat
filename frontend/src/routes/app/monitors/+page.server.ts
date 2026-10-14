import { db } from '$lib/server/db';
import { listMonitors } from '$lib/server/monitors';
import { fetchMonitorStatus, NinjacatError, type MonitorStatus } from '$lib/server/ninjacat';
import { editorMethod } from '$lib/monitors/definition';
import type { PageServerLoad } from './$types';

// The definitions come from Postgres; their state from what the monitor
// service stored. A query process that is down leaves the list readable,
// with the states unknown.
export const load: PageServerLoad = async ({ locals }) => {
	const tenantId = locals.tenant?.id;
	if (!tenantId) {
		return { monitors: [], statusError: null };
	}

	const rows = await listMonitors(db, tenantId);
	const statuses = new Map<number, MonitorStatus>();
	let statusError: string | null = null;
	if (rows.length > 0) {
		try {
			for (const s of await fetchMonitorStatus(rows.map((r) => r.id))) statuses.set(s.id, s);
		} catch (e) {
			statusError = e instanceof NinjacatError ? e.message : String(e);
		}
	}

	return {
		monitors: rows.map((r) => {
			const status = statuses.get(r.id);
			return {
				id: r.id,
				name: r.name,
				method: editorMethod({ method: r.editorMethod }),
				query: r.query,
				tags: r.tags,
				priority: r.priority,
				known: status !== undefined,
				state: status?.state ?? null,
				groups: status?.groups ?? null,
				lastTriggeredAt: status?.lastTriggeredAt ?? null
			};
		}),
		statusError
	};
};
