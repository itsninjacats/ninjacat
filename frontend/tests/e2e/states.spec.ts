import type { Page } from '@playwright/test';
import { expectNoOverflow } from './overflow';
import { switchMethod, type Method, type MonitorView } from '../../src/lib/monitors/definition';
import { monitors } from './monitors';
import { routes } from './routes';
import { password, people, stateFile, tenants } from './people';
import { shoot } from './shots';
import { expect, open, test } from './test';

// What only shows after a click: the sidebar opened or collapsed, the menus,
// the dialogs, and the forms with their errors.

const isPhone = (page: Page) => page.viewportSize()!.width < 768;

// On a phone the sidebar is a sheet that has to be opened first.
async function sidebar(page: Page) {
	if (isPhone(page)) {
		await page.locator('[data-sidebar="trigger"]').click();
	}
	return page.locator('[data-slot="sidebar"]').filter({ visible: true });
}

test.describe('sidebar', () => {
	test.use({ storageState: stateFile('longEmail') });

	test('opened on a phone, collapsed elsewhere', async ({ page }, testInfo) => {
		await open(page, '/app/settings/keys');
		await page.locator('[data-sidebar="trigger"]').click();
		await expectNoOverflow(page, testInfo, 'sidebar toggled');
		await shoot(page, testInfo, 'sidebar-toggled', false);
	});

	test('the user menu', async ({ page }, testInfo) => {
		await open(page, '/app');
		await (
			await sidebar(page)
		)
			.locator('[data-sidebar="footer"] [data-sidebar="menu-button"]')
			.click();
		await expect(page.getByRole('menu')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'user menu');
		await shoot(page, testInfo, 'user-menu', false);
	});
});

test.describe('tenant switcher', () => {
	test.use({ storageState: stateFile('admin') });

	test('lists two long tenant names', async ({ page }, testInfo) => {
		await open(page, '/app');
		await (
			await sidebar(page)
		)
			.locator('[data-sidebar="header"] [data-sidebar="menu-button"]')
			.click();
		await expect(page.getByRole('menuitem')).toHaveCount(2);
		await expectNoOverflow(page, testInfo, 'tenant switcher');
		await shoot(page, testInfo, 'tenant-switcher', false);
	});
});

// A refused form answers 4xx, which the browser logs as a failed load.
const refusals = [/status of 40[09]/];

test.describe('API keys', () => {
	test.use({ storageState: stateFile('memberB'), allowedErrors: refusals });

	test('a refused name, a new key, and its delete confirmation', async ({ page }, testInfo) => {
		// Unique per project, since the projects run at once; deleted at the end.
		const name = `e2e-b-temporary-${testInfo.project.name}-`.padEnd(100, 'x');
		const rows = page.getByRole('row').filter({ hasText: name });
		const confirm = () =>
			page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();

		await open(page, '/app/settings/keys');
		// What an earlier, interrupted run left behind.
		while ((await rows.count()) > 0) {
			await rows.first().getByRole('button', { name: 'Delete' }).click();
			await confirm();
			await expect(page.getByRole('alertdialog')).toBeHidden();
		}

		// Spaces pass the browser's `required`; the server trims them and refuses.
		await page.getByLabel('Name', { exact: true }).fill('   ');
		await page.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(page.locator('[data-slot="field-error"]')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'key name refused');
		await shoot(page, testInfo, 'keys-name-refused', false);

		await page.getByLabel('Name', { exact: true }).fill(name);
		await page.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(rows).toHaveCount(1);
		await expectNoOverflow(page, testInfo, 'key created');
		await shoot(page, testInfo, 'keys-created');

		await rows.getByRole('button', { name: 'Delete' }).click();
		await expect(page.getByRole('alertdialog')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'key delete confirmation');
		await shoot(page, testInfo, 'keys-delete-dialog', false);

		await confirm();
		await expect(rows).toHaveCount(0);
	});
});

test.describe('monitor editor', () => {
	test.use({ storageState: stateFile('longEmail'), allowedErrors: refusals });

	test('each detection method, and a refused save', async ({ page }, testInfo) => {
		await open(page, '/app/monitors/new');
		for (const method of ['Change', 'Anomaly', 'Outlier', 'Forecast', 'Threshold']) {
			await page.getByRole('button', { name: new RegExp(`^${method}`) }).click();
			await expect(page.getByRole('button', { name: new RegExp(`^${method}`) })).toHaveAttribute(
				'aria-pressed',
				'true'
			);
			await expectNoOverflow(page, testInfo, `${method} monitor`);
			await shoot(page, testInfo, `monitor-new-${method.toLowerCase()}`);
		}

		// Enter in a field must not save the monitor half edited. With a name the
		// browser lets the form go, and the server would refuse it out loud.
		await page.getByLabel('Name', { exact: true }).fill('e2e half edited');
		const metric = page.getByPlaceholder('metric…');
		await metric.fill('e2e.billing.queue.depth');
		await metric.press('Enter');
		await expect(page).toHaveURL(/\/app\/monitors\/new$/);
		await expect(page.getByText('The monitor was not saved.')).toHaveCount(0);

		// Spaces pass the browser's `required`; the server trims them and refuses.
		await page.getByLabel('Name', { exact: true }).fill('   ');
		await page.getByRole('button', { name: 'Create monitor' }).click();
		await expect(page.getByRole('alert')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'monitor refused');
		await shoot(page, testInfo, 'monitor-new-refused');
	});

	test('the delete confirmation', async ({ page }, testInfo) => {
		await open(page, `/app/monitors/${monitors.threshold.id}`);
		await page.getByRole('button', { name: 'Delete' }).click();
		await expect(page.getByRole('alertdialog')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'monitor delete confirmation');
		await shoot(page, testInfo, 'monitor-delete-dialog', false);
		await page.getByRole('alertdialog').getByRole('button', { name: 'Cancel' }).click();
	});
});

// The editor writes monitor queries the service's parser has to accept; only
// the query process can say it does.
test.describe('monitor definitions', () => {
	test.use({ storageState: stateFile('memberA') });
	test.skip(({ viewport }) => viewport?.width !== 1280, 'runs at desktop width only');

	test("pass the query process's check, for every method", async ({ page }) => {
		const views: [string, MonitorView][] = Object.entries(monitors).map(([k, m]) => [k, m.view]);
		for (const method of ['change', 'anomaly', 'outlier', 'forecast'] as Method[]) {
			const v = structuredClone(monitors.threshold.view);
			switchMethod(v, method);
			views.push([`threshold as ${method}`, v]);
		}
		for (const [name, view] of views) {
			const response = await page.request.post('/app/monitors/validate', { data: view });
			expect(response.ok(), name).toBe(true);
			expect((await response.json()).problems, name).toEqual([]);
		}
	});

	// A seeded history carries each monitor's query hash; a stale one makes the
	// monitor service close every group.
	test('keep their seeded groups', async ({ page }) => {
		await open(page, `/app/monitors/${monitors.threshold.id}`);
		await expect(
			page.getByText(`Groups (${monitors.threshold.groups})`, { exact: true })
		).toBeVisible();
	});
});

test.describe('chart zoom', () => {
	test.use({ storageState: stateFile('memberA') });
	test.skip(({ viewport }) => viewport?.width !== 1280, 'runs at desktop width only');

	async function drag(page: Page) {
		const box = (await page.locator('.u-over').boundingBox())!;
		const y = box.y + box.height / 2;
		await page.mouse.move(box.x + box.width * 0.3, y);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.6, y, { steps: 5 });
		await page.mouse.up();
	}

	test("puts Datadog's fixed range in the URL, and a preset takes it out", async ({ page }) => {
		await open(page, `/app/monitors/${monitors.threshold.id}`);
		await drag(page);
		await expect(page).toHaveURL(/\?from_ts=\d+&to_ts=\d+&live=false$/);

		await page.getByRole('link', { name: '1h', exact: true }).click();
		await expect(page).toHaveURL(/\?range=-1h$/);
	});

	test('in the explorer, Back undoes a zoom', async ({ page }) => {
		await page.route('**/app/metrics/query', async (route) => {
			const { from, to } = route.request().postDataJSON();
			const points = [0, 1, 2, 3, 4].map((i) => ({ t: from + ((to - from) / 4) * i, v: i }));
			await route.fulfill({
				json: { series: [{ output: 0, tags: [], points, band: [], forecast: [] }] }
			});
		});
		await open(page, routes.find((r) => r.name === 'metrics-long-query')!.path);
		await drag(page);
		await expect(page).toHaveURL(/&from_ts=\d+&to_ts=\d+&live=false$/);

		await page.goBack();
		await expect(page).toHaveURL(/\/app\/metrics\?view=[^&]+$/);
	});
});

test.describe('monitor save', () => {
	test.use({ storageState: stateFile('memberB'), allowedErrors: refusals });

	test('a monitor written as text, saved, shown and deleted', async ({ page }, testInfo) => {
		// Unique per project, since the projects run at once; deleted at the end.
		const name = `e2e-b-temporary-monitor-${testInfo.project.name}-`.padEnd(120, 'x');
		const listed = page.getByRole('link', { name });

		await open(page, '/app/monitors');
		// What an earlier, interrupted run left behind.
		while ((await listed.count()) > 0) {
			await listed.first().click();
			await page.getByRole('button', { name: 'Delete' }).click();
			await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
			await page.waitForURL('**/app/monitors');
		}

		await open(page, '/app/monitors/new');
		await page.getByRole('button', { name: 'Edit as text' }).click();
		await page
			.getByPlaceholder('avg:system.cpu.user{env:prod} by {host}')
			.fill('avg:e2e.billing.queue.depth{env:e2e} by {host}');
		await page.getByLabel('Alert threshold').fill('100');
		await page.getByLabel('Name', { exact: true }).fill(name);
		await expect(page.getByText('The monitor can be evaluated as it stands.')).toBeVisible();
		await page.getByRole('button', { name: 'Create monitor' }).click();
		await page.waitForURL(/\/app\/monitors\/\d+$/);
		await expect(page.getByRole('heading', { name })).toBeVisible();
		await expectNoOverflow(page, testInfo, 'monitor created');
		await shoot(page, testInfo, 'monitor-created');

		await page.getByRole('button', { name: 'Delete' }).click();
		await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
		await page.waitForURL('**/app/monitors');
		await expect(listed).toHaveCount(0);
	});
});

test.describe('admin forms', () => {
	test.use({ storageState: stateFile('admin'), allowedErrors: refusals });

	test('a tenant refused for its slug', async ({ page }, testInfo) => {
		await open(page, '/app/admin/tenants');
		await page
			.getByLabel('Name', { exact: true })
			.fill('e2e-refused '.padEnd(100, 'name-without-a-break-'));
		await page.getByLabel('Slug').fill('Not A Valid Slug, Because It Has Spaces And Capitals');
		await page.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(page.locator('[data-slot="field-error"]')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'tenant refused');
		await shoot(page, testInfo, 'admin-tenants-refused');
	});

	test('an account refused for its password', async ({ page }, testInfo) => {
		await open(page, '/app/admin/users');
		await page
			.getByLabel('E-mail', { exact: true })
			.fill(people.longEmail.email.replace('e2e-', 'e2e-refused-'));
		await page.getByLabel('Name', { exact: true }).fill(people.longEmail.name);
		await page.getByLabel('Password', { exact: true }).fill('short');
		await page.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(page.locator('[data-slot="field-error"]')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'account refused');
		await shoot(page, testInfo, 'admin-users-refused');
	});

	test("the last owner's role refused, and a member's removal confirmed", async ({
		page
	}, testInfo) => {
		await open(page, `/app/admin/tenants/${tenants.b.id}`);

		// The admin is e2e-b's only owner, so this is refused and nothing changes.
		const owner = page.getByRole('row').filter({ hasText: people.admin.email });
		await owner.getByRole('combobox').selectOption('member');
		await expect(page.getByRole('alert')).toBeVisible();
		await expect(owner.getByRole('combobox')).toHaveValue('owner');
		await expectNoOverflow(page, testInfo, 'last owner refused');
		await shoot(page, testInfo, 'admin-tenant-b-refused');

		const member = page.getByRole('row').filter({ hasText: people.memberB.email });
		await member.getByRole('button', { name: 'Remove' }).click();
		await expect(page.getByRole('alertdialog')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'member removal confirmation');
		await shoot(page, testInfo, 'admin-tenant-b-remove-dialog', false);
		await page.getByRole('alertdialog').getByRole('button', { name: 'Cancel' }).click();
	});
});

test.describe('sign-in', () => {
	// Better Auth answers a wrong password with a 401.
	test.use({ storageState: { cookies: [], origins: [] }, allowedErrors: [/status of 401/] });

	test('a wrong password', async ({ page }, testInfo) => {
		// The limiter counts per address; each project signs in from its own.
		const projects = testInfo.config.projects.map((project) => project.name);
		await page.context().setExtraHTTPHeaders({
			'x-forwarded-for': `192.0.2.${100 + projects.indexOf(testInfo.project.name)}`
		});
		await open(page, '/login');
		await page.getByLabel('E-mail address').fill(people.longEmail.email);
		await page.getByLabel('Password').fill(`${password}-wrong`);
		await page.getByRole('button', { name: 'Sign in' }).click();
		await expect(page.getByText('Wrong e-mail or password')).toBeVisible();
		await expectNoOverflow(page, testInfo, 'sign-in refused');
		await shoot(page, testInfo, 'login-refused');
	});
});
