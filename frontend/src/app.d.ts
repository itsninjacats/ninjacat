import type { auth } from '$lib/server/auth';
import type { Tenant } from '$lib/tenant';

// With the plugins' fields: user.role, session.activeOrganizationId.
type AuthSession = typeof auth.$Infer.Session;

// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		interface Locals {
			user?: AuthSession['user'];
			session?: AuthSession['session'];
			// Every tenant the user is a member of, and the one this request is
			// for. Set by hooks.server.ts; null without a membership.
			tenants: Tenant[];
			tenant: Tenant | null;
		}

		// interface Error {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
