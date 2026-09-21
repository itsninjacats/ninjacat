import { fetchMetricNames, NinjacatError } from '$lib/server/ninjacat';
import type { PageServerLoad } from './$types';

// The metric list is the only thing this page needs before it can render.
// Points arrive afterwards through data/+server.ts, so switching metric or
// range never reloads the page.
//
// A Go server that is down must not blank the screen: the load returns an
// error string and the page explains itself instead of throwing a 500.
export const load: PageServerLoad = async () => {
	try {
		return { metrics: await fetchMetricNames(), error: null };
	} catch (e) {
		return {
			metrics: [] as string[],
			error: e instanceof NinjacatError ? e.message : 'nie mogę połączyć się z serwerem'
		};
	}
};
