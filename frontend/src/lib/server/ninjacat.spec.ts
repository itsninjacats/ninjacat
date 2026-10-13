import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { jwtVerify } from 'jose';

const secret = '0123456789abcdef0123456789abcdef';

const request = vi.hoisted(() => ({ locals: {} as Partial<App.Locals> }));
const env = vi.hoisted(() => ({}) as Record<string, string | undefined>);

vi.mock('$app/server', () => ({ getRequestEvent: () => request }));
vi.mock('$env/dynamic/private', () => ({ env }));

const {
	fetchMetricNames,
	queryScalar,
	validateMonitor,
	fetchMonitorStatus,
	fetchMonitorTransitions,
	NinjacatError
} = await import('./ninjacat');

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

describe('the monitor calls', () => {
	function urlOf(call: number): URL {
		return new URL(fetchMock.mock.calls[call][0]);
	}

	it('validate posts the definition and reads the problems', async () => {
		fetchMock.mockResolvedValue(
			Response.json({ problems: ['bad query'], not_acted_on: ['renotify_interval'] })
		);
		const definition = { type: 'query alert', query: 'avg(last_5m):avg:x{*} > 1', options: {} };

		expect(await validateMonitor(definition)).toEqual({
			problems: ['bad query'],
			notActedOn: ['renotify_interval']
		});
		const init: RequestInit = fetchMock.mock.calls[0][1];
		expect(urlOf(0).pathname).toBe('/internal/monitors/validate');
		expect(init.method).toBe('POST');
		expect(JSON.parse(String(init.body))).toEqual(definition);
		expect((await jwtVerify(bearerOf(0), new TextEncoder().encode(secret))).payload.tenant).toBe(
			'acme'
		);
	});

	it('status joins the ids with commas, and absent fields are null', async () => {
		fetchMock.mockResolvedValue(
			Response.json({
				monitors: [
					{ id: 7, state: 'Alert', groups: { OK: 1, Warn: 0, Alert: 1, 'No Data': 0 } },
					{ id: 8, groups: { OK: 0, Warn: 0, Alert: 0, 'No Data': 0 } }
				]
			})
		);

		const statuses = await fetchMonitorStatus([7, 8]);
		expect(urlOf(0).searchParams.get('ids')).toBe('7,8');
		expect(statuses.map((s) => [s.id, s.state, s.lastTriggeredAt])).toEqual([
			[7, 'Alert', null],
			[8, null, null]
		]);
	});

	it('status asks for many ids in batches the server takes, and keeps their order', async () => {
		fetchMock.mockImplementation(async (url: string) => {
			const ids = new URL(url).searchParams.get('ids')!.split(',').map(Number);
			return Response.json({
				monitors: ids.map((id) => ({ id, groups: { OK: 0, Warn: 0, Alert: 0, 'No Data': 0 } }))
			});
		});
		const ids = Array.from({ length: 450 }, (_, i) => 1000 - i);

		const statuses = await fetchMonitorStatus(ids);
		expect(
			fetchMock.mock.calls.map((_, i) => urlOf(i).searchParams.get('ids')!.split(',').length)
		).toEqual([200, 200, 50]);
		expect(statuses.map((s) => s.id)).toEqual(ids);
	});

	it('status of no monitors asks nothing', async () => {
		expect(await fetchMonitorStatus([])).toEqual([]);
		expect(fetchMock).not.toHaveBeenCalled();
	});

	it('transitions pass the range and the group, and come back camel-cased', async () => {
		fetchMock.mockResolvedValue(
			Response.json({
				transitions: [
					{
						group: 'host:a',
						tags: ['host:a'],
						at: '2026-10-14T10:00:00+00:00',
						to_state: 'Alert',
						reason: 'new',
						value: 95,
						crossed: 'critical',
						comparator: '>',
						critical: 90,
						window_from: '2026-10-14T09:55:00+00:00',
						window_to: '2026-10-14T10:00:00+00:00',
						first_triggered_at: '2026-10-14T10:00:00+00:00',
						monitor_version: 3
					}
				],
				initial: [
					{
						group: 'host:b',
						tags: ['host:b'],
						at: '2026-10-13T08:00:00+00:00',
						from_state: 'Alert',
						to_state: 'OK',
						reason: 'recovery',
						crossed: 'critical_recovery',
						comparator: '>',
						monitor_version: 2
					}
				],
				truncated: false
			})
		);

		const { initial, transitions, truncated } = await fetchMonitorTransitions(7, {
			from: '-1d',
			group: 'host:a'
		});
		const url = urlOf(0);
		expect(url.pathname).toBe('/internal/monitors/7/transitions');
		expect(Object.fromEntries(url.searchParams)).toEqual({ from: '-1d', group: 'host:a' });
		expect(truncated).toBe(false);
		expect(transitions[0]).toMatchObject({
			fromState: null,
			toState: 'Alert',
			crossed: 'critical',
			criticalRecovery: null,
			windowFrom: '2026-10-14T09:55:00+00:00',
			firstTriggeredAt: '2026-10-14T10:00:00+00:00',
			lastTriggeredAt: null,
			monitorVersion: 3
		});
		expect(initial.map((t) => [t.group, t.fromState, t.toState, t.value])).toEqual([
			['host:b', 'Alert', 'OK', null]
		]);
	});

	it("a refused request surfaces the server's reason", async () => {
		fetchMock.mockResolvedValue(Response.json({ error: 'ids is required' }, { status: 400 }));
		await expect(fetchMonitorStatus([1])).rejects.toThrow('ids is required');
	});
});
