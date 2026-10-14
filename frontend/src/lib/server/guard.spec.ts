import { describe, expect, it } from 'vitest';
import { isHttpError, isRedirect, type RequestEvent } from '@sveltejs/kit';
import { handleGuard } from './guard';

const acme = { id: 'acme', name: 'Acme', role: 'member' };

// A signed-in user is a member of acme unless `tenant` says otherwise.
function event(over: {
	route: string | null;
	user?: boolean;
	admin?: boolean;
	tenant?: typeof acme | null;
	method?: string;
	accept?: string;
	isDataRequest?: boolean;
	enhanced?: boolean;
}): RequestEvent {
	const signedIn = over.user || over.admin;
	return {
		locals: signedIn
			? {
					user: { id: 'u', role: over.admin ? 'admin' : 'user' },
					tenant: over.tenant === undefined ? acme : over.tenant
				}
			: {},
		route: { id: over.route },
		isDataRequest: over.isDataRequest ?? false,
		request: new Request('http://panel.test/x', {
			method: over.method ?? 'GET',
			headers: {
				accept: over.accept ?? 'text/html',
				...(over.enhanced ? { 'x-sveltekit-action': 'true' } : {})
			}
		})
	} as unknown as RequestEvent;
}

const resolve = async () => new Response('resolved');

async function outcome(e: RequestEvent): Promise<string> {
	try {
		const response = await handleGuard({ event: e, resolve });
		return response.status === 200 ? await response.text() : String(response.status);
	} catch (thrown) {
		if (isRedirect(thrown)) return `redirect ${thrown.location}`;
		if (isHttpError(thrown)) return String(thrown.status);
		throw thrown;
	}
}

describe('the guard in front of every route', () => {
	it('lets a visitor reach only the login page', async () => {
		expect(await outcome(event({ route: '/login' }))).toBe('resolved');
		expect(await outcome(event({ route: '/' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/app/logs' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/app/settings/keys' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/no-tenant' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/app/admin' }))).toBe('redirect /login');
		// A path with no route: a remote function, or nothing at all.
		expect(await outcome(event({ route: null }))).toBe('redirect /login');
	});

	it('redirects a data request, which never runs the layout that used to guard /app', async () => {
		const data = event({ route: '/app/logs', isDataRequest: true, accept: '*/*' });
		expect(await outcome(data)).toBe('redirect /login');
	});

	it('answers 401 to what cannot follow a redirect: endpoints, plain form posts, remote functions', async () => {
		expect(
			await outcome(event({ route: '/app/metrics/query', method: 'POST', accept: '*/*' }))
		).toBe('401');
		expect(await outcome(event({ route: '/app', method: 'POST' }))).toBe('401');
		expect(await outcome(event({ route: null, accept: '*/*' }))).toBe('401');
	});

	it('redirects an enhanced form action, which SvelteKit answers with a redirect result', async () => {
		const action = {
			route: '/app/settings/keys',
			method: 'POST',
			accept: 'application/json',
			enhanced: true
		};
		expect(await outcome(event(action))).toBe('redirect /login');
		expect(await outcome(event({ ...action, user: true, tenant: null }))).toBe(
			'redirect /no-tenant'
		);
	});

	it('lets a signed-in user through everywhere', async () => {
		expect(await outcome(event({ route: '/app/logs', user: true }))).toBe('resolved');
		expect(await outcome(event({ route: null, user: true, accept: '*/*' }))).toBe('resolved');
	});

	it('sends a user who is a member of no tenant to the page that says so, and nowhere else', async () => {
		const noTenant = { user: true, tenant: null };
		expect(await outcome(event({ route: '/no-tenant', ...noTenant }))).toBe('resolved');
		expect(await outcome(event({ route: '/app', ...noTenant }))).toBe('redirect /no-tenant');
		expect(await outcome(event({ route: '/app/logs', ...noTenant, isDataRequest: true }))).toBe(
			'redirect /no-tenant'
		);
		expect(await outcome(event({ route: null, ...noTenant }))).toBe('redirect /no-tenant');
		expect(
			await outcome(
				event({ route: '/app/metrics/query', ...noTenant, method: 'POST', accept: '*/*' })
			)
		).toBe('403');
		expect(await outcome(event({ route: '/app/settings/keys', ...noTenant, method: 'POST' }))).toBe(
			'403'
		);
	});

	it('lets the instance admin reach the admin pages without a tenant, and only those', async () => {
		const admin = { admin: true, tenant: null };
		expect(await outcome(event({ route: '/app/admin', ...admin }))).toBe('resolved');
		expect(await outcome(event({ route: '/app/admin/tenants', ...admin, method: 'POST' }))).toBe(
			'resolved'
		);
		expect(await outcome(event({ route: '/app/logs', ...admin }))).toBe('redirect /no-tenant');
		expect(await outcome(event({ route: '/app/administrator', ...admin }))).toBe(
			'redirect /no-tenant'
		);
	});

	it('keeps everyone else off the admin pages, a tenant or not', async () => {
		expect(await outcome(event({ route: '/app/admin', user: true }))).toBe('403');
		expect(
			await outcome(
				event({ route: '/app/admin/tenants', user: true, tenant: null, method: 'POST' })
			)
		).toBe('403');
	});

	it('treats a remote call by its tenant, never as an admin route', async () => {
		expect(await outcome(event({ route: null, admin: true, tenant: null, accept: '*/*' }))).toBe(
			'403'
		);
	});
});
