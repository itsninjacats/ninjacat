import { json } from '@sveltejs/kit';
import { fetchMetricSeries, NinjacatError } from '$lib/server/ninjacat';
import type { RequestHandler } from './$types';

// Proxy between the browser and the Go panel API.
//
// It exists so :8081 never has to be reachable from a browser. The page asks
// this route, this route asks Go, and the internal port stays internal.
//
// It is a sibling route rather than a +server.ts next to +page.svelte,
// because SvelteKit does not allow both in one directory.
//
// The session check is repeated here ON PURPOSE. +layout.server.ts guards the
// /app branch for PAGES, but its load never runs for a +server.ts route — the
// same gap that lets form actions bypass a layout guard. Without this check
// the endpoint would be open to anyone who guessed the path, which is exactly
// the data the guard above it exists to protect.
export const GET: RequestHandler = async ({ url, locals }) => {
	if (!locals.user) {
		return json({ error: 'unauthorized' }, { status: 401 });
	}

	const metric = url.searchParams.get('metric');
	if (!metric) {
		return json({ error: 'missing metric name' }, { status: 400 });
	}

	try {
		const series = await fetchMetricSeries({
			metric,
			from: url.searchParams.get('from') ?? '-1h',
			agg: url.searchParams.get('agg') ?? 'avg',
			tags: url.searchParams.getAll('tag'),
			by: url.searchParams.getAll('by')
		});
		return json(series);
	} catch (e) {
		const message = e instanceof NinjacatError ? e.message : 'unknown error';
		return json({ error: message }, { status: 502 });
	}
};
