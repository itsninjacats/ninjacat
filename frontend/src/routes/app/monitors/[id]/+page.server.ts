import { error, fail, redirect } from '@sveltejs/kit';
import { asObject } from '$lib/metrics/query';
import { editorMethod, effectiveComparator, viewOf } from '$lib/monitors/definition';
import { severity } from '$lib/monitors/states';
import { db } from '$lib/server/db';
import { formText } from '$lib/server/form';
import { deleteMonitor, findMonitor, monitorId } from '$lib/server/monitors';
import {
	fetchMonitorEvaluations,
	fetchMonitorGroups,
	fetchMonitorTransitions,
	reasonOf,
	type MonitorGroup,
	type MonitorRange
} from '$lib/server/ninjacat';
import { m } from '$lib/paraglide/messages';
import type { Actions, PageServerLoad } from './$types';

// Evaluations are kept 7 days, so the longest range is a week.
const RANGES = ['-1h', '-4h', '-1d', '-1w'] as const;
const RANGE_MS: Record<(typeof RANGES)[number], number> = {
	'-1h': 3_600_000,
	'-4h': 4 * 3_600_000,
	'-1d': 86_400_000,
	'-1w': 7 * 86_400_000
};

// Lines beyond these are counted, not drawn.
const MAX_LINES = 20;

const numberOrNull = (value: unknown) => (typeof value === 'number' ? value : null);

/**
 * The values of the groups the chart draws. The query process keeps the
 * newest rows of a request only, so one request for every group would leave
 * most of a long range out; one per drawn group leaves out less.
 */
async function evaluationsOf(id: number, window: MonitorRange, groups: MonitorGroup[] | null) {
	if (!groups || groups.length === 0) {
		return fetchMonitorEvaluations(id, window);
	}
	const drawn = groups
		.toSorted((a, b) => severity(a.state) - severity(b.state) || a.group.localeCompare(b.group))
		.slice(0, MAX_LINES);
	const parts = await Promise.all(
		drawn.map((g) => fetchMonitorEvaluations(id, { ...window, group: g.group }))
	);
	return {
		evaluations: parts.flatMap((p) => p.evaluations),
		truncated: parts.some((p) => p.truncated)
	};
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const id = monitorId(params.id);
	const tenantId = locals.tenant?.id;
	const found = id !== null && tenantId ? await findMonitor(db, tenantId, id) : undefined;
	if (!found) {
		error(404, m.monitor_not_found());
	}

	const asked = url.searchParams.get('range');
	const range = RANGES.find((r) => r === asked) ?? '-1d';
	const to = new Date();
	const from = new Date(to.getTime() - RANGE_MS[range]);
	const window = { from: from.toISOString(), to: to.toISOString() };

	// Each part is shown on its own, so one that fails leaves the others.
	const [groups, transitions] = await Promise.allSettled([
		fetchMonitorGroups(found.id),
		fetchMonitorTransitions(found.id, window)
	]);
	const [evaluations] = await Promise.allSettled([
		evaluationsOf(found.id, window, groups.status === 'fulfilled' ? groups.value : null)
	]);
	const failed = [groups, transitions, evaluations].find((r) => r.status === 'rejected');

	// Without the editor's view the thresholds are still in the options, but
	// which side alerts is only in the query.
	const view = viewOf(found)?.view;
	const thresholds = asObject(asObject(found.options).thresholds);
	return {
		monitor: {
			id: found.id,
			name: found.name,
			query: found.query,
			message: found.message,
			options: found.options,
			tags: found.tags,
			priority: found.priority,
			createdAt: found.createdAt.toISOString(),
			updatedAt: found.updatedAt.toISOString(),
			method: editorMethod(found.editor)
		},
		thresholds: {
			comparator: view ? effectiveComparator(view) : null,
			critical: numberOrNull(thresholds.critical),
			warning: numberOrNull(thresholds.warning),
			criticalRecovery: numberOrNull(thresholds.critical_recovery),
			warningRecovery: numberOrNull(thresholds.warning_recovery)
		},
		maxLines: MAX_LINES,
		range,
		window,
		groups: groups.status === 'fulfilled' ? groups.value : null,
		transitions: transitions.status === 'fulfilled' ? transitions.value : null,
		evaluations: evaluations.status === 'fulfilled' ? evaluations.value : null,
		readError: failed?.status === 'rejected' ? reasonOf(failed.reason) : null
	};
};

export const actions: Actions = {
	delete: async ({ request, locals, params }) => {
		if (!locals.user) {
			redirect(302, '/login');
		}
		const form = await request.formData();
		const tenantId = locals.tenant?.id;
		if (!tenantId) {
			return fail(403, { deleteError: m.no_tenant() });
		}
		if (formText(form, 'tenant') !== tenantId) {
			return fail(409, { deleteError: m.tenant_changed() });
		}
		const id = monitorId(params.id);
		const deleted = id === null ? null : await deleteMonitor(db, tenantId, id, locals.user.id);
		if (!deleted) {
			return fail(404, { deleteError: m.monitor_not_found() });
		}
		redirect(303, '/app/monitors');
	}
};
