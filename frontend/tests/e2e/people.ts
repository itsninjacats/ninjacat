// Who and what the view tests run against. Everything is prefixed `e2e-` so the
// seed can share a development database: it adds these rows and touches no others.

export const password = 'e2e-password-not-a-secret';

// Long on purpose: the point of these tests is what a long name does to a layout.
// One name has spaces to wrap at; the other has none, like a URL pasted as a name.
export const tenants = {
	a: {
		id: 'e2e-a',
		// Not a bare `e2e-a`, which every e2e-a… e-mail and key name contains: the
		// isolation tests look for the slug in what another tenant is shown.
		slug: 'e2e-a-northwind',
		name: 'e2e-a Northwind Logistics International Holdings, Production Observability Workspace (EU West)'
	},
	b: {
		id: 'e2e-b',
		slug: 'e2e-b-telemetry-example-corporation-internal-production-eu-west-1',
		name: 'e2e-b-https://telemetry.example-corporation.internal/tenants/production/eu-west-1/observability'
	}
} as const;

export type TenantKey = keyof typeof tenants;

// At most 100 characters, the limit the keys form enforces.
export const keyNames: Record<TenantKey, string[]> = {
	a: [
		'e2e-a production Kubernetes cluster in Frankfurt, every node agent and the cluster agent',
		'e2e-a-staging-cluster-node-agents-without-any-spaces-in-the-name-at-all-0123456789'
	],
	b: [
		'e2e-b billing service workers running on the old bare-metal fleet in the Warsaw data centre',
		'e2e-b-ci-runners-ephemeral-build-machines-with-a-name-nobody-will-ever-read-to-the-end'
	]
};

type Person = {
	email: string;
	name: string;
	admin: boolean;
	// Joined in this order: the first is the tenant a new session starts in.
	tenants: { tenant: TenantKey; role: 'owner' | 'admin' | 'member' }[];
	// Better Auth counts sign-ins per client address (5 a minute), and believes a
	// single-address X-Forwarded-For. Each person signing in from an address of
	// their own (TEST-NET-1) keeps the test run from tripping the limiter.
	address: string;
};

export const people = {
	admin: {
		email: 'e2e-admin@ninjacat.test',
		name: 'e2e Instance Admin',
		admin: true,
		tenants: [
			{ tenant: 'a', role: 'owner' },
			{ tenant: 'b', role: 'owner' }
		],
		address: '192.0.2.10'
	},
	memberA: {
		email: 'e2e-member-a@ninjacat.test',
		name: 'e2e Member of A',
		admin: false,
		tenants: [{ tenant: 'a', role: 'member' }],
		address: '192.0.2.11'
	},
	memberB: {
		email: 'e2e-member-b@ninjacat.test',
		name: 'e2e Member of B',
		admin: false,
		tenants: [{ tenant: 'b', role: 'member' }],
		address: '192.0.2.12'
	},
	// The name is the e-mail, so the sidebar shows it twice and neither can wrap.
	longEmail: {
		email:
			'e2e-maximilian.alexander.wolfeschlegelsteinhausen-bergerdorff@observability.example-corporation-international.test',
		name: 'e2e-maximilian.alexander.wolfeschlegelsteinhausen-bergerdorff@observability.example-corporation-international.test',
		admin: false,
		tenants: [{ tenant: 'a', role: 'admin' }],
		address: '192.0.2.13'
	},
	// No hyphen after the prefix: nothing to wrap at but the prefix's own.
	noTenant: {
		email:
			'e2e-somebody_who_was_removed_from_every_single_tenant@examplecorporationinternational.test',
		name: 'e2e Nobody',
		admin: false,
		tenants: [],
		address: '192.0.2.14'
	},
	// Signs in afresh inside the switching test, so other tests never see the
	// tenant it switches to.
	switcher: {
		email: 'e2e-switcher@ninjacat.test',
		name: 'e2e Member of A and B',
		admin: false,
		tenants: [
			{ tenant: 'a', role: 'member' },
			{ tenant: 'b', role: 'member' }
		],
		address: '192.0.2.15'
	}
} satisfies Record<string, Person>;

export type PersonKey = keyof typeof people;

// The storage state of a signed-in person; gitignored.
export function stateFile(person: PersonKey): string {
	return `playwright/.auth/${person}.json`;
}
