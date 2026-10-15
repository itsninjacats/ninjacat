import { describe, expect, it } from 'vitest';
import { betterAuth } from 'better-auth/minimal';
import { drizzle } from 'drizzle-orm/postgres-js';
import { authOptions, isInstanceAdmin } from './auth-options';
import * as schema from './db/schema';

// A database with no connection: disabled paths are refused before any handler
// runs, and a permission asked for a role is decided in memory.
const auth = betterAuth(
	authOptions(drizzle.mock({ schema }), {
		ORIGIN: 'http://panel.test',
		BETTER_AUTH_SECRET: 'a-test-secret-that-is-long-enough-to-pass'
	})
);

describe('auth options', () => {
	// A Better Auth upgrade that adds an invitation route must not open it.
	it('switches off every invitation route the organization plugin has', async () => {
		const invitationPaths = Object.values(auth.api)
			.map((endpoint) => (endpoint as { path?: string }).path)
			.filter((path) => path !== undefined && /invit/.test(path));

		expect(invitationPaths.length).toBeGreaterThan(0);
		expect(auth.options.disabledPaths).toEqual(expect.arrayContaining(invitationPaths));

		for (const path of invitationPaths) {
			const response = await auth.handler(
				new Request(`http://panel.test/api/auth${path}`, { method: 'POST' })
			);
			expect(response.status, path).toBe(404);
		}
	});

	it('gives the admin role no impersonation', async () => {
		const allowed = (
			user: ('create' | 'set-role' | 'ban' | 'impersonate' | 'impersonate-admins')[]
		) =>
			auth.api
				.userHasPermission({ body: { role: 'admin', permissions: { user } } })
				.then((answer) => answer.success);

		expect(await allowed(['create', 'set-role', 'ban'])).toBe(true);
		expect(await allowed(['impersonate'])).toBe(false);
		expect(await allowed(['impersonate-admins'])).toBe(false);
	});

	it('reads the instance admin out of a list of roles', () => {
		expect(isInstanceAdmin('admin')).toBe(true);
		expect(isInstanceAdmin('user,admin')).toBe(true);
		expect(isInstanceAdmin('user')).toBe(false);
		expect(isInstanceAdmin(null)).toBe(false);
	});
});
