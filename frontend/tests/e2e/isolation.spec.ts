import type { Page } from '@playwright/test';
import { keyNames, password, people, stateFile, tenants, type TenantKey } from './people';
import { routes } from './routes';
import { expect, open, test } from './test';

// What one tenant's member can see of another tenant: nothing, on any page,
// including what the page carries without showing (its serialised load data).

// It does not depend on the width; one project is enough.
test.skip(({ viewport }) => viewport?.width !== 1280, 'runs at desktop width only');

// A member of both tenants is shown both names in the switcher; their keys and
// members stay apart all the same.
function marks(tenant: TenantKey, { member }: { member: 'of one' | 'of both' }): string[] {
	const others = tenant === 'a' ? people.memberA : people.memberB;
	const names = member === 'of one' ? [tenants[tenant].name, tenants[tenant].slug] : [];
	return [...names, ...keyNames[tenant], others.email];
}

// The HTML holds what the page was first loaded with, so after a switch in
// place only what is on screen says what the page shows now.
async function expectNothingOf(
	page: Page,
	tenant: TenantKey,
	member: 'of one' | 'of both',
	where: 'html' | 'screen' = 'html'
) {
	const shown = where === 'html' ? await page.content() : await page.locator('body').innerText();
	for (const mark of marks(tenant, { member })) {
		expect(shown.includes(mark), `${page.url()} shows "${mark}"`).toBe(false);
	}
}

async function openTenantMenu(page: Page) {
	await page.locator('[data-sidebar="header"] [data-sidebar="menu-button"]').click();
	await expect(page.getByRole('menu')).toBeVisible();
}

const tenantPages = routes.filter(
	(route) => route.as !== null && route.as !== 'admin' && route.as !== 'noTenant'
);

for (const [person, own, other] of [
	['memberA', 'a', 'b'],
	['memberB', 'b', 'a']
] as const) {
	test.describe(`${person} sees only ${tenants[own].id}`, () => {
		test.use({ storageState: stateFile(person) });

		test('on every page of the panel', async ({ page }) => {
			for (const route of tenantPages) {
				await open(page, route.path);
				await expectNothingOf(page, other, 'of one');
			}
		});

		test('in the tenant switcher', async ({ page }) => {
			await open(page, '/app');
			await openTenantMenu(page);
			await expect(page.getByRole('menuitem')).toHaveText([new RegExp(escape(tenants[own].name))]);
			await expectNothingOf(page, other, 'of one');
		});

		test("in the keys list, which holds the tenant's own keys", async ({ page }) => {
			await open(page, '/app/settings/keys');
			for (const name of keyNames[own]) {
				await expect(page.getByRole('cell', { name, exact: true })).toBeVisible();
			}
			await expectNothingOf(page, other, 'of one');
		});
	});
}

test.describe('switching tenants', () => {
	test.use({ storageState: { cookies: [], origins: [] } });

	test('changes the keys, the name in the sidebar, and nothing else leaks', async ({
		page,
		baseURL
	}) => {
		// A session of its own, so the tenant it switches to is no other test's.
		const person = people.switcher;
		await page.context().setExtraHTTPHeaders({ 'x-forwarded-for': person.address });
		const signIn = await page.request.post('/api/auth/sign-in/email', {
			data: { email: person.email, password },
			headers: { origin: baseURL! }
		});
		expect(signIn.status()).toBe(200);

		const shows = async (tenant: TenantKey, other: TenantKey, where: 'html' | 'screen') => {
			const header = page.locator('[data-sidebar="header"]');
			await expect(header).toContainText(tenants[tenant].name);
			for (const name of keyNames[tenant]) {
				await expect(page.getByRole('cell', { name, exact: true })).toBeVisible();
			}
			await expectNothingOf(page, other, 'of both', where);
		};

		await open(page, '/app/settings/keys');
		await shows('a', 'b', 'html');

		await openTenantMenu(page);
		await page.getByRole('menuitem').filter({ hasText: tenants.b.name }).click();
		await shows('b', 'a', 'screen');

		await open(page, '/app/settings/keys');
		await shows('b', 'a', 'html');

		// Each sign-in adds a session row; this one is not kept for another run.
		const signOut = await page.request.post('/api/auth/sign-out', {
			headers: { origin: baseURL! }
		});
		expect(signOut.status()).toBe(200);
	});
});

function escape(text: string): string {
	return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
