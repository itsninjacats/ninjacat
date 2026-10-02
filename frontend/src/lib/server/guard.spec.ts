import { describe, expect, it } from 'vitest';
import { isRedirect, type RequestEvent } from '@sveltejs/kit';
import { handleGuard } from './guard';

function event(over: {
	route: string | null;
	user?: boolean;
	method?: string;
	accept?: string;
	isDataRequest?: boolean;
}): RequestEvent {
	return {
		locals: over.user ? { user: { id: 'u' } } : {},
		route: { id: over.route },
		isDataRequest: over.isDataRequest ?? false,
		request: new Request('http://panel.test/x', {
			method: over.method ?? 'GET',
			headers: { accept: over.accept ?? 'text/html' }
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
		throw thrown;
	}
}

describe('the guard in front of every route', () => {
	it('lets a visitor reach only the login page', async () => {
		expect(await outcome(event({ route: '/login' }))).toBe('resolved');
		expect(await outcome(event({ route: '/' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/app/logs' }))).toBe('redirect /login');
		expect(await outcome(event({ route: '/app/ustawienia/klucze' }))).toBe('redirect /login');
		// A path with no route: a remote function, or nothing at all.
		expect(await outcome(event({ route: null }))).toBe('redirect /login');
	});

	it('redirects a data request, which never runs the layout that used to guard /app', async () => {
		const data = event({ route: '/app/logs', isDataRequest: true, accept: '*/*' });
		expect(await outcome(data)).toBe('redirect /login');
	});

	it('answers 401 to what is not a page: endpoints, form actions, remote functions', async () => {
		expect(
			await outcome(event({ route: '/app/metrics/query', method: 'POST', accept: '*/*' }))
		).toBe('401');
		expect(await outcome(event({ route: '/app', method: 'POST' }))).toBe('401');
		expect(await outcome(event({ route: null, accept: '*/*' }))).toBe('401');
	});

	it('lets a signed-in user through everywhere', async () => {
		expect(await outcome(event({ route: '/app/logs', user: true }))).toBe('resolved');
		expect(await outcome(event({ route: null, user: true, accept: '*/*' }))).toBe('resolved');
	});
});
