import { beforeEach, describe, expect, it, vi } from 'vitest';
import { isHttpError, isRedirect, type RequestEvent } from '@sveltejs/kit';

// Every read and write of the admin pages goes through these two. Here they
// only record that they were reached, so a test can tell whether an action
// acted before it checked who was asking.
const reached = vi.hoisted(() => ({ count: 0 }));
const recorder = vi.hoisted(
	() => (): unknown =>
		new Proxy(
			{},
			{
				get() {
					reached.count++;
					throw new Error('reached');
				}
			}
		)
);
vi.mock('$lib/server/db', () => ({ db: recorder() }));
vi.mock('$lib/server/auth', () => ({ auth: recorder() }));

const adminIndex = await import('./+page.server');
const tenants = await import('./tenants/+page.server');
const tenant = await import('./tenants/[id]/+page.server');
const users = await import('./users/+page.server');

type Handler = (event: RequestEvent) => unknown;

// A filled-in form for each action, so that only the caller decides whether it
// gets as far as the database.
const forms: Record<string, Record<string, string>> = {
	'tenants create': { name: 'Acme', slug: 'acme', owner: 'u2' },
	'tenant add': { user: 'u2', role: 'member' },
	'tenant role': { member: 'm1', role: 'admin' },
	'tenant remove': { member: 'm1' },
	'users create': { email: 'new@example.com', name: 'New', password: 'long enough password' }
};

const handlers: [string, Handler][] = [
	['index load', adminIndex.load as Handler],
	['tenants load', tenants.load as Handler],
	['tenants create', tenants.actions.create as Handler],
	['tenant load', tenant.load as Handler],
	['tenant add', tenant.actions.add as Handler],
	['tenant role', tenant.actions.role as Handler],
	['tenant remove', tenant.actions.remove as Handler],
	['users load', users.load as Handler],
	['users create', users.actions.create as Handler]
];

function event(name: string, user: { id: string; role: string | null } | undefined) {
	const body = new FormData();
	for (const [key, value] of Object.entries(forms[name] ?? {})) {
		body.set(key, value);
	}
	return {
		locals: { user, tenant: { id: 'acme', name: 'Acme', role: 'owner' }, tenants: [] },
		params: { id: 'acme' },
		request: new Request('http://panel.test/app/admin', { method: 'POST', body })
	} as unknown as RequestEvent;
}

async function outcome(handler: Handler, e: RequestEvent): Promise<string> {
	try {
		await handler(e);
		return 'done';
	} catch (thrown) {
		if (isHttpError(thrown)) return String(thrown.status);
		if (isRedirect(thrown)) return `redirect ${thrown.location}`;
		if (thrown instanceof Error && thrown.message === 'reached') return 'reached';
		throw thrown;
	}
}

beforeEach(() => {
	reached.count = 0;
});

describe('the admin pages', () => {
	// The guard keeps these off by route already; each one asks again.
	it.each(handlers)('refuse %s to a user who is not the instance admin', async (name, handler) => {
		// The owner of a tenant included: these pages are the instance's.
		expect(await outcome(handler, event(name, { id: 'u', role: 'user' }))).toBe('403');
		expect(await outcome(handler, event(name, undefined))).toBe('403');
		expect(reached.count).toBe(0);
	});

	it.each(handlers)('serve %s to the instance admin', async (name, handler) => {
		const answer = await outcome(handler, event(name, { id: 'u', role: 'admin' }));
		if (name === 'index load') {
			expect(answer).toBe('redirect /app/admin/tenants');
		} else {
			expect(answer).toBe('reached');
		}
	});
});
