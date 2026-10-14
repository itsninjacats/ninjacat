// A monitor's state timeline: per group, the stretches of time it spent in
// each state, from its transitions over a range.

import { severity } from './states';

export type Step = {
	group: string;
	at: string;
	fromState: string | null;
	toState: string;
	reason: string;
};

/** null: the group was not open then (not seen yet, or closed). */
export type Segment = { state: string | null; from: number; to: number };

export type TimelineRow = { group: string; segments: Segment[] };

/** After these the group is closed: no longer evaluated or shown. */
const CLOSING = ['expired', 'definition_changed', 'deleted'];

/**
 * `initial` is each open group's latest transition before `from`, the state
 * it entered the range in; `steps` are the transitions inside the range.
 * Rows come worst state at `to` first, then by group.
 *
 * `truncated`: `steps` are only the newest of the range, so `initial` no
 * longer says what came before them. Before the oldest step nothing is known;
 * after it, a group was in its first step's from-state until that step.
 */
export function timelines(
	initial: Step[],
	steps: Step[],
	from: number,
	to: number,
	truncated = false
): TimelineRow[] {
	const clamp = (t: number) => Math.min(Math.max(t, from), to);
	const known = truncated
		? clamp(steps.reduce((oldest, s) => Math.min(oldest, Date.parse(s.at)), to))
		: from;
	const groups = new Map<string, { start: Step | undefined; steps: Step[] }>();
	for (const s of initial) groups.set(s.group, { start: s, steps: [] });
	for (const s of steps) {
		const g = groups.get(s.group) ?? { start: undefined, steps: [] };
		g.steps.push(s);
		groups.set(s.group, g);
	}

	const stateAfter = (s: Step) => (CLOSING.includes(s.reason) ? null : s.toState);

	const rows = [...groups.entries()].map(([group, g]) => {
		const segments: Segment[] = [];
		const ordered = g.steps.toSorted((a, b) => Date.parse(a.at) - Date.parse(b.at));
		let state: string | null = null;
		let since = from;
		if (!truncated) {
			state = g.start ? stateAfter(g.start) : null;
		} else if (ordered.length > 0) {
			if (known > from) segments.push({ state: null, from, to: known });
			since = known;
			state = ordered[0].fromState;
		} else {
			since = to;
			segments.push({ state: null, from, to });
		}
		for (const s of ordered) {
			const at = clamp(Date.parse(s.at));
			if (at > since) segments.push({ state, from: since, to: at });
			state = stateAfter(s);
			since = Math.max(since, at);
		}
		if (to > since) segments.push({ state, from: since, to });
		return { group, segments };
	});

	const last = (row: TimelineRow) => row.segments.at(-1)?.state ?? null;
	return rows.sort(
		(a, b) => severity(last(a)) - severity(last(b)) || a.group.localeCompare(b.group)
	);
}
