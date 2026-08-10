import { test as base, expect, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { resolve } from "node:path";

type Fixtures = { page: Page };
type WorkerFixtures = { rendererOrigin: string };

async function reserveLoopbackPort(): Promise<number> {
	const server = createServer();
	await new Promise<void>((resolvePromise, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolvePromise);
	});
	const address = server.address();
	if (!address || typeof address === "string") {
		server.close();
		throw new Error("Could not reserve an isolated Playwright renderer port");
	}
	await new Promise<void>((resolvePromise, reject) => {
		server.close((error) => error ? reject(error) : resolvePromise());
	});
	return address.port;
}

export const test = base.extend<Fixtures, WorkerFixtures>({
	rendererOrigin: [async ({}, use) => {
		if (process.env.SYNTH_DESKTOP_TEST_URL) {
			await use(process.env.SYNTH_DESKTOP_TEST_URL);
			return;
		}
		const appRoot = resolve(import.meta.dirname, "../..");
		const workshopRoot = resolve(appRoot, "../..");
		const port = await reserveLoopbackPort();
		let server: ChildProcess | undefined;
		server = spawn(resolve(workshopRoot, "node_modules/.bin/vite"), [
			"--host", "127.0.0.1",
			"--port", String(port),
			"--strictPort"
		], {
			cwd: appRoot,
			stdio: "ignore"
		});
		const origin = `http://127.0.0.1:${port}`;
		let ready = false;
		for (let attempt = 0; attempt < 300; attempt += 1) {
			if (server.exitCode !== null) {
				throw new Error(`Vite exited before becoming ready (code ${server.exitCode})`);
			}
			try {
				if ((await fetch(origin)).ok) {
					ready = true;
					break;
				}
			} catch { /* Vite is starting. */ }
			await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
		}
		if (!ready) throw new Error(`Vite did not become ready at ${origin} within 30 seconds`);
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
		await page.getByTestId("titlebar").waitFor();
		await use(page);
	}
});

export { expect };
