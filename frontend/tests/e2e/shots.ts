import type { Page, TestInfo } from '@playwright/test';
import { contentPane, settle } from './overflow';
import { errorsIn } from './test';

// Screenshots for a person to look through, not baselines: nothing compares them.
const directory = process.env.E2E_SHOTS_DIR ?? 'test-results/shots';

// The app shell is as tall as the viewport and scrolls its content pane, so a
// full-page screenshot would stop at the fold. Growing the viewport to the
// content's height shows all of it.
export async function shoot(page: Page, testInfo: TestInfo, name: string, whole = true) {
	// A page that broke (SvelteKit's 500 after a failed import) would overwrite
	// the good picture an earlier attempt took.
	if ((errorsIn.get(page) ?? []).length > 0) return;
	const path = `${directory}/${name}/${testInfo.project.name}.png`;
	const viewport = page.viewportSize()!;
	const height = whole
		? await page.evaluate((pane) => {
				const scroller = document.querySelector(pane);
				return scroller
					? scroller.getBoundingClientRect().top + scroller.scrollHeight
					: document.documentElement.scrollHeight;
			}, contentPane)
		: viewport.height;

	if (height > viewport.height) {
		await page.setViewportSize({ width: viewport.width, height: Math.ceil(height) });
		await settle(page);
	}
	await page.screenshot({ path, fullPage: true, animations: 'disabled' });
	if (height > viewport.height) {
		await page.setViewportSize(viewport);
	}
}
