import { test as base, _electron as electron, type ElectronApplication, type Page } from "@playwright/test";
import electronPath from "electron";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

type Fixtures = {
	electronApp: ElectronApplication;
	page: Page;
};

const appRoot = resolve(import.meta.dirname, "../..");
const workshopRoot = resolve(appRoot, "../..");

async function stopRuntime(runtimeHome: string): Promise<void> {
	try {
		const connection = JSON.parse(
			await readFile(resolve(runtimeHome, "connection.json"), "utf8")
		) as { url: string; token?: string | null };
		await fetch(`${connection.url.replace(/\/$/, "")}/v1/shutdown`, {
			method: "POST",
			headers: connection.token
				? { Authorization: `Bearer ${connection.token}` }
				: undefined
		});
	} catch {
		// A failed boot has no daemon to stop.
	}
}

export const test = base.extend<Fixtures>({
	electronApp: async ({}, use) => {
		const runtimeHome = await mkdtemp(resolve(tmpdir(), "synth-playwright-"));
		const app = await electron.launch({
			executablePath: electronPath,
			args: [appRoot],
			cwd: workshopRoot,
			env: {
				...process.env,
				SYNTH_RUNTIME_HOME: runtimeHome,
				SYNTH_INTERN_DEMO: "1",
				SYNTH_LAGUNA_AUTO_START: "0",
				SYNTH_WORKSHOP_ROOT: workshopRoot,
				SYNTH_VISUALS_ROOT: resolve(workshopRoot, "visuals")
			}
		});
		try {
			await use(app);
		} finally {
			await app.close().catch(() => undefined);
			await stopRuntime(runtimeHome);
			await rm(runtimeHome, { recursive: true, force: true });
		}
	},
	page: async ({ electronApp }, use) => {
		const page = await electronApp.firstWindow();
		await page.getByTestId("runtime-status").waitFor();
		await use(page);
	}
});

export { expect } from "@playwright/test";
