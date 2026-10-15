import { describe, expect, it } from 'vitest';
import type { Tenant } from '../tenant';
import { resolveTenant } from './tenant';

const acme = { id: 'acme', name: 'Acme', role: 'owner' };
const globex = { id: 'globex', name: 'Globex', role: 'member' };

describe('resolving the tenant in the hook', () => {
	function store(tenants: Tenant[]) {
		const calls = { memberships: [] as string[], setActive: [] as (string | null)[] };
		return {
			calls,
			memberships: async (userId: string) => {
				calls.memberships.push(userId);
				return tenants;
			},
			setActive: async (organizationId: string | null) => {
				calls.setActive.push(organizationId);
			}
		};
	}

	it('gives a visitor no tenant without asking the database', async () => {
		const s = store([acme]);
		expect(await resolveTenant(null, s)).toEqual({ tenants: [], tenant: null });
		expect(s.calls).toEqual({ memberships: [], setActive: [] });
	});

	it("reads the memberships of the session's own user and leaves a valid session alone", async () => {
		const s = store([acme, globex]);
		const resolved = await resolveTenant({ userId: 'u1', activeOrganizationId: 'globex' }, s);
		expect(resolved).toEqual({ tenants: [acme, globex], tenant: globex });
		expect(s.calls).toEqual({ memberships: ['u1'], setActive: [] });
	});

	it('moves the session to the tenant it fell back to', async () => {
		const removed = store([globex]);
		expect(
			(await resolveTenant({ userId: 'u1', activeOrganizationId: 'acme' }, removed)).tenant
		).toBe(globex);
		expect(removed.calls.setActive).toEqual(['globex']);

		const unset = store([acme, globex]);
		expect((await resolveTenant({ userId: 'u1', activeOrganizationId: null }, unset)).tenant).toBe(
			acme
		);
		expect((await resolveTenant({ userId: 'u1' }, unset)).tenant).toBe(acme);
		expect(unset.calls.setActive).toEqual(['acme', 'acme']);
	});

	it('clears the session of a user removed from every tenant', async () => {
		const s = store([]);
		const resolved = await resolveTenant({ userId: 'u1', activeOrganizationId: 'acme' }, s);
		expect(resolved).toEqual({ tenants: [], tenant: null });
		expect(s.calls.setActive).toEqual([null]);
	});

	it('gives a member of nothing no tenant, never `default`, and writes nothing', async () => {
		const s = store([]);
		expect(
			(await resolveTenant({ userId: 'u1', activeOrganizationId: null }, s)).tenant
		).toBeNull();
		expect(s.calls.setActive).toEqual([]);
		expect(
			(await resolveTenant({ userId: 'u1', activeOrganizationId: 'default' }, s)).tenant
		).toBeNull();
	});
});
