import { test as base, expect, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";

type Fixtures = { page: Page };
type WorkerFixtures = { rendererOrigin: string };

export const test = base.extend<Fixtures, WorkerFixtures>({
	rendererOrigin: [async ({}, use) => {
		if (process.env.SYNTH_DESKTOP_TEST_URL) {
			await use(process.env.SYNTH_DESKTOP_TEST_URL);
			return;
		}
		const appRoot = resolve(import.meta.dirname, "../..");
		const workshopRoot = resolve(appRoot, "../..");
		let server: ChildProcess | undefined;
		server = spawn(resolve(workshopRoot, "node_modules/.bin/vite"), ["--host", "127.0.0.1"], {
			cwd: appRoot,
			stdio: "ignore"
		});
		const origin = "http://127.0.0.1:1420";
		for (let attempt = 0; attempt < 100; attempt += 1) {
			try { if ((await fetch(origin)).ok) break; } catch { /* Vite is starting. */ }
			await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
		}
		try { await use(origin); } finally { server.kill("SIGTERM"); }
	}, { scope: "worker" }],
	page: async ({ page, rendererOrigin }, use) => {
		await page.addInitScript(() => {
			(window as typeof window & { synthRuntime?: unknown }).synthRuntime = {
				async request(path: string) {
					if (path === "/v1/health") return {
						runtimeId: "renderer-test",
						local: { mode: "unavailable", modelPath: null },
						intern: { mode: "demo" },
						openrouter: { mode: "unconfigured" },
						inventory: { containers: 0, traces: 0, visuals: 0 }
					};
					if (path === "/v1/sessions") return { sessions: [] };
					if (path === "/v1/projects") return { projects: [] };
					throw new Error(`Unexpected renderer test request: ${path}`);
				},
				async subscribe() { return { close() {} }; }
			};
		});
		await page.goto(rendererOrigin);
		await page.getByTestId("runtime-status").waitFor();
		await use(page);
	}
});

export { expect };
