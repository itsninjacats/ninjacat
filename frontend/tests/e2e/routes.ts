import { tenants, type PersonKey } from './people';

// Every page of the panel, as the person who sees the most of it. A new page is
// one more entry; `name` is the screenshot's folder.
export type ViewRoute = { name: string; path: string; as: PersonKey | null };

const longTag =
	'kube_deployment:e2e-payments-reconciliation-worker-with-a-very-long-deployment-name';
const longTagKey = 'e2e_custom_tag_key_set_by_the_platform_team_for_cost_allocation_reports';
const longMetric =
	'e2e.kubernetes_state.deployment.replicas_unavailable_because_of_a_very_long_metric_name';

const metricsView = {
	range: '-1h',
	display: 'line',
	aggregator: 'avg',
	queries: [
		{
			name: 'a',
			mode: 'builder',
			builder: {
				metric: longMetric,
				agg: 'avg',
				filters: [
					{ key: 'kube_cluster_name', value: 'e2e-production-eu-west-1-primary-cluster' },
					{ key: 'kube_namespace', value: 'e2e-payments-and-billing-reconciliation' }
				],
				groupBy: ['kube_deployment', 'kube_namespace', 'availability-zone']
			},
			text: '',
			hidden: false,
			functions: [],
			alias: ''
		}
	],
	formulas: [{ text: 'a / 1024 / 1024 * 100', hidden: false, functions: [], alias: '' }]
};

export const routes: ViewRoute[] = [
	{ name: 'login', path: '/login', as: null },
	{ name: 'home', path: '/', as: 'memberA' },
	{ name: 'no-tenant', path: '/no-tenant', as: 'noTenant' },
	{ name: 'overview', path: '/app', as: 'longEmail' },
	{ name: 'metrics', path: '/app/metrics', as: 'longEmail' },
	{
		name: 'metrics-long-query',
		path: `/app/metrics?${new URLSearchParams({ view: JSON.stringify(metricsView) })}`,
		as: 'longEmail'
	},
	{ name: 'logs', path: '/app/logs', as: 'longEmail' },
	{
		name: 'logs-long-filters',
		path: `/app/logs?${new URLSearchParams([
			['q', 'e2e connection reset by peer while talking to the payments reconciliation service'],
			['tag', longTag],
			['tag', 'kube_namespace:e2e-payments-and-billing-reconciliation'],
			['tag', `${longTagKey}:e2e-eu-west-1`],
			['service', 'e2e-payments-reconciliation-worker-with-a-very-long-service-name']
		])}`,
		as: 'longEmail'
	},
	{ name: 'keys', path: '/app/settings/keys', as: 'longEmail' },
	{ name: 'admin-tenants', path: '/app/admin/tenants', as: 'admin' },
	{ name: 'admin-tenant-a', path: `/app/admin/tenants/${tenants.a.id}`, as: 'admin' },
	{ name: 'admin-tenant-b', path: `/app/admin/tenants/${tenants.b.id}`, as: 'admin' },
	{ name: 'admin-users', path: '/app/admin/users', as: 'admin' }
];
