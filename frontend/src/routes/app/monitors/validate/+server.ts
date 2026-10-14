import { json } from '@sveltejs/kit';
import { monitorOf, readMonitorView, unfilled } from '$lib/monitors/definition';
import { panelProblems } from '$lib/server/monitor-save';
import { reasonOf, validateMonitor } from '$lib/server/ninjacat';
import { m } from '$lib/paraglide/messages';
import type { RequestHandler } from './$types';

// The editor's check while it is being filled in: the same problems a save
// would refuse, and the Datadog JSON it would store. The guard keeps
// visitors out; the query process needs the request's tenant to answer.
export const POST: RequestHandler = async ({ request, locals }) => {
	if (!locals.user || !locals.tenant) {
		return json({ problems: [m.no_tenant()], notActedOn: [], monitor: null }, { status: 403 });
	}
	const view = readMonitorView(await request.json().catch(() => null));
	const monitor = monitorOf(view);
	const problems = panelProblems(view);
	// Without a query or a threshold the server could only say where its parser stopped.
	if (unfilled(view).some((what) => what !== 'name')) {
		return json({ problems, notActedOn: [], monitor });
	}
	try {
		const checked = await validateMonitor(monitor);
		return json({
			problems: [...problems, ...checked.problems],
			notActedOn: checked.notActedOn,
			monitor
		});
	} catch (e) {
		return json(
			{ problems: [m.monitor_validate_failed({ reason: reasonOf(e) })], notActedOn: [], monitor },
			{ status: 502 }
		);
	}
};
