import { error } from '@sveltejs/kit';
import { viewOf } from '$lib/monitors/definition';
import { db } from '$lib/server/db';
import { saveMonitor } from '$lib/server/monitor-save';
import { findMonitor, monitorId } from '$lib/server/monitors';
import { fetchMetricNames, NinjacatError } from '$lib/server/ninjacat';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals }) => {
	const id = monitorId(params.id);
	const tenantId = locals.tenant?.id;
	const found = id !== null && tenantId ? await findMonitor(db, tenantId, id) : undefined;
	if (!found) {
		error(404, m.monitor_not_found());
	}
	const opened = viewOf(found);
	const page = {
		id: found.id,
		name: found.name,
		query: found.query,
		initial: opened?.view ?? null,
		drifted: opened?.drifted ?? false
	};
	if (!opened) {
		return { ...page, metrics: [] as string[], metricsError: null };
	}
	try {
		return { ...page, metrics: await fetchMetricNames(), metricsError: null };
	} catch (e) {
		return {
			...page,
			metrics: [] as string[],
			metricsError: e instanceof NinjacatError ? e.message : String(e)
		};
	}
};

export const actions: Actions = {
	save: (event) => {
		const id = monitorId(event.params.id);
		if (id === null) {
			error(404, m.monitor_not_found());
		}
		return saveMonitor(event, id);
	}
};
