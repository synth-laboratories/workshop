import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";

async function installSourceHost(page: Page) {
	const pageErrors: string[] = [];
	page.on("pageerror", (error) => pageErrors.push(error.message));
	await page.evaluate(() => {
		const host = window as typeof window & { __sourceCalls?: Array<{ name: string; args: any }>; __sourceApproval?: string; __TAURI_INTERNALS__?: unknown };
		const row = (path: string, origin = "configured") => ({ path, containers: true, recipes: true, origin,
			inspection: { path, status: "valid", code: null, message: null, containers: ["fixture"], recipes: ["eval.fixture"] }, lastScannedAt: null });
		const catalog = { configPath: "/fixture/config.toml", sources: [row("/fixture/approved")], implicitRoots: [row("/fixture/launcher", "environment")] };
		let requests = [{ id: "request-1", sessionId: "chat-1", requestedPath: "/fixture/requested", canonicalPath: "/fixture/requested",
			reason: "Run the declared fixture", containers: false, recipes: true, attachToConversation: true, status: "pending", createdAt: "2026-09-10", resolvedAt: null }];
		host.__sourceCalls = [];
		host.__sourceApproval = "cancel";
		(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
		host.__TAURI_INTERNALS__ = { transformCallback: () => 1, invoke: async (name: string, args: any) => {
			if (name === "plugin:event|listen") return 1;
			if (name === "plugin:event|unlisten") return;
			host.__sourceCalls!.push({ name, args });
			if (name === "project_sources_get" || name === "project_sources_refresh") return structuredClone(catalog);
			if (name === "project_source_requests_list") return structuredClone(requests);
			if (name === "project_source_add") { catalog.sources.push({ ...row("/fixture/new"), containers: args.containers, recipes: args.recipes }); return structuredClone(catalog); }
			if (name === "project_source_remove") { catalog.sources = catalog.sources.filter((source) => source.path !== args.path); return structuredClone(catalog); }
			if (name === "project_source_deny") { const request = requests[0]; requests = []; return { ...request, status: "denied", resolvedAt: "2026-09-10" }; }
			if (name === "project_source_approve") {
				if (host.__sourceApproval === "cancel") return null;
				if (host.__sourceApproval === "mismatch") throw new Error("selected folder does not match the exact requested folder");
				const request = { ...requests[0], status: "approved", resolvedAt: "2026-09-10" };
				requests = []; catalog.sources.push(row("/fixture/requested"));
				return structuredClone({ request, catalog, source: catalog.sources.at(-1), scope: null, attachmentError: "Source approved, but conversation attachment failed: fixture failure" });
			}
			throw new Error(`Unexpected native call: ${name}`);
		} };
	});
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
	await page.getByTestId("settings-nav-workspace").click();
	await expect(page.getByTestId("project-source-request")).toBeVisible();
	return pageErrors;
}

test("source controls preserve capability choices and distinguish launcher grants", async ({ page }) => {
	const errors = await installSourceHost(page);
	const panel = page.getByTestId("project-sources-settings");
	await expect(panel.getByText("Not scanned yet")).toHaveCount(0);
	await expect(panel.getByRole("button", { name: "Remove project source /fixture/launcher", exact: true })).toHaveCount(0);
	await panel.getByRole("checkbox", { name: "Recipes", exact: true }).uncheck();
	await panel.getByTestId("add-project-source").click();
	await expect(panel.getByRole("button", { name: "Remove project source /fixture/new", exact: true })).toBeVisible();
	const calls = await page.evaluate(() => (window as any).__sourceCalls);
	expect(calls.find((call: any) => call.name === "project_source_add").args).toEqual({ containers: true, recipes: false });
	await panel.getByRole("button", { name: "Remove project source /fixture/new", exact: true }).click();
	await expect(panel.getByRole("button", { name: "Remove project source /fixture/new", exact: true })).toHaveCount(0);
	await panel.getByRole("button", { name: "Deny", exact: true }).click();
	await expect(panel.getByText("No pending source requests.")).toBeVisible();
	expect(errors).toEqual([]);
});

test("unavailable native controls do not present an empty request list as verified", async ({ page }) => {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
	await page.getByTestId("settings-nav-workspace").click();
	const panel = page.getByTestId("project-sources-settings");
	await expect(panel.getByRole("alert")).toContainText("require Synth Desktop");
	await expect(panel.getByText("Source requests are unavailable.")).toBeVisible();
	await expect(panel.getByText("No pending source requests.")).toHaveCount(0);
});

test("cancelled or mismatched picker does not settle a request; partial attachment failure is visible", async ({ page }) => {
	const errors = await installSourceHost(page);
	const panel = page.getByTestId("project-sources-settings");
	const approve = panel.getByRole("button", { name: "Choose exact folder and approve…", exact: true });
	await expect(panel.getByText(/Also attach with read\/write access to conversation chat-1/)).toBeVisible();
	await approve.click();
	await expect(approve).toBeEnabled();
	await expect(page.getByTestId("project-source-request")).toBeVisible();
	await page.evaluate(() => { (window as any).__sourceApproval = "mismatch"; });
	await approve.click();
	await expect(panel.getByRole("alert")).toContainText("does not match");
	await expect(page.getByTestId("project-source-request")).toBeVisible();
	await page.evaluate(() => { (window as any).__sourceApproval = "approve"; });
	await approve.click();
	await expect(panel.getByRole("status")).toContainText("Source approved, but conversation attachment failed");
	await expect(page.getByTestId("project-source-request")).toHaveCount(0);
	const calls = await page.evaluate(() => (window as any).__sourceCalls.filter((call: any) => call.name === "project_source_approve"));
	expect(calls).toHaveLength(3);
	for (const call of calls) expect(call.args).toEqual({ requestId: "request-1" });
	await panel.screenshot({ path: "test-results/project-sources-settings.png" });
	expect(errors).toEqual([]);
});
