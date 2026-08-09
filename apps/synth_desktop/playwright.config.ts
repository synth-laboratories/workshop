import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: "./tests/playwright",
	fullyParallel: false,
	workers: 1,
	timeout: 45_000,
	expect: { timeout: 8_000 },
	forbidOnly: Boolean(process.env.CI),
	retries: process.env.CI ? 1 : 0,
	reporter: [["line"]],
	outputDir: "./test-results/playwright",
	use: {
		trace: "retain-on-failure",
		screenshot: "only-on-failure"
	}
});
