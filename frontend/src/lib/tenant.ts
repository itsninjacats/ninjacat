// A tenant as the panel sees it: the organization, and the user's role in it.
export type Tenant = { id: string; name: string; role: string };

// The organization plugin's own roles, unchanged.
export const tenantRoles = ['owner', 'admin', 'member'] as const;
export type TenantRole = (typeof tenantRoles)[number];

export function isTenantRole(role: unknown): role is TenantRole {
	return tenantRoles.some((known) => known === role);
}
