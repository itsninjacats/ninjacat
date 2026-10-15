import { describe, expect, it } from 'vitest';
import { isHttpError } from '@sveltejs/kit';
import { leavesNoOwner, requireInstanceAdmin } from './admin';

const locals = (role: string | null) =>
	({ user: { id: 'u', role }, tenant: null, tenants: [] }) as unknown as App.Locals;

function status(run: () => unknown): number | 'passed' {
	try {
		run();
		return 'passed';
	} catch (thrown) {
		if (isHttpError(thrown)) return thrown.status;
		throw thrown;
	}
}

describe('the instance admin check', () => {
	it('passes the instance admin only', () => {
		expect(status(() => requireInstanceAdmin(locals('admin')))).toBe('passed');
		expect(status(() => requireInstanceAdmin(locals('user,admin')))).toBe('passed');
		expect(status(() => requireInstanceAdmin(locals('user')))).toBe(403);
		expect(status(() => requireInstanceAdmin(locals(null)))).toBe(403);
		expect(status(() => requireInstanceAdmin({ tenant: null, tenants: [] }))).toBe(403);
	});
});

describe('the last owner of a tenant', () => {
	const owner = { id: 'a', role: 'owner' };
	const admin = { id: 'b', role: 'admin' };
	const secondOwner = { id: 'c', role: 'owner' };

	it('cannot be removed or demoted', () => {
		expect(leavesNoOwner([owner, admin], 'a', null)).toBe(true);
		expect(leavesNoOwner([owner, admin], 'a', 'member')).toBe(true);
	});

	it('can go once another member is an owner', () => {
		expect(leavesNoOwner([owner, admin, secondOwner], 'a', null)).toBe(false);
		expect(leavesNoOwner([owner, { id: 'c', role: 'member,owner' }], 'a', 'admin')).toBe(false);
	});

	it('does not hold back changes to anyone else', () => {
		expect(leavesNoOwner([owner, admin], 'b', null)).toBe(false);
		expect(leavesNoOwner([owner, admin], 'a', 'owner')).toBe(false);
		// A tenant without an owner already: its members can still be removed.
		expect(leavesNoOwner([admin], 'b', null)).toBe(false);
		expect(leavesNoOwner([owner], 'missing', null)).toBe(false);
	});
});
