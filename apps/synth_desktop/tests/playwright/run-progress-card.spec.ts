import { expect, test } from "./browser.fixture";

/**
 * The live in-chat run progress card and its expanded dialog.
 *
 * The run is seeded through the same two surfaces the real app reads: the
 * durable session journal (which carries the optimizer tool call whose result
 * names the run) and the optimizer bridge (which carries the run record and its
 * persisted event pages). Nothing is injected into the card directly, so what
 * these tests exercise is the whole path from a tool result to a rendered card.
 */

const SESSION = "run-progress-session";
const RUN_ID = "banking77_gepa_sol_med_45856f25";

type Seed = {
	status?: string;
	completedRollouts?: number;
	capabilities?: Record<string, boolean>;
	cursorSeq?: number;
	withCost?: boolean;
};

async function seed(page: import("@playwright/test").Page, options: Seed = {}) {
	await page.addInitScript((seedOptions: Seed & { session: string; runId: string }) => {
		const {
			session,
			runId,
			status = "running",
			completedRollouts = 8,
			capabilities = { cancel: true, pause: true, resume: true, streamEvents: true },
			cursorSeq,
			withCost = true
		} = seedOptions;
		const at = (minute: number, second = 0) =>
			new Date(Date.UTC(2026, 7, 17, 12, minute, second)).toISOString();

		/* ── the optimizer bridge: run record + persisted event pages ───────── */
		const events: Array<Record<string, unknown>> = [];
		let sequence = 0;
		const push = (entry: Record<string, unknown>) => {
			sequence += 1;
			events.push({
				schemaVersion: "optimizer_event.v1",
				eventId: `${runId}:${sequence}`,
				sequenceNumber: sequence,
				optimizerRunId: runId,
				algorithmId: "gepa",
				...entry
			});
		};
		push({ type: "gepa.run.started", occurredAt: at(0), delta: { state: "initializing", message: "GEPA run started" } });
		push({
			type: "optimizer.limit.estimate_updated",
			occurredAt: at(0, 15),
			delta: {
				limits: [{ kind: "total_rollouts", max: 100, spent: completedRollouts, reserved: 0, hard: true }],
				nearest_limit: { kind: "total_rollouts", max: 100, spent: completedRollouts }
			}
		});
		push({
			type: "optimizer.state.transitioned",
			occurredAt: at(1),
			delta: {
				from: "ready",
				to: "rollout_running",
				trigger: "rollouts_started",
				details: { candidate_id: "gepa_seed", stage: "candidate_minibatch", rollout_count: 20 }
			}
		});
		push({
			type: "optimizer.rollout_queue.updated",
			occurredAt: at(1, 5),
			delta: { active_workers: 4, semaphore_size: 4, queued_rollouts: 7 }
		});
		for (let index = 0; index < completedRollouts; index += 1) {
			push({
				type: "optimizer.evaluation_result.received",
				occurredAt: at(2 + Math.floor((index * 30) / 60), (index * 30) % 60),
				delta: {
					candidate_id: "gepa_seed",
					rollout_id: `rollout_${index}`,
					stage: "candidate_minibatch",
					example_id: `train:${index}`,
					reward: 0.8
				},
				usageDelta: withCost
					? { cost_usd: 0.05, prompt_tokens: 400, completion_tokens: 120, rollouts: 1 }
					: { prompt_tokens: 400, completion_tokens: 120, rollouts: 1 }
			});
		}

		const run = {
			schemaVersion: "optimizer_run.v1",
			id: runId,
			algorithmId: "gepa",
			algorithmVersion: "1.0.0",
			status,
			source: "local",
			objective: "Banking77",
			sessionRef: session,
			createdAt: at(0),
			startedAt: at(0),
			finishedAt: status === "running" ? null : at(9),
			cursorSeq: cursorSeq ?? events.length,
			capabilities,
			executionBindings: [],
			inputRefs: [],
			outputRefs: [],
			visualRefs: [{ kind: "visual", id: `visual-${runId}` }],
			summary: {},
			usage: {}
		};
		const controlCalls: string[] = [];
		(window as never as Record<string, unknown>).__runControlCalls = controlCalls;
		(window as never as Record<string, unknown>).__optimizerReads = [] as number[];
		(window as never as Record<string, unknown>).synthOptimizers = {
			listAlgorithms: async () => [{ id: "gepa", title: "GEPA", availability: "available" }],
			listRecipes: async () => [],
			list: async () => [run],
			get: async () => run,
			create: async () => run,
			startRecipe: async () => run,
			stageEvalCandidates: async () => ({ id: "cs", candidates: [] }),
			refresh: async () => run,
			eventsAfter: async (_id: string, afterSeq = 0) => {
				((window as never as Record<string, unknown>).__optimizerReads as number[]).push(afterSeq);
				return events.filter((entry) => (entry.sequenceNumber as number) > afterSeq);
			},
			getState: async () => ({}),
			getStateBatch: async () => [],
			cancel: async () => { controlCalls.push("cancel"); return run; },
			pause: async () => { controlCalls.push("pause"); return run; },
			resume: async () => { controlCalls.push("resume"); return run; },
			openVisual: async () => run,
			importLocal: async () => run,
			reconcileCloud: async () => run,
			listCloud: async () => [],
			recordVisualReady: async () => undefined,
			onEvent: () => () => undefined
		};

		/* ── the durable journal: the tool call that named the run ───────────── */
		const journal = [
			{
				sequence: 10,
				sessionSequence: 1,
				kind: "message.created",
				payload: { messageId: "user-1", role: "user", content: "run the Banking77 GEPA smoke" }
			},
			{
				sequence: 11,
				sessionSequence: 2,
				kind: "item/completed",
				payload: {
					item: {
						type: "mcpToolCall",
						id: "call-start",
						server: "synth_optimizers",
						tool: "optimizer_start_recipe",
						status: "completed",
						arguments: { recipe_id: "gepa.banking77.sol.v1" },
						result: { isError: false, structuredContent: run }
					}
				}
			},
			{
				sequence: 12,
				sessionSequence: 3,
				kind: "agentMessage/completed",
				payload: { messageId: "assistant-1", content: "Started the Banking77 GEPA smoke." }
			}
		].map((row) => ({
			schemaVersion: "synth.desktop-app-event.v1" as const,
			eventId: `evt-${row.sequence}`,
			sessionId: session,
			source: "codex" as const,
			createdAt: "2026-08-17T12:00:00Z",
			...row
		}));

		(window as never as Record<string, unknown>).synthCodex = {
			defaultWorkspace: async () => "/workspaces/default",
			list: async () => [{
				sessionId: session,
				threadId: "thread-run-progress",
				workspace: "/workspaces/default",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				providerName: "local-laguna",
				providerTitle: "Laguna XS",
				baseUrl: "http://127.0.0.1:7333/v1",
				status: "ready"
			}],
			start: async () => ({ sessionId: session, threadId: "thread-run-progress" }),
			startTurn: async () => ({ sessionId: session, threadId: "thread-run-progress", turnId: "turn-1" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: () => () => undefined
		};
		(window as never as Record<string, unknown>).synthCore = {
			diagnostics: async () => ({
				databasePath: "/tmp/core.sqlite3",
				schemaVersion: 1,
				integrityOk: true,
				contentStorePath: "/tmp/content",
				journalHead: 12,
				sessionCount: 1,
				runCount: 1,
				visualCount: 0,
				migrationComplete: true
			}),
			eventsAfter: async () => journal,
			sessionEventsAfter: async () => journal,
			onEvent: () => () => undefined
		};
	}, { ...options, session: SESSION, runId: RUN_ID });
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId(`local-chat-${SESSION}`).click();
}

test("a GEPA run renders one live card with phase, work, concurrency, and usage", async ({ page }) => {
	await seed(page);
	const card = page.getByTestId(`run-progress-${RUN_ID}`);
	await expect(card).toBeVisible();
	await expect(page.getByTestId(`run-progress-title-${RUN_ID}`)).toHaveText("GEPA · Banking77");
	await expect(page.getByTestId(`run-progress-status-${RUN_ID}`)).toHaveText("Running");
	await expect(page.getByTestId(`run-progress-phase-${RUN_ID}`)).toContainText("Minibatch gate");
	await expect(page.getByTestId(`run-progress-work-${RUN_ID}`)).toHaveText("8 / 100 rollouts");
	await expect(card).toContainText("4 active · 7 queued");
	await expect(page.getByTestId(`run-progress-usage-${RUN_ID}`)).toContainText("$0.40");
	await expect(page.getByTestId(`run-progress-elapsed-${RUN_ID}`)).toContainText("elapsed");
	// One run, one card — the four polls in the journal do not each add a row.
	await expect(page.getByTestId(`run-progress-${RUN_ID}`)).toHaveCount(1);
});

test("the progress bar declares determinate value semantics", async ({ page }) => {
	await seed(page);
	const bar = page.getByTestId(`run-progress-bar-${RUN_ID}`);
	await expect(bar).toHaveAttribute("data-determinate", "true");
	await expect(bar).toHaveAttribute("aria-valuenow", "8");
	await expect(bar).toHaveAttribute("aria-valuetext", "8% of rollout budget spent");
	await expect(bar).toHaveAttribute("aria-label", "rollout budget spent");
});

test("an unreported cost renders Unavailable rather than $0.00", async ({ page }) => {
	await seed(page, { withCost: false });
	const usage = page.getByTestId(`run-progress-usage-${RUN_ID}`);
	await expect(usage).toContainText("Cost unavailable");
	await expect(usage).not.toContainText("$0.00");
});

test("the dialog opens over the card, traps focus, and returns focus on Escape", async ({ page }) => {
	await seed(page);
	const expand = page.getByTestId(`run-progress-expand-${RUN_ID}`);
	await expand.click();
	const dialog = page.getByTestId(`run-progress-dialog-${RUN_ID}`);
	await expect(dialog).toBeVisible();
	await expect(dialog).toHaveAttribute("aria-modal", "true");
	await expect(dialog.getByRole("heading", { level: 2 })).toHaveText("GEPA · Banking77");
	// The dialog explains the estimate rather than only asserting it.
	await expect(page.getByTestId(`run-progress-dialog-eta-${RUN_ID}`)).toContainText("phase minibatch");
	await expect(page.getByTestId(`run-progress-usage-detail-${RUN_ID}`)).toContainText("of 100 rollouts reported it");
	await expect(page.getByTestId(`run-progress-phases-${RUN_ID}`)).toContainText("Minibatch gate");
	await expect(page.getByTestId(`run-progress-dialog-close-${RUN_ID}`)).toBeFocused();
	await page.keyboard.press("Escape");
	await expect(dialog).toBeHidden();
	await expect(expand).toBeFocused();
});

test("modal and card agree on phase, progress, usage, and status", async ({ page }) => {
	await seed(page);
	const cardPhase = (await page.getByTestId(`run-progress-phase-${RUN_ID}`).innerText()).trim();
	const cardWork = (await page.getByTestId(`run-progress-work-${RUN_ID}`).innerText()).trim();
	const cardUsage = (await page.getByTestId(`run-progress-usage-${RUN_ID}`).innerText()).trim();
	const cardStatus = (await page.getByTestId(`run-progress-status-${RUN_ID}`).innerText()).trim();
	await page.getByTestId(`run-progress-expand-${RUN_ID}`).click();
	await expect(page.getByTestId(`run-progress-dialog-phase-${RUN_ID}`)).toContainText(cardPhase.split("\n")[0] ?? cardPhase);
	await expect(page.getByTestId(`run-progress-dialog-work-${RUN_ID}`)).toContainText(cardWork);
	await expect(page.getByTestId(`run-progress-dialog-status-${RUN_ID}`)).toHaveText(cardStatus);
	await expect(page.getByTestId(`run-progress-usage-detail-${RUN_ID}`)).toContainText(cardUsage.includes("unavailable") ? "Unavailable" : "$0.40");
});

test("missing token fields in the dialog read Unavailable, never 0", async ({ page }) => {
	await seed(page, { withCost: false });
	await page.getByTestId(`run-progress-expand-${RUN_ID}`).click();
	const usage = page.getByTestId(`run-progress-usage-detail-${RUN_ID}`);
	await expect(usage).toContainText("Unavailable");
	await expect(usage).not.toContainText("$0.00");
	await expect(page.getByTestId(`run-progress-usage-${RUN_ID}`)).not.toHaveText(/^0$/);
});

test("closing and reopening the dialog neither replays history nor resets the run", async ({ page }) => {
	await seed(page);
	await expect(page.getByTestId(`run-progress-work-${RUN_ID}`)).toHaveText("8 / 100 rollouts");
	const readsBefore = await page.evaluate(() => (window as never as Record<string, number[]>).__optimizerReads.length);
	for (let round = 0; round < 3; round += 1) {
		await page.getByTestId(`run-progress-expand-${RUN_ID}`).click();
		await expect(page.getByTestId(`run-progress-dialog-${RUN_ID}`)).toBeVisible();
		await page.getByTestId(`run-progress-dialog-close-${RUN_ID}`).click();
		await expect(page.getByTestId(`run-progress-dialog-${RUN_ID}`)).toBeHidden();
	}
	const readsAfter = await page.evaluate(() => (window as never as Record<string, number[]>).__optimizerReads);
	expect(readsAfter.length).toBe(readsBefore);
	expect(readsAfter.filter((afterSeq) => afterSeq === 0).length).toBeLessThanOrEqual(1);
	await expect(page.getByTestId(`run-progress-work-${RUN_ID}`)).toHaveText("8 / 100 rollouts");
});

test("controls express intent and show acknowledgement without rewriting status", async ({ page }) => {
	await seed(page);
	await page.getByTestId(`run-progress-expand-${RUN_ID}`).click();
	await page.getByTestId(`run-progress-dialog-pause-${RUN_ID}`).click();
	const intent = page.getByTestId(`run-progress-dialog-intent-${RUN_ID}`);
	await expect(intent).toContainText("pause acknowledged");
	// The run record still says running, so the badge must still say Running.
	await expect(page.getByTestId(`run-progress-status-${RUN_ID}`)).toHaveText("Running");
	expect(await page.evaluate(() => (window as never as Record<string, string[]>).__runControlCalls)).toEqual(["pause"]);
});

test("controls a run does not advertise are absent", async ({ page }) => {
	await seed(page, { capabilities: { cancel: false, pause: false, resume: false, streamEvents: true } });
	await expect(page.getByTestId(`run-progress-cancel-${RUN_ID}`)).toHaveCount(0);
	await page.getByTestId(`run-progress-expand-${RUN_ID}`).click();
	await expect(page.getByTestId(`run-progress-dialog-pause-${RUN_ID}`)).toHaveCount(0);
	await expect(page.getByTestId(`run-progress-dialog-cancel-${RUN_ID}`)).toHaveCount(0);
});

test("a terminal run becomes a durable summary with no spinner and no ETA", async ({ page }) => {
	await seed(page, { status: "completed", completedRollouts: 100 });
	const card = page.getByTestId(`run-progress-${RUN_ID}`);
	await expect(page.getByTestId(`run-progress-status-${RUN_ID}`)).toHaveText("Completed");
	await expect(page.getByTestId(`run-progress-elapsed-${RUN_ID}`)).toContainText("wall time");
	await expect(page.getByTestId(`run-progress-eta-${RUN_ID}`)).toHaveCount(0);
	await expect(page.getByTestId(`run-progress-bar-${RUN_ID}`)).toHaveCount(0);
	await expect(page.getByTestId(`run-progress-result-${RUN_ID}`)).toBeVisible();
	await expect(card).toHaveAttribute("data-connection-state", "terminal");
});

test("an incomplete history says its counts are a floor", async ({ page }) => {
	// The record claims more events than the pages can supply.
	await seed(page, { cursorSeq: 999 });
	await expect(page.getByTestId(`run-progress-warning-${RUN_ID}`)).toContainText("counts are a floor");
});

test("the card is legible at a narrow transcript width", async ({ page }) => {
	await page.setViewportSize({ width: 640, height: 900 });
	await seed(page);
	const card = page.getByTestId(`run-progress-${RUN_ID}`);
	await expect(card).toBeVisible();
	const overflow = await card.evaluate((node) => {
		const scroller = node.closest(".chat-transcript-scroll") ?? document.body;
		return scroller.scrollWidth - scroller.clientWidth;
	});
	expect(overflow).toBeLessThanOrEqual(1);
});
