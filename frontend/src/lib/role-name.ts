import { m } from '$lib/paraglide/messages';

export function roleName(role: string): string {
	switch (role) {
		case 'owner':
			return m.tenant_role_owner();
		case 'admin':
			return m.tenant_role_admin();
		case 'member':
			return m.tenant_role_member();
		default:
			return role;
	}
}
