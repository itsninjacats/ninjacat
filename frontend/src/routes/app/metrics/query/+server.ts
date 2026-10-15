import { json } from '@sveltejs/kit';
import {
	NinjacatError,
	QueryError,
	queryScalar,
	queryTimeseries,
	type NamedQuery
} from '$lib/server/ninjacat';
import type { RequestHandler } from './$types';

// Proxy between the explorer and the F# query API, for the same reason as
// data/+server.ts: the browser never learns :8081 exists. The session check is
// repeated on purpose — the /app layout guard does not run for +server.ts.

const AGGREGATORS = ['avg', 'min', 'max', 'sum', 'last'];

type Body = {
	kind?: unknown;
	from?: unknown;
	to?: unknown;
	queries?: unknown;
	formulas?: unknown;
	aggregator?: unknown;
};

function namedQueries(value: unknown): NamedQuery[] | null {
	if (!Array.isArray(value)) return null;
	const queries = value.filter(
		(q): q is NamedQuery => typeof q?.name === 'string' && typeof q?.query === 'string'
	);
	return queries.length === value.length ? queries : null;
}

export const POST: RequestHandler = async ({ request, locals }) => {
	if (!locals.user) {
		return json({ errors: ['unauthorized'] }, { status: 401 });
	}

	const body: Body = await request.json().catch(() => ({}));
	const queries = namedQueries(body.queries);
	const formulas = Array.isArray(body.formulas) ? body.formulas.map(String) : [];

	if (typeof body.from !== 'number' || typeof body.to !== 'number' || !queries) {
		return json({ errors: ['expected from, to and queries'] }, { status: 400 });
	}

	try {
		if (body.kind === 'scalar') {
			const aggregator = AGGREGATORS.includes(String(body.aggregator))
				? String(body.aggregator)
				: 'avg';
			return json(
				await queryScalar({ from: body.from, to: body.to, queries, formulas, aggregator })
			);
		}
		return json({
			series: await queryTimeseries({ from: body.from, to: body.to, queries, formulas })
		});
	} catch (e) {
		if (e instanceof QueryError) return json({ errors: e.problems }, { status: 400 });
		const message = e instanceof NinjacatError ? e.message : 'unknown error';
		return json({ errors: [message] }, { status: 502 });
	}
};
