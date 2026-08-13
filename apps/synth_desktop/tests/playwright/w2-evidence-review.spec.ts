import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "./browser.fixture";

const PROOF_DIR = resolve(import.meta.dirname, "../../../../docs/receipts/2026-08-12/w2-visual-proof");

const visual = {
	schemaVersion: "synth.desktop-visual.v1",
	id: "vis_ff81361fa5f54ec6b92c2df286be41f0",
	title: "W2 rollout evidence review · evidence summary revision",
	templateId: "trace.rollout_inspector.v1",
	status: "draft",
	createdAt: "2026-08-12T14:00:00Z",
	updatedAt: "2026-08-12T14:00:06Z",
	bindings: {
		schemaVersion: "synth.visual-bindings.v1",
		slots: [{
			slot: "projection", kind: "inline", schema: "synth.trace-projection.rollout-inspector.v1",
			data: {
				schema_version: "synth.trace-projection.rollout-inspector.v1",
				trace_id: "trace_w2_review_fixture", trace_digest: "sha256:w2-prestart-review-evidence",
				evidence_digest: "sha256:w2-evaluator-evidence",
				visual: {
					run_id: "run_w2_review", task_id: "workshop-w2", state: "completed", visibility_ceiling: "operator",
					summary: { visual_item_count: 5, review_surface: "evidence-summary-v2" },
					usage: { requests: 2, prompt_tokens: 1280, completion_tokens: 340, cached_tokens: 512, provenance: "reported" },
					lanes: [
						{ lane_id: "policy", display_name: "Policy", actor_kind: "model", detail: { status: "completed", coverage: { messages: "complete", tools: "complete" } } },
						{ lane_id: "evaluator", display_name: "Evaluator", actor_kind: "evaluator", detail: { status: "completed", coverage: { evidence: "complete" } } }
					],
					items: [
						{ item_id: "m1", kind: "message.user", title: "Task opened", sequence: 1, lane_id: "policy", occurred_at: "2026-08-12T14:00:00Z", detail: { text: "Inspect the declared environment and preserve exact evidence." } },
						{ item_id: "t1", kind: "tool.command_completed", title: "Probe environment", status: "completed", sequence: 2, lane_id: "policy", occurred_at: "2026-08-12T14:00:02Z", detail: { native: { command: "container_probe --declared-endpoint" }, output: "healthy=true\ncapabilities=advertised" } },
						{ item_id: "m2", kind: "codex.agent_message", title: "Policy response", sequence: 3, lane_id: "policy", occurred_at: "2026-08-12T14:00:04Z", detail: { text: "Used only the server-advertised environment and evaluator." } },
						{ item_id: "e1", kind: "evidence.verdict", title: "Policy pin verified", status: "passed", sequence: 4, lane_id: "evaluator", occurred_at: "2026-08-12T14:00:05Z", detail: { verdict: "pass", score: 1, rationale: "Exact advertised policy and evaluator matched the prepared descriptor." } },
						{ item_id: "e2", kind: "evidence.receipt", title: "Durable evidence sealed", status: "complete", sequence: 5, lane_id: "evaluator", occurred_at: "2026-08-12T14:00:06Z", detail: { trace_id: "trace_w2_review_fixture", evidence_url: "cas://w2/evidence" } }
					]
				}
			}
		}]
	},
	metadata: {}
};

test("W2 revised evidence review is rendered and legible wide and compact", async ({ page }) => {
	await page.addInitScript((record) => {
		(window as any).synthVisuals = {
			listTemplates: async () => [{ id: "trace.rollout_inspector.v1", title: "Trace rollout inspector", genre: "trace", path: "", exampleBinding: null }],
			list: async () => [record], get: async () => record, create: async () => record, update: async () => record,
			show: async () => record, archive: async () => record, bind: async () => record,
			onEvent: () => () => undefined, onShow: () => () => undefined
		};
	}, visual);
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-visuals").click();
	await page.getByTestId(`visuals-card-${visual.id}`).getByRole("button", { name: "Open" }).click();
	mkdirSync(PROOF_DIR, { recursive: true });

	for (const state of [{ name: "wide", width: 1280, height: 900 }, { name: "compact", width: 768, height: 1024 }]) {
		await page.setViewportSize({ width: state.width, height: state.height });
		if (state.name === "compact") await page.getByTestId("toggle-visual-expand").click();
		await page.waitForTimeout(250);
		const summary = page.getByTestId("visual-pane").getByTestId("trace-evidence-summary");
		await expect(summary).toBeVisible();
		await expect(summary).toContainText("2/2 decisive · digest bound");
		await expect(summary).toContainText("Policy pin verified");
		await expect(summary).toContainText("Exact advertised policy and evaluator matched the prepared descriptor.");
		const geometry = await page.evaluate(() => ({
			scrollWidth: document.documentElement.scrollWidth,
			clientWidth: document.documentElement.clientWidth,
			summary: document.querySelector('[data-testid="visual-pane"] [data-testid="trace-evidence-summary"]')?.getBoundingClientRect().toJSON()
		}));
		expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.clientWidth + 1);
		expect(geometry.summary?.left).toBeGreaterThanOrEqual(0);
		expect(geometry.summary?.right).toBeLessThanOrEqual(state.width + 1);
		await page.screenshot({ path: resolve(PROOF_DIR, `${state.name}-${state.width}x${state.height}.png`), fullPage: true });
		if (state.name === "compact") await page.getByTestId("toggle-visual-expand").click();
	}
});
