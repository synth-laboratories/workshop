import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";

async function openSettings(page: Page) {
	await page.getByTestId("account-menu-trigger").click();
	await page.getByTestId("account-menu-settings").click();
}

/**
 * Design-debt and intended-architecture flags.
 *
 * - Plain `test(...)`: must pass — locks product intent.
 * - Expected-failing cases: known violations. Assert the *intended* end state so the
 *   case keeps failing until the stub is replaced. Flip to a plain `test`
 *   when fixed.
 *
 * Static greps for stub strings live in `tests/design_debt.test.mjs`.
 */

test.describe("design locks (must pass)", () => {
	test("composer model menu does not advertise deferred LoRA adapters", async ({ page }) => {
		await page.getByTestId("composer-model").click();
		const menu = page.getByTestId("composer-model-menu");
		await menu.getByTestId("composer-model-access-local").click();
		await expect(menu).toBeVisible();
		await expect(menu.getByText("Laguna LoRAs")).toHaveCount(0);
		await expect(menu.getByText("Remote LoRAs")).toHaveCount(0);
		await expect(menu.getByText("Base (no adapter)")).toHaveCount(0);
		await expect(menu.getByText("Craftax triage")).toHaveCount(0);
		await expect(page.getByTestId("open-finetunes-settings")).toHaveCount(0);
	});

	test("Settings has no Finetunes or adapter-placeholder UI", async ({ page }) => {
		await openSettings(page);
		await expect(page.getByTestId("settings-page")).toBeVisible();
		await expect(page.getByRole("button", { name: "Finetunes" })).toHaveCount(0);
		await expect(page.getByTestId("settings-finetunes")).toHaveCount(0);
		await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();
		await expect(page.getByTestId("settings-models")).toBeVisible();
		await expect(page.getByTestId("settings-models")).not.toContainText("Adapters");
	});

	test("Runtime settings are not exposed", async ({ page }) => {
		await openSettings(page);
		await expect(page.getByRole("button", { name: "Runtime", exact: true })).toHaveCount(0);
		await expect(page.getByTestId("settings-runtime")).toHaveCount(0);
	});

	test("About ships the current release changelog", async ({ page }) => {
		await openSettings(page);
		await page.getByRole("button", { name: "About", exact: true }).click();
		const changelog = page.getByTestId("about-changelog");
		await expect(changelog).toBeVisible();
		await expect(changelog).toContainText("Version 0.4.0");
		await expect(changelog).toContainText("Version 0.3.0");
		await expect(changelog).toContainText("Version 0.1.0");
		await expect(changelog).toContainText("New");
		await expect(changelog).toContainText("Improved");
		await expect(changelog).toContainText("Fixed");
		await expect(changelog).toContainText("Muse Spark");
		await expect(changelog).toContainText("passive stable-channel update check");
		await expect(changelog).toContainText("build provenance");
	});

	test("Inventory Attach container starts with empty name and URL", async ({ page }) => {
		await page.getByTestId("open-inventory").click();
		await expect(page.getByTestId("inventory-page")).toBeVisible();
		await page.getByTestId("attach-container").click();
		const form = page.locator(".inventory-attach-form");
		await expect(form).toBeVisible();
		await expect(form.locator('input[inputmode="url"]')).toHaveValue("");
		await expect(form.locator("input").first()).toHaveValue("");
	});

	test("Inventory Traces honestly scopes the v0.2 catalog as read-only", async ({ page }) => {
		await page.getByTestId("open-inventory").click();
		await page.getByTestId("inventory-tab-traces").click();
		await expect(page.getByTestId("trace-catalog-read-only")).toBeVisible();
		await expect(page.getByTestId("import-trace-v5")).toHaveCount(0);
		await expect(page.getByTestId("filter-traces")).toBeVisible();
	});

	test("sidebar account menu opens Account settings, not a dead control", async ({ page }) => {
		await page.getByTestId("account-menu-trigger").click();
		await page.getByTestId("open-account-settings").click();
		await expect(page.getByTestId("settings-page")).toBeVisible();
		// Account leads with the user-facing sections; endpoint/key configuration
		// is deliberately demoted behind Advanced connection but stays reachable.
		await expect(page.getByTestId("settings-account")).toBeVisible();
		await expect(page.getByTestId("account-page-profile")).toBeVisible();
		await expect(page.getByTestId("account-page-plan")).toBeVisible();
		await expect(page.getByTestId("backend-settings")).toBeHidden();
		await page.getByTestId("account-page-advanced").getByText("Advanced connection").click();
		await expect(page.getByTestId("backend-settings")).toBeVisible();
		await expect(page.getByText("Account — stub", { exact: true })).toHaveCount(0);
	});

	test("composer permission control selects and persists an approval policy", async ({ page }) => {
		await page.addInitScript(() => {
			window.synthLaguna = {
				getStatus: async () => ({
					phase: "ready",
					modelId: "laguna-xs",
					modelPath: "/tmp/model",
					detail: "ready",
					resident: true,
					idleFreeAt: null,
					memoryBytes: 1,
					discovered: []
				}),
				onStatus: () => () => undefined
			};
		});
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		const permission = page.getByTestId("approval-mode-select");
		await expect(permission).toBeEnabled();
		await permission.click();
		const menu = page.getByTestId("approval-mode-menu");
		await expect(menu).toBeVisible();
		await expect(menu.getByRole("option")).toHaveCount(6);
		await expect(menu.getByRole("option", { name: /Always ask/ })).toBeVisible();
		await expect(menu.getByRole("option", { name: /Full system access/ })).toBeVisible();
		await menu.getByRole("option", { name: /Ask for risky actions/ }).click();
		await menu.getByRole("option", { name: /Full system access/ }).click();
		await expect(permission).toHaveText("RiskyFull");
		await expect(permission).toHaveAttribute("aria-label", "Permissions: Ask for risky actions; Full system access");
		await expect(permission).toHaveCSS("white-space", "nowrap");
		expect((await permission.boundingBox())?.height ?? Number.POSITIVE_INFINITY).toBeLessThanOrEqual(32);
	});
});

test.describe("design debt (expected fail until fixed)", () => {
	test("titlebar has no Account menu or Expand stub chrome", async ({ page }) => {
		await expect(page.getByRole("button", { name: "Account menu" })).toHaveCount(0);
		await expect(page.getByRole("button", { name: "Expand" })).toHaveCount(0);
		await expect(page.getByText("Account menu — stub", { exact: true })).toHaveCount(0);
		await expect(page.getByText("Expand — stub", { exact: true })).toHaveCount(0);
		await page.getByTestId("account-menu-trigger").click();
		await expect(page.getByTestId("open-account-settings")).toBeVisible();
	});


	test("Settings has no legacy Python migration surface", async ({ page }) => {
		await openSettings(page);
		await expect(page.getByTestId("settings-runtime")).toHaveCount(0);
		await expect(page.getByText("Legacy Python Runtime Data")).toHaveCount(0);
		await expect(page.getByText("Legacy runtime.sqlite3 path")).toHaveCount(0);
		await expect(page.getByRole("button", { name: "Inspect migration" })).toHaveCount(0);
	});

	test("Landing has no Set up an agent stub card", async ({ page }) => {
		await expect(page.getByTestId("quick-setup-agent")).toHaveCount(0);
		await expect(page.getByText("Set up an agent", { exact: true })).toHaveCount(0);
		await expect(page.getByText("Set up agent — stub", { exact: true })).toHaveCount(0);
		await expect(page.getByTestId("landing-page")).toBeVisible();
	});

	test("Settings Reload Laguna invokes the bridge and reports completion", async ({ page }) => {
		await page.addInitScript(() => {
			(window as typeof window & { __lagunaReloads?: number }).__lagunaReloads = 0;
			const previous = window.synthLaguna;
			const ready = {
				phase: "ready" as const, baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
				loadedModel: "laguna-xs", detail: "Laguna XS reloaded and ready.", memoryBytes: 1,
				updatedAt: Date.now()
			};
			window.synthLaguna = {
				...(previous as object),
				getStatus: previous?.getStatus ?? (async () => ready),
				onStatus: previous?.onStatus ?? (() => () => undefined),
				reload: async () => {
					(window as typeof window & { __lagunaReloads: number }).__lagunaReloads += 1;
					return ready;
				}
			} as typeof window.synthLaguna;
		});
		await page.reload();
		await page.getByTestId("titlebar").waitFor();
		await openSettings(page);
		await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();
		await page.getByRole("button", { name: "Reload" }).click();
		await expect
			.poll(async () => page.evaluate(() => (window as typeof window & { __lagunaReloads?: number }).__lagunaReloads ?? 0))
			.toBeGreaterThan(0);
		await expect(page.getByTestId("laguna-reload-status")).toHaveAttribute("data-state", "ready");
		await expect(page.getByTestId("laguna-reload-status")).toContainText("reloaded and ready");
	});

	test("Settings Reload Laguna surfaces a bridge failure", async ({ page }) => {
		await page.addInitScript(() => {
			window.synthLaguna = {
				getStatus: async () => ({
					phase: "error", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
					loadedModel: null, detail: "Sidecar is unavailable.", memoryBytes: null, updatedAt: Date.now()
				}),
				reload: async () => { throw new Error("Sidecar is unavailable."); },
				onStatus: () => () => undefined,
				listModels: async () => [], chooseModelDirectory: async () => null,
				setModelDirectory: async () => { throw new Error("unused"); }, clearModelDirectory: async () => undefined
			};
		});
		await page.reload();
		await openSettings(page);
		await page.getByTestId("settings-page").getByRole("button", { name: "Models" }).click();
		await page.getByRole("button", { name: "Reload" }).click();
		const status = page.getByTestId("laguna-reload-status");
		await expect(status).toHaveAttribute("data-state", "error");
		await expect(status).toHaveAttribute("role", "alert");
		await expect(status).toContainText("Sidecar is unavailable.");
	});

	test("async leave-safe banner is projection-driven, not `!isSync`", async ({ page }) => {
		const source = await page.evaluate(async () => {
			const response = await fetch("/src/components/CloudDesk.tsx");
			return response.ok ? response.text() : "";
		});
		expect(source.length).toBeGreaterThan(100);
		expect(source).not.toMatch(/const leaveSafe = !isSync/);
		expect(source).toMatch(/props\.intern\.leaveSafe === true/);
	});

	test("a needs-input Async Intern is unreachable in v0.1", async ({ page }) => {
		const asyncSession = {
			id: "async-needs-input",
			title: "Async Intern",
			target: { kind: "intern", mode: "async" },
			remoteId: "smr.intern-async-runtime.v1/needs-input",
			status: "waiting_for_input",
			latestCursor: 0,
			createdAt: "2026-08-09T12:00:00.000Z",
			updatedAt: "2026-08-09T12:00:00.000Z",
			metadata: { runtime: "rust-intern" }
		};
		await page.addInitScript((session) => {
			window.synthRuntime = {
				async request(path: string) {
					if (path === "/v1/health") return {
						runtimeId: "renderer-test", local: { mode: "unavailable", modelPath: null },
						intern: { mode: "remote" }, openrouter: { mode: "unconfigured" },
						inventory: { containers: 0, traces: 0, visuals: 0 }
					};
					if (path === "/v1/sessions") return { sessions: [session] };
					if (path === "/v1/projects") return { projects: [] };
					if (path.startsWith(`/v1/sessions/${session.id}/events`)) return { events: [], nextSequence: 0, hasMore: false };
					throw new Error(`Unexpected renderer test request: ${path}`);
				},
				async subscribe() { return { close() {} }; }
			};
		}, asyncSession);
		await page.reload();
		/*
		 * v0.1 removal contract: even with a needs-input Async Intern session
		 * present in the runtime, no surface may expose it. The dormant
		 * CloudDesk intervention control keeps its own honesty assertion in
		 * design_debt.test.mjs so v0.2 re-entry stays covered.
		 */
		await expect(page.getByTestId("sidebar")).toBeVisible();
		await expect(page.getByTestId("async-intern-pin")).toHaveCount(0);
		await expect(page.getByTestId("cloud-list")).toHaveCount(0);
		await expect(page.getByTestId("new-sync-session")).toHaveCount(0);
		await page.getByTestId("open-search").click();
		await expect(page.getByTestId("conversation-search")).toBeVisible();
		await expect(page.getByText("Background Intern", { exact: true })).toHaveCount(0);
	});

	test("agent-authored analysis visuals render the persisted type-block payload from CUA", async ({ page }) => {
		await page.addInitScript(() => {
			const visual = {
				schemaVersion: "synth.desktop-visual.v1",
				id: "laguna-prompt-trim-preinstall",
				currentRevision: 1,
				title: "Laguna Prompt Trim Preinstall",
				templateId: "analysis.visual.v1",
				status: "draft",
				rendererKind: "template",
				bindings: {
					spec: {
						title: "Laguna Prompt Trim Preinstall",
						blocks: [
							{ type: "metrics", items: [
								{ label: "Visual schemas before", value: "13" },
								{ label: "Advertised tools after", value: "1" }
							] },
							{ type: "note", text: "Compact visual operations load only when needed." }
						]
					}
				},
				sessionId: null,
				messageId: null,
				runId: null,
				traceId: null,
				parentVisualId: null,
				sourceAgentId: "laguna",
				sourceModel: "laguna-xs-2.1",
				contentDigest: null,
				previewDigest: null,
				metadata: {},
				createdAt: "2026-08-09T13:24:48.000Z",
				updatedAt: "2026-08-09T13:24:48.000Z"
			};
			window.synthVisuals = {
				listTemplates: async () => [{ id: "analysis.visual.v1", title: "Agent-authored analysis", genre: "analysis" }],
				getTemplate: async () => ({ id: "analysis.visual.v1", title: "Agent-authored analysis" }),
				list: async () => [visual], get: async () => visual, revisions: async () => [],
				create: async () => visual, update: async () => visual, save: async () => visual,
				fork: async () => visual, archive: async () => visual, show: async () => visual,
				onEvent: () => () => undefined, onShow: () => () => undefined
			} as typeof window.synthVisuals;
		});
		await page.reload();
		await page.getByTestId("open-visuals").click();
		const preview = page.getByTestId("visuals-preview");
		await expect(preview.getByTestId("visual-analysis-spec")).toBeVisible();
		await expect(preview.getByTestId("visual-invalid")).toHaveCount(0);
	});

	test("malformed analysis blocks skip instead of crashing the visual host", async ({ page }) => {
		// CUA found a persisted ranked-bars payload without `items`; the old
		// shell cast and called `.map` on undefined. Normalize drops the bad
		// block and still renders valid siblings.
		await page.addInitScript(() => {
			const visual = {
				schemaVersion: "synth.desktop-visual.v1",
				id: "malformed-ranked-bars",
				currentRevision: 1,
				title: "Malformed ranked bars",
				templateId: "analysis.visual.v1",
				status: "draft",
				rendererKind: "template",
				bindings: {
					spec: {
						title: "Malformed ranked bars",
						blocks: [
							{ type: "ranked-bars", title: "Broken" },
							{ type: "note", text: "Still visible after skipping the bad block." }
						]
					}
				},
				sessionId: null,
				messageId: null,
				runId: null,
				traceId: null,
				parentVisualId: null,
				sourceAgentId: "laguna",
				sourceModel: "laguna-xs-2.1",
				contentDigest: null,
				previewDigest: null,
				metadata: {},
				createdAt: "2026-08-12T00:00:00.000Z",
				updatedAt: "2026-08-12T00:00:00.000Z"
			};
			window.synthVisuals = {
				listTemplates: async () => [{ id: "analysis.visual.v1", title: "Agent-authored analysis", genre: "analysis" }],
				getTemplate: async () => ({ id: "analysis.visual.v1", title: "Agent-authored analysis" }),
				list: async () => [visual], get: async () => visual, revisions: async () => [],
				create: async () => visual, update: async () => visual, save: async () => visual,
				fork: async () => visual, archive: async () => visual, show: async () => visual,
				onEvent: () => () => undefined, onShow: () => () => undefined
			} as typeof window.synthVisuals;
		});
		await page.reload();
		await page.getByTestId("open-visuals").click();
		const preview = page.getByTestId("visuals-preview");
		await expect(preview.getByTestId("visual-analysis-spec")).toBeVisible();
		await expect(preview.getByText("Still visible after skipping the bad block.")).toBeVisible();
		await expect(preview.getByTestId("visual-invalid")).toHaveCount(0);
	});

	test("Attaching a container in the browser harness registers via inventory, not a dead POST", async ({ page }) => {
		await page.addInitScript(() => {
			const timestamp = "2026-08-09T00:00:00Z";
			const containers: Array<Record<string, unknown>> = [];
			window.synthInventory = {
				async listContainers() { return containers as never; },
				async getContainer(id: string) {
					const hit = containers.find((row) => row.id === id);
					if (!hit) throw new Error("missing");
					return hit as never;
				},
				async registerContainer(request: { name?: string; baseUrl: string }) {
					const row = {
						id: "attached-craftax",
						name: request.name ?? "Craftax Rust",
						location: "local",
						status: "ready",
						baseUrl: request.baseUrl,
						taskFamily: "craftax-singleplayer",
						health: { ok: true },
						metadata: { info: { lane: "rust", env_family: "craftax-singleplayer" }, hydratedAt: timestamp },
						createdAt: timestamp,
						updatedAt: timestamp
					};
					containers.splice(0, containers.length, row);
					return row as never;
				},
				async probeContainer(id: string) { return this.getContainer(id); },
				async listTraces() { return []; },
				async getTrace() { throw new Error("none"); },
				async chooseTraceInput() { return null; },
				async ingestTraceBundle() { throw new Error("unused"); },
				async resolveTraceProjection() { throw new Error("unused"); },
				async listUsage() { return []; },
				async counts() { return { containers: containers.length, traces: 0, usage: 0 }; }
			};
			window.synthVisuals ??= { async list() { return []; } } as typeof window.synthVisuals;
		});
		await page.reload();
		await page.getByTestId("open-inventory").click();
		await page.getByTestId("attach-container").click();
		await page.locator(".inventory-attach-form button[type='submit']").click();
		await expect(page.getByTestId("inventory-container-attached-craftax")).toBeVisible();
		await page.getByRole("button", { name: "Inspect Craftax Rust" }).click();
		await expect(page.getByTestId("container-pane")).toBeVisible();
		await expect(page.getByTestId("container-pane")).toContainText("craftax-singleplayer");
		const pane = page.getByTestId("container-pane");
		const before = await pane.boundingBox();
		const handle = page.getByRole("separator", { name: "Resize container inspector" });
		const handleBox = await handle.boundingBox();
		if (!before || !handleBox) throw new Error("container split geometry unavailable");
		await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + 80);
		await page.mouse.down();
		await page.mouse.move(handleBox.x - 120, handleBox.y + 80, { steps: 4 });
		await page.mouse.up();
		const after = await pane.boundingBox();
		expect(after?.width ?? 0).toBeGreaterThan(before.width + 80);
	});

	test("Opening a Trace V5 row shows a first-class rollout inspector visual idempotently", async ({ page }) => {
		await page.addInitScript(() => {
			const timestamp = "2026-08-09T00:00:00Z";
			const digest = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
			let created: Record<string, unknown> | null = null;
			(window as typeof window & { __traceVisualCreates?: number }).__traceVisualCreates = 0;
			window.synthInventory = {
				async listContainers() { return []; },
				async getContainer() { throw new Error("none"); },
				async registerContainer() { throw new Error("none"); },
				async probeContainer() { throw new Error("none"); },
				async listTraces() {
					return [{
						id: "debt-trace",
						digest,
						title: "Design-debt Trace V5",
						source: "local",
						reward: 1.5,
						metrics: [],
						metadata: {},
						createdAt: timestamp
					}];
				},
				async getTrace() { return (await this.listTraces())[0]; },
				async chooseTraceInput() { return null; },
				async ingestTraceBundle() { throw new Error("unused"); },
				async resolveTraceProjection(traceDigest: string) {
					return {
						traceDigest,
						projectionKind: "rollout-inspector",
						projectionSchema: "synth.trace-projection.rollout-inspector.v1",
						payloadDigest: "sha256:projectionabcdef",
						relativePath: "projections/rollout-inspector/example.json",
						payload: {
							schema_version: "synth.trace-projection.rollout-inspector.v1",
							trace_id: "debt-trace",
							trace_digest: traceDigest,
							visual: {
								lanes: [{ lane_id: "lane-1", title: "agent" }],
								items: [
									{ item_id: "command-start", kind: "codex.command_started", title: "Run command", sequence: 1, lane_id: "lane-1", detail: { command: "python solve.py" } },
									{ item_id: "command-end", kind: "codex.command_finished", title: "Command complete", sequence: 2, lane_id: "lane-1", detail: { exit_code: 0 } },
									{ item_id: "verifier", kind: "evidence.verifier_result", title: "Verifier passed", sequence: 3, detail: { score: 1 } }
								]
							}
						}
					};
				},
				async listUsage() { return []; },
				async counts() { return { containers: 0, traces: 1, usage: 0 }; }
			};
			window.synthVisuals = {
				async list() { return created ? [created] : []; },
				async listTemplates() {
					return [{ id: "trace.rollout_inspector.v1", title: "Trace rollout inspector", genre: "trace", path: "", exampleBinding: {} }];
				},
				async create(input) {
					(window as typeof window & { __traceVisualCreates: number }).__traceVisualCreates += 1;
					created = {
						id: input.id,
						templateId: input.templateId,
						title: input.title ?? "Trace visual",
						traceId: input.traceId,
						bindings: input.bindings ?? {},
						status: "open",
						createdAt: timestamp,
						updatedAt: timestamp,
						metadata: input.metadata ?? {}
					};
					return created as never;
				},
				async get() {
					if (!created) throw new Error("missing");
					return created as never;
				},
				async update(_visualId, input) {
					created = { ...created, ...input, updatedAt: timestamp };
					return created as never;
				},
				async show() { return created as never; },
				async save() { return created as never; },
				async fork() { return created as never; },
				async archive() { return created as never; },
				async revisions() { return []; },
				onShow() { return () => undefined; }
			} as typeof window.synthVisuals;
		});
		await page.reload();
		await page.getByTestId("open-inventory").click();
		await page.getByTestId("inventory-tab-traces").click();
		await page.getByTestId("open-trace-debt-trace").click();
		await expect(page.getByTestId("visual-trace-rollout-inspector")).toBeVisible();
		await expect(page.getByTestId("inventory-traces")).toBeVisible();
		const inventoryBox = await page.getByTestId("inventory-page").boundingBox();
		const viewerBox = await page.getByTestId("visual-trace-rollout-inspector").boundingBox();
		expect(inventoryBox?.width ?? 0).toBeGreaterThan(350);
		expect(viewerBox?.width ?? 0).toBeGreaterThan(350);
		await expect(page.getByTestId("visual-trace-rollout-inspector")).toContainText("Tool calls2");
		await expect(page.getByTestId("visual-trace-rollout-inspector")).toContainText("Evidence1");
		await page.getByRole("button", { name: "Play playback" }).click();
		await expect(page.getByRole("button", { name: "Pause playback" })).toBeVisible();

		await page.getByTestId("open-trace-debt-trace").click();
		await expect(page.getByTestId("visual-trace-rollout-inspector")).toBeVisible();
		await expect.poll(async () => page.evaluate(() =>
			(window as typeof window & { __traceVisualCreates?: number }).__traceVisualCreates ?? 0
		)).toBe(1);
	});
});
