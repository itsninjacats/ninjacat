import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RequestEvent } from '@sveltejs/kit';
import { keyHash } from '$lib/server/api-keys';

// Drizzle's callback driver: every statement the page sends is recorded as SQL
// text and parameters, and no database is needed. A delete matches the rows in
// `matched`.
const sent = vi.hoisted(() => [] as { sql: string; params: unknown[] }[]);
const matched = vi.hoisted(() => ({ rows: [] as unknown[][] }));
vi.mock('$lib/server/db', async () => {
	const { drizzle } = await import('drizzle-orm/pg-proxy');
	const schema = await import('$lib/server/db/schema');
	return {
		db: drizzle(
			async (sql, params) => {
				sent.push({ sql, params });
				return { rows: sql.startsWith('delete') ? matched.rows : [] };
			},
			{ schema }
		)
	};
});

const page = await import('./+page.server');

type Handler = (event: RequestEvent) => unknown;
const load = page.load as unknown as Handler;
const create = page.actions.create as Handler;
const remove = page.actions.delete as Handler;

function event(form: Record<string, string>, tenant: string | null = 'acme') {
	const body = new FormData();
	for (const [key, value] of Object.entries(form)) {
		body.set(key, value);
	}
	return {
		locals: {
			user: { id: 'u1', role: 'user' },
			tenant: tenant ? { id: tenant, name: tenant, role: 'member' } : null
		},
		request: new Request('http://panel.test/app/settings/keys', { method: 'POST', body })
	} as unknown as RequestEvent;
}

const keyId = '2d8d0550-2e4d-41e8-abda-78f28f513383';

const statements = (verb: string) => sent.filter((s) => s.sql.startsWith(verb));

beforeEach(() => {
	sent.length = 0;
	matched.rows = [];
});

describe('the API keys page', () => {
	it('lists only the keys of the tenant it is shown for', async () => {
		await load(event({}));

		const [select] = statements('select');
		expect(select.sql).toMatch(/where "api_key"\."tenant_id" = \$1/);
		expect(select.params).toEqual(['acme']);
	});

	it('makes a key for the tenant it is shown for, and returns it once', async () => {
		const answer = (await create(event({ name: 'prod', tenant: 'acme' }))) as {
			created: string;
		};

		const [insert] = statements('insert');
		expect(insert.sql).toMatch(/"tenant_id"/);
		expect(insert.params).toContain('acme');
		expect(insert.params).toContain(keyHash(answer.created));
		// The key itself never reaches the database.
		expect(insert.params).not.toContain(answer.created);
		expect(statements('NOTIFY')).toHaveLength(1);
	});

	it('deletes a key only if it belongs to the tenant', async () => {
		matched.rows = [[keyId]];
		const answer = (await remove(event({ id: keyId, tenant: 'acme' }))) as { deleted: boolean };

		const [del] = statements('delete');
		expect(del.sql).toMatch(/where \("api_key"\."id" = \$1 and "api_key"\."tenant_id" = \$2\)/);
		expect(del.params).toEqual([keyId, 'acme']);
		expect(answer.deleted).toBe(true);
		expect(statements('NOTIFY')).toHaveLength(1);
	});

	it('says so when the key is not in the tenant, another tenant’s included', async () => {
		const answer = (await remove(event({ id: keyId, tenant: 'acme' }))) as { status: number };

		expect(answer.status).toBe(404);
		expect(statements('NOTIFY')).toHaveLength(0);
	});

	it('refuses an id that is not a key’s before asking the database', async () => {
		const answer = (await remove(event({ id: 'not-a-uuid', tenant: 'acme' }))) as {
			status: number;
		};

		expect(answer.status).toBe(400);
		expect(sent).toEqual([]);
	});

	it.each([
		['create', create, { name: 'prod' }],
		['delete', remove, { id: keyId }]
	])('does not %s for a tenant other than the page was shown for', async (_, action, form) => {
		const changed = (await action(event({ ...form, tenant: 'other' }))) as { status: number };
		expect(changed.status).toBe(409);

		const none = (await action(event({ ...form, tenant: 'acme' }, null))) as { status: number };
		expect(none.status).toBe(409);

		expect(sent).toEqual([]);
	});
});
