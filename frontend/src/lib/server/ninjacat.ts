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
export type MetricSeries = { metric: string; host: string; points: SeriesPoint[] };
export type SeriesResponse = {
	metric: string;
	from: string;
	to: string;
	step: number;
	series: MetricSeries[];
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
			throw new NinjacatError('serwer nie odpowiedział na czas');
		}
		throw new NinjacatError(e instanceof Error ? e.message : 'nieznany błąd');
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

/** Hosts that reported a metric. */
export async function fetchMetricHosts(metric: string): Promise<string[]> {
	const q = new URLSearchParams(metric ? { metric } : {});
	const body = await get<{ hosts: string[] }>(`/internal/metrics/hosts?${q}`);
	return body.hosts ?? [];
}

/**
 * One metric over a time range, one series per host.
 *
 * `from` accepts a relative offset such as "-6h" or "-7d", which is what a
 * dashboard actually wants: a saved view means "the last six hours", not six
 * hours before the day it was saved.
 */
export async function fetchMetricSeries(opts: {
	metric: string;
	from: string;
	to?: string;
	hosts?: string[];
	step?: string;
	agg?: string;
}): Promise<SeriesResponse> {
	const q = new URLSearchParams({ metric: opts.metric, from: opts.from });
	if (opts.to) q.set('to', opts.to);
	if (opts.step) q.set('step', opts.step);
	if (opts.agg) q.set('agg', opts.agg);
	for (const h of opts.hosts ?? []) q.append('host', h);
	return get<SeriesResponse>(`/internal/metrics/query?${q}`);
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
