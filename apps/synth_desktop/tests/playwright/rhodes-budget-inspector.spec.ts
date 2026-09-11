/** Operational replay facts must not turn an unknown scientific reward into zero. */
import { expect, test } from "./browser.fixture";

test("Rhodes inspector separates reward and operational receipts", async ({ page }, testInfo) => {
  await page.addInitScript(() => {
    localStorage.setItem("synth.preferences.v1", JSON.stringify({ navigation: { visiblePluginIds: ["optimizers"] } }));
    const run = {
      schemaVersion: "optimizer_run.v1", id: "rhodes_budget_fixture", algorithmId: "eval",
      status: "failed", source: "rhodes", objective: "Rhodes budget visibility fixture",
      createdAt: "2026-09-11T05:00:00Z", cursorSeq: 8,
      capabilities: {}, executionBindings: [], inputRefs: [], outputRefs: [], visualRefs: [], usage: {},
      summary: { rhodes: {
        rolloutId: "rollout_budget_fixture", sourceSequence: 6, cleanupPending: false,
        publicationPending: false, inferencePending: true, drained: false,
        resultSnapshot: { score: null, limits: { max_calls: 0, timeout_s: 300 }, acceptedExecutionLimits: { timeout_s: 0 }, requiredLimitCapabilities: [], executionDeadlineAt: "2026-09-11T05:00:30Z", resultPublication: { status: "committed" },
          summary: { trace_publication: { status: "failed" } } },
        lastLimitRefusal: { event_type: "rollout.limit_refused", error_code: "spend_cap_exhausted",
          scope: { kind: "project", id: "fixture", revision: 2 }, sequence: 5 },
        lastResourceIntent: { event_type: "rollout.resource_intent", provider: "docker", owner: "synth-harbor-231220f655d342b5ad8872c10366471f", sequence: 1 },
        lastExecutionPhase: { event_type: "rollout.phase", phase: "verification", state: "exited", exit_code: 0, sequence: 7 },
        lastInferenceAccounting: { event_type: "rollout.inference_accounting", accounting_status: "uncertain_post_dispatch", sequence: 6 }
      } }
    };
    (window as any).synthOptimizers = {
      listAlgorithms: async () => [{ id: "eval", title: "Eval", availability: "available" }],
      list: async () => [run], get: async () => run, refresh: async () => run,
      listRecipes: async () => [], listCloud: async () => [], eventsAfter: async () => [],
      getState: async () => ({}), getStateBatch: async () => [], onEvent: () => () => undefined,
      runViewV2: async () => { throw new Error("Fixture has only durable run observations"); }
    };
  });
  await page.reload();
  await page.getByTestId("titlebar").waitFor();
  await page.getByTestId("open-optimizers").click();
  const inspector = page.getByTestId("rhodes-evaluation-status");
  await expect(inspector).toBeVisible();
  await expect(inspector).toContainText("spend_cap_exhausted · project");
  await expect(inspector).toContainText("uncertain_post_dispatch");
  await expect(inspector).toContainText("Awaiting reconciliation");
  for (const [label, value] of [["Latest sandbox intent", "docker"], ["Sandbox owner", "synth-harbor-231220f655d342b5ad8872c10366471f"], ["Latest execution phase", "verification · exited · exit 0"], ["Score", "—"], ["Cleanup", "Confirmed"], ["Result publication", "committed"], ["Trace publication", "failed"], ["Accepted work limit", "0 s"], ["Required guarantees", "None declared"]]) {
    await expect(inspector.locator("dt", { hasText: new RegExp(`^${label}$`) }).locator("+ dd")).toHaveText(value);
  }
  await inspector.locator("dt", { hasText: /^Latest execution phase$/ }).scrollIntoViewIfNeeded();
  await expect(inspector.locator("dt", { hasText: /^Latest execution phase$/ }).locator("+ dd")).toBeVisible();
  await inspector.screenshot({ path: testInfo.outputPath("rhodes-phase-inspector.png") });
  await inspector.locator("dt", { hasText: /^Required guarantees$/ }).scrollIntoViewIfNeeded();
  await expect(inspector.locator("dt", { hasText: /^Required guarantees$/ }).locator("+ dd")).toBeVisible();
  await inspector.screenshot({ path: testInfo.outputPath("rhodes-budget-inspector.png") });
});
