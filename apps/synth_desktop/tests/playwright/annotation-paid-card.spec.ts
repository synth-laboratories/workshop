/**
 * Post-eval annotation paid card: the modal must name the campaign, container,
 * job count and cap, then Approve once must call the native resolver.
 */
import { expect, test } from "./browser.fixture";

const sessionId = "annotation-paid-card-session";

function journalEvent(sequence: number, kind: string, payload: Record<string, unknown>) {
	return {
		schemaVersion: "synth.desktop-app-event.v1" as const,
		eventId: `evt-${sequence}`,
		sessionId,
		source: "codex" as const,
		createdAt: `2026-09-01T15:00:0${sequence}Z`,
		sequence,
		sessionSequence: sequence,
		kind,
		payload
	};
}

test("annotation campaign paid card click-through resolves once", async ({ page }) => {
	const events = [
		journalEvent(1, "message.created", { messageId: "user-1", role: "user", content: "run the eval with annotations" }),
		journalEvent(2, "run.started", { runId: "turn-annotation" }),
		journalEvent(3, "approval.requested", {
			approvalId: "appr_annotation_campaign_1",
			kind: "paid_compute",
			operation: "annotation.post_rollout_campaign",
			requestingAgent: "eval-worker",
			estimatedCostUsdMicros: 2_000_000,
			requestedCap: { maxCostUsdMicros: 2_000_000 },
			parameters: {
				runId: "eval_run_1",
				containerId: "evals-banking77",
				jobs: 2,
				estimate: { max_cost_usd: 2 }
			},
			alwaysSupported: false
		})
	];
	await page.addInitScript(({ rows }) => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		const decisions: Array<{ sessionId: string; approvalId: string; decision: string }> = [];
		(window as typeof window & { __approvalDecisions?: () => typeof decisions }).__approvalDecisions = () => decisions;
		(window as typeof window & { synthLaguna?: unknown }).synthLaguna = {
			getStatus: async () => ({
				phase: "ready",
				baseUrl: "http://127.0.0.1:7333",
				backend: "mlx_lm",
				loadedModel: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				detail: "Laguna XS ready",
				memoryBytes: null,
				updatedAt: Date.now()
			}),
			onStatus: () => () => undefined,
			listModels: async () => []
		};
		(window as typeof window & { synthCodex?: unknown }).synthCodex = {
			defaultWorkspace: async () => "/Users/joshuapurtell",
			list: async () => [{
				sessionId: "annotation-paid-card-session",
				threadId: "annotation-paid-card-thread",
				workspace: "/Users/joshuapurtell",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				providerName: "local-laguna",
				providerTitle: "Laguna XS Responses",
				baseUrl: "http://127.0.0.1:7333/v1",
				status: "running",
				title: "Annotating eval"
			}],
			start: async () => ({ sessionId: "annotation-paid-card-session", threadId: "annotation-paid-card-thread" }),
			startTurn: async () => ({ sessionId: "annotation-paid-card-session", threadId: "annotation-paid-card-thread", turnId: "turn-annotation" }),
			interrupt: async () => undefined,
			resolveApproval: async (id: string, approvalId: string, decision: string) => {
				decisions.push({ sessionId: id, approvalId, decision });
			},
			close: async () => undefined,
			onEvent: (_next: (event: Event) => void) => () => undefined
		};
		(window as typeof window & { synthCore?: unknown }).synthCore = {
			diagnostics: async () => ({
				databasePath: "/tmp/core.sqlite3",
				schemaVersion: 1,
				integrityOk: true,
				contentStorePath: "/tmp/content",
				journalHead: rows.length,
				sessionCount: 1,
				runCount: 1,
				visualCount: 0,
				migrationComplete: true
			}),
			eventsAfter: async () => rows,
			sessionEventsAfter: async () => rows,
			onEvent: () => () => undefined
		};
	}, { rows: events });
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId(`local-chat-${sessionId}`).click();

	const modal = page.getByTestId("paid-compute-approval-modal");
	await expect(modal).toBeVisible();
	await expect(modal).toContainText("Approve this paid annotation?");
	await expect(modal).toContainText("evals-banking77");
	await expect(modal).toContainText("annotation.post_rollout_campaign");
	await expect(modal).toContainText("$2.00");
	await expect(modal.getByRole("button", { name: "Approve", exact: true })).toBeVisible();
	await expect(modal.getByRole("button", { name: "Reject" })).toBeVisible();
	await modal.getByRole("button", { name: "Approve", exact: true }).click();
	await expect(modal).toBeHidden();
	expect(await page.evaluate(() => (window as typeof window & { __approvalDecisions: () => unknown[] }).__approvalDecisions())).toEqual([
		{ sessionId, approvalId: "appr_annotation_campaign_1", decision: "once" }
	]);
});
