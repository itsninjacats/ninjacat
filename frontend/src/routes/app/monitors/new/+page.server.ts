import { defaultMonitorView, fromExplorer } from '$lib/monitors/definition';
import { readView } from '$lib/metrics/query';
import { saveMonitor } from '$lib/server/monitor-save';
import { fetchMetricNames, reasonOf } from '$lib/server/ninjacat';
import type { Actions, PageServerLoad } from './$types';

// The explorer's "Create monitor" opens this page with its view (?view=).
export const load: PageServerLoad = async ({ url }) => {
	const initial = url.searchParams.has('view')
		? fromExplorer(readView(url.searchParams))
		: defaultMonitorView();
	try {
		return { initial, metrics: await fetchMetricNames(), metricsError: null };
	} catch (e) {
		return {
			initial,
			metrics: [] as string[],
			metricsError: reasonOf(e)
		};
	}
};

export const actions: Actions = {
	save: (event) => saveMonitor(event, null)
};
