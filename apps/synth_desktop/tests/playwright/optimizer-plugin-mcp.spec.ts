import { expect, test } from "./browser.fixture";

test("plugin phases remain visible and Optimizers nav hides when disabled", async ({ page }) => {
	await page.addInitScript(() => {
		let phase = "downloading";
		let releaseChannel = "official";
		const pluginStatus = () => ({
			schemaVersion: "synth.plugin-status.v1",
			pluginId: "optimizers",
			enabled: true,
			phase,
			installedVersion: phase === "not_installed" ? null : "0.2.5",
			selectedVersion: releaseChannel === "dev" ? "0.2.9.dev20260814" : "0.2.5",
			releaseChannel,
			catalogVersion: releaseChannel === "dev" ? "0.2.9.dev20260814" : "0.2.5",
			digest: phase === "installed" || phase === "ready" ? "sha256:abc" : null,
			service: { phase, activeRuns: 0 },
			algorithms: ["gepa"],
			templates: ["optimizer.gepa.live.v1"]
		});
		(window as any).synthPlugins = {
			status: async () => pluginStatus(),
			list: async () => [],
			setReleaseChannel: async (_pluginId: string, next: string) => {
				releaseChannel = next;
				return pluginStatus();
			}
		};
		(window as any).__setPluginPhase = (next: string) => { phase = next; };
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [],
			listRecipes: async () => [],
			list: async () => [],
			get: async () => { throw new Error("unused"); },
			create: async () => { throw new Error("unused"); },
			startRecipe: async () => { throw new Error("unused"); },
			refresh: async () => { throw new Error("unused"); },
			eventsAfter: async () => [],
			getState: async () => ({}),
			getStateBatch: async () => [],
			cancel: async () => { throw new Error("unused"); },
			pause: async () => { throw new Error("unused"); },
			resume: async () => { throw new Error("unused"); },
			openVisual: async () => { throw new Error("unused"); },
			importLocal: async () => { throw new Error("unused"); },
			reconcileCloud: async () => { throw new Error("unused"); },
			listCloud: async () => [],
			onEvent: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-optimizers").click();
	await expect(page.getByTestId("optimizer-plugin-phase")).toHaveText("Downloading");
	await page.evaluate(() => (window as any).__setPluginPhase("verifying"));
	await expect(page.getByTestId("optimizer-plugin-phase")).toHaveText("Verifying");
	await page.evaluate(() => (window as any).__setPluginPhase("installed"));
	await expect(page.getByTestId("optimizer-plugin-phase")).toHaveText("Installed");
	await expect(page.getByTestId("optimizer-plugin-status")).toContainText("sha256:abc");
	await page.getByTestId("optimizer-release-channel").selectOption("dev");
	await expect(page.getByTestId("optimizer-release-warning")).toBeVisible();
	await expect(page.getByTestId("optimizer-plugin-status")).toContainText("0.2.9.dev20260814");
});

const disabledPlugin = {
	schemaVersion: "synth.plugin-status.v1",
	pluginId: "optimizers",
	enabled: false,
	phase: "disabled",
	releaseChannel: "official",
	catalogVersion: "0.2.5",
	service: { phase: "stopped", activeRuns: 0 },
	algorithms: [],
	templates: []
};

// A disabled plugin used to vanish from the sidebar, which left the user with
// no way to find out where it went or turn it back on. It stays, says why, and
// still opens its page.
test("disabled Optimizers plugin stays navigable and says why", async ({ page }) => {
	await page.addInitScript((plugin) => {
		(window as any).synthPlugins = {
			status: async () => plugin,
			list: async () => [plugin],
			setReleaseChannel: async () => { throw new Error("unused"); }
		};
	}, disabledPlugin);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();

	const row = page.getByTestId("open-optimizers");
	await expect(row).toHaveCount(1);
	await expect(page.getByTestId("plugin-status-optimizers")).toContainText("Disabled");

	await row.click();
	await expect(page.getByTestId("optimizers-page")).toBeVisible();
});

test("a disabled plugin holding live runs does not imply they stopped", async ({ page }) => {
	await page.addInitScript((plugin) => {
		const busy = { ...plugin, service: { phase: "ready", activeRuns: 2 } };
		(window as any).synthPlugins = {
			status: async () => busy,
			list: async () => [busy],
			setReleaseChannel: async () => { throw new Error("unused"); }
		};
	}, disabledPlugin);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await expect(page.getByTestId("plugin-status-optimizers")).toContainText("2 running");
});

test("the Plugins section replaces the Research and Data groupings", async ({ page }) => {
	await expect(page.getByTestId("plugins-nav")).toHaveCount(1);
	await expect(page.getByTestId("research-nav")).toHaveCount(0);
	await expect(page.getByTestId("inventory-nav")).toHaveCount(0);
	for (const testId of ["open-visuals", "open-reports", "open-optimizers", "open-inventory"]) {
		await expect(page.getByTestId(testId)).toHaveCount(1);
	}
});

test("optimizer visual posts a subscription receipt after replay", async ({ page }) => {
	await page.addInitScript(() => {
		const now = "2026-08-14T16:00:00.000Z";
		const run = {
			schemaVersion: "optimizer_run.v1",
			id: "banking77_ready",
			algorithmId: "gepa",
			status: "waiting_for_viewer",
			source: "local",
			objective: "Ready gate",
			createdAt: now,
			cursorSeq: 0,
			capabilities: { streamEvents: true },
			executionBindings: [],
			inputRefs: [],
			outputRefs: [],
			visualRefs: [{ kind: "visual", id: "visual-banking77_ready" }],
			summary: {},
			usage: {}
		};
		(window as any).__visualReady = [];
		(window as any).synthOptimizers = {
			listAlgorithms: async () => [{ id: "gepa", title: "GEPA", availability: "available" }],
			listRecipes: async () => [],
			list: async () => [run],
			get: async () => run,
			create: async () => run,
			startRecipe: async () => run,
			refresh: async () => run,
			eventsAfter: async () => [],
			getState: async () => ({}),
			getStateBatch: async () => [],
			cancel: async () => run,
			pause: async () => run,
			resume: async () => run,
			openVisual: async () => run,
			importLocal: async () => run,
			reconcileCloud: async () => run,
			listCloud: async () => [],
			recordVisualReady: async (request: unknown) => {
				(window as any).__visualReady.push(request);
				return request;
			},
			onEvent: () => () => undefined
		};
		(window as any).synthVisuals = {
			get: async () => ({
				schemaVersion: "synth.desktop-visual.v1",
				id: "visual-banking77_ready",
				templateId: "optimizer.gepa.live.v1",
				title: "Banking77 GEPA",
				status: "saved",
				createdAt: now,
				updatedAt: now,
				bindings: {
					schemaVersion: "synth.visual-bindings.v1",
					slots: [{ slot: "optimizer_run", kind: "optimizer_run", source: "banking77_ready" }]
				},
				metadata: {}
			}),
			onEvent: () => () => undefined,
			onShow: () => () => undefined
		};
	});
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-optimizers").click();
	await page.getByTestId("open-optimizer-visual").click();
	await expect.poll(() => page.evaluate(() => (window as any).__visualReady.length)).toBeGreaterThan(0);
	const receipt = await page.evaluate(() => (window as any).__visualReady[0]);
	expect(receipt.optimizerRunId).toBe("banking77_ready");
	expect(receipt.subscribedFrom).toBe(receipt.replayedThrough + 1);
	await expect(page.getByTestId("visual-connection-state")).toHaveText(/subscribed|terminal/);
});
