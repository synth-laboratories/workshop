import { expect, test, type Page } from "./browser.fixture";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const evidenceDir = resolve(import.meta.dirname, "../../test-results/mander-mvp");
mkdirSync(evidenceDir, { recursive: true });

test.use({ video: { mode: "on", size: { width: 1280, height: 840 } } });

async function openLab(page: Page) {
	await page.evaluate(() => {
		window.location.hash = "mander-lab";
	});
	await expect(page.getByTestId("mander-lab")).toBeVisible();
	await expect(page.getByTestId("mander")).toBeVisible();
}

test.describe("Mander Lab", () => {
	test("is fixture-routed and absent from sidebar navigation", async ({ page }) => {
		await expect(page.getByTestId("sidebar")).toBeVisible();
		await expect(page.getByTestId("sidebar")).not.toContainText("Mander Lab");
		await expect(page.getByTestId("mander-lab")).toHaveCount(0);
		await openLab(page);
		await expect(page.getByTestId("mander-lab")).toBeVisible();
	});

	test("keeps the SVG mounted across state changes", async ({ page }) => {
		await openLab(page);
		const mountId = await page.locator('[data-testid="mander"]').evaluate((element) => {
			const svg = element as SVGSVGElement;
			svg.dataset.mountId = "stable-mander-root";
			return svg.dataset.mountId;
		});
		await page.getByRole("button", { name: "Thinking", exact: true }).click();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-state", "thinking");
		await expect(page.locator('[data-testid="mander"]')).toHaveAttribute("data-mount-id", mountId!);
		await page.getByRole("button", { name: "Idle", exact: true }).click();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-state", "idle");
		await expect(page.locator('[data-testid="mander"]')).toHaveAttribute("data-mount-id", mountId!);
	});

	test("exposes the full 4x4 matrix, motion modes, and sizes", async ({ page }) => {
		await openLab(page);
		for (const label of ["Idle loop", "Working loop", "Success loop", "Idle → thinking", "Working → success"]) {
			await expect(page.getByRole("button", { name: label })).toBeVisible();
		}
		await expect(page.locator(".mander-lab-matrix button")).toHaveCount(16);
		for (const motion of ["auto", "full", "reduced", "still"]) {
			await expect(page.getByRole("button", { name: motion, exact: true })).toBeVisible();
		}
		for (const size of [16, 24, 32, 64, 128, 256]) {
			await expect(page.getByTestId(`mander-size-${size}`)).toBeVisible();
		}
		await page.getByTestId("mander-matrix-idle-to-thinking").click();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-state", "thinking");
		await page.getByRole("button", { name: "still", exact: true }).click();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-motion", "still");
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-running", "false");
		await page.getByRole("button", { name: "reduced", exact: true }).click();
		await page.getByRole("button", { name: "Idle", exact: true }).click();
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-motion", "reduced");
		await page.waitForTimeout(220);
		await expect(page.getByTestId("mander")).toHaveAttribute("data-mander-running", "false");
	});

	test("rapid toggle and chaos do not shift layout", async ({ page }) => {
		await openLab(page);
		const before = await page.getByTestId("mander").boundingBox();
		await page.getByTestId("mander-rapid-toggle").click();
		await page.waitForTimeout(600);
		await page.getByTestId("mander-rapid-toggle").click();
		await page.getByTestId("mander-chaos").click();
		await page.waitForTimeout(900);
		await page.getByTestId("mander-chaos").click();
		const after = await page.getByTestId("mander").boundingBox();
		expect(before).toBeTruthy();
		expect(after).toBeTruthy();
		expect(after!.width).toBeCloseTo(before!.width, 0);
		expect(after!.height).toBeCloseTo(before!.height, 0);
		await page.getByTestId("mander-lab-stage").screenshot({ path: resolve(evidenceDir, "chaos-stage.png") });
	});

	test("captures idle, thinking, working, and success at 24, 64, and 128 in both themes", async ({ page }) => {
		await openLab(page);
		for (const theme of ["Light", "Dark"] as const) {
			await page.getByRole("button", { name: theme, exact: true }).click();
			for (const state of ["Idle", "Thinking", "Working", "Success"] as const) {
				await page.getByRole("button", { name: state, exact: true }).click();
				for (const size of [24, 64, 128] as const) {
					await page.getByTestId(`mander-size-${size}`).click();
					await expect(page.getByTestId("mander")).toHaveAttribute("width", String(size));
					await page.getByTestId("mander").screenshot({
						path: resolve(evidenceDir, `${theme.toLowerCase()}-${state.toLowerCase()}-${size}.png`)
					});
				}
			}
		}
	});

	test("labels the SVG when a name is supplied and hides parts", async ({ page }) => {
		await openLab(page);
		const svg = page.getByTestId("mander");
		await expect(svg).toHaveAttribute("role", "img");
		await expect(svg).toHaveAttribute("aria-label", "Synth is idle");
		await page.getByRole("button", { name: "Thinking", exact: true }).click();
		await expect(svg).toHaveAttribute("aria-label", "Synth is thinking");
		await expect(svg.locator("title")).toHaveText("Synth is thinking");
		await page.getByRole("button", { name: "Working", exact: true }).click();
		await expect(svg).toHaveAttribute("aria-label", "Synth is working");
		await page.getByRole("button", { name: "Success", exact: true }).click();
		await expect(svg).toHaveAttribute("aria-label", "Synth succeeded");
		await expect(svg.locator("[data-mander-part]").first()).toHaveAttribute("aria-hidden", "true");
	});
});
