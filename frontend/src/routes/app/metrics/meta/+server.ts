import { json } from '@sveltejs/kit';
import {
	fetchMetricNames,
	fetchMetricTagKeys,
	fetchMetricTagValues,
	NinjacatError
} from '$lib/server/ninjacat';
import type { RequestHandler } from './$types';

// Lookup proxy for the explorer's pickers: metric names, tag keys and tag
// values. Same reasoning as data/+server.ts: the browser asks this route,
// this route asks the Go panel API, and :8081 stays internal.
//
// The session check is repeated on purpose — +layout.server.ts guards the
// /app branch for pages, but its load never runs for a +server.ts route.
export const GET: RequestHandler = async ({ url, locals }) => {
	if (!locals.user) {
		return json({ error: 'unauthorized' }, { status: 401 });
	}

	const what = url.searchParams.get('what');
	const metric = url.searchParams.get('metric') ?? '';
	const search = url.searchParams.get('search') ?? '';

	try {
		switch (what) {
			case 'names':
				return json({ names: await fetchMetricNames(search, 50) });
			case 'tags':
				if (!metric) return json({ error: 'missing metric name' }, { status: 400 });
				return json({ keys: await fetchMetricTagKeys(metric) });
			case 'values': {
				const key = url.searchParams.get('key');
				if (!metric || !key) {
					return json({ error: 'missing metric name or tag key' }, { status: 400 });
				}
				return json({ values: await fetchMetricTagValues(metric, key, search, 100) });
			}
			default:
				return json({ error: 'unknown lookup' }, { status: 400 });
		}
	} catch (e) {
		const message = e instanceof NinjacatError ? e.message : 'unknown error';
		return json({ error: message }, { status: 502 });
	}
};
