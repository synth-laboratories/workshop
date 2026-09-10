import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

test("captured Trace V5 native export remains interactive without a producer", async ({page}) => {
  const path = process.env.WORKSHOP_TEST_TRACE_SEAL_EXPORT;
  if (!path) throw new Error("WORKSHOP_TEST_TRACE_SEAL_EXPORT must name the real native exported HTML");
  const errors: string[] = [], requests: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/*", route => { requests.push(route.request().url()); return route.abort(); });
  await page.setContent(await readFile(path, "utf8"));
  await expect(page.getByRole("heading", {name:"Captured trace offline",exact:true}).first()).toBeVisible();
  const inspector=page.getByTestId("visual-trace-rollout-inspector");
  await expect(inspector).toBeVisible();
  await inspector.locator('[data-density="full"]').click();
  await expect(inspector.locator(".sv-count")).toContainText("complete projection");
  const itemCount=await inspector.locator('[data-role="events"]').evaluate(node => node.children.length);
  expect(itemCount).toBeGreaterThan(0);
  await expect(inspector.getByText("No projected items match these filters.")).toHaveCount(0);
  await inspector.locator('[data-tab="metadata"]').click();
  await expect(inspector.locator('[data-tab="metadata"]')).toHaveClass("active");
  await inspector.locator('[data-tab="trace"]').click();
  expect(errors).toEqual([]); expect(requests).toEqual([]);
  await inspector.screenshot({path:resolve("test-results/frozen-captured-trace.png")});
});
