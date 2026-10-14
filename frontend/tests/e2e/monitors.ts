import {
	defaultMonitorView,
	effectiveComparator,
	monitorOf,
	switchMethod,
	type MonitorView
} from '../../src/lib/monitors/definition';
import { emptyFormula, emptyQuery, type ExplorerQuery } from '../../src/lib/metrics/query';
import type { TenantKey } from './people';

// The monitors the view tests open, built with the editor's own conversion so
// they are what the editor saves. Their ids are fixed, far above what the
// development database hands out, so the routes can name them.

function query(name: string, metric: string, groupBy: string[]): ExplorerQuery {
	const q = emptyQuery(name, metric);
	q.builder.filters = [
		{ key: 'kube_cluster_name', value: 'e2e-production-eu-west-1-primary-cluster' },
		{ key: 'kube_namespace', value: 'e2e-payments-and-billing-reconciliation' }
	];
	q.builder.groupBy = groupBy;
	return q;
}

function view(change: (v: MonitorView) => void): MonitorView {
	const v = defaultMonitorView();
	change(v);
	return v;
}

const threshold = view((v) => {
	v.name =
		'e2e-a Payments reconciliation workers are running out of replicas in the primary production cluster (EU West)';
	v.queries = [
		query(
			'a',
			'e2e.kubernetes_state.deployment.replicas_unavailable_because_of_a_very_long_metric_name',
			['kube_deployment', 'kube_namespace']
		)
	];
	v.window = 'last_15m';
	v.critical = 3;
	v.warning = 2;
	v.criticalRecovery = 1;
	v.missing = 'notify';
	v.noDataTimeframe = 30;
	v.evaluationDelay = 60;
	v.message =
		'{{#is_alert}}e2e {{kube_deployment.name}} in {{kube_namespace.name}} has {{value}} replicas unavailable.{{/is_alert}} @e2e-payments-oncall-rotation-with-a-long-handle@example-corporation-international.test';
	v.tags = [
		'team:e2e-payments-and-billing-reconciliation',
		'service:e2e-payments-reconciliation-worker-with-a-very-long-service-name',
		'env:production',
		'region:eu-west-1',
		'kube_cluster_name:e2e-production-eu-west-1-primary-cluster',
		'tier:1',
		'owner:platform',
		'cost-centre:e2e-0001-observability-and-reliability-engineering',
		'runbook:https://wiki.example-corporation.internal/runbooks/e2e/payments/replicas-unavailable',
		'pagerduty',
		'slo:availability',
		'e2e_custom_tag_key_set_by_the_platform_team_for_cost_allocation_reports:e2e-eu-west-1'
	];
	v.priority = 1;
	v.renotifyInterval = 60;
	v.escalationMessage = 'e2e still unavailable after an hour, escalating to the platform team';
	v.otherOptions = { notify_by: ['kube_deployment'], renotify_statuses: ['alert'] };
});

const anomaly = view((v) => {
	v.name =
		'e2e-a-error-rate-of-the-checkout-api-compared-with-its-weekly-pattern-without-any-spaces-at-all';
	v.queries = [
		query('a', 'e2e.trace.http.request.errors', ['service']),
		query('b', 'e2e.trace.http.request.hits', ['service'])
	];
	v.formulas = [emptyFormula('a / b * 100')];
	switchMethod(v, 'anomaly');
	v.anomaly.algorithm = 'agile';
	v.anomaly.direction = 'above';
	v.anomaly.interval = 60;
	v.anomaly.timezone = 'Europe/Warsaw';
	v.criticalRecovery = 0;
	v.tags = ['team:e2e-checkout', 'env:production', 'service:e2e-checkout-api'];
	v.priority = 3;
});

const forecast = view((v) => {
	v.name = 'e2e-a Disk space on the database hosts will run out within a week';
	v.queries = [query('a', 'e2e.system.disk.in_use', ['host', 'device'])];
	v.comparator = '>=';
	switchMethod(v, 'forecast');
	v.forecast.algorithm = 'seasonal';
	v.forecast.seasonality = 'daily';
	v.critical = 0.9;
	v.warning = 0.8;
	v.tags = ['team:e2e-databases'];
});

const other = view((v) => {
	v.name = 'e2e-b Billing service workers on the bare-metal fleet stopped reporting';
	v.queries = [query('a', 'e2e.billing.worker.heartbeat', ['host'])];
	v.comparator = '<';
	v.critical = 1;
	v.missing = 'show_and_notify_no_data';
	v.tags = ['team:e2e-billing'];
});

export type E2eMonitor = {
	id: number;
	tenant: TenantKey;
	view: MonitorView;
	/**
	 * State.queryHash of the monitor's query (api/src/Engine/Monitor/State.fs).
	 * The seeded transitions carry it: under another hash the monitor service
	 * would take them for a query change and close the groups.
	 */
	queryHash: string;
	/** How many groups the seed gives it a history for. */
	groups: number;
};

export const monitors = {
	threshold: {
		id: 990_000_001,
		tenant: 'a',
		view: threshold,
		queryHash: '725dfdd3809a012af1ab808bc22a28732008fb5e37b95155427285902a511285',
		groups: 40
	},
	anomaly: {
		id: 990_000_002,
		tenant: 'a',
		view: anomaly,
		queryHash: '6905076b5c3c828bb3a8741ed7ce4d75a76001eca4a90dca9855ad3836636bb1',
		groups: 3
	},
	forecast: {
		id: 990_000_003,
		tenant: 'a',
		view: forecast,
		queryHash: '90aa2613881e2864010b956398258c41821ace6602e106cd6e381e3cb38d6218',
		groups: 0
	},
	other: {
		id: 990_000_004,
		tenant: 'b',
		view: other,
		queryHash: '9ea56d63cdd0b2a494031dff1f623bd6b142935e336a22ff84dc4c084c901b33',
		groups: 0
	}
} satisfies Record<string, E2eMonitor>;

/** The row the seed writes for a monitor. */
export function definitionOf(monitor: E2eMonitor) {
	return { ...monitorOf(monitor.view), editor: monitor.view };
}

// ---- what the monitors went through, as the monitor service would store it ----

type State = 'OK' | 'Warn' | 'Alert' | 'No Data';

const groupTags: Record<keyof typeof monitors, (i: number) => string[]> = {
	threshold: (i) => [
		i % 7 === 0
			? `kube_deployment:e2e-payments-reconciliation-worker-with-a-very-long-deployment-name-${i}`
			: `kube_deployment:e2e-payments-worker-${i}`,
		'kube_namespace:e2e-payments-and-billing-reconciliation'
	],
	anomaly: (i) => [
		['service:e2e-checkout-api', 'service:e2e-checkout-api-canary', 'service:e2e-checkout-web'][i]
	],
	forecast: () => [],
	other: () => []
};

// A few groups in each state; the rest OK.
const stateOf = (i: number): State => (i < 3 ? 'Alert' : i < 5 ? 'Warn' : i < 7 ? 'No Data' : 'OK');

const valueOf = (key: keyof typeof monitors, state: State) =>
	key === 'anomaly'
		? { OK: 0, Warn: 0.5, Alert: 1, 'No Data': null }[state]
		: { OK: 0.5, Warn: 2.5, Alert: 4, 'No Data': null }[state];

const HOUR = 3_600_000;

/**
 * Transitions and evaluations for each group of the monitor, ending at
 * `base`. A run within the same `base` writes the same transition keys, which
 * ClickHouse collapses.
 */
export function historyOf(key: keyof typeof monitors, tenantId: string, base: number) {
	const monitor: E2eMonitor = monitors[key];
	const m = monitorOf(monitor.view);
	const thresholds = (m.options.thresholds ?? {}) as Record<string, number | undefined>;
	const comparator = effectiveComparator(monitor.view);
	const iso = (t: number) => new Date(t).toISOString();

	const transitions: Record<string, unknown>[] = [];
	const evaluations: Record<string, unknown>[] = [];

	for (let i = 0; i < monitor.groups; i++) {
		const tags = groupTags[key](i).toSorted();
		const group = tags.join(',');
		const state = stateOf(i);
		const steps: {
			at: number;
			from: State | 'none';
			to: State;
			reason: string;
			crossed: string;
		}[] = [{ at: base - 18 * HOUR, from: 'none', to: 'OK', reason: 'new', crossed: 'none' }];
		if (i % 5 === 4) {
			steps.push(
				{ at: base - 10 * HOUR, from: 'OK', to: 'Alert', reason: 'threshold', crossed: 'critical' },
				{
					at: base - 8 * HOUR,
					from: 'Alert',
					to: 'OK',
					reason: 'recovery',
					crossed: 'critical_recovery'
				}
			);
		}
		if (state === 'Alert') {
			steps.push(
				{ at: base - 3 * HOUR, from: 'OK', to: 'Warn', reason: 'threshold', crossed: 'warning' },
				{ at: base - 2 * HOUR, from: 'Warn', to: 'Alert', reason: 'threshold', crossed: 'critical' }
			);
		} else if (state === 'Warn') {
			steps.push({
				at: base - 2 * HOUR,
				from: 'OK',
				to: 'Warn',
				reason: 'threshold',
				crossed: 'warning'
			});
		} else if (state === 'No Data') {
			steps.push({
				at: base - 1.5 * HOUR,
				from: 'OK',
				to: 'No Data',
				reason: 'no_data',
				crossed: 'none'
			});
		}

		let firstTriggered: number | null = null;
		let lastTriggered: number | null = null;
		for (const step of steps) {
			if (step.to === 'Alert' || step.to === 'Warn') {
				firstTriggered ??= step.at;
				lastTriggered = step.at;
			} else if (step.to === 'OK') {
				firstTriggered = null;
			}
			transitions.push({
				tenant_id: tenantId,
				monitor_id: monitor.id,
				monitor_version: 1,
				query_hash: monitor.queryHash,
				monitor_kind: 'metric',
				group_key: group,
				group_tags: tags,
				at: iso(step.at),
				from_state: step.from,
				to_state: step.to,
				reason: step.reason,
				actor: null,
				value: valueOf(key, step.to),
				crossed: step.crossed,
				comparator,
				critical: thresholds.critical ?? null,
				warning: thresholds.warning ?? null,
				critical_recovery: thresholds.critical_recovery ?? null,
				warning_recovery: thresholds.warning_recovery ?? null,
				window_from: iso(step.at - 15 * 60_000),
				window_to: iso(step.at),
				first_triggered_at: firstTriggered === null ? null : iso(firstTriggered),
				last_triggered_at: lastTriggered === null ? null : iso(lastTriggered)
			});
		}

		// One evaluation every 15 minutes, in the state each step left.
		for (let at = base - 18 * HOUR; at <= base; at += 15 * 60_000) {
			const current = steps.filter((s) => s.at <= at).at(-1)!.to;
			const value = valueOf(key, current);
			evaluations.push({
				tenant_id: tenantId,
				monitor_id: monitor.id,
				group_key: group,
				at: iso(at),
				value: value === null ? null : value + (i % 10) * 0.01,
				state: current,
				outcome: 'evaluated',
				detail: ''
			});
		}
	}
	return { transitions, evaluations };
}
