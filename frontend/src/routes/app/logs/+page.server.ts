import {
	fetchLogFacets,
	searchLogs,
	NinjacatError,
	type LogEntry,
	type LogFacets
} from '$lib/server/ninjacat';
import type { PageServerLoad } from './$types';

const EMPTY_FACETS: LogFacets = { services: [], hosts: [], statuses: [] };

function errorMessage(e: unknown): string {
	return e instanceof NinjacatError ? e.message : 'cannot reach the server';
}

// The whole explorer is URL-driven: every control on the page navigates to a
// new query string and this load re-runs with it, so any view is reproducible
// by pasting the address.
//
// Search and facets are fetched in parallel and fail independently — a facet
// query going wrong must not blank the result list, and vice versa. Errors
// come back as strings instead of thrown 500s so the page can explain itself.
export const load: PageServerLoad = async ({ url }) => {
	const params = {
		q: url.searchParams.get('q') ?? '',
		service: url.searchParams.get('service') ?? '',
		host: url.searchParams.get('host') ?? '',
		status: url.searchParams.get('status') ?? '',
		tags: url.searchParams.getAll('tag'),
		from: url.searchParams.get('from') ?? '-1h'
	};

	const [search, facets] = await Promise.allSettled([
		searchLogs({ ...params, limit: 200 }),
		fetchLogFacets(params.from)
	]);

	return {
		params,
		logs: search.status === 'fulfilled' ? search.value.logs : ([] as LogEntry[]),
		count: search.status === 'fulfilled' ? search.value.count : 0,
		searchError: search.status === 'rejected' ? errorMessage(search.reason) : null,
		facets: facets.status === 'fulfilled' ? facets.value : EMPTY_FACETS,
		facetsError: facets.status === 'rejected' ? errorMessage(facets.reason) : null
	};
};
