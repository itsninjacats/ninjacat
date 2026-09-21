import { env } from '$env/dynamic/private';

// Address of the Go panel API. This is NOT the agent intake port (8080),
// which is exposed to the machines running agents. The panel talks to 8081,
// which can stay on an internal network.
//
// Everything here runs server-side only, so the browser never learns this
// address exists.
const BASE = env.NINJACAT_INTERNAL_URL || 'http://localhost:8081';

// Control calls are best-effort and must never hold up a page.
const CONTROL_TIMEOUT_MS = 2000;

// Queries hit ClickHouse, so they get more room. Still shorter than the Go
// side's own budget, so a slow query surfaces as our timeout rather than
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

/** Thrown by the query helpers when the Go server cannot answer. */
export class NinjacatError extends Error {}

async function get<T>(path: string, timeoutMs = QUERY_TIMEOUT_MS): Promise<T> {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), timeoutMs);
	try {
		const res = await fetch(`${BASE}${path}`, { signal: controller.signal });
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

/**
 * Asks the Go server to re-read the API keys from the database now.
 *
 * Called after a key is added or removed so the change takes effect
 * immediately instead of waiting for the periodic refresh.
 *
 * NEVER throws: a Go server that is down must not break an operation that
 * already succeeded in Postgres — the keeper will pick the key up within 30
 * seconds anyway. It reports whether it worked instead.
 */
export async function refreshApiKeys(): Promise<{ ok: boolean; reason?: string }> {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), CONTROL_TIMEOUT_MS);
	try {
		const res = await fetch(`${BASE}/internal/apikeys/refresh`, {
			method: 'POST',
			signal: controller.signal
		});
		if (!res.ok) return { ok: false, reason: `server responded ${res.status}` };
		return { ok: true };
	} catch (e) {
		return { ok: false, reason: e instanceof Error ? e.message : 'unknown error' };
	} finally {
		clearTimeout(timer);
	}
}
