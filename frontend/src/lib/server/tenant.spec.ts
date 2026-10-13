import { describe, expect, it } from 'vitest';
import type { Tenant } from '../tenant';
import { activeTenant, resolveTenant } from './tenant';

const acme = { id: 'acme', name: 'Acme', role: 'owner' };
const globex = { id: 'globex', name: 'Globex', role: 'member' };

describe('the tenant a request is for', () => {
	it("is the session's active organization while the user is a member of it", () => {
		expect(activeTenant([acme, globex], 'globex')).toBe(globex);
	});

	it('is the first membership when the session has no active organization', () => {
		expect(activeTenant([acme, globex], null)).toBe(acme);
		expect(activeTenant([acme, globex], undefined)).toBe(acme);
	});

	// Removing a member leaves the active organization in their other sessions.
	it('is never a tenant the user was removed from', () => {
		expect(activeTenant([globex], 'acme')).toBe(globex);
		expect(activeTenant([], 'acme')).toBeNull();
	});

	it('is none, never `default`, for a member of nothing', () => {
		expect(activeTenant([], null)).toBeNull();
		expect(activeTenant([], 'default')).toBeNull();
	});
});

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

		const unset = store([acme]);
		expect((await resolveTenant({ userId: 'u1', activeOrganizationId: null }, unset)).tenant).toBe(
			acme
		);
		expect(unset.calls.setActive).toEqual(['acme']);
	});

	it('clears the session of a user removed from every tenant', async () => {
		const s = store([]);
		const resolved = await resolveTenant({ userId: 'u1', activeOrganizationId: 'acme' }, s);
		expect(resolved).toEqual({ tenants: [], tenant: null });
		expect(s.calls.setActive).toEqual([null]);
	});

	it('writes nothing for a member of nothing whose session names nothing', async () => {
		const s = store([]);
		await resolveTenant({ userId: 'u1', activeOrganizationId: null }, s);
		expect(s.calls.setActive).toEqual([]);
	});
});
