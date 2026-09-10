import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

test("native sealed live evidence renders offline without the app or producer", async ({ page }) => {
    const file = process.env.WORKSHOP_TEST_SEAL_EXPORT;
    expect(file, "Run the native registry seal test with WORKSHOP_TEST_SEAL_EXPORT first").toBeTruthy();
    const errors: string[] = [];
    const network: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.route("**/*", route => { network.push(route.request().url()); return route.abort(); });
    await page.setContent(readFileSync(file!, "utf8"));
    await expect(page.getByRole("heading", { name: "Offline live evidence" })).toBeVisible();
    const metric = (name: string) => page.locator("dt").filter({ hasText: new RegExp(`^${name}$`) }).locator("xpath=following-sibling::dd[1]");
    await expect(metric("projected envelopes")).toHaveText("2");
    await expect(metric("reward")).toHaveText("0.75");
    await expect(metric("cost \\(usd\\)")).toHaveText("—");
    await expect(page.locator(".visual pre")).toHaveCount(0);
    expect(network).toEqual([]);
    expect(errors).toEqual([]);
    await page.screenshot({ path: resolve("test-results/frozen-live-evidence.png"), fullPage: true });
});
