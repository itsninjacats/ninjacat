import { describe, expect, it } from 'vitest';
import { emptyFormula, emptyQuery, type ExplorerQuery } from '$lib/metrics/query';
import {
	alertZone,
	chartRequest,
	defaultMonitorView,
	editorMethod,
	expressionOf,
	fromExplorer,
	groupKeysOf,
	monitorOf,
	readMonitorView,
	switchMethod,
	unfilled,
	viewOf,
	type MonitorView
} from './definition';

function query(name: string, metric: string, groupBy: string[] = []): ExplorerQuery {
	const q = emptyQuery(name, metric);
	q.builder.groupBy = groupBy;
	return q;
}

function view(change: (v: MonitorView) => void): MonitorView {
	const v = defaultMonitorView();
	v.queries = [query('a', 'system.cpu.user', ['host'])];
	v.critical = 90;
	v.name = 'CPU';
	change(v);
	return v;
}

// What a save stores, read back the way the editor opens it.
function reopened(v: MonitorView) {
	const m = monitorOf(v);
	return viewOf({ ...m, editor: JSON.parse(JSON.stringify(v)) });
}

describe('expressionOf', () => {
	it('is the first query, with its functions around it', () => {
		const a = query('a', 'system.cpu.user', ['host']);
		a.functions = [
			{ name: 'rollup', args: ['sum', '60'] },
			{ name: 'abs', args: [] }
		];
		expect(expressionOf([a], [])).toBe('abs(avg:system.cpu.user{*} by {host}.rollup(sum, 60))');
	});

	it('writes the queries into the formula, leaving function names and quotes alone', () => {
		const a = query('a', 'system.cpu.user');
		const b = query('b', 'system.cpu.idle');
		a.functions = [{ name: 'abs', args: [] }];
		const formula = emptyFormula("a / (a + b) * 100 + top(b, 5, 'a')");
		expect(expressionOf([a, b], [formula])).toBe(
			"avg:system.cpu.user{*} / (avg:system.cpu.user{*} + avg:system.cpu.idle{*}) * 100 + top(avg:system.cpu.idle{*}, 5, 'a')"
		);
	});

	it('keeps a text row whole inside a formula', () => {
		const a = emptyQuery('a');
		a.mode = 'text';
		a.text = 'avg:x{*} - avg:y{*}';
		expect(expressionOf([a], [emptyFormula('100 - a')])).toBe('100 - (avg:x{*} - avg:y{*})');
	});

	it('does not write a query into a longer name that starts with it', () => {
		const a = query('a', 'm');
		expect(expressionOf([a], [emptyFormula('abs(a)')])).toBe('abs(avg:m{*})');
	});
});

describe('monitorOf', () => {
	it('writes a threshold monitor', () => {
		const m = monitorOf(
			view((v) => {
				v.warning = 80;
				v.criticalRecovery = 85;
			})
		);
		expect(m).toEqual({
			name: 'CPU',
			type: 'query alert',
			query: 'avg(last_5m):avg:system.cpu.user{*} by {host} > 90',
			message: '',
			tags: [],
			priority: null,
			options: { thresholds: { critical: 90, warning: 80, critical_recovery: 85 } }
		});
	});

	it('writes a change monitor', () => {
		const m = monitorOf(
			view((v) => {
				v.method = 'change';
				v.change = { measure: 'pct_change', shift: '1h_ago' };
				v.comparator = '<';
				v.critical = -50;
			})
		);
		expect(m.query).toBe('pct_change(avg(last_5m),1h_ago):avg:system.cpu.user{*} by {host} < -50');
	});

	it('writes an anomaly monitor, with its trigger and recovery windows', () => {
		const m = monitorOf(
			view((v) => {
				switchMethod(v, 'anomaly');
				v.anomaly.direction = 'above';
				v.anomaly.interval = 60;
				v.anomaly.timezone = 'Europe/Warsaw';
			})
		);
		expect(m.query).toBe(
			"avg(last_4h):anomalies(avg:system.cpu.user{*} by {host}, 'agile', 2, direction='above', alert_window='last_15m', interval=60, count_default_zero='true', seasonality='weekly', timezone='Europe/Warsaw') >= 1"
		);
		expect(m.options).toEqual({
			thresholds: { critical: 1 },
			threshold_windows: { trigger_window: 'last_15m', recovery_window: 'last_15m' }
		});
	});

	it('writes a basic anomaly monitor without seasonality', () => {
		const m = monitorOf(
			view((v) => {
				switchMethod(v, 'anomaly');
				v.anomaly.algorithm = 'basic';
			})
		);
		expect(m.query).not.toContain('seasonality');
	});

	it('writes a forecast monitor, max with >= and min with <=', () => {
		const up = monitorOf(view((v) => switchMethod(v, 'forecast')));
		expect(up.query).toBe(
			"max(next_1w):forecast(avg:system.cpu.user{*} by {host}, 'linear', 1, history='1w', model='default') >= 90"
		);
		const down = monitorOf(
			view((v) => {
				v.comparator = '<';
				switchMethod(v, 'forecast');
				v.forecast.algorithm = 'seasonal';
				v.forecast.seasonality = 'daily';
				v.critical = 5;
			})
		);
		expect(down.query).toBe(
			"min(next_1w):forecast(avg:system.cpu.user{*} by {host}, 'seasonal', 1, seasonality='daily') <= 5"
		);
	});

	it('writes an outlier monitor as > 0, with no other thresholds', () => {
		const m = monitorOf(
			view((v) => {
				switchMethod(v, 'outlier');
				v.window = 'last_30m';
				v.outlier = { algorithm: 'mad', tolerance: 3, percentage: 20 };
				v.warning = 50;
			})
		);
		expect(m.query).toBe(
			"avg(last_30m):outliers(avg:system.cpu.user{*} by {host}, 'mad', 3, 20) > 0"
		);
		expect(m.options).toEqual({ thresholds: { critical: 0 } });
	});

	it('sets either notify_no_data or on_missing_data, never both', () => {
		const notify = monitorOf(
			view((v) => {
				v.missing = 'notify';
				v.noDataTimeframe = 20;
			})
		);
		expect(notify.options).toMatchObject({ notify_no_data: true, no_data_timeframe: 20 });
		expect(notify.options).not.toHaveProperty('on_missing_data');

		const resolve = monitorOf(view((v) => (v.missing = 'resolve')));
		expect(resolve.options).toMatchObject({ on_missing_data: 'resolve' });
		expect(resolve.options).not.toHaveProperty('notify_no_data');
	});

	it('keeps options the editor has no field for', () => {
		const m = monitorOf(
			view((v) => {
				v.otherOptions = { notify_by: ['host'], renotify_statuses: ['alert'] };
				v.renotifyInterval = 60;
				v.includeTags = false;
			})
		);
		expect(m.options).toEqual({
			notify_by: ['host'],
			renotify_statuses: ['alert'],
			thresholds: { critical: 90 },
			renotify_interval: 60,
			include_tags: false
		});
	});
});

describe('viewOf', () => {
	it('reopens what was saved, for every method', () => {
		const views = [
			view((v) => {
				v.warning = 80;
				v.criticalRecovery = 70;
				v.warningRecovery = 60;
				v.missing = 'notify';
				v.noDataTimeframe = 10;
				v.evaluationDelay = 300;
				v.newGroupDelay = 120;
				v.requireFullWindow = true;
				v.timeoutH = 4;
				v.renotifyInterval = 30;
				v.escalationMessage = 'still down';
				v.notifyAudit = true;
				v.message = 'CPU is high on {{host.name}} @ops';
				v.tags = ['team:infra', 'env:prod'];
				v.priority = 2;
			}),
			view((v) => {
				v.method = 'change';
				v.change = { measure: 'change', shift: 'last_15m' };
				v.missing = 'show_and_notify_no_data';
			}),
			view((v) => {
				switchMethod(v, 'anomaly');
				v.comparator = '>=';
				v.anomaly.algorithm = 'robust';
				v.anomaly.seasonality = 'daily';
				v.anomaly.recoveryWindow = 'last_10m';
				v.criticalRecovery = 0;
			}),
			view((v) => {
				switchMethod(v, 'forecast');
				v.forecast.horizon = 'next_3mo';
				v.forecast.interval = 3600;
				v.forecast.model = 'reactive';
			}),
			view((v) => {
				switchMethod(v, 'outlier');
				v.critical = 0;
				v.comparator = '>';
			}),
			view((v) => {
				v.queries = [
					query('a', 'requests.errors', ['service']),
					query('b', 'requests.total', ['service'])
				];
				v.formulas = [emptyFormula('a / b * 100')];
			})
		];
		for (const v of views) {
			expect(reopened(v)).toEqual({ view: v, drifted: false });
		}
	});

	it('keeps the stored options the editor has no field for', () => {
		const v = view(() => {});
		const stored = monitorOf(v);
		stored.options = { ...stored.options, notify_by: ['host'] };
		expect(viewOf({ ...stored, editor: v })).toEqual({
			view: { ...v, otherOptions: { notify_by: ['host'] } },
			drifted: false
		});
	});

	it('says when the stored definition is no longer what the view saves as', () => {
		const v = view(() => {});
		const stored = { ...monitorOf(v), editor: v };
		expect(viewOf({ ...stored, query: 'avg(last_5m):avg:m{env:staging} > 90' })?.drifted).toBe(
			true
		);
		expect(
			viewOf({ ...stored, options: { thresholds: { critical: 90, warning: 1 } } })?.drifted
		).toBe(true);
	});

	it('does not open a monitor the editor did not write', () => {
		const stored = {
			name: 'from the API',
			query: 'avg(last_5m):avg:system.load.1{env:prod} by {host} >= 4',
			message: '',
			options: { thresholds: { critical: 4 } },
			tags: [],
			priority: null
		};
		expect(viewOf({ ...stored, editor: null })).toBeNull();
		expect(viewOf({ ...stored, editor: { queries: [], formulas: [] } })).toBeNull();
		expect(editorMethod({ method: 'anomaly' })).toBe('anomaly');
		expect(editorMethod({ method: 'nonsense' })).toBeNull();
	});
});

describe('readMonitorView', () => {
	it('keeps only what it knows, with the right types', () => {
		const read = readMonitorView({
			method: 'drop table',
			critical: '90',
			warning: 'eighty',
			tags: ['a', 1, null, ' b', 'b'],
			priority: 9,
			tenantId: 'someone-else',
			otherOptions: { foo: 1 }
		});
		expect(read.method).toBe('threshold');
		expect(read.critical).toBe(90);
		expect(read.warning).toBeNull();
		expect(read.tags).toEqual(['a', 'b']);
		expect(read.priority).toBeNull();
		expect(read).not.toHaveProperty('tenantId');
		expect(read.otherOptions).toEqual({});
	});

	it('reads back a view as the editor sends it', () => {
		const v = view((v) => {
			switchMethod(v, 'anomaly');
			v.priority = 3;
		});
		expect(readMonitorView(JSON.parse(JSON.stringify(v)))).toEqual(v);
	});
});

describe('groupKeysOf', () => {
	it('reads the keys of the queries the formula uses only', () => {
		const v = view((v) => {
			v.queries = [
				query('a', 'requests.errors', ['service']),
				query('b', 'requests.total', ['service', 'env']),
				query('c', 'unused', ['host'])
			];
			v.formulas = [emptyFormula('a / b')];
		});
		expect(groupKeysOf(v)).toEqual(['service', 'env']);
		v.formulas = [];
		expect(groupKeysOf(v)).toEqual(['service']);
	});
});

describe('fromExplorer', () => {
	it('takes the first visible formula with every query', () => {
		const a = query('a', 'requests.errors');
		const b = query('b', 'requests.total');
		b.hidden = true;
		const v = fromExplorer({
			range: '-1h',
			display: 'line',
			aggregator: 'avg',
			queries: [a, b],
			formulas: [{ ...emptyFormula('a / b'), alias: 'error rate' }]
		});
		expect(v.queries.map((q) => [q.name, q.hidden])).toEqual([
			['a', false],
			['b', false]
		]);
		expect(v.formulas).toEqual([emptyFormula('a / b')]);
	});

	it('takes the first visible query alone without a formula', () => {
		const a = query('a', 'hidden.one');
		a.hidden = true;
		const v = fromExplorer({
			range: '-1h',
			display: 'line',
			aggregator: 'avg',
			queries: [a, query('b', 'shown.one')],
			formulas: []
		});
		expect(v.queries).toHaveLength(1);
		expect(v.queries[0]).toMatchObject({ name: 'a', builder: { metric: 'shown.one' } });
	});
});

describe('unfilled', () => {
	it('names what is missing', () => {
		const v = defaultMonitorView();
		v.queries.push(query('b', 'm'));
		expect(unfilled(v)).toEqual(['metric', 'formula', 'critical', 'name']);
		expect(unfilled(view(() => {}))).toEqual([]);
	});

	it("asks for the detection method's parameters", () => {
		const anomaly = view((v) => switchMethod(v, 'anomaly'));
		anomaly.anomaly.bounds = null;
		expect(unfilled(anomaly)).toEqual(['parameters']);
		const outlier = view((v) => switchMethod(v, 'outlier'));
		outlier.outlier.algorithm = 'mad';
		outlier.outlier.percentage = null;
		expect(unfilled(outlier)).toEqual(['parameters']);
	});
});

describe('chartRequest', () => {
	it('draws a change per bucket against the shifted series', () => {
		const v = view((v) => {
			v.method = 'change';
			v.change = { measure: 'pct_change', shift: '5m_ago' };
		});
		expect(chartRequest(v)).toEqual({
			queries: [{ name: 'a', query: 'avg:system.cpu.user{*} by {host}' }],
			formulas: ['((a) - timeshift(a, -300)) / timeshift(a, -300) * 100']
		});
	});

	it('draws the series and its outliers', () => {
		const v = view((v) => switchMethod(v, 'outlier'));
		expect(chartRequest(v).formulas).toEqual(['a', "outliers(a, 'dbscan', 3)"]);
	});

	it('asks a forecast for the horizon in days', () => {
		const v = view((v) => {
			switchMethod(v, 'forecast');
			v.forecast.horizon = 'next_1mo';
		});
		expect(chartRequest(v).formulas).toEqual([
			"forecast(a, 'linear', 1, model='default', history='1w', horizon='31d')"
		]);
	});
});

describe('alertZone', () => {
	it('lies on the alerting side of critical', () => {
		expect(alertZone('>', 90)).toEqual({ from: 90, to: null });
		expect(alertZone('<=', 5)).toEqual({ from: null, to: 5 });
		expect(alertZone('==', 1)).toBeNull();
		expect(alertZone('>', null)).toBeNull();
	});
});
