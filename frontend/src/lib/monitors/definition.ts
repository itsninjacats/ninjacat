// The query is never parsed back here: backend/engine/src/monitor/parse.rs is the only parser.

// Relative, not $lib: the e2e seed builds its monitors with this module, outside Vite.
import { wrap } from '../metrics/functions';
import {
	asObject,
	emptyQuery,
	pick,
	queryText,
	readFormula,
	readQuery,
	type ExplorerFormula,
	type ExplorerQuery,
	type ExplorerView
} from '../metrics/query';

export const METHODS = ['threshold', 'change', 'anomaly', 'outlier', 'forecast', 'check'] as const;
export type Method = (typeof METHODS)[number];

export const TIME_AGGREGATORS = ['avg', 'sum', 'min', 'max'] as const;
export type TimeAggregator = (typeof TIME_AGGREGATORS)[number];

export const COMPARATORS = ['>', '>=', '<', '<=', '==', '!='] as const;
export type Comparator = (typeof COMPARATORS)[number];

export const WINDOWS = [
	'last_1m',
	'last_5m',
	'last_10m',
	'last_15m',
	'last_30m',
	'last_1h',
	'last_2h',
	'last_4h',
	'last_1d',
	'last_2d',
	'last_1w',
	'last_1mo'
];
/** Datadog's documented timeshifts; a change looks at most two days back. */
export const SHIFTS = [
	'5m_ago',
	'10m_ago',
	'15m_ago',
	'30m_ago',
	'1h_ago',
	'2h_ago',
	'4h_ago',
	'1d_ago'
];
export const HORIZONS = [
	'next_12h',
	'next_1d',
	'next_2d',
	'next_3d',
	'next_1w',
	'next_2w',
	'next_1mo',
	'next_3mo'
];
export const HISTORIES = ['1d', '3d', '1w', '2w', '4w'];

export const ANOMALY_ALGORITHMS = ['basic', 'agile', 'robust'] as const;
export const DIRECTIONS = ['both', 'above', 'below'] as const;
export const SEASONALITIES = ['hourly', 'daily', 'weekly'] as const;
export const FORECAST_ALGORITHMS = ['linear', 'seasonal'] as const;
export const FORECAST_MODELS = ['default', 'simple', 'reactive'] as const;
export const OUTLIER_ALGORITHMS = ['dbscan', 'scaledbscan', 'mad', 'scaledmad'] as const;

// `keep` and `notify` are the older notify_no_data option (off, or on after
// no_data_timeframe); the rest are on_missing_data's values. Datadog does not
// take both on one monitor.
export const MISSING = [
	'keep',
	'notify',
	'default',
	'show_no_data',
	'show_and_notify_no_data',
	'resolve'
] as const;
export type Missing = (typeof MISSING)[number];

export const PRIORITIES = [1, 2, 3, 4, 5] as const;

export type MonitorView = {
	method: Method;
	queries: ExplorerQuery[];
	// At most one: the monitored expression when present, else the first query.
	formulas: ExplorerFormula[];
	aggregator: TimeAggregator;
	window: string;
	change: { measure: 'change' | 'pct_change'; shift: string };
	anomaly: {
		algorithm: (typeof ANOMALY_ALGORITHMS)[number];
		bounds: number | null;
		direction: (typeof DIRECTIONS)[number];
		alertWindow: string;
		recoveryWindow: string;
		interval: number | null;
		countDefaultZero: boolean;
		seasonality: (typeof SEASONALITIES)[number];
		timezone: string;
	};
	forecast: {
		algorithm: (typeof FORECAST_ALGORITHMS)[number];
		deviations: number | null;
		horizon: string;
		history: string;
		model: (typeof FORECAST_MODELS)[number];
		seasonality: (typeof SEASONALITIES)[number];
		interval: number | null;
		timezone: string;
	};
	outlier: {
		algorithm: (typeof OUTLIER_ALGORITHMS)[number];
		tolerance: number | null;
		percentage: number | null;
	};
	// A service check: its runs are counted, the queries above are not used.
	check: {
		name: string;
		over: string[];
		by: string[];
		last: number | null;
		ok: number | null;
		// UNKNOWN runs in a row that put a group in No Data; null: never.
		unknown: number | null;
	};
	comparator: Comparator;
	critical: number | null;
	warning: number | null;
	criticalRecovery: number | null;
	warningRecovery: number | null;
	missing: Missing;
	noDataTimeframe: number | null;
	evaluationDelay: number | null;
	newGroupDelay: number | null;
	requireFullWindow: boolean;
	timeoutH: number | null;
	// Stored, not acted on: nothing is sent yet.
	renotifyInterval: number | null;
	escalationMessage: string;
	includeTags: boolean;
	notifyAudit: boolean;
	name: string;
	message: string;
	tags: string[];
	priority: number | null;
	// A stored monitor's options the editor has no field for. Shown, and kept
	// on save from the stored row, never from the browser.
	otherOptions: Record<string, unknown>;
};

export type DatadogMonitor = {
	name: string;
	type: 'query alert' | 'service check';
	query: string;
	message: string;
	tags: string[];
	priority: number | null;
	options: Record<string, unknown>;
};

export function defaultMonitorView(): MonitorView {
	return {
		method: 'threshold',
		queries: [emptyQuery('a')],
		formulas: [],
		aggregator: 'avg',
		window: 'last_5m',
		change: { measure: 'change', shift: '5m_ago' },
		anomaly: {
			algorithm: 'agile',
			bounds: 2,
			direction: 'both',
			alertWindow: 'last_15m',
			recoveryWindow: 'last_15m',
			interval: null,
			countDefaultZero: true,
			seasonality: 'weekly',
			timezone: ''
		},
		forecast: {
			algorithm: 'linear',
			deviations: 1,
			horizon: 'next_1w',
			history: '1w',
			model: 'default',
			seasonality: 'weekly',
			interval: null,
			timezone: ''
		},
		outlier: { algorithm: 'dbscan', tolerance: 3, percentage: 20 },
		// No `by` at first: grouped by host, the cluster-level checks (host '') would be left out.
		check: { name: '', over: [], by: [], last: null, ok: null, unknown: null },
		comparator: '>',
		critical: null,
		warning: null,
		criticalRecovery: null,
		warningRecovery: null,
		missing: 'keep',
		noDataTimeframe: null,
		evaluationDelay: null,
		newGroupDelay: null,
		requireFullWindow: false,
		timeoutH: null,
		renotifyInterval: null,
		escalationMessage: '',
		includeTags: true,
		notifyAudit: false,
		name: '',
		message: '',
		tags: [],
		priority: null,
		otherOptions: {}
	};
}

// ---- the monitored expression -----------------------------------------------------

// Quoted text and function names are not query names: `top(b, 5, 'a')`.
function replaceNames(formula: string, replace: (name: string) => string | undefined): string {
	let out = '';
	let i = 0;
	while (i < formula.length) {
		const c = formula[i];
		if (c === "'" || c === '"') {
			const end = formula.indexOf(c, i + 1);
			const next = end === -1 ? formula.length : end + 1;
			out += formula.slice(i, next);
			i = next;
			continue;
		}
		const word = /^[A-Za-z_][A-Za-z0-9_]*/.exec(formula.slice(i))?.[0];
		if (word) {
			const call = formula
				.slice(i + word.length)
				.trimStart()
				.startsWith('(');
			out += (!call && replace(word)) || word;
			i += word.length;
			continue;
		}
		out += c;
		i++;
	}
	return out;
}

const monitoredFormula = (formulas: ExplorerFormula[]) => formulas.find((f) => f.text.trim());

/**
 * The expression a monitor measures, with the queries written out:
 * `avg:a{*} by {host} / avg:b{*} by {host}`. Formulas use the queries as
 * queried, without the query rows' own functions, as in the explorer.
 */
export function expressionOf(queries: ExplorerQuery[], formulas: ExplorerFormula[]): string {
	const formula = monitoredFormula(formulas);
	if (!formula) {
		const first = queries[0];
		const text = first ? queryText(first) : '';
		return text ? wrap(text, first.functions) : '';
	}
	// A text row may hold an expression of its own (`a - b`), which a formula
	// around it must not split; a builder row is always one query.
	const texts = new Map(
		queries.map((q) => [q.name, q.mode === 'text' ? `(${queryText(q)})` : queryText(q)])
	);
	return wrap(
		replaceNames(formula.text.trim(), (name) => texts.get(name)),
		formula.functions
	);
}

/** The `by` keys of the monitored queries: one alert per combination, none for a simple alert. */
export function groupKeysOf(view: MonitorView): string[] {
	if (view.method === 'check') return view.check.by;
	const formula = monitoredFormula(view.formulas);
	let monitored = view.queries.slice(0, 1);
	if (formula) {
		const used = new Set<string>();
		replaceNames(formula.text, (name) => {
			used.add(name);
			return undefined;
		});
		monitored = view.queries.filter((q) => used.has(q.name));
	}
	const keys: string[] = [];
	for (const q of monitored) {
		for (const match of queryText(q).matchAll(/\bby\s*\{([^}]*)\}/g)) {
			for (const key of match[1].split(',').map((k) => k.trim())) {
				if (key && !keys.includes(key)) keys.push(key);
			}
		}
	}
	return keys;
}

const numberText = (n: number | null) => (n === null ? '' : String(n));
const quoted = (text: string) => `'${text.replace(/'/g, '')}'`;

function measured(view: MonitorView, expression: string): string {
	switch (view.method) {
		case 'anomaly': {
			const a = view.anomaly;
			const args = [
				expression,
				quoted(a.algorithm),
				numberText(a.bounds),
				`direction=${quoted(a.direction)}`,
				`alert_window=${quoted(a.alertWindow)}`
			];
			if (a.interval !== null) args.push(`interval=${a.interval}`);
			args.push(`count_default_zero=${quoted(String(a.countDefaultZero))}`);
			if (a.algorithm !== 'basic') args.push(`seasonality=${quoted(a.seasonality)}`);
			if (a.timezone.trim()) args.push(`timezone=${quoted(a.timezone.trim())}`);
			return `anomalies(${args.join(', ')})`;
		}
		case 'forecast': {
			const f = view.forecast;
			const args = [expression, quoted(f.algorithm), numberText(f.deviations)];
			if (f.interval !== null) args.push(`interval=${f.interval}`);
			if (f.algorithm === 'linear') {
				args.push(`history=${quoted(f.history)}`, `model=${quoted(f.model)}`);
			} else {
				args.push(`seasonality=${quoted(f.seasonality)}`);
			}
			if (f.timezone.trim()) args.push(`timezone=${quoted(f.timezone.trim())}`);
			return `forecast(${args.join(', ')})`;
		}
		case 'outlier': {
			const o = view.outlier;
			const args = [expression, quoted(o.algorithm), numberText(o.tolerance)];
			if (o.algorithm.endsWith('mad')) args.push(numberText(o.percentage));
			return `outliers(${args.join(', ')})`;
		}
		default:
			return expression;
	}
}

/** Forecast compares the upper bound with `>=` or the lower with `<=`; outliers are `> 0`. */
export function effectiveComparator(view: MonitorView): Comparator {
	switch (view.method) {
		case 'anomaly':
			return '>=';
		case 'outlier':
			return '>';
		case 'forecast':
			return view.comparator === '<=' || view.comparator === '<' ? '<=' : '>=';
		default:
			return view.comparator;
	}
}

function head(view: MonitorView): string {
	switch (view.method) {
		case 'change':
			return `${view.change.measure}(${view.aggregator}(${view.window}),${view.change.shift})`;
		case 'forecast':
			return `${effectiveComparator(view) === '<=' ? 'min' : 'max'}(${view.forecast.horizon})`;
		default:
			return `${view.aggregator}(${view.window})`;
	}
}

function critical(view: MonitorView): number | null {
	return view.method === 'outlier' ? 0 : view.critical;
}

const doubleQuoted = (text: string) => `"${text.replace(/"/g, '')}"`;

/** The runs a check counts in when none are given: as many as its largest threshold needs. */
export const defaultCheckLast = (view: MonitorView) =>
	Math.max(view.critical ?? 1, view.warning ?? 1, view.check.ok ?? 1, view.check.unknown ?? 1);

/** `"name".over("a","b")[.by("k")].last(N).count_by_status()`. */
export function checkQuery(view: MonitorView): string {
	const c = view.check;
	const list = (items: string[]) => (items.length > 0 ? items.map(doubleQuoted).join(',') : '"*"');
	const by = c.by.length > 0 ? `.by(${list(c.by)})` : '';
	return `${doubleQuoted(c.name.trim())}.over(${list(c.over)})${by}.last(${c.last ?? defaultCheckLast(view)}).count_by_status()`;
}

export function monitorQuery(view: MonitorView): string {
	if (view.method === 'check') return checkQuery(view);
	const expression = measured(view, expressionOf(view.queries, view.formulas));
	return `${head(view)}:${expression} ${effectiveComparator(view)} ${numberText(critical(view))}`;
}

// ---- options ----------------------------------------------------------------------

const EDITOR_OPTIONS = [
	'thresholds',
	'threshold_windows',
	'evaluation_delay',
	'new_group_delay',
	'new_host_delay',
	'require_full_window',
	'notify_no_data',
	'no_data_timeframe',
	'on_missing_data',
	'timeout_h',
	'renotify_interval',
	'escalation_message',
	'include_tags',
	'notify_audit'
];

function optionsOf(view: MonitorView): Record<string, unknown> {
	const options: Record<string, unknown> = { ...view.otherOptions };
	const thresholds: Record<string, number> = {};
	const value = critical(view);
	if (value !== null) thresholds.critical = value;
	if (view.method !== 'outlier' && view.warning !== null) thresholds.warning = view.warning;
	if (view.method === 'check') {
		if (view.check.ok !== null) thresholds.ok = view.check.ok;
		if (view.check.unknown !== null) thresholds.unknown = view.check.unknown;
	} else if (view.method !== 'outlier') {
		if (view.criticalRecovery !== null) thresholds.critical_recovery = view.criticalRecovery;
		if (view.warningRecovery !== null) thresholds.warning_recovery = view.warningRecovery;
	}
	options.thresholds = thresholds;

	if (view.method === 'anomaly') {
		options.threshold_windows = {
			trigger_window: view.anomaly.alertWindow,
			recovery_window: view.anomaly.recoveryWindow
		};
	}

	if (view.missing === 'notify') {
		options.notify_no_data = true;
		if (view.noDataTimeframe !== null) options.no_data_timeframe = view.noDataTimeframe;
	} else if (view.missing !== 'keep') {
		options.on_missing_data = view.missing;
	}

	if (view.evaluationDelay !== null) options.evaluation_delay = view.evaluationDelay;
	if (view.newGroupDelay !== null) options.new_group_delay = view.newGroupDelay;
	if (view.requireFullWindow && view.method !== 'check') options.require_full_window = true;
	if (view.timeoutH !== null && view.timeoutH > 0) options.timeout_h = view.timeoutH;

	if (view.renotifyInterval !== null) options.renotify_interval = view.renotifyInterval;
	if (view.escalationMessage.trim()) options.escalation_message = view.escalationMessage;
	if (!view.includeTags) options.include_tags = false;
	if (view.notifyAudit) options.notify_audit = true;
	return options;
}

export function monitorOf(view: MonitorView): DatadogMonitor {
	return {
		name: view.name.trim(),
		type: view.method === 'check' ? 'service check' : 'query alert',
		query: monitorQuery(view),
		message: view.message,
		tags: view.tags,
		priority: view.priority,
		options: optionsOf(view)
	};
}

// ---- reading a stored monitor back ------------------------------------------------

function numberOr(value: unknown, fallback: number | null): number | null {
	if (typeof value === 'number' && Number.isFinite(value)) return value;
	if (typeof value === 'string' && value.trim() !== '' && Number.isFinite(Number(value)))
		return Number(value);
	return fallback;
}

const textOr = (value: unknown, fallback: string) => (typeof value === 'string' ? value : fallback);

/** Trimmed, without blanks or repeats. */
export const uniqueTrimmed = (items: string[]) => [
	...new Set(items.map((t) => t.trim()).filter(Boolean))
];

const textList = (value: unknown) =>
	Array.isArray(value)
		? uniqueTrimmed(value.filter((t): t is string => typeof t === 'string'))
		: [];

/** JSON with sorted keys: Postgres hands jsonb back in an order of its own. */
export function canonicalJson(value: unknown): string {
	return JSON.stringify(value, (_, v) =>
		v && typeof v === 'object' && !Array.isArray(v)
			? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b)))
			: v
	);
}

export type StoredMonitor = {
	name: string;
	query: string;
	message: string;
	options: unknown;
	tags: string[];
	priority: number | null;
	editor: unknown;
};

export function otherOptionsOf(options: unknown): Record<string, unknown> {
	return Object.fromEntries(
		Object.entries(asObject(options)).filter(([key]) => !EDITOR_OPTIONS.includes(key))
	);
}

/** The method the editor saved a monitor with; null for one it did not write. */
export function editorMethod(editor: unknown): Method | null {
	const method = asObject(editor).method;
	return METHODS.includes(method as Method) ? (method as Method) : null;
}

/**
 * The editor's view of a stored monitor, or null when the editor did not
 * write it. `drifted`: the stored query or options are no longer what the
 * view saves as, so a save would replace them.
 */
export function viewOf(stored: StoredMonitor): { view: MonitorView; drifted: boolean } | null {
	if (editorMethod(stored.editor) === null) return null;
	const view = readMonitorView(stored.editor);
	view.name = stored.name;
	view.message = stored.message;
	view.tags = [...stored.tags];
	view.priority = stored.priority;
	view.otherOptions = otherOptionsOf(stored.options);
	const saved = monitorOf(view);
	const drifted =
		saved.query !== stored.query || canonicalJson(saved.options) !== canonicalJson(stored.options);
	return { view, drifted };
}

/**
 * A view as the browser submitted it, or as the editor stored it. Every field
 * is checked and anything else is dropped: the save path builds the stored
 * monitor from this alone.
 */
export function readMonitorView(raw: unknown): MonitorView {
	const r = asObject(raw);
	const view = defaultMonitorView();
	const a = asObject(r.anomaly);
	const f = asObject(r.forecast);
	const o = asObject(r.outlier);
	const c = asObject(r.change);
	const k = asObject(r.check);

	const queries = Array.isArray(r.queries) ? r.queries.map(readQuery) : [];
	view.queries = queries.length > 0 ? queries : view.queries;
	view.formulas = Array.isArray(r.formulas) ? r.formulas.map(readFormula).slice(0, 1) : [];
	view.method = pick(METHODS, r.method, view.method);
	view.aggregator = pick(TIME_AGGREGATORS, r.aggregator, view.aggregator);
	view.window = textOr(r.window, view.window);
	view.change = {
		measure: c.measure === 'pct_change' ? 'pct_change' : 'change',
		shift: textOr(c.shift, view.change.shift)
	};
	view.anomaly = {
		algorithm: pick(ANOMALY_ALGORITHMS, a.algorithm, view.anomaly.algorithm),
		bounds: numberOr(a.bounds, null),
		direction: pick(DIRECTIONS, a.direction, view.anomaly.direction),
		alertWindow: textOr(a.alertWindow, view.anomaly.alertWindow),
		recoveryWindow: textOr(a.recoveryWindow, view.anomaly.recoveryWindow),
		interval: numberOr(a.interval, null),
		countDefaultZero: a.countDefaultZero !== false,
		seasonality: pick(SEASONALITIES, a.seasonality, view.anomaly.seasonality),
		timezone: textOr(a.timezone, '')
	};
	view.forecast = {
		algorithm: pick(FORECAST_ALGORITHMS, f.algorithm, view.forecast.algorithm),
		deviations: numberOr(f.deviations, null),
		horizon: textOr(f.horizon, view.forecast.horizon),
		history: textOr(f.history, view.forecast.history),
		model: pick(FORECAST_MODELS, f.model, view.forecast.model),
		seasonality: pick(SEASONALITIES, f.seasonality, view.forecast.seasonality),
		interval: numberOr(f.interval, null),
		timezone: textOr(f.timezone, '')
	};
	view.outlier = {
		algorithm: pick(OUTLIER_ALGORITHMS, o.algorithm, view.outlier.algorithm),
		tolerance: numberOr(o.tolerance, null),
		percentage: numberOr(o.percentage, null)
	};
	view.check = {
		name: textOr(k.name, ''),
		over: textList(k.over),
		by: textList(k.by),
		last: numberOr(k.last, null),
		ok: numberOr(k.ok, null),
		unknown: numberOr(k.unknown, null)
	};
	view.comparator = pick(COMPARATORS, r.comparator, view.comparator);
	view.critical = numberOr(r.critical, null);
	view.warning = numberOr(r.warning, null);
	view.criticalRecovery = numberOr(r.criticalRecovery, null);
	view.warningRecovery = numberOr(r.warningRecovery, null);
	view.missing = pick(MISSING, r.missing, view.missing);
	view.noDataTimeframe = numberOr(r.noDataTimeframe, null);
	view.evaluationDelay = numberOr(r.evaluationDelay, null);
	view.newGroupDelay = numberOr(r.newGroupDelay, null);
	view.requireFullWindow = r.requireFullWindow === true;
	view.timeoutH = numberOr(r.timeoutH, null);
	view.renotifyInterval = numberOr(r.renotifyInterval, null);
	view.escalationMessage = textOr(r.escalationMessage, '');
	view.includeTags = r.includeTags !== false;
	view.notifyAudit = r.notifyAudit === true;
	view.name = textOr(r.name, '');
	view.message = textOr(r.message, '');
	const tags = Array.isArray(r.tags)
		? r.tags.filter((t): t is string => typeof t === 'string')
		: [];
	view.tags = [...new Set(tags.map((t) => t.trim()))];
	view.priority = PRIORITIES.find((p) => p === r.priority) ?? null;
	return view;
}

/**
 * A monitor started from the explorer: its queries, and its first visible
 * formula if it has one, else its first visible query alone.
 */
export function fromExplorer(explorer: ExplorerView): MonitorView {
	const view = defaultMonitorView();
	const formula = explorer.formulas.find((f) => !f.hidden && f.text.trim());
	const shown = explorer.queries.filter((q) => !q.hidden);
	if (formula) {
		view.queries = explorer.queries.map((q) => ({ ...q, hidden: false, alias: '' }));
		view.formulas = [{ ...formula, hidden: false, alias: '' }];
	} else if (shown.length > 0) {
		view.queries = [{ ...shown[0], name: 'a', hidden: false, alias: '' }];
	}
	return view;
}

/** Moves the view to another method, with thresholds that fit it. */
export function switchMethod(view: MonitorView, method: Method) {
	const fraction = (n: number | null) => n !== null && n > 0 && n <= 1;
	if (method === 'anomaly' && view.method !== 'anomaly') {
		if (!fraction(view.critical)) view.critical = 1;
		view.warning = null;
		view.criticalRecovery = null;
		view.warningRecovery = null;
		if (windowSeconds(view.window) < windowSeconds(view.anomaly.alertWindow)) {
			view.window = 'last_4h';
		}
	}
	if (method === 'forecast' && view.comparator !== '>=' && view.comparator !== '<=') {
		view.comparator = view.comparator.startsWith('<') ? '<=' : '>=';
	}
	// A metric's thresholds are values, a check's are counts of runs: neither is the other.
	if (method === 'check' && view.method !== 'check') {
		view.critical = 2;
		view.warning = null;
		view.criticalRecovery = null;
		view.warningRecovery = null;
		if (view.missing !== 'keep' && view.missing !== 'notify') view.missing = 'keep';
	} else if (method !== 'check' && view.method === 'check') {
		view.critical = null;
		view.warning = null;
	}
	if (method === 'change' && windowSeconds(view.window) > 2 * 86_400) {
		view.window = 'last_5m';
	}
	view.method = method;
}

export type Unfilled = 'name' | 'metric' | 'check' | 'critical' | 'formula' | 'parameters';

function parametersUnfilled(view: MonitorView): boolean {
	switch (view.method) {
		case 'anomaly':
			return view.anomaly.bounds === null;
		case 'forecast':
			return view.forecast.deviations === null;
		case 'outlier':
			return (
				view.outlier.tolerance === null ||
				(view.outlier.algorithm.endsWith('mad') && view.outlier.percentage === null)
			);
		default:
			return false;
	}
}

export function unfilled(view: MonitorView): Unfilled[] {
	const missing: Unfilled[] = [];
	if (view.method === 'check') {
		if (!view.check.name.trim()) missing.push('check');
		if (view.critical === null) missing.push('critical');
		if (!view.name.trim()) missing.push('name');
		return missing;
	}
	if (!expressionOf(view.queries, view.formulas)) missing.push('metric');
	if (view.queries.length > 1 && !monitoredFormula(view.formulas)) missing.push('formula');
	if (view.method !== 'outlier' && view.critical === null) missing.push('critical');
	if (parametersUnfilled(view)) missing.push('parameters');
	if (!view.name.trim()) missing.push('name');
	return missing;
}

// ---- the chart --------------------------------------------------------------------

const UNIT_SECONDS: Record<string, number> = {
	m: 60,
	h: 3600,
	d: 86_400,
	w: 604_800,
	mo: 31 * 86_400
};

/** 0 when unreadable. */
export function windowSeconds(text: string): number {
	const m = /^(?:last_|next_)?(\d+)(mo|m|h|d|w)(?:_ago)?$/.exec(text);
	return m ? Number(m[1]) * UNIT_SECONDS[m[2]] : 0;
}

// A change is drawn per bucket, as it is computed before the time aggregation.
export function chartRequest(view: MonitorView): {
	queries: { name: string; query: string }[];
	formulas: string[];
} {
	if (view.method === 'check') return { queries: [], formulas: [] };
	const queries = view.queries
		.map((q) => ({ name: q.name, query: queryText(q) }))
		.filter((q) => q.query !== '');
	if (queries.length === 0) return { queries, formulas: [] };

	const formula = monitoredFormula(view.formulas);
	const first = view.queries[0];
	const base = formula
		? wrap(formula.text.trim(), formula.functions)
		: wrap(first.name, first.functions);

	switch (view.method) {
		case 'change': {
			const shifted = `timeshift(${base}, -${windowSeconds(view.change.shift)})`;
			const difference = `(${base}) - ${shifted}`;
			return {
				queries,
				formulas: [
					view.change.measure === 'pct_change' ? `(${difference}) / ${shifted} * 100` : difference
				]
			};
		}
		case 'anomaly': {
			const a = view.anomaly;
			const seasonality = a.algorithm === 'basic' ? '' : `, seasonality='${a.seasonality}'`;
			return {
				queries,
				formulas: [`anomalies(${base}, '${a.algorithm}', ${a.bounds ?? 2}${seasonality})`]
			};
		}
		case 'forecast': {
			const f = view.forecast;
			const model =
				f.algorithm === 'linear'
					? `model='${f.model}', history='${f.history}'`
					: `seasonality='${f.seasonality}'`;
			const seconds = windowSeconds(f.horizon);
			const horizon =
				seconds % 86_400 === 0 ? `${seconds / 86_400}d` : `${Math.round(seconds / 3600)}h`;
			return {
				queries,
				formulas: [
					`forecast(${base}, '${f.algorithm}', ${f.deviations ?? 1}, ${model}, horizon='${horizon}')`
				]
			};
		}
		case 'outlier': {
			const o = view.outlier;
			const percentage = o.algorithm.endsWith('mad') ? `, ${o.percentage ?? 20}` : '';
			return {
				queries,
				formulas: [base, `outliers(${base}, '${o.algorithm}', ${o.tolerance ?? 3}${percentage})`]
			};
		}
		default:
			return { queries, formulas: [base] };
	}
}

export function chartRangeSeconds(view: MonitorView): number {
	if (view.method === 'forecast') {
		return view.forecast.algorithm === 'linear'
			? windowSeconds(view.forecast.history) || 604_800
			: 2 * 604_800;
	}
	const wanted = 4 * windowSeconds(view.window);
	return [3600, 4 * 3600, 86_400, 604_800].find((s) => s >= wanted) ?? 604_800;
}

/** The values that alert: above or below critical; none for == and !=. */
export function alertZone(
	comparator: string,
	criticalValue: number | null
): { from: number | null; to: number | null } | null {
	if (criticalValue === null) return null;
	if (comparator === '>' || comparator === '>=') return { from: criticalValue, to: null };
	if (comparator === '<' || comparator === '<=') return { from: null, to: criticalValue };
	return null;
}
