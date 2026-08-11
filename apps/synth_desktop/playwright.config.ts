import { defineConfig } from "@playwright/test";

// Each worker boots its own Vite on a reserved loopback port (see browser.fixture.ts),
// and Playwright gives each test its own browser context — so fullyParallel is safe.
const workersFromEnv = Number(process.env.PLAYWRIGHT_WORKERS);
const workers = Number.isFinite(workersFromEnv) && workersFromEnv > 0
	? Math.floor(workersFromEnv)
	: process.env.CI
		? 4
		: 8;

export default defineConfig({
	testDir: "./tests/playwright",
	fullyParallel: true,
	workers,
	timeout: 45_000,
	expect: { timeout: 8_000 },
	forbidOnly: Boolean(process.env.CI),
	retries: process.env.CI ? 1 : 0,
	reporter: [["line"]],
	outputDir: "./test-results/playwright",
	use: {
		// Full traces on every failure are expensive locally; keep them for CI.
		trace: process.env.CI ? "retain-on-failure" : "on-first-retry",
		screenshot: "only-on-failure"
	}
});
