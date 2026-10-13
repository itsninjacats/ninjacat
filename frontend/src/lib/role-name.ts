import { m } from '$lib/paraglide/messages';

const roleNames: Record<string, () => string> = {
	owner: m.tenant_role_owner,
	admin: m.tenant_role_admin,
	member: m.tenant_role_member
};

export function roleName(role: string): string {
	return roleNames[role]?.() ?? role;
}
