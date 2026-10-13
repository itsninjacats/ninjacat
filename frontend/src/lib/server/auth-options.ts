import type { BetterAuthOptions, BetterAuthPlugin } from 'better-auth';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { admin, organization } from 'better-auth/plugins';
import { defaultAc, userAc } from 'better-auth/plugins/admin/access';
import type { PostgresJsDatabase } from 'drizzle-orm/postgres-js';
import * as schema from './db/schema';
import { memberships } from './tenant';

// The options shared by the panel (auth.ts) and scripts/create-user.ts. No
// $env or $app here: the script runs as a plain process, outside SvelteKit.

export type Database = PostgresJsDatabase<typeof schema>;

// $env/dynamic/private in the panel, process.env in the script.
export type AuthEnv = Record<string, string | undefined>;

// The admin plugin's own admin role, less impersonation: an instance admin
// manages accounts, but never acts as someone else.
const instanceAdmin = defaultAc.newRole({
	user: [
		'create',
		'list',
		'set-role',
		'ban',
		'delete',
		'set-password',
		'set-email',
		'get',
		'update'
	],
	session: ['list', 'revoke', 'delete']
});

// The organization plugin checks that a user is not a member yet and then
// inserts, with nothing in between: two adds at once made two rows, and
// removing one left the user in. Its options take no index, so this plugin
// adds one to its table; the CLI writes it into auth.schema.ts.
const oneMembershipPerTenant = {
	id: 'one-membership-per-tenant',
	schema: {
		member: {
			fields: {},
			indexes: [{ fields: ['organizationId', 'userId'], unique: true }]
		}
	}
} satisfies BetterAuthPlugin;

// The admin plugin keeps several roles in one comma-separated column.
export function isInstanceAdmin(role: string | null | undefined): boolean {
	return (role ?? '').split(',').includes('admin');
}

export function authOptions(db: Database, env: AuthEnv) {
	return {
		baseURL: env.ORIGIN,
		secret: env.BETTER_AUTH_SECRET,
		database: drizzleAdapter(db, { provider: 'pg' }),
		emailAndPassword: {
			enabled: true,
			// Registration is closed: an open sign-up on a panel that reads every
			// metric in the cluster is a hole. Better Auth's sign-up route itself
			// refuses, whatever the UI does. Accounts come from
			// scripts/create-user.ts and the admin pages, through the admin
			// plugin's createUser, which this flag does not stop.
			disableSignUp: true
		},
		// Sign-in attempts are counted per client address. Better Auth's limiter is
		// on only in production unless told otherwise, and its default for sign-in
		// (3 in 10 seconds) still allows eighteen guesses a minute.
		//
		// The counters live in this process's memory: enough for one panel pod.
		// With several, switch `storage` to "database" (it needs a rateLimit table).
		rateLimit: {
			enabled: true,
			customRules: { '/sign-in/email': { window: 60, max: 5 } }
		},
		// The client's address is read from X-Forwarded-For. Without this list
		// only a header holding a single address is believed; behind a proxy that
		// appends to the header nothing is, and every visitor shares one counter.
		// The same setting, with the same meaning, as the intake's.
		advanced: {
			ipAddress: {
				trustedProxies: (env.NINJACAT_TRUSTED_PROXIES ?? '')
					.split(',')
					.map((proxy) => proxy.trim())
					.filter((proxy) => proxy !== '')
			}
		},
		// Invitations need e-mail, which an on-prem install may not have; members
		// are added by the admin pages instead. The plugin has no switch for them,
		// so their routes answer 404. The server-side auth.api calls are unaffected.
		disabledPaths: [
			'/organization/invite-member',
			'/organization/accept-invitation',
			'/organization/reject-invitation',
			'/organization/cancel-invitation',
			'/organization/get-invitation',
			'/organization/list-invitations',
			'/organization/list-user-invitations'
		],
		databaseHooks: {
			session: {
				create: {
					before: async (session) => {
						const [first] = await memberships(db, session.userId);
						return { data: { activeOrganizationId: first?.id ?? null } };
					}
				}
			}
		},
		plugins: [
			admin({ roles: { admin: instanceAdmin, user: userAc } }),
			organization({
				allowUserToCreateOrganization: (user) => isInstanceAdmin(user.role),
				// Deleting a tenant would orphan its API keys and its ClickHouse rows.
				disableOrganizationDeletion: true,
				// The plugin's default of 100 is a team's size; a tenant can be a company.
				membershipLimit: 10_000
			}),
			oneMembershipPerTenant
		]
	} satisfies BetterAuthOptions;
}
