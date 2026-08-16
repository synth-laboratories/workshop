import { test as base, expect, type Page } from "@playwright/test";
import { mkdtemp, rm } from "node:fs/promises";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

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
		const cacheDir = await mkdtemp(join(tmpdir(), "synth-desktop-playwright-vite-"));
		let server: ChildProcess | undefined;
		server = spawn(resolve(workshopRoot, "node_modules/.bin/vite"), [
			"--host", "127.0.0.1",
			"--port", String(port),
			"--strictPort"
		], {
			cwd: appRoot,
			env: { ...process.env, SYNTH_DESKTOP_VITE_CACHE_DIR: cacheDir },
			stdio: "ignore"
		});
		try {
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
			await use(origin);
		} finally {
			server.kill("SIGTERM");
			await rm(cacheDir, { recursive: true, force: true });
		}
	}, { scope: "worker" }],
	page: async ({ page, rendererOrigin }, use) => {
		page.on("pageerror", (error) => {
			console.error(`[renderer pageerror] ${error.stack ?? error.message}`);
		});
		page.on("console", (message) => {
			if (message.type() === "error") console.error(`[renderer console] ${message.text()}`);
		});
		await page.addInitScript(() => {
			let coreBridge: Record<string, unknown>;
			const coreDefaults = {
				diagnostics: async () => ({
					databasePath: "browser-memory://core-runtime",
					schemaVersion: 0,
					integrityOk: true,
					contentStorePath: "browser-memory://content",
					journalHead: 0,
					sessionCount: 0,
					runCount: 0,
					visualCount: 0,
					migrationComplete: true
				}),
				eventsAfter: async () => [],
				sessionEventsAfter: async () => [],
				sessionEventsTail: async (sessionId: string, limit = 200) => {
					const legacy = coreBridge.sessionEventsAfter;
					return typeof legacy === "function"
						? await (legacy as (id: string, after: number, cap: number) => Promise<unknown[]>)(sessionId, 0, limit)
						: [];
				},
				sessionEventsBefore: async () => [],
				onEvent: () => () => undefined
			};
			coreBridge = coreDefaults;
			Object.defineProperty(window, "synthCore", {
				configurable: true,
				get: () => coreBridge,
				set: (fixture) => {
					coreBridge = { ...coreDefaults, ...(fixture as Record<string, unknown>) };
				}
			});
			const oauthDefaults = {
				begin: async () => { throw new Error("OAuth fixture missing"); },
				completeManual: async () => ({ configured: false }),
				ensureReady: async () => ({ configured: false }),
				status: async () => ({ configured: false }),
				disconnect: async () => ({ configured: false }),
				cancel: async () => undefined
			};
			let oauthBridge: Record<string, unknown> = oauthDefaults;
			Object.defineProperty(window, "synthCodexOauth", {
				configurable: true,
				get: () => oauthBridge,
				set: (fixture) => {
					oauthBridge = { ...oauthDefaults, ...(fixture as Record<string, unknown>) };
				}
			});
			// Feature specs often replace only the visual methods they exercise.
			// Keep those fixtures forward-compatible with additions to the native
			// bridge instead of turning an unrelated method into a page-wide load
			// failure. The setter merges each focused fixture over a complete,
			// deterministic browser-only bridge.
			const visualDefaults = {
				listTemplates: async () => [],
				getTemplate: async () => { throw new Error("visual template fixture missing"); },
				list: async () => [],
				get: async () => { throw new Error("visual fixture missing"); },
				reportObservation: async () => undefined,
				revisions: async () => [],
				annotations: async () => [],
				createAnnotation: async () => { throw new Error("annotation fixture missing"); },
				listSeals: async () => [],
				seal: async () => { throw new Error("visual seal fixture missing"); },
				getSeal: async () => { throw new Error("visual seal fixture missing"); },
				uploadStatus: async () => null,
				shareSeal: async () => { throw new Error("visual share fixture missing"); },
				openShared: async () => { throw new Error("shared visual fixture missing"); },
				create: async () => { throw new Error("visual create fixture missing"); },
				update: async () => { throw new Error("visual update fixture missing"); },
				save: async () => { throw new Error("visual save fixture missing"); },
				fork: async () => { throw new Error("visual fork fixture missing"); },
				archive: async () => { throw new Error("visual archive fixture missing"); },
				show: async () => { throw new Error("visual show fixture missing"); },
				content: async () => null,
				renditions: async () => [],
				rendition: async () => null,
				render: async () => { throw new Error("visual render fixture missing"); },
				onEvent: () => () => undefined,
				onShow: () => () => undefined
			};
			let visualBridge: Record<string, unknown> = visualDefaults;
			Object.defineProperty(window, "synthVisuals", {
				configurable: true,
				get: () => visualBridge,
				set: (fixture) => {
					visualBridge = { ...visualDefaults, ...(fixture as Record<string, unknown>) };
				}
			});
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
