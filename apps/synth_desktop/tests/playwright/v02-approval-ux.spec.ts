/**
 * v0.2 approval UX: a pending shell approval must pin above Working…,
 * expose Reject / Approve once, and terminalize when its origin stops.
 */
import { expect, test } from "./browser.fixture";

const sessionId = "v02-approval-session";

function journalEvent(sequence: number, kind: string, payload: Record<string, unknown>) {
	return {
		schemaVersion: "synth.desktop-app-event.v1" as const,
		eventId: `evt-${sequence}`,
		sessionId,
		source: "codex" as const,
		createdAt: `2026-08-13T13:00:0${sequence}Z`,
		sequence,
		sessionSequence: sequence,
		kind,
		payload
	};
}

async function installPendingApproval(page: import("@playwright/test").Page, running = true) {
	const events = [
		journalEvent(1, "message.created", { messageId: "user-1", role: "user", content: "list the workspace" }),
		journalEvent(2, "run.started", { runId: "turn-approval" }),
		journalEvent(3, "approval.requested", {
			approvalId: "appr_shell_1",
			kind: "shell_command",
			command: "ls /Users/joshuapurtell",
			detail: "ls /Users/joshuapurtell",
			alwaysSupported: true
		})
	];
	if (!running) {
		events.push(journalEvent(4, "run.cancelled", { runId: "turn-approval" }));
		events.push(journalEvent(5, "approval.expired", {
			approvalId: "appr_shell_1",
			kind: "shell_command",
			decision: "expired",
			reason: "origin_turn_ended"
		}));
	}
	await page.addInitScript(({ rows, live }) => {
		type Event = { sessionId: string; method: string; params: Record<string, unknown> };
		let listener: ((event: Event) => void) | undefined;
		(window as typeof window & { __emitApproval?: (event: Event) => void }).__emitApproval = (event) => listener?.(event);
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
				sessionId: "v02-approval-session",
				threadId: "v02-approval-thread",
				workspace: "/Users/joshuapurtell",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				providerName: "local-laguna",
				providerTitle: "Laguna XS Responses",
				baseUrl: "http://127.0.0.1:7333/v1",
				status: live ? "running" : "ready",
				title: "Waiting approval"
			}],
			start: async () => ({ sessionId: "v02-approval-session", threadId: "v02-approval-thread" }),
			startTurn: async () => ({ sessionId: "v02-approval-session", threadId: "v02-approval-thread", turnId: "turn-approval" }),
			interrupt: async () => undefined,
			close: async () => undefined,
			onEvent: (next: (event: Event) => void) => {
				listener = next;
				return () => { listener = undefined; };
			}
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
	}, { rows: events, live: running });
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId(`local-chat-${sessionId}`).click();
}

test("[v0.2] pending approval pins above Working and exposes Reject / Approve once", async ({ page }) => {
	await installPendingApproval(page, true);
	const card = page.getByTestId("approval-activity-3");
	await expect(card).toBeVisible();
	await expect(card).toContainText("Approval requested");
	await expect(card.getByRole("button", { name: "Reject" })).toBeVisible();
	await expect(card.getByRole("button", { name: "Approve once" })).toBeVisible();
	await expect(card.getByRole("button", { name: "Always allow for this session" })).toBeVisible();
	const working = page.getByTestId("model-working");
	await expect(working).toContainText("Working…");
	const cardBox = await card.boundingBox();
	const workingBox = await working.boundingBox();
	expect(cardBox, "approval card should have a box").toBeTruthy();
	expect(workingBox, "Working… should have a box").toBeTruthy();
	expect(cardBox!.y, "approval card must sit above Working…").toBeLessThan(workingBox!.y);
});

test("[v0.2] stopped turns show terminal approval history instead of dead buttons", async ({ page }) => {
	await installPendingApproval(page, false);
	await expect(page.getByTestId("model-working")).toHaveCount(0);
	await expect(page.locator(".approval-card")).toHaveCount(0);
	await expect(page.getByRole("button", { name: "Approve once" })).toHaveCount(0);
	await expect(page.getByText("Approval expired", { exact: true })).toBeVisible();
});
