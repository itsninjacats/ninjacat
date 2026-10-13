import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { jwtVerify } from 'jose';

const secret = '0123456789abcdef0123456789abcdef';

const request = vi.hoisted(() => ({ locals: {} as Partial<App.Locals> }));
const env = vi.hoisted(() => ({}) as Record<string, string | undefined>);

vi.mock('$app/server', () => ({ getRequestEvent: () => request }));
vi.mock('$env/dynamic/private', () => ({ env }));

const { fetchMetricNames, queryScalar, NinjacatError } = await import('./ninjacat');

const fetchMock = vi.fn();

beforeEach(() => {
	env.NINJACAT_QUERY_TOKEN_SECRET = secret;
	request.locals = {
		user: { id: 'user-1' } as App.Locals['user'],
		tenant: { id: 'acme', name: 'Acme', role: 'admin' }
	};
	fetchMock.mockReset();
	fetchMock.mockResolvedValue(Response.json({ metrics: [] }));
	vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
	vi.unstubAllGlobals();
});

function bearerOf(call: number): string {
	const init: RequestInit = fetchMock.mock.calls[call][1];
	const authorization = new Headers(init.headers).get('authorization') ?? '';
	expect(authorization).toMatch(/^Bearer /);
	return authorization.slice('Bearer '.length);
}

describe('the query token', () => {
	it('names the user, the tenant and the role, for the query process, for a minute', async () => {
		await fetchMetricNames();

		const { payload, protectedHeader } = await jwtVerify(
			bearerOf(0),
			new TextEncoder().encode(secret),
			{ issuer: 'ninjacat-panel', audience: 'ninjacat-query' }
		);
		expect(protectedHeader.alg).toBe('HS256');
		expect(payload).toMatchObject({ sub: 'user-1', tenant: 'acme', role: 'admin' });
		expect(payload.exp! - payload.iat!).toBe(60);
	});

	it('goes with the query API calls too', async () => {
		fetchMock.mockResolvedValue(Response.json({ data: { attributes: { columns: [] } } }));
		await queryScalar({ from: 0, to: 1, queries: [], formulas: [], aggregator: 'avg' });

		const { payload } = await jwtVerify(bearerOf(0), new TextEncoder().encode(secret));
		expect(payload.tenant).toBe('acme');
	});

	it('is never sent without a tenant: nothing is asked at all', async () => {
		request.locals = { user: { id: 'user-1' } as App.Locals['user'], tenant: null };

		await expect(fetchMetricNames()).rejects.toBeInstanceOf(NinjacatError);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('is never sent without a user', async () => {
		request.locals = { tenant: { id: 'acme', name: 'Acme', role: 'admin' } };

		await expect(fetchMetricNames()).rejects.toBeInstanceOf(NinjacatError);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('is not signed with a missing or short secret', async () => {
		for (const short of [undefined, '', 'too-short']) {
			env.NINJACAT_QUERY_TOKEN_SECRET = short;
			await expect(fetchMetricNames()).rejects.toThrow('NINJACAT_QUERY_TOKEN_SECRET');
		}
		expect(fetchMock).not.toHaveBeenCalled();
	});
});
