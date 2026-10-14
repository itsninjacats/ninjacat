import { db } from '$lib/server/db';
import { listMonitors } from '$lib/server/monitors';
import { fetchMonitorStatus } from '$lib/server/ninjacat';
import type { PageServerLoad } from './$types';

// The monitor tile: how many the tenant has, and how many are alerting. A
// query process that is down leaves the count without the states.
export const load: PageServerLoad = async ({ locals }) => {
	const tenantId = locals.tenant?.id;
	if (!tenantId) {
		return { monitors: { total: 0, alert: 0, warn: 0, noData: 0, known: true } };
	}
	const rows = await listMonitors(db, tenantId);
	try {
		const statuses = rows.length > 0 ? await fetchMonitorStatus(rows.map((r) => r.id)) : [];
		const count = (state: string) => statuses.filter((s) => s.state === state).length;
		return {
			monitors: {
				total: rows.length,
				alert: count('Alert'),
				warn: count('Warn'),
				noData: count('No Data'),
				known: true
			}
		};
	} catch {
		return { monitors: { total: rows.length, alert: 0, warn: 0, noData: 0, known: false } };
	}
};
