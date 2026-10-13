import { env } from '$env/dynamic/private';
import { getRequestEvent } from '$app/server';
import { SignJWT } from 'jose';

// Address of the server's internal port. This is NOT the agent intake port (8080),
// which is exposed to the machines running agents. The panel talks to 8081,
// which can stay on an internal network.
//
// Everything here runs server-side only, so the browser never learns this
// address exists.
const BASE = env.NINJACAT_INTERNAL_URL || 'http://localhost:8081';

// Queries hit ClickHouse, so they get more room. Still shorter than the
// server's own budget, so a slow query surfaces as our timeout rather than
// hanging a request handler.
const QUERY_TIMEOUT_MS = 10_000;

export type SeriesPoint = { t: string; v: number };

/** One line on a chart, named after the group-by values that produced it. */
export type MetricSeries = {
	name: string;
	tags: Record<string, string>;
	points: SeriesPoint[];
};

export type SeriesResponse = {
	metric: string;
	from: string;
	to: string;
	step: number;
	series: MetricSeries[];
};

export type LogEntry = {
	timestamp: string;
	host: string;
	service: string;
	source: string;
	status: string;
	message: string;
	/** A tag key can carry several values, so every key maps to a list. */
	tags: Record<string, string[]>;
};

export type LogSearchResponse = { logs: LogEntry[]; count: number };

export type FacetCount = { value: string; count: number };

export type LogFacets = {
	services: FacetCount[];
	hosts: FacetCount[];
	statuses: FacetCount[];
};

/** Thrown by the query helpers when the server cannot answer. */
export class NinjacatError extends Error {}

/**
 * The query process answers only a token the panel signed, and runs every
 * query for the tenant it names (api/src/Server/QueryToken.fs). One is minted
 * per call, for the user and tenant of the request being served.
 */
async function queryToken(): Promise<string> {
	const { locals } = getRequestEvent();
	if (!locals.user || !locals.tenant) {
		throw new NinjacatError('this request has no tenant to query for');
	}
	const secret = new TextEncoder().encode(env.NINJACAT_QUERY_TOKEN_SECRET ?? '');
	if (secret.length < 32) {
		throw new NinjacatError('NINJACAT_QUERY_TOKEN_SECRET must be at least 32 bytes');
	}
	return new SignJWT({ tenant: locals.tenant.id, role: locals.tenant.role })
		.setProtectedHeader({ alg: 'HS256' })
		.setSubject(locals.user.id)
		.setIssuer('ninjacat-panel')
		.setAudience('ninjacat-query')
		.setIssuedAt()
		.setExpirationTime('60s')
		.sign(secret);
}

async function get<T>(path: string, timeoutMs = QUERY_TIMEOUT_MS): Promise<T> {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), timeoutMs);
	try {
		const res = await fetch(`${BASE}${path}`, {
			headers: { authorization: `Bearer ${await queryToken()}` },
			signal: controller.signal
		});
		const body = await res.json().catch(() => null);
		if (!res.ok) {
			throw new NinjacatError(body?.error ?? `server responded ${res.status}`);
		}
		return body as T;
	} catch (e) {
		if (e instanceof NinjacatError) throw e;
		if (e instanceof Error && e.name === 'AbortError') {
			throw new NinjacatError('the server did not respond in time');
		}
		throw new NinjacatError(e instanceof Error ? e.message : 'unknown error');
	} finally {
		clearTimeout(timer);
	}
}

/** Metric names, optionally narrowed by a substring. */
export async function fetchMetricNames(search = '', limit = 500): Promise<string[]> {
	const q = new URLSearchParams({ limit: String(limit) });
	if (search) q.set('search', search);
	const body = await get<{ metrics: string[] }>(`/internal/metrics/names?${q}`);
	return body.metrics ?? [];
}

/** Tag keys seen on a metric. */
export async function fetchMetricTagKeys(metric: string): Promise<string[]> {
	const q = new URLSearchParams({ metric });
	const body = await get<{ keys: string[] }>(`/internal/metrics/tags?${q}`);
	return body.keys ?? [];
}

/** Values recorded for one tag key of a metric, optionally narrowed by substring. */
export async function fetchMetricTagValues(
	metric: string,
	key: string,
	search = '',
	limit = 100
): Promise<string[]> {
	const q = new URLSearchParams({ metric, key, limit: String(limit) });
	if (search) q.set('search', search);
	const body = await get<{ values: string[] }>(`/internal/metrics/tag-values?${q}`);
	return body.values ?? [];
}

/**
 * One metric over a time range, one series per group-by combination.
 *
 * `from` accepts a relative offset such as "-6h" or "-7d", which is what a
 * dashboard actually wants: a saved view means "the last six hours", not six
 * hours before the day it was saved.
 *
 * `tags` are "key:value" filters. Different keys AND together; repeating the
 * same key means OR within that key. `by` lists the tag keys to group by.
 */
export async function fetchMetricSeries(opts: {
	metric: string;
	from: string;
	to?: string;
	step?: string;
	agg?: string;
	tags?: string[];
	by?: string[];
}): Promise<SeriesResponse> {
	const q = new URLSearchParams({ metric: opts.metric, from: opts.from });
	if (opts.to) q.set('to', opts.to);
	if (opts.step) q.set('step', opts.step);
	if (opts.agg) q.set('agg', opts.agg);
	for (const t of opts.tags ?? []) q.append('tag', t);
	for (const b of opts.by ?? []) q.append('by', b);
	return get<SeriesResponse>(`/internal/metrics/query?${q}`);
}

// ---- the query API: Datadog's v2 query endpoints ----------------------------------

// Datadog's query language (formulas, functions, anomalies, forecasts), on the
// same internal port as the calls above.

/** One query as Datadog writes it, e.g. `avg:system.cpu.user{env:prod} by {host}`. */
export type NamedQuery = { name: string; query: string };

export type TimePoint = { t: number; v: number };
export type BandPoint = { t: number; lower: number; upper: number };
export type ForecastPoint = { t: number; v: number; lower: number; upper: number };

/** One series of one formula (or of one query, when there are no formulas). */
export type Timeseries = {
	/** Index into the formulas, or into the queries when there are none. */
	output: number;
	/** `key:value` for each group-by key; empty for an ungrouped series. */
	tags: string[];
	points: TimePoint[];
	/** anomalies(): the expected range, where known. */
	band: BandPoint[];
	/** forecast(): points after the window. */
	forecast: ForecastPoint[];
};

export type ScalarTable = {
	/** Column names of the values: the formulas, or the queries. */
	names: string[];
	rows: { tags: string[]; values: (number | null)[] }[];
};

/**
 * The query API rejects a request with every problem listed, in Datadog's
 * `{"errors": [...]}` envelope. They are kept apart so the page can show each.
 */
export class QueryError extends NinjacatError {
	constructor(readonly problems: string[]) {
		super(problems.join('; '));
	}
}

async function post<T>(path: string, body: unknown): Promise<T> {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), QUERY_TIMEOUT_MS);
	try {
		const res = await fetch(`${BASE}${path}`, {
			method: 'POST',
			headers: {
				authorization: `Bearer ${await queryToken()}`,
				'content-type': 'application/json'
			},
			body: JSON.stringify(body),
			signal: controller.signal
		});
		const json = await res.json().catch(() => null);
		if (!res.ok) {
			const problems: unknown = json?.errors;
			if (Array.isArray(problems) && problems.length > 0)
				throw new QueryError(problems.map(String));
			throw new NinjacatError(`query API responded ${res.status}`);
		}
		return json as T;
	} catch (e) {
		if (e instanceof NinjacatError) throw e;
		if (e instanceof Error && e.name === 'AbortError') {
			throw new NinjacatError('the query API did not respond in time');
		}
		throw new NinjacatError(e instanceof Error ? e.message : 'unknown error');
	} finally {
		clearTimeout(timer);
	}
}

function datadogQueries(queries: NamedQuery[]) {
	return queries.map((q) => ({ data_source: 'metrics', name: q.name, query: q.query }));
}

type TimeseriesResponse = {
	data: {
		attributes: {
			series: { group_tags: string[]; query_index: number }[];
			times: number[];
			values: (number | null)[][];
			ninjacat_bounds?: ({ upper: (number | null)[]; lower: (number | null)[] } | null)[];
			ninjacat_forecast?: ({
				times: number[];
				values: number[];
				upper: number[];
				lower: number[];
			} | null)[];
		};
	};
};

/** `POST /api/v2/query/timeseries`, reshaped from parallel arrays into one record per series. */
export async function queryTimeseries(opts: {
	from: number;
	to: number;
	queries: NamedQuery[];
	formulas: string[];
}): Promise<Timeseries[]> {
	const body = await post<TimeseriesResponse>('/api/v2/query/timeseries', {
		data: {
			type: 'timeseries_request',
			attributes: {
				from: opts.from,
				to: opts.to,
				queries: datadogQueries(opts.queries),
				...(opts.formulas.length > 0 && { formulas: opts.formulas.map((formula) => ({ formula })) })
			}
		}
	});

	const a = body.data.attributes;
	return a.series.map((s, i) => {
		const values = a.values[i] ?? [];
		const bounds = a.ninjacat_bounds?.[i];
		const forecast = a.ninjacat_forecast?.[i];

		const points: TimePoint[] = [];
		const band: BandPoint[] = [];
		a.times.forEach((t, j) => {
			const v = values[j];
			if (v !== null && v !== undefined) points.push({ t, v });
			const lower = bounds?.lower[j];
			const upper = bounds?.upper[j];
			if (lower != null && upper != null) band.push({ t, lower, upper });
		});

		return {
			output: s.query_index,
			tags: s.group_tags,
			points,
			band,
			forecast: forecast
				? forecast.times.map((t, j) => ({
						t,
						v: forecast.values[j],
						lower: forecast.lower[j],
						upper: forecast.upper[j]
					}))
				: []
		};
	});
}

type ScalarResponse = {
	data: {
		attributes: {
			columns: (
				| { name: string; type: 'group'; values: string[][] }
				| { name: string; type: 'number'; values: (number | null)[] }
			)[];
		};
	};
};

/** `POST /api/v2/query/scalar`: the window reduced to one value per group and formula. */
export async function queryScalar(opts: {
	from: number;
	to: number;
	queries: NamedQuery[];
	formulas: string[];
	aggregator: string;
}): Promise<ScalarTable> {
	const body = await post<ScalarResponse>('/api/v2/query/scalar', {
		data: {
			type: 'scalar_request',
			attributes: {
				from: opts.from,
				to: opts.to,
				queries: opts.queries.map((q) => ({
					...datadogQueries([q])[0],
					aggregator: opts.aggregator
				})),
				...(opts.formulas.length > 0 && { formulas: opts.formulas.map((formula) => ({ formula })) })
			}
		}
	});

	const columns = body.data.attributes.columns;
	const groups = columns.filter((c) => c.type === 'group');
	const numbers = columns.filter((c) => c.type === 'number');
	const rowCount = numbers[0]?.values.length ?? 0;

	return {
		names: numbers.map((c) => c.name),
		rows: Array.from({ length: rowCount }, (_, r) => ({
			tags: groups.flatMap((g) => g.values[r].map((v) => `${g.name}:${v}`)),
			values: numbers.map((c) => c.values[r])
		}))
	};
}

/** Full-text log search with optional facet and tag filters. */
export async function searchLogs(opts: {
	q?: string;
	service?: string;
	host?: string;
	status?: string;
	tags?: string[];
	from: string;
	to?: string;
	limit?: number;
}): Promise<LogSearchResponse> {
	const q = new URLSearchParams({ from: opts.from });
	if (opts.q) q.set('q', opts.q);
	if (opts.service) q.set('service', opts.service);
	if (opts.host) q.set('host', opts.host);
	if (opts.status) q.set('status', opts.status);
	if (opts.to) q.set('to', opts.to);
	if (opts.limit) q.set('limit', String(opts.limit));
	for (const t of opts.tags ?? []) q.append('tag', t);
	const body = await get<LogSearchResponse>(`/internal/logs/search?${q}`);
	return { logs: body.logs ?? [], count: body.count ?? 0 };
}

/** Service, host and status counts for the log explorer's facet sidebar. */
export async function fetchLogFacets(from: string, to?: string): Promise<LogFacets> {
	const q = new URLSearchParams({ from });
	if (to) q.set('to', to);
	const body = await get<LogFacets>(`/internal/logs/facets?${q}`);
	return {
		services: body.services ?? [],
		hosts: body.hosts ?? [],
		statuses: body.statuses ?? []
	};
}

// ---- monitors ----------------------------------------------------------------------

// What the monitor service stored, for the token's tenant, and the check a
// definition passes before it is saved (api/src/Server/MonitorHandlers.fs).
// Times are RFC 3339 strings in UTC.

/** Datadog's states of a monitor group. */
export type MonitorState = 'OK' | 'Warn' | 'Alert' | 'No Data';

export type MonitorValidation = {
	/** Empty when the monitor would be evaluated. */
	problems: string[];
	/** Options Datadog has that are stored but not acted on yet. */
	notActedOn: string[];
};

/** Checks a monitor as it would be saved: its type, query and options. */
export async function validateMonitor(definition: {
	type: string;
	query: string;
	options: Record<string, unknown>;
}): Promise<MonitorValidation> {
	const body = await post<{ problems: string[]; not_acted_on: string[] }>(
		'/internal/monitors/validate',
		definition
	);
	return { problems: body.problems ?? [], notActedOn: body.not_acted_on ?? [] };
}

export type MonitorStatus = {
	id: number;
	/** The most severe open group's; null when no group is open ("no groups found"). */
	state: MonitorState | null;
	/** Open groups by state. */
	groups: Record<MonitorState, number>;
	/** When a group last went into Alert or Warn. */
	lastTriggeredAt: string | null;
};

// The server's limit per request (Read.maxIds), which keeps the URL short.
const statusBatch = 200;

/** One status per id, in the order given. */
export async function fetchMonitorStatus(ids: number[]): Promise<MonitorStatus[]> {
	const batches: number[][] = [];
	for (let i = 0; i < ids.length; i += statusBatch) batches.push(ids.slice(i, i + statusBatch));
	const answers = await Promise.all(batches.map(fetchStatusBatch));
	return answers.flat();
}

async function fetchStatusBatch(ids: number[]): Promise<MonitorStatus[]> {
	const q = new URLSearchParams({ ids: ids.join(',') });
	const body = await get<{
		monitors: {
			id: number;
			state?: MonitorState;
			groups: Record<MonitorState, number>;
			last_triggered_at?: string;
		}[];
	}>(`/internal/monitors/status?${q}`);
	return body.monitors.map((m) => ({
		id: m.id,
		state: m.state ?? null,
		groups: m.groups,
		lastTriggeredAt: m.last_triggered_at ?? null
	}));
}

export type MonitorGroup = {
	/** Sorted `key:value` pairs joined with `,`; `*` for a simple alert. */
	group: string;
	tags: string[];
	state: MonitorState;
	since: string;
	/** Why it is in that state: the reason of the transition into it. */
	reason: string;
	/** null when OK. */
	firstTriggeredAt: string | null;
	lastTriggeredAt: string | null;
	/** The latest evaluated value; null when none judged the group in the last day. */
	value: number | null;
	evaluatedAt: string | null;
};

/** The open groups of a monitor. */
export async function fetchMonitorGroups(id: number): Promise<MonitorGroup[]> {
	const body = await get<{
		groups: {
			group: string;
			tags: string[];
			state: MonitorState;
			since: string;
			reason: string;
			first_triggered_at?: string;
			last_triggered_at?: string;
			value?: number;
			evaluated_at?: string;
		}[];
	}>(`/internal/monitors/${id}/groups`);
	return body.groups.map((g) => ({
		group: g.group,
		tags: g.tags,
		state: g.state,
		since: g.since,
		reason: g.reason,
		firstTriggeredAt: g.first_triggered_at ?? null,
		lastTriggeredAt: g.last_triggered_at ?? null,
		value: g.value ?? null,
		evaluatedAt: g.evaluated_at ?? null
	}));
}

/** `from` and `to` as the other queries take them: RFC 3339, "now" or an offset like "-1d". */
export type MonitorRange = { from: string; to?: string; group?: string };

function rangeQuery(range: MonitorRange): URLSearchParams {
	const q = new URLSearchParams({ from: range.from });
	if (range.to) q.set('to', range.to);
	if (range.group) q.set('group', range.group);
	return q;
}

export type MonitorTransition = {
	group: string;
	tags: string[];
	at: string;
	/** null for the group's first transition. */
	fromState: MonitorState | null;
	toState: MonitorState;
	reason: string;
	/** The user whose change caused it. */
	actor: string | null;
	value: number | null;
	/** The threshold crossed: critical, warning, critical_recovery or warning_recovery. */
	crossed: string | null;
	comparator: string;
	/** The thresholds it was judged by, which may since have changed. */
	critical: number | null;
	warning: number | null;
	criticalRecovery: number | null;
	warningRecovery: number | null;
	/** The window judged; null for a transition outside an evaluation. */
	windowFrom: string | null;
	windowTo: string | null;
	firstTriggeredAt: string | null;
	lastTriggeredAt: string | null;
	monitorVersion: number;
};

type TransitionJson = {
	group: string;
	tags: string[];
	at: string;
	from_state?: MonitorState;
	to_state: MonitorState;
	reason: string;
	actor?: string;
	value?: number;
	crossed?: string;
	comparator: string;
	critical?: number;
	warning?: number;
	critical_recovery?: number;
	warning_recovery?: number;
	window_from?: string;
	window_to?: string;
	first_triggered_at?: string;
	last_triggered_at?: string;
	monitor_version: number;
};

function transitionOf(t: TransitionJson): MonitorTransition {
	return {
		group: t.group,
		tags: t.tags,
		at: t.at,
		fromState: t.from_state ?? null,
		toState: t.to_state,
		reason: t.reason,
		actor: t.actor ?? null,
		value: t.value ?? null,
		crossed: t.crossed ?? null,
		comparator: t.comparator,
		critical: t.critical ?? null,
		warning: t.warning ?? null,
		criticalRecovery: t.critical_recovery ?? null,
		warningRecovery: t.warning_recovery ?? null,
		windowFrom: t.window_from ?? null,
		windowTo: t.window_to ?? null,
		firstTriggeredAt: t.first_triggered_at ?? null,
		lastTriggeredAt: t.last_triggered_at ?? null,
		monitorVersion: t.monitor_version
	};
}

/**
 * A monitor's transitions in a range, oldest first: its state timeline.
 * `initial` is each open group's latest transition before `from`, the state it
 * enters the range in. Past the server's limit the oldest transitions are left
 * out and `truncated` says so.
 */
export async function fetchMonitorTransitions(
	id: number,
	range: MonitorRange
): Promise<{ initial: MonitorTransition[]; transitions: MonitorTransition[]; truncated: boolean }> {
	const body = await get<{
		initial: TransitionJson[];
		transitions: TransitionJson[];
		truncated: boolean;
	}>(`/internal/monitors/${id}/transitions?${rangeQuery(range)}`);
	return {
		initial: body.initial.map(transitionOf),
		transitions: body.transitions.map(transitionOf),
		truncated: body.truncated
	};
}

export type MonitorEvaluation = {
	/** Empty for an evaluation skipped or failed as a whole. */
	group: string;
	at: string;
	value: number | null;
	/** The group's state after it; "none" before its first transition. */
	state: MonitorState | 'none';
	outcome: 'evaluated' | 'skipped' | 'error';
	/** Why it was skipped or failed. */
	detail: string | null;
};

/** A monitor's evaluations in a range, oldest first: the values for its graph. */
export async function fetchMonitorEvaluations(
	id: number,
	range: MonitorRange
): Promise<{ evaluations: MonitorEvaluation[]; truncated: boolean }> {
	const body = await get<{
		evaluations: {
			group: string;
			at: string;
			value?: number;
			state: MonitorState | 'none';
			outcome: MonitorEvaluation['outcome'];
			detail?: string;
		}[];
		truncated: boolean;
	}>(`/internal/monitors/${id}/evaluations?${rangeQuery(range)}`);
	return {
		evaluations: body.evaluations.map((e) => ({
			group: e.group,
			at: e.at,
			value: e.value ?? null,
			state: e.state,
			outcome: e.outcome,
			detail: e.detail ?? null
		})),
		truncated: body.truncated
	};
}
