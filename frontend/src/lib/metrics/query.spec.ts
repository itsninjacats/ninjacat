import { describe, expect, it } from 'vitest';
import {
	builderText,
	defaultView,
	emptyFormula,
	emptyQuery,
	filterText,
	nextQueryName,
	rangeMs,
	readView,
	requestOf,
	writeView
} from './query';

describe('filterText', () => {
	it('is * with no filters', () => {
		expect(filterText([])).toBe('*');
	});

	it('uses the comma form while every key has one value', () => {
		expect(
			filterText([
				{ key: 'env', value: 'prod' },
				{ key: 'service', value: 'api' }
			])
		).toBe('env:prod,service:api');
	});

	it('switches to the boolean form once a key has two values', () => {
		expect(
			filterText([
				{ key: 'env', value: 'prod' },
				{ key: 'env', value: 'staging' },
				{ key: 'service', value: 'api' }
			])
		).toBe('(env:prod OR env:staging) AND service:api');
	});
});

describe('builderText', () => {
	it('is empty until a metric is picked', () => {
		expect(builderText(emptyQuery('a').builder)).toBe('');
	});

	it('writes aggregator, metric, filter and grouping', () => {
		expect(
			builderText({
				metric: 'system.cpu.user',
				agg: 'max',
				filters: [{ key: 'env', value: 'prod' }],
				groupBy: ['host', 'env']
			})
		).toBe('max:system.cpu.user{env:prod} by {host,env}');
	});
});

describe('nextQueryName', () => {
	it('takes the first free letter', () => {
		expect(nextQueryName([])).toBe('a');
		expect(nextQueryName(['a', 'c'])).toBe('b');
	});

	it('goes past z', () => {
		const alphabet = [...'abcdefghijklmnopqrstuvwxyz'];
		expect(nextQueryName(alphabet)).toBe('aa');
	});
});

describe('the view in the URL', () => {
	it('round-trips', () => {
		const view = defaultView();
		view.display = 'toplist';
		view.formulas = [{ ...emptyFormula('a / b'), functions: [{ name: 'abs', args: [] }] }];
		view.queries = [
			emptyQuery('a', 'x'),
			{ ...emptyQuery('b'), mode: 'text', text: 'sum:y{*}', hidden: true }
		];
		expect(readView(new URLSearchParams(writeView(view)))).toEqual(view);
	});

	it('opens the old explorer links as a first query', () => {
		const view = readView(new URLSearchParams('metric=cpu&agg=max&tag=env:prod&by=host&from=-6h'));
		expect(view.range).toBe('-6h');
		expect(builderText(view.queries[0].builder)).toBe('max:cpu{env:prod} by {host}');
	});

	it('reads formulas saved as plain strings', () => {
		const view = readView(new URLSearchParams(`view=${JSON.stringify({ formulas: ['a * 2'] })}`));
		expect(view.formulas).toEqual([emptyFormula('a * 2')]);
	});

	it('falls back to the default view on garbage', () => {
		expect(readView(new URLSearchParams('view=not-json'))).toEqual(defaultView());
	});

	it('names unnamed queries', () => {
		const view = readView(new URLSearchParams(`view=${JSON.stringify({ queries: [{}, {}] })}`));
		expect(view.queries.map((q) => q.name)).toEqual(['a', 'b']);
	});
});

describe('requestOf', () => {
	it('sends every complete query, draws the visible ones and the formulas', () => {
		const view = defaultView();
		view.queries = [
			{
				...emptyQuery('a', 'x'),
				functions: [
					{ name: 'rollup', args: ['sum', '60'] },
					{ name: 'anomalies', args: ["'basic'", '2'] }
				]
			},
			{ ...emptyQuery('b', 'y'), hidden: true },
			emptyQuery('c')
		];
		view.formulas = [
			emptyFormula('a / b'),
			{ ...emptyFormula('b'), hidden: true },
			emptyFormula(' ')
		];

		expect(requestOf(view)).toEqual({
			queries: [
				{ name: 'a', query: 'avg:x{*}.rollup(sum, 60)' },
				{ name: 'b', query: 'avg:y{*}' }
			],
			outputs: [
				{ label: "anomalies(a, 'basic', 2)", formula: "anomalies(a, 'basic', 2)" },
				{ label: 'a / b', formula: 'a / b' }
			]
		});
	});
});

describe('aliases', () => {
	it('name the output instead of the formula', () => {
		const view = defaultView();
		view.queries = [{ ...emptyQuery('a', 'x'), alias: 'cpu' }];
		view.formulas = [{ ...emptyFormula('a * 100'), alias: ' percent ' }];
		expect(requestOf(view).outputs).toEqual([
			{ label: 'cpu', formula: 'a' },
			{ label: 'percent', formula: 'a * 100' }
		]);
	});
});

describe('rangeMs', () => {
	it('reads minutes, hours and days', () => {
		expect(rangeMs('-15m')).toBe(900_000);
		expect(rangeMs('-6h')).toBe(21_600_000);
		expect(rangeMs('-7d')).toBe(604_800_000);
	});
});
