import { asc, eq } from 'drizzle-orm';
import type { Tenant } from '../tenant';
import type { Database } from './auth-options';
import { member, organization } from './db/schema';

// Oldest first, and always in the same order: the first is the tenant a new
// session starts in, and the one a request falls back to.
export function memberships(db: Database, userId: string): Promise<Tenant[]> {
	return db
		.select({ id: organization.id, name: organization.name, role: member.role })
		.from(member)
		.innerJoin(organization, eq(organization.id, member.organizationId))
		.where(eq(member.userId, userId))
		.orderBy(asc(member.createdAt), asc(member.id));
}

// The session's active organization counts only while the user is still a
// member: removing a member does not clear it in their other sessions. Without
// one, the first membership, as a new session gets; never `default`.
export function activeTenant(tenants: Tenant[], activeOrganizationId: string | null | undefined) {
	return tenants.find((tenant) => tenant.id === activeOrganizationId) ?? tenants[0] ?? null;
}

export type TenantStore = {
	memberships(userId: string): Promise<Tenant[]>;
	setActive(organizationId: string | null): Promise<unknown>;
};

// When the request falls back to another tenant, the session is moved there
// too: Better Auth's calls that default to the session's active organization
// would otherwise act on a tenant the panel does not show.
export async function resolveTenant(
	session: { userId: string; activeOrganizationId?: string | null } | null,
	store: TenantStore
): Promise<{ tenants: Tenant[]; tenant: Tenant | null }> {
	if (!session) {
		return { tenants: [], tenant: null };
	}
	const tenants = await store.memberships(session.userId);
	const tenant = activeTenant(tenants, session.activeOrganizationId);
	if ((tenant?.id ?? null) !== (session.activeOrganizationId ?? null)) {
		await store.setActive(tenant?.id ?? null);
	}
	return { tenants, tenant };
}
