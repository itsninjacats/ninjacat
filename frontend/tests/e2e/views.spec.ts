import { expectNoOverflow } from './overflow';
import { stateFile } from './people';
import { routes } from './routes';
import { shoot } from './shots';
import { open, test } from './test';

for (const route of routes) {
	test.describe(route.name, () => {
		test.use({ storageState: route.as ? stateFile(route.as) : { cookies: [], origins: [] } });

		test(`${route.name} does not overflow`, async ({ page }, testInfo) => {
			await open(page, route.path);
			await expectNoOverflow(page, testInfo, route.name);
			await shoot(page, testInfo, route.name);
		});
	});
}
