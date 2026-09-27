// The explorer's queries: what the builder holds, the Datadog query text it
// turns into, and how a whole view fits into the page URL.

import { functionSpec, modifierText, wrap, type FunctionCall } from './functions';

export const SPACE_AGGREGATORS = ['avg', 'sum', 'min', 'max'] as const;
export type SpaceAggregator = (typeof SPACE_AGGREGATORS)[number];

export type TagFilter = { key: string; value: string };

export type BuilderQuery = {
	metric: string;
	agg: SpaceAggregator;
	filters: TagFilter[];
	groupBy: string[];
};

/** One query row: built with the controls, or typed as text. */
export type ExplorerQuery = {
	name: string;
	mode: 'builder' | 'text';
	builder: BuilderQuery;
	text: string;
	/** Hidden rows are still queried, so formulas can use them; they are just not drawn. */
	hidden: boolean;
	functions: FunctionCall[];
	/** `as …`: the name shown in the legend instead of the formula. */
	alias: string;
};

/** One formula row over the queries' names, e.g. `a / b * 100`. */
export type ExplorerFormula = {
	text: string;
	hidden: boolean;
	functions: FunctionCall[];
	alias: string;
};

export const DISPLAYS = ['line', 'area', 'bars', 'toplist', 'value'] as const;
export type Display = (typeof DISPLAYS)[number];

export const SCALAR_AGGREGATORS = ['avg', 'min', 'max', 'sum', 'last'] as const;
export type ScalarAggregator = (typeof SCALAR_AGGREGATORS)[number];

export type ExplorerView = {
	range: string;
	display: Display;
	aggregator: ScalarAggregator;
	queries: ExplorerQuery[];
	formulas: ExplorerFormula[];
};

/**
 * The tag filter in Datadog syntax. Values of one key are OR'd, keys are AND'd.
 * The comma form cannot express OR, so as soon as one key has two values the
 * whole filter switches to the boolean form — the two cannot be mixed.
 */
export function filterText(filters: TagFilter[]): string {
	if (filters.length === 0) return '*';

	const byKey = new Map<string, string[]>();
	for (const f of filters) {
		const values = byKey.get(f.key);
		if (values) values.push(f.value);
		else byKey.set(f.key, [f.value]);
	}

	const groups = [...byKey.entries()];
	if (groups.every(([, values]) => values.length === 1)) {
		return groups.map(([key, [value]]) => `${key}:${value}`).join(',');
	}

	return groups
		.map(([key, values]) => {
			const terms = values.map((v) => `${key}:${v}`);
			return terms.length === 1 ? terms[0] : `(${terms.join(' OR ')})`;
		})
		.join(' AND ');
}

/** `avg:system.cpu.user{env:prod} by {host}`, or '' while no metric is picked. */
export function builderText(q: BuilderQuery): string {
	if (!q.metric) return '';
	const by = q.groupBy.length > 0 ? ` by {${q.groupBy.join(',')}}` : '';
	return `${q.agg}:${q.metric}{${filterText(q.filters)}}${by}`;
}

/** The query as sent: builder or typed text, with the row's modifiers appended. */
export function queryText(q: ExplorerQuery): string {
	const base = q.mode === 'text' ? q.text.trim() : builderText(q.builder);
	return base ? base + modifierText(q.functions) : '';
}

/** a, b, … z, then aa, ab, … — the first name no query uses yet. */
export function nextQueryName(taken: string[]): string {
	const letters = 'abcdefghijklmnopqrstuvwxyz';
	for (let i = 0; ; i++) {
		const name =
			i < letters.length ? letters[i] : letters[Math.floor(i / 26) - 1] + letters[i % 26];
		if (!taken.includes(name)) return name;
	}
}

export function emptyQuery(name: string, metric = ''): ExplorerQuery {
	return {
		name,
		mode: 'builder',
		builder: { metric, agg: 'avg', filters: [], groupBy: [] },
		text: '',
		hidden: false,
		functions: [],
		alias: ''
	};
}

export function emptyFormula(text = ''): ExplorerFormula {
	return { text, hidden: false, functions: [], alias: '' };
}

// ---- what is sent -------------------------------------------------------------------

/** One line (or group of lines) on the chart and the formula that draws it. */
export type Output = { label: string; formula: string };

/**
 * The request a view makes. Every complete query is sent, hidden or not, so
 * formulas can use it; what is drawn are the outputs — visible queries with
 * their functions, then visible formulas. Formulas use the queries as queried,
 * without the query rows' own functions, as in Datadog.
 */
export function requestOf(view: ExplorerView): {
	queries: { name: string; query: string }[];
	outputs: Output[];
} {
	const queries = view.queries
		.map((q) => ({ row: q, name: q.name, query: queryText(q) }))
		.filter((q) => q.query !== '');

	const outputs: Output[] = [];
	for (const q of queries) {
		if (q.row.hidden) continue;
		const formula = wrap(q.name, q.row.functions);
		outputs.push({ label: q.row.alias.trim() || formula, formula });
	}
	for (const f of view.formulas) {
		const text = f.text.trim();
		if (f.hidden || !text) continue;
		const formula = wrap(text, f.functions);
		outputs.push({ label: f.alias.trim() || formula, formula });
	}

	return { queries: queries.map(({ name, query }) => ({ name, query })), outputs };
}

// ---- the view in the URL -----------------------------------------------------------

export function defaultView(): ExplorerView {
	return {
		range: '-1h',
		display: 'line',
		aggregator: 'avg',
		queries: [emptyQuery('a')],
		formulas: []
	};
}

function pick<T extends string>(allowed: readonly T[], value: unknown, fallback: T): T {
	return allowed.includes(value as T) ? (value as T) : fallback;
}

function asObject(value: unknown): Record<string, unknown> {
	return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
}

function readFunctions(value: unknown): FunctionCall[] {
	if (!Array.isArray(value)) return [];
	return value
		.map(asObject)
		.filter((f) => typeof f.name === 'string' && functionSpec(f.name) !== undefined)
		.map((f) => ({
			name: f.name as string,
			args: Array.isArray(f.args) ? f.args.map(String) : []
		}));
}

function readQuery(raw: unknown): ExplorerQuery {
	const r = asObject(raw);
	const b = asObject(r.builder);

	const filters = Array.isArray(b.filters)
		? b.filters.filter(
				(f): f is TagFilter => typeof f?.key === 'string' && typeof f?.value === 'string'
			)
		: [];

	return {
		name: typeof r.name === 'string' ? r.name : '',
		mode: r.mode === 'text' ? 'text' : 'builder',
		builder: {
			metric: typeof b.metric === 'string' ? b.metric : '',
			agg: pick(SPACE_AGGREGATORS, b.agg, 'avg'),
			filters,
			groupBy: Array.isArray(b.groupBy) ? b.groupBy.filter((k) => typeof k === 'string') : []
		},
		text: typeof r.text === 'string' ? r.text : '',
		hidden: r.hidden === true,
		functions: readFunctions(r.functions),
		alias: typeof r.alias === 'string' ? r.alias : ''
	};
}

function readFormula(raw: unknown): ExplorerFormula {
	// A bare string is how a formula was saved before rows had functions.
	if (typeof raw === 'string') return emptyFormula(raw);
	const r = asObject(raw);
	return {
		text: typeof r.text === 'string' ? r.text : '',
		hidden: r.hidden === true,
		functions: readFunctions(r.functions),
		alias: typeof r.alias === 'string' ? r.alias : ''
	};
}

/**
 * The view a URL describes. `view` holds it as JSON; the older explorer's
 * `metric`, `agg`, `tag` and `by` parameters still open as a first query, so
 * links pasted before this page changed keep working.
 */
export function readView(params: URLSearchParams): ExplorerView {
	const view = defaultView();
	const raw = params.get('view');

	if (raw) {
		let v: Record<string, unknown>;
		try {
			v = asObject(JSON.parse(raw));
		} catch {
			return view;
		}

		const queries = Array.isArray(v.queries) ? v.queries.map(readQuery) : [];
		for (const q of queries) {
			if (!q.name) q.name = nextQueryName(queries.map((other) => other.name));
		}

		return {
			range: typeof v.range === 'string' ? v.range : view.range,
			display: pick(DISPLAYS, v.display, view.display),
			aggregator: pick(SCALAR_AGGREGATORS, v.aggregator, view.aggregator),
			queries: queries.length > 0 ? queries : view.queries,
			formulas: Array.isArray(v.formulas) ? v.formulas.map(readFormula) : []
		};
	}

	const metric = params.get('metric');
	if (metric) {
		const q = emptyQuery('a', metric);
		q.builder.agg = pick(SPACE_AGGREGATORS, params.get('agg'), 'avg');
		q.builder.filters = params
			.getAll('tag')
			.map((t) => ({ key: t.slice(0, t.indexOf(':')), value: t.slice(t.indexOf(':') + 1) }))
			.filter((t) => t.key.length > 0);
		q.builder.groupBy = params.getAll('by');
		view.queries = [q];
	}
	view.range = params.get('from') ?? view.range;
	return view;
}

export function writeView(view: ExplorerView): string {
	return new URLSearchParams({ view: JSON.stringify(view) }).toString();
}

/** `-15m`, `-6h`, `-7d` → milliseconds back from now. */
export function rangeMs(range: string): number {
	const m = /^-(\d+)([mhd])$/.exec(range);
	if (!m) return 3_600_000;
	const n = Number(m[1]);
	return n * { m: 60_000, h: 3_600_000, d: 86_400_000 }[m[2] as 'm' | 'h' | 'd'];
}
