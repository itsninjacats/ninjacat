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

// When the request falls back to another tenant, the session is moved there
// too: Better Auth's calls that default to the session's active organization
// would otherwise act on a tenant the panel does not show.
export async function resolveTenant(
	session: { userId: string; activeOrganizationId?: string | null } | null,
	store: {
		memberships(userId: string): Promise<Tenant[]>;
		setActive(organizationId: string | null): Promise<unknown>;
	}
): Promise<{ tenants: Tenant[]; tenant: Tenant | null }> {
	if (!session) {
		return { tenants: [], tenant: null };
	}
	const tenants = await store.memberships(session.userId);
	// Removing a member does not clear their other sessions' active organization.
	const tenant = tenants.find((t) => t.id === session.activeOrganizationId) ?? tenants[0] ?? null;
	if ((tenant?.id ?? null) !== (session.activeOrganizationId ?? null)) {
		await store.setActive(tenant?.id ?? null);
	}
	return { tenants, tenant };
}
