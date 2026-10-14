import { fail, redirect, type RequestEvent } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { formText } from '$lib/server/form';
import { createMonitor, findMonitor, updateMonitor } from '$lib/server/monitors';
import { NinjacatError, validateMonitor } from '$lib/server/ninjacat';
import {
	monitorOf,
	otherOptionsOf,
	readMonitorView,
	unfilled,
	type MonitorView,
	type Unfilled
} from '$lib/monitors/definition';
import { m } from '$lib/paraglide/messages';

const NAME_MAX = 500;
// Datadog's limit for a tag.
const TAG_MAX = 200;

export function unfilledMessage(what: Unfilled): string {
	switch (what) {
		case 'name':
			return m.monitor_unfilled_name();
		case 'metric':
			return m.monitor_unfilled_metric();
		case 'critical':
			return m.monitor_unfilled_critical();
		case 'formula':
			return m.monitor_unfilled_formula();
		case 'parameters':
			return m.monitor_unfilled_parameters();
	}
}

/** What the panel itself refuses before asking the query process. */
export function panelProblems(view: MonitorView): string[] {
	const problems = unfilled(view).map(unfilledMessage);
	const names = view.queries.map((q) => q.name);
	if (new Set(names).size !== names.length) problems.push(m.monitor_query_names_repeated());
	if (view.name.trim().length > NAME_MAX) problems.push(m.monitor_name_too_long({ max: NAME_MAX }));
	if (view.tags.some((t) => !t.trim() || t.length > TAG_MAX)) {
		problems.push(m.monitor_tag_invalid({ max: TAG_MAX }));
	}
	return problems;
}

/**
 * The editor's save: the view it submitted becomes the Datadog monitor, which
 * the query process checks with the code the monitor service evaluates with;
 * only then is it written, with the notification, for the request's tenant.
 */
export async function saveMonitor({ request, locals }: RequestEvent, id: number | null) {
	if (!locals.user) {
		redirect(302, '/login');
	}
	const form = await request.formData();
	const tenantId = locals.tenant?.id;
	if (!tenantId) {
		return fail(403, { problems: [m.no_tenant()] });
	}
	// The session's tenant can change in another tab; the monitor would land there.
	if (formText(form, 'tenant') !== tenantId) {
		return fail(409, { problems: [m.tenant_changed()] });
	}

	let raw: unknown;
	try {
		raw = JSON.parse(formText(form, 'view'));
	} catch {
		return fail(400, { problems: [m.monitor_form_invalid()] });
	}
	const view = readMonitorView(raw);
	if (id !== null) {
		const stored = await findMonitor(db, tenantId, id);
		if (!stored) {
			return fail(404, { problems: [m.monitor_not_found()] });
		}
		view.otherOptions = otherOptionsOf(stored.options);
	}

	const problems = panelProblems(view);
	if (problems.length > 0) {
		return fail(400, { problems });
	}

	const monitor = monitorOf(view);
	try {
		const checked = await validateMonitor(monitor);
		if (checked.problems.length > 0) {
			return fail(400, { problems: checked.problems });
		}
	} catch (e) {
		const reason = e instanceof NinjacatError ? e.message : String(e);
		return fail(502, { problems: [m.monitor_validate_failed({ reason })] });
	}

	const definition = { ...monitor, editor: view };
	const saved =
		id === null
			? await createMonitor(db, tenantId, locals.user.id, definition)
			: await updateMonitor(db, tenantId, id, locals.user.id, definition);
	if (!saved) {
		return fail(404, { problems: [m.monitor_not_found()] });
	}
	redirect(303, `/app/monitors/${saved.id}`);
}
