import { people, password } from './people';
import { expect, open, test } from './test';

// Before the page hydrates, a submit is the browser's own. Without a method it
// was a GET, which put the password into the URL, the history and the logs.
test.describe('sign-in before the script runs', () => {
	test.use({ storageState: { cookies: [], origins: [] }, javaScriptEnabled: false });

	test('sends nothing, and never the password in a URL', async ({ page }) => {
		const urls: string[] = [];
		page.on('request', (request) => urls.push(request.url()));
		await open(page, '/login');

		await expect(page.getByRole('button', { name: 'Sign in' })).toBeDisabled();
		await expect(page.locator('form')).toHaveAttribute('method', 'post');

		await page.getByLabel('E-mail address').fill(people.longEmail.email);
		await page.getByLabel('Password').fill(password);
		await page.getByLabel('Password').press('Enter');
		await page.waitForLoadState('networkidle');

		expect(new URL(page.url()).search).toBe('');
		expect(urls.filter((url) => url.includes(encodeURIComponent(password)))).toEqual([]);
	});
});
