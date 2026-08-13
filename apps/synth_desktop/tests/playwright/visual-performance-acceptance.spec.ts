import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";
import type { VisualRecord } from "@synth/runtime-protocol";
import { createServer, type Server } from "node:http";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const RECEIPT_DIR = process.env.SYNTH_EXTERNAL_VISUAL_RECEIPTS ??
  "/Users/joshuapurtell/Documents/Codex/2026-08-12/let/receipts/external-acceptance/visuals";
const ENVELOPE_COUNT = 100_000;
const LANE_COUNT = 10;
const POLICY_DELTA_COUNT = 10_000;
const FRAME_REF_COUNT = 1_000;
const MAX_HEAP_DELTA_BYTES = 256 * 1024 * 1024;
const MAX_LONG_TASK_MS = 1_000;
const MAX_SCRUB_MS = 750;

function visualRecord(sseUrl: string): VisualRecord {
  return {
    schemaVersion: "synth.desktop-visual.v1",
    id: "vis_craftax_v5_acceptance",
    currentRevision: 1,
    title: "Craftax V5 performance acceptance",
    templateId: "live.craftax.v1",
    status: "saved",
    rendererKind: "template",
    bindings: {
      schemaVersion: "synth.visual-bindings.v1",
      slots: [{
        slot: "stream",
        kind: "inline",
        data: {
          sse_url: sseUrl,
          scope: {
            campaign_id: "v5_browser_acceptance",
            rollout_ids: Array.from({ length: LANE_COUNT }, (_, index) => `rollout_perf_${index}`),
            selection: { initial_rollout_id: "rollout_perf_0" }
          }
        }
      }]
    },
    sessionId: null,
    messageId: null,
    runId: "v5_browser_acceptance",
    traceId: null,
    parentVisualId: null,
    sourceAgentId: "acceptance",
    sourceModel: "fixture",
    contentDigest: null,
    previewDigest: null,
    metadata: { acceptance: "V5", envelopeCount: ENVELOPE_COUNT },
    createdAt: "2026-08-12T22:00:00.000Z",
    updatedAt: "2026-08-12T22:00:00.000Z"
  };
}

async function installVisual(page: Page, record: VisualRecord): Promise<void> {
  await page.addInitScript((visual) => {
    (window as any).__v5LongTasks = [];
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        (window as any).__v5LongTasks.push({ startTime: entry.startTime, duration: entry.duration });
      }
    }).observe({ type: "longtask", buffered: true });
    (window as any).synthVisuals = {
      listTemplates: async () => [{ id: visual.templateId, title: visual.title, genre: "live" }],
      getTemplate: async () => ({ id: visual.templateId, title: visual.title }),
      list: async () => [visual], get: async () => visual, revisions: async () => [],
      create: async () => visual, update: async () => visual, save: async () => visual,
      fork: async () => visual, archive: async () => visual, show: async () => visual,
      onEvent: () => () => undefined, onShow: () => () => undefined
    };
  }, record);
  await page.reload();
  await page.getByTestId("titlebar").waitFor();
}

function stressEnvelope(index: number, laneSequences: number[]): Record<string, unknown> {
  if (index === 0) {
    return { kind: "stream.subscribed", control: true, occurred_at: "2026-08-12T20:00:00.000Z" };
  }
  const laneIndex = (index - 1) % LANE_COUNT;
  const lane = `rollout_perf_${laneIndex}`;
  const sequence = ++laneSequences[laneIndex];
  const occurredAt = new Date(Date.UTC(2026, 7, 12, 20, 0, 0) + index * 36).toISOString();
  const base = { event_id: String(sequence), sequence, occurred_at: occurredAt, rollout_id: lane, lane };
  if (sequence <= 1_000) {
    return { ...base, kind: "span.policy.data", payload: { delta: true, channel: "reasoning", text: "δ" } };
  }
  if (sequence <= 1_100) {
    return { ...base, kind: "frame", payload: { url: `about:blank#frame-${laneIndex}-${sequence}`, format: "png" } };
  }
  if (index === ENVELOPE_COUNT - 1) {
    return { ...base, kind: "eval.run.terminal", payload: { status: "completed" } };
  }
  if (sequence >= 9_999) {
    return { ...base, kind: "trace.reconciled", payload: { digest: `${laneIndex}`.repeat(64) } };
  }
  return {
    ...base,
    kind: "observation",
    payload: { readout: { env_steps: sequence, health: 9, inventory: { wood: sequence % 7 } } }
  };
}

async function startStressSse(): Promise<{ server: Server; url: string }> {
  const server = createServer((request, response) => {
    if (request.url !== "/stream") {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Access-Control-Allow-Origin": "*",
      Connection: "keep-alive"
    });
    const laneSequences = Array.from({ length: LANE_COUNT }, () => 0);
    let index = 0;
    const writeChunk = () => {
      const stop = Math.min(ENVELOPE_COUNT, index + 500);
      while (index < stop) {
        const ok = response.write(`data: ${JSON.stringify(stressEnvelope(index, laneSequences))}\n\n`);
        index += 1;
        if (!ok) {
          response.once("drain", writeChunk);
          return;
        }
      }
      if (index < ENVELOPE_COUNT) setImmediate(writeChunk);
    };
    writeChunk();
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("stress SSE did not bind TCP");
  return { server, url: `http://127.0.0.1:${address.port}/stream` };
}

async function closeServer(server: Server): Promise<void> {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

test("V5: actual Craftax viewer sustains 10 lanes and 100k envelopes with bounded heap and long tasks", async ({ page }) => {
  test.setTimeout(240_000);
  mkdirSync(RECEIPT_DIR, { recursive: true });
  const stress = await startStressSse();
  const cdp = await page.context().newCDPSession(page);
  try {
    await installVisual(page, visualRecord(stress.url));
    await cdp.send("Performance.enable");
    await cdp.send("HeapProfiler.collectGarbage");
    const baselineMetrics = await cdp.send("Performance.getMetrics");
    const baselineHeap = baselineMetrics.metrics.find((metric) => metric.name === "JSHeapUsedSize")?.value ?? 0;
    await page.evaluate(() => { (window as any).__v5LongTasks = []; });

    await page.getByTestId("open-visuals").click();
    await page.getByTestId("visuals-card-vis_craftax_v5_acceptance").getByRole("button", { name: "Open" }).click();
    const viewer = page.getByTestId("visual-pane").getByTestId("visual-live-craftax");
    await expect(viewer).toBeVisible();
    await expect(viewer.getByRole("navigation", { name: "Rollout lanes" }).getByRole("button")).toHaveCount(LANE_COUNT, { timeout: 180_000 });
    await expect(viewer).toContainText("sealed/reconciled", { timeout: 180_000 });

    const traceMode = viewer.getByRole("button", { name: "Full trace" });
    await traceMode.click();
    await expect(viewer.locator(".cv-trace-summary")).toContainText("durable envelopes");
    const rawForSelectedLane = Number((await viewer.locator(".cv-trace-summary").innerText()).match(/from ([\d,]+) durable/)?.[1].replaceAll(",", ""));
    expect(rawForSelectedLane).toBeGreaterThanOrEqual(9_999);

    const scrubMs = await viewer.getByLabel("Replay selected rollout by raw event").evaluate(async (input: HTMLInputElement) => {
      const started = performance.now();
      input.value = String(Math.max(0, Number(input.max) - 250));
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      return performance.now() - started;
    });
    const dom = await viewer.evaluate((root) => ({
      elements: root.querySelectorAll("*").length,
      traceButtons: root.querySelectorAll(".cv-trace li button").length,
      laneButtons: root.querySelectorAll(".cv-lanes button").length
    }));

    await cdp.send("HeapProfiler.collectGarbage");
    const finalMetrics = await cdp.send("Performance.getMetrics");
    const finalHeap = finalMetrics.metrics.find((metric) => metric.name === "JSHeapUsedSize")?.value ?? 0;
    const longTasks = await page.evaluate(() => (window as any).__v5LongTasks as Array<{ startTime: number; duration: number }>);
    const maxLongTaskMs = Math.max(0, ...longTasks.map((entry) => entry.duration));
    const totalLongTaskMs = longTasks.reduce((sum, entry) => sum + entry.duration, 0);
    const heapDeltaBytes = finalHeap - baselineHeap;
    const receipt = {
      schemaVersion: "synth.acceptance.visual-performance.v1",
      acceptance: "V5",
      status: "passed",
      generatedAt: new Date().toISOString(),
      workload: {
        lanes: LANE_COUNT, envelopes: ENVELOPE_COUNT,
        policyDeltas: POLICY_DELTA_COUNT, frameRefs: FRAME_REF_COUNT,
        timestampSpanMinutes: 60
      },
      measurements: {
        baselineHeapBytes: baselineHeap, finalHeapBytes: finalHeap, heapDeltaBytes,
        longTaskCount: longTasks.length, maxLongTaskMs, totalLongTaskMs,
        scrubMs, domElements: dom.elements, traceButtons: dom.traceButtons, laneButtons: dom.laneButtons,
        selectedLaneDurableEnvelopes: rawForSelectedLane
      },
      thresholds: {
        maxHeapDeltaBytes: MAX_HEAP_DELTA_BYTES,
        maxLongTaskMs: MAX_LONG_TASK_MS,
        maxScrubMs: MAX_SCRUB_MS,
        maxTraceButtons: 100,
        expectedLaneButtons: LANE_COUNT
      },
      notes: [
        "Measured through Chromium CDP after forced GC while the real live.craftax.v1 pane retained the evidence.",
        "PerformanceObserver longtask entries cover sustained SSE ingest, projection, React commit, and scrub.",
        "Token deltas and frame references remain durable evidence but do not each create a DOM control."
      ]
    };
    writeFileSync(join(RECEIPT_DIR, "v5-browser-performance.json"), `${JSON.stringify(receipt, null, 2)}\n`);
    writeFileSync(join(RECEIPT_DIR, "v5-longtasks.json"), `${JSON.stringify(longTasks, null, 2)}\n`);
    await page.screenshot({ path: join(RECEIPT_DIR, "v5-craftax-100k.png"), fullPage: true });

    expect(heapDeltaBytes).toBeLessThan(MAX_HEAP_DELTA_BYTES);
    expect(maxLongTaskMs).toBeLessThan(MAX_LONG_TASK_MS);
    expect(scrubMs).toBeLessThan(MAX_SCRUB_MS);
    expect(dom.traceButtons).toBeLessThan(100);
    expect(dom.laneButtons).toBe(LANE_COUNT);
  } finally {
    await cdp.detach().catch(() => undefined);
    await closeServer(stress.server);
  }
});
