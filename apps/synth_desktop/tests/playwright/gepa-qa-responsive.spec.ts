/**
 * The dedicated GEPA QA page is intentionally fixture-backed so responsive
 * review does not depend on a provider credential or a historical local run.
 * Its evidence density makes a plain DOM smoke test insufficient: this gate
 * proves the compact visual never manufactures a horizontal scrolling pane.
 */

import type { Page } from "@playwright/test";
import { expect, test } from "./browser.fixture";

const VIEWPORTS = [1440, 1024, 768, 680, 480, 390] as const;

async function assertNoHorizontalOverflow(page: Page, label: string) {
  const layout = await page.evaluate(() => {
    const root = document.scrollingElement ?? document.documentElement;
    const limit = window.innerWidth + 1;
    const offenders = [...document.querySelectorAll("body *")]
      .map((element) => {
        const rect = element.getBoundingClientRect();
        return {
          testId: element.getAttribute("data-testid"),
          tag: element.tagName.toLowerCase(),
          text: (element.textContent ?? "").trim().slice(0, 72),
          right: Math.round(rect.right)
        };
      })
      .filter((row) => row.right > limit + 4)
      .slice(0, 5);
    return { scrollWidth: root.scrollWidth, clientWidth: root.clientWidth, offenders };
  });
  expect(layout.scrollWidth, `${label}: ${JSON.stringify(layout.offenders)}`).toBeLessThanOrEqual(layout.clientWidth + 1);
}

test("GEPA QA surface preserves compact evidence without horizontal scrolling", async ({ page }) => {
  await page.goto(new URL("gepa-qa.html", page.url()).toString());
  await expect(page.getByTestId("gepa-workspace")).toBeVisible();
  await expect(page.getByTestId("gepa-config-card")).toBeVisible();
  await expect(page.getByTestId("gepa-dataset-card")).toBeVisible();
  await expect(page.getByTestId("gepa-container-card")).toBeVisible();
  await expect(page.getByTestId("gepa-related-work-card")).toBeVisible();
  await expect(page.getByTestId("gepa-candidate-count")).toBeVisible();
  await expect(page.getByTestId("gepa-rollout-count")).toBeVisible();
  const headerGeometry = await page.evaluate(() => {
    const root = document.querySelector<HTMLElement>(".synth-visual-root")?.getBoundingClientRect();
    const header = document.querySelector<HTMLElement>('[data-testid="gepa-run-header"]')?.getBoundingClientRect();
    if (!root || !header) throw new Error("GEPA workspace header geometry is unavailable");
    return { rootTop: root.top, headerTop: header.top, headerHeight: header.height };
  });
  expect(headerGeometry.headerTop).toBeGreaterThanOrEqual(headerGeometry.rootTop);
  expect(headerGeometry.headerHeight).toBeGreaterThan(40);

  for (const width of VIEWPORTS) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(100);
    await assertNoHorizontalOverflow(page, `GEPA QA @ ${width}px`);
  }

  await page.setViewportSize({ width: 768, height: 1024 });
  await page.evaluate(() => { document.documentElement.style.zoom = "2"; });
  await page.waitForTimeout(100);
  await assertNoHorizontalOverflow(page, "GEPA QA @ 768px / 200% zoom");
});
