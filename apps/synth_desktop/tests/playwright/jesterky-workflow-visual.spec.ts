import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, openVisual } from "./v02-helpers";

test("Jesterky workflow renders program state, shards, and retained identity", async ({ page }) => {
  const transition = (sequence: number, eventKind: string, node: string, index?: number) => ({
    schemaVersion: "evals.event-stream.v1", run_id: "jesterky-demo", kind: "jesterky.transition", sequence,
    logical_time: sequence, ts: "2026-09-16T12:00:00Z", payload: { event_kind: eventKind,
      node_path: [{ kind: "node", value: node }, ...(index == null ? [] : [{ kind: "index", value: index }])], iteration: 0, local_seq: sequence }
  });
  const events = [
    { schemaVersion: "evals.event-stream.v1", run_id: "jesterky-demo", kind: "stream.subscribed", sequence: null, ts: "2026-09-16T12:00:00Z", payload: { ready: true, "stream.id": "jesterky-demo" } },
    transition(1, "node_started", "expand"), transition(2, "node_completed", "expand"),
    transition(3, "node_started", "research"), transition(4, "map_item_started", "research", 0),
    transition(5, "map_item_completed", "research", 0), transition(6, "map_item_started", "research", 1)
  ];
  await installVisuals(page, [liveVisual({ id: "vis_jesterky", templateId: "workflow.jesterky.v1", title: "Local research swarm",
    bindings: { schemaVersion: "synth.visual-bindings.v1", inputs: [
      { input: "stream", kind: "inline", data: { events } },
      { input: "workflow", kind: "inline", data: { schemaVersion: "synth.jesterky-workflow.v1", workflowName: "local_research",
        runId: "jesterky-demo", status: "running", nodes: [
          { id: "expand", kind: "program" }, { id: "research", kind: "map", actor: "researcher", concurrency: 3 }, { id: "synthesize", kind: "reduce" }
        ] } }
    ] } })]);
  const viewer = (await openVisual(page, "vis_jesterky")).getByTestId("visual-workflow-jesterky");
  await expect(viewer).toBeVisible();
  await expect(viewer).toContainText("Local research swarm");
  await expect(viewer).toContainText("expand");
  await expect(viewer).toContainText("research");
  await expect(viewer).toContainText("synthesize");
  await expect(viewer).toContainText("1/3 complete");
  await viewer.getByRole("button", { name: /research/i }).click();
  await expect(viewer.getByLabel("Selected node")).toContainText("Actor calls");
  await expect(viewer.getByLabel("Recent transitions")).toContainText("map_item_started");
  await page.setViewportSize({ width: 900, height: 800 });
  const compactOverflow = await viewer.locator(".jk-graph").evaluate((graph) => {
    const parent = graph.getBoundingClientRect();
    return { clientWidth: graph.clientWidth, scrollWidth: graph.scrollWidth,
      nodes: [...graph.children].map((node) => { const rect = node.getBoundingClientRect();
        return { left: rect.left - parent.left, right: rect.right - parent.left }; }) };
  });
  expect(compactOverflow.scrollWidth).toBeLessThanOrEqual(compactOverflow.clientWidth + 1);
  expect(compactOverflow.nodes.every((node) => node.left >= 0 && node.right <= compactOverflow.clientWidth + 1)).toBe(true);
});
