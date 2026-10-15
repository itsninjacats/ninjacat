import { test as base, expect, type Page, type Request } from '@playwright/test';

// What went wrong in each page so far, for the screenshots to skip a broken page.
export const errorsIn = new WeakMap<Page, string[]>();

// A page that broke while loading (a failed import shows SvelteKit's 500 in
// place of the page) would measure as a page with nothing to overflow, so any
// error in the page, a Content-Security-Policy refusal among them, fails the
// test. A test that provokes one on purpose names it in `allowedErrors`.
export const test = base.extend<{ allowedErrors: RegExp[]; pageErrors: void }>({
	allowedErrors: [[], { option: true }],
	pageErrors: [
		async ({ page, allowedErrors }, use) => {
			const errors: string[] = [];
			errorsIn.set(page, errors);
			const note = (text: string) => {
				if (!allowedErrors.some((allowed) => allowed.test(text))) errors.push(text);
			};
			page.on('pageerror', (error) => note(error.message));
			page.on('console', (message) => {
				if (message.type() === 'error') note(message.text());
			});
			await use();
			expect(errors, 'nothing went wrong in the page').toEqual([]);
		},
		{ auto: true }
	]
});

export { expect };

// On a development machine a container starting or stopping changes the
// network under Chromium, which aborts the loads in flight with
// ERR_NETWORK_CHANGED and leaves a broken page. Such a load is tried again, and
// what went wrong in it is forgotten; any other failure stands.
export async function open(page: Page, path: string): Promise<void> {
	for (let attempt = 1; ; attempt++) {
		const errors = errorsIn.get(page) ?? [];
		const before = errors.length;
		let changed = false;
		const aborted = (request: Request) => {
			if (request.failure()?.errorText === 'net::ERR_NETWORK_CHANGED') changed = true;
		};
		page.on('requestfailed', aborted);
		const response = await page.goto(path).catch((error: Error) => {
			if (!error.message.includes('ERR_NETWORK_CHANGED')) throw error;
			changed = true;
			return null;
		});
		await page.waitForLoadState('networkidle');
		page.off('requestfailed', aborted);
		if (changed && attempt < 5) {
			errors.splice(before);
			continue;
		}
		expect(response?.status(), `${path} loads`).toBeLessThan(400);
		expect(new URL(page.url()).pathname, `${path} is not redirected`).toBe(path.split('?')[0]);
		return;
	}
}
