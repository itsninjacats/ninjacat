import { defineConfig, devices, type Project } from '@playwright/test';

// The seed reads DATABASE_URL and BETTER_AUTH_SECRET from the panel's .env.
try {
	process.loadEnvFile('.env');
} catch {
	// No .env: the variables come from the environment.
}

const baseURL = 'http://localhost:5174';

const sizes = {
	phone: { width: 360, height: 740, mobile: true },
	tablet: { width: 768, height: 1024, mobile: true },
	desktop: { width: 1280, height: 800, mobile: false },
	wide: { width: 1920, height: 1080, mobile: false }
};

// The panel has no dark mode yet: it defines a .dark palette, but nothing puts
// the class on the page, so a dark colour scheme would render the same pixels.
// Add 'dark' here once something does.
const schemes = ['light'] as const;

const views: Project[] = Object.entries(sizes).flatMap(([size, { width, height, mobile }]) =>
	schemes.map((colorScheme) => ({
		name: `${size}-${colorScheme}`,
		dependencies: ['seed'],
		testIgnore: /\.setup\.ts$/,
		use: {
			...devices['Desktop Chrome'],
			viewport: { width, height },
			isMobile: mobile,
			hasTouch: mobile,
			colorScheme
		}
	}))
);

export default defineConfig({
	testDir: 'tests/e2e',
	fullyParallel: true,
	// A page waits up to 10s for the query process (src/lib/server/ninjacat.ts).
	timeout: 60_000,
	forbidOnly: !!process.env.CI,
	// On a development machine a container starting or stopping changes the
	// network under Chromium, which aborts loads in flight (ERR_NETWORK_CHANGED)
	// and fails the page; open() reloads it. A retry covers what else that
	// aborts, and a test that passes only on a retry still fails the run, so an
	// overflow that comes and goes is not let through.
	retries: 2,
	failOnFlakyTests: true,
	reporter: [['list'], ['html', { open: 'never' }]],
	use: { baseURL, trace: 'retain-on-failure' },
	projects: [{ name: 'seed', testMatch: /\.setup\.ts$/ }, ...views],
	webServer: {
		command: 'bun run dev --port 5174 --strictPort',
		// Better Auth's base URL; the .env of a panel on :5173 would refuse the sign-ins.
		env: { ORIGIN: baseURL },
		url: `${baseURL}/login`,
		reuseExistingServer: true,
		timeout: 120_000
	}
});
