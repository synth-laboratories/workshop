import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "./browser.fixture";
import type { Page } from "@playwright/test";
import type { VisualRecord } from "@synth/runtime-protocol";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const RECEIPT_DIR = process.env.SYNTH_EXTERNAL_VISUAL_RECEIPTS ??
  "/Users/joshuapurtell/Documents/Codex/2026-08-12/let/receipts/external-acceptance/visuals";
const VISUAL_ROOT = resolve(import.meta.dirname, "../../../../visuals");

type Json = Record<string, any>;
type Fixture = { id: string; family: string; templateId: string; data: Json; testId: string };

function json(path: string): Json {
  return JSON.parse(readFileSync(join(VISUAL_ROOT, path), "utf8"));
}

function geloFixture(): Json {
  return {
    run: {
      id: "gelo_accessibility", algorithmId: "go-ex", status: "running",
      objective: "Craftax GELO accessibility", cursorSeq: 1, source: "hosted"
    },
    events: [{
      type: "goex.state.batch.updated", sequenceNumber: 1,
      occurredAt: "2026-08-12T20:00:00Z", optimizerRunId: "gelo_accessibility", algorithmId: "go-ex",
      snapshot: { slices: {
        board: { data: { phase: "core_proposal", tick: 2 } },
        themes: { data: { themes: [{ theme_id: "survival", title: "Survival" }] } },
        candidates: { data: { candidates: [{ candidate_id: "cand_1", reward_mean: 0.5, prompt_text: "Prioritize shelter." }] } },
        frontier: { data: { candidate_frontier: { global: ["cand_1"] } } },
        agents: { data: { coreProposer: { status: "running", round_index: 1 } } },
        "data-engine": { data: { child_streams: [{
          rollout_id: "rollout_craftax_1", candidate_id: "cand_1", seed: 101,
          split: "train", state: "running", reward: null,
          stream: { id: "stream:rollout_craftax_1", transports: { poll: { url: "/rollouts/rollout_craftax_1/events" } } }
        }] } }
      } }
    }]
  };
}

function digbenchFixture(): Json {
  const fixture = json("templates/live.digbench.v1/examples/events.json");
  const events = fixture.events as Json[];
  return {
    ...fixture,
    replay_ms: 10,
    events: [
      ...events.map((event) => ({ ...event, lane: "basic-react", run_id: "digbench_basic" })),
      ...events.map((event) => ({ ...event, lane: "agentic-mcp", run_id: "digbench_agentic" }))
    ]
  };
}

const FIXTURES: Fixture[] = [
  { id: "vis_a11y_gepa", family: "GEPA", templateId: "optimizer.run.v1", data: json("templates/optimizer.run.v1/examples/gepa_events.json"), testId: "visual-optimizer-run" },
  { id: "vis_a11y_gelo", family: "GELO", templateId: "optimizer.run.v1", data: geloFixture(), testId: "visual-optimizer-run" },
  { id: "vis_a11y_sft", family: "SFT", templateId: "optimizer.run.v1", data: json("templates/optimizer.run.v1/examples/sft_events.json"), testId: "visual-optimizer-run" },
  { id: "vis_a11y_craftax", family: "Craftax", templateId: "live.craftax.v1", data: { ...json("templates/live.craftax.v1/examples/events.json"), replay_ms: 10 }, testId: "visual-live-craftax" },
  { id: "vis_a11y_harbor", family: "Harbor", templateId: "live.harbor_eval.v1", data: { ...json("templates/live.harbor_eval.v1/examples/events.json"), replay_ms: 10 }, testId: "visual-live-harbor-eval" },
  { id: "vis_a11y_digbench", family: "dig.bench", templateId: "live.digbench.v1", data: digbenchFixture(), testId: "visual-live-digbench" }
];

function record(fixture: Fixture): VisualRecord {
  return {
    schemaVersion: "synth.desktop-visual.v1", id: fixture.id, currentRevision: 1,
    title: `${fixture.family} accessibility acceptance`, templateId: fixture.templateId,
    status: "saved", rendererKind: "template",
    bindings: { schemaVersion: "synth.visual-bindings.v1", slots: [{
      slot: fixture.templateId === "optimizer.run.v1" ? "optimizer_run" : "stream",
      kind: "inline", data: fixture.data
    }] },
    sessionId: null, messageId: null, runId: fixture.id, traceId: null,
    parentVisualId: null, sourceAgentId: "acceptance", sourceModel: "fixture",
    contentDigest: null, previewDigest: null, metadata: { acceptance: "V6", family: fixture.family },
    createdAt: "2026-08-12T22:00:00.000Z", updatedAt: "2026-08-12T22:00:00.000Z"
  };
}

async function installVisuals(page: Page): Promise<void> {
  const records = FIXTURES.map(record);
  await page.addInitScript((visuals) => {
    (window as any).synthVisuals = {
      listTemplates: async () => visuals.map((visual) => ({ id: visual.templateId, title: visual.title, genre: "acceptance" })),
      getTemplate: async (templateId: string) => ({ id: templateId, title: templateId }),
      list: async () => visuals,
      get: async (id: string) => visuals.find((visual) => visual.id === id),
      revisions: async () => [], create: async () => visuals[0], update: async () => visuals[0],
      save: async () => visuals[0], fork: async () => visuals[0], archive: async () => visuals[0],
      show: async (id: string) => visuals.find((visual) => visual.id === id),
      onEvent: () => () => undefined, onShow: () => () => undefined
    };
  }, records);
  await page.reload();
  await page.getByTestId("titlebar").waitFor();
  await page.getByTestId("open-visuals").click();
}

async function openFixture(page: Page, fixture: Fixture) {
  await page.getByTestId(`visuals-card-${fixture.id}`).getByRole("button", { name: "Open" }).click();
  const pane = page.getByTestId("visual-pane");
  const visual = pane.getByTestId(fixture.testId);
  await expect(visual).toBeVisible();
  return { pane, visual };
}

test("V6: axe, browser AX tree, keyboard names, focus, reduced motion, and 200% zoom", async ({ page }) => {
  test.setTimeout(180_000);
  mkdirSync(RECEIPT_DIR, { recursive: true });
  await installVisuals(page);
  const cdp = await page.context().newCDPSession(page);
  const receipt: Json = {
    schemaVersion: "synth.acceptance.visual-accessibility.v1",
    acceptance: "V6", status: "passed", generatedAt: new Date().toISOString(),
    screenReaderEvidence: {
      kind: "chromium_accessibility_tree",
      claim: "Browser accessibility-tree evidence; this is not a claim that macOS VoiceOver was manually operated."
    },
    families: []
  };
  try {
    await cdp.send("Accessibility.enable");
    for (const fixture of FIXTURES) {
      const { pane, visual } = await openFixture(page, fixture);
      await page.getByTestId("toggle-visual-expand").evaluate((button: HTMLButtonElement) => button.click());
      await expect(visual).toBeVisible();
      const axe = await new AxeBuilder({ page })
        .include('[data-testid="visual-pane"]')
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        .analyze();
      const blocking = axe.violations.filter((violation) => violation.impact === "critical" || violation.impact === "serious");

      const focusables = await visual.locator('button, input, select, textarea, summary, a[href], [tabindex]:not([tabindex="-1"])').evaluateAll((elements) =>
        elements.filter((element) => !(element as HTMLButtonElement).disabled).map((element) => ({
          tag: element.tagName.toLowerCase(),
          name: (element.getAttribute("aria-label") ?? element.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 180),
          role: element.getAttribute("role")
        }))
      );
      expect(focusables.every((item) => item.name.length > 0), `${fixture.family} unnamed controls`).toBe(true);

      const firstFocusable = visual.locator('button:not(:disabled), input:not(:disabled), select:not(:disabled), summary, a[href]').first();
      let focusIndicator: Json | null = null;
      if (await firstFocusable.count()) {
        await firstFocusable.focus();
        // Return to the control through keyboard navigation so :focus-visible
        // is evaluated in keyboard modality rather than pointer modality.
        await page.keyboard.press("Tab");
        await page.keyboard.press("Shift+Tab");
        focusIndicator = await firstFocusable.evaluate((element) => {
          const style = getComputedStyle(element);
          return { outlineStyle: style.outlineStyle, outlineWidth: style.outlineWidth, outlineColor: style.outlineColor, boxShadow: style.boxShadow };
        });
        expect(
          focusIndicator.outlineStyle !== "none" && focusIndicator.outlineWidth !== "0px" || focusIndicator.boxShadow !== "none",
          `${fixture.family} focus indicator`
        ).toBe(true);
      }

      // Keyboard operation receipts for each interaction family: selection,
      // native disclosure, and range scrubbing are exercised without clicks.
      if (fixture.family === "GEPA") {
        const candidate = visual.locator('[data-testid^="optimizer-candidate-"]').first();
        await candidate.focus();
        await page.keyboard.press("Enter");
        await expect(candidate).toHaveAttribute("aria-pressed", "true");
      } else if (fixture.family === "GELO") {
        const disclosure = visual.getByTestId("gelo-candidate-frontier").locator("summary").first();
        await disclosure.focus();
        await page.keyboard.press("Enter");
        await expect(disclosure.locator("..")).toHaveAttribute("open", "");
      } else if (fixture.family === "SFT") {
        const scrub = visual.getByLabel("Historical scrub");
        await scrub.focus();
        await page.keyboard.press("ArrowLeft");
        await expect(scrub).toBeFocused();
      } else if (fixture.family === "Craftax") {
        const lane = visual.getByRole("navigation", { name: "Rollout lanes" }).getByRole("button").first();
        await expect(lane).toBeVisible();
        await lane.focus();
        await page.keyboard.press("Enter");
        await expect(lane).toHaveAttribute("aria-current", "true");
      } else if (fixture.family === "dig.bench") {
        const lane = visual.getByRole("navigation", { name: "Harness lanes" }).getByRole("button").first();
        await expect(lane).toBeVisible();
        await lane.focus();
        await page.keyboard.press("Enter");
        await expect(lane).toHaveAttribute("aria-pressed", "true");
      }

      const ax = await cdp.send("Accessibility.getFullAXTree");
      const axNodes = ax.nodes
        .filter((node) => !node.ignored && node.role?.value !== "none" && node.role?.value !== "generic")
        .map((node) => ({ role: node.role?.value, name: node.name?.value, description: node.description?.value }))
        .filter((node) => node.name || ["main", "navigation", "region", "status"].includes(String(node.role)));
      expect(axNodes.some((node) => node.role === "heading" && node.name), `${fixture.family} named heading in AX tree`).toBe(true);

      await cdp.send("Emulation.setPageScaleFactor", { pageScaleFactor: 2 });
      await page.setViewportSize({ width: 640, height: 900 });
      const zoomMetrics = await visual.evaluate((root) => ({
        width: root.getBoundingClientRect().width,
        scrollWidth: root.scrollWidth,
        clientWidth: root.clientWidth,
        visible: root.getBoundingClientRect().height > 0
      }));
      expect(zoomMetrics.visible).toBe(true);
      expect(zoomMetrics.scrollWidth).toBeLessThanOrEqual(zoomMetrics.clientWidth + 2);
      await page.screenshot({ path: join(RECEIPT_DIR, `v6-${fixture.family.toLowerCase().replaceAll(".", "-")}-200pct.png`), fullPage: true });
      await cdp.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1 });
      await page.setViewportSize({ width: 1280, height: 900 });

      receipt.families.push({
        family: fixture.family, templateId: fixture.templateId,
        axe: {
          passes: axe.passes.length,
          violations: axe.violations.map((violation) => ({
            id: violation.id, impact: violation.impact, help: violation.help,
            targets: violation.nodes.map((node) => node.target)
          })),
          blockingViolationCount: blocking.length
        },
        focusableCount: focusables.length, controls: focusables,
        focusIndicator, axNodeCount: axNodes.length, axNodes,
        zoom200: zoomMetrics
      });
      expect(blocking, `${fixture.family} serious/critical axe violations`).toEqual([]);
      await page.getByTestId("toggle-visual-expand").evaluate((button: HTMLButtonElement) => button.click());
    }

    await page.emulateMedia({ reducedMotion: "reduce" });
    const craftax = FIXTURES.find((fixture) => fixture.family === "Craftax")!;
    const { visual } = await openFixture(page, craftax);
    const motion = await visual.evaluate((root) => {
      const durations = [...root.querySelectorAll("*")].flatMap((element) => {
        const style = getComputedStyle(element);
        const seconds = (value: string) => value.split(",").map((part) => {
          const trimmed = part.trim();
          return trimmed.endsWith("ms") ? Number.parseFloat(trimmed) / 1000 : Number.parseFloat(trimmed);
        }).filter(Number.isFinite);
        return [...seconds(style.animationDuration), ...seconds(style.transitionDuration)];
      });
      return { maxDurationSeconds: Math.max(0, ...durations) };
    });
    expect(motion.maxDurationSeconds).toBeLessThanOrEqual(0.001);
    receipt.reducedMotion = motion;
    receipt.summary = {
      families: FIXTURES.length,
      totalBlockingAxeViolations: receipt.families.reduce((sum: number, family: Json) => sum + family.axe.blockingViolationCount, 0),
      totalFocusableControls: receipt.families.reduce((sum: number, family: Json) => sum + family.focusableCount, 0),
      axEvidence: "Chromium Accessibility.getFullAXTree",
      manualVoiceOver: "not performed"
    };
  } catch (error) {
    receipt.status = "failed";
    receipt.error = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    writeFileSync(join(RECEIPT_DIR, "v6-accessibility.json"), `${JSON.stringify(receipt, null, 2)}\n`);
    await cdp.detach().catch(() => undefined);
  }
});
