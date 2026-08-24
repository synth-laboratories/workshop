import { expect, test } from "./browser.fixture";
import type { UsageBreakdown, UsageDayPoint } from "../../src/renderer/src/runtime/usageDashboard";
import {
	chartSeries,
	compactTokens,
	costQuality,
	niceCeil,
	percent,
	providerRollup,
	spendUsd,
	usd
} from "../../src/renderer/src/runtime/usageDashboard";

/*
 * Data → Usage.
 *
 * The dashboard's job is to make a large ledger legible without making any of
 * it up. These tests hold the two halves of that: the reductions produce the
 * right numbers, and the page never dresses a missing fact as a zero.
 */

function breakdown(overrides: Partial<UsageBreakdown> = {}): UsageBreakdown {
	return {
		provider: "openrouter",
		modelId: "openai/gpt-5.6-luna",
		requests: 1,
		inputTokens: 0,
		cachedInputTokens: null,
		nonCachedInputTokens: null,
		cacheWriteTokens: null,
		reasoningTokens: null,
		outputTokens: 0,
		totalTokens: 0,
		cacheHitRate: null,
		billedCostUsd: null,
		estimatedCostUsd: null,
		costSource: "none",
		decodeTpsP50: null,
		decodeTpsP95: null,
		endToEndTpsP50: null,
		endToEndTpsP95: null,
		ttftMsP50: null,
		ttftMsP95: null,
		perfSampleCount: 0,
		...overrides
	};
}

function day(stamp: string, provider: string, overrides: Partial<UsageBreakdown> = {}): UsageDayPoint {
	return { day: stamp, totals: breakdown({ provider, modelId: "all", ...overrides }) };
}

// ── Formatting: an unreported field and a real zero must never look alike ──

test("dollars keep sub-cent amounts legible and say Unavailable when unpriced", () => {
	expect(usd(41_615.03)).toBe("$41,615.03");
	expect(usd(0)).toBe("$0.00");
	expect(usd(0.0004)).toBe("$0.0004");
	expect(usd(null)).toBe("Unavailable");
	expect(usd(Number.NaN)).toBe("Unavailable");
});

test("token counts read at a glance and never fake a zero", () => {
	expect(compactTokens(48_000_000_000)).toBe("48B");
	expect(compactTokens(1_270_000_000)).toBe("1.27B");
	expect(compactTokens(142_000_000)).toBe("142M");
	expect(compactTokens(12_400)).toBe("12.4K");
	expect(compactTokens(940)).toBe("940");
	expect(compactTokens(0)).toBe("0");
	expect(compactTokens(null)).toBe("Unavailable");
});

test("percentages report Unavailable rather than 0% for an unreported rate", () => {
	expect(percent(0.973)).toBe("97.3%");
	expect(percent(0)).toBe("0.0%");
	expect(percent(null)).toBe("Unavailable");
});

// ── Spend: only authoritative receipts count ──

test("spend ignores estimates and stays null without an actual receipt", () => {
	expect(spendUsd(breakdown({ billedCostUsd: 0.42, estimatedCostUsd: 0.07 }))).toBeCloseTo(0.42);
	expect(spendUsd(breakdown({ billedCostUsd: 0.42 }))).toBeCloseTo(0.42);
	expect(spendUsd(breakdown({ estimatedCostUsd: 0.07 }))).toBeNull();
	expect(spendUsd(breakdown())).toBeNull();
	// A local run is priced at zero, which is not the same as never priced.
	expect(spendUsd(breakdown({ provider: "local-laguna", billedCostUsd: 0 }))).toBe(0);
});

test("the provider rollup ranks by spend and leaves an unpriced provider unpriced", () => {
	const rolls = providerRollup([
		breakdown({ provider: "openrouter", billedCostUsd: 0.30, totalTokens: 150_000 }),
		breakdown({ provider: "openrouter", estimatedCostUsd: 0.10, totalTokens: 48_000 }),
		breakdown({ provider: "synth-cloud", billedCostUsd: 0.20, totalTokens: 20_000 }),
		breakdown({ provider: "local-laguna", totalTokens: 14_000 })
	]);
	expect(rolls.map((roll) => roll.provider)).toEqual(["openrouter", "synth-cloud", "local-laguna"]);
	expect(rolls[0].spendUsd).toBeCloseTo(0.30);
	expect(rolls[0].totalTokens).toBe(198_000);
	expect(rolls[0].share).toBeCloseTo(0.6);
	expect(rolls[1].share).toBeCloseTo(0.4);
	// On-device work carries tokens and no dollars, and is not charged $0.00.
	expect(rolls[2].spendUsd).toBeNull();
	expect(rolls[2].totalTokens).toBe(14_000);
});

// ── The chart is a view of the same rows, not a second source ──

test("the chart zero-fills the calendar between the days that reported", () => {
	const series = chartSeries(
		[
			day("2026-08-10", "openrouter", { billedCostUsd: 1, totalTokens: 100 }),
			day("2026-08-13", "openrouter", { billedCostUsd: 3, totalTokens: 300 })
		],
		["openrouter"],
		"cost"
	);
	expect(series.days).toEqual(["2026-08-10", "2026-08-11", "2026-08-12", "2026-08-13"]);
	expect(series.values[0]).toEqual([1, 0, 0, 3]);
	expect(series.max).toBe(3);
	expect(series.truncatedFrom).toBeNull();
});

test("the chart stacks providers in the page's order and peaks on the stacked total", () => {
	const points = [
		day("2026-08-10", "openrouter", { billedCostUsd: 2, totalTokens: 200 }),
		day("2026-08-10", "synth-cloud", { billedCostUsd: 3, totalTokens: 30 }),
		day("2026-08-11", "openrouter", { billedCostUsd: 1, totalTokens: 100 })
	];
	const cost = chartSeries(points, ["openrouter", "synth-cloud"], "cost");
	expect(cost.values).toEqual([[2, 1], [3, 0]]);
	// The peak is the stack, not the tallest single band.
	expect(cost.max).toBe(5);

	const tokens = chartSeries(points, ["openrouter", "synth-cloud"], "tokens");
	expect(tokens.values).toEqual([[200, 100], [30, 0]]);
	expect(tokens.max).toBe(230);
});

test("a long span is capped and says so instead of silently dropping days", () => {
	const points = Array.from({ length: 120 }, (_unused, index) => {
		const date = new Date(Date.UTC(2026, 0, 1 + index));
		return day(date.toISOString().slice(0, 10), "openrouter", { billedCostUsd: 1 });
	});
	const series = chartSeries(points, ["openrouter"], "cost");
	expect(series.days).toHaveLength(90);
	expect(series.truncatedFrom).toBe(120);
	expect(series.days[series.days.length - 1]).toBe("2026-04-30");
});

test("the axis is drawn to a readable tick, never the raw peak", () => {
	expect(niceCeil(111.54)).toBe(200);
	expect(niceCeil(42.6)).toBe(50);
	expect(niceCeil(230)).toBe(250);
	expect(niceCeil(3)).toBe(5);
	expect(niceCeil(1)).toBe(1);
	expect(niceCeil(0)).toBe(0);
	// The plot scales to the tick, so a bar's height is readable against a label.
	expect(chartSeries([day("2026-08-10", "openrouter", { billedCostUsd: 3 })], ["openrouter"], "cost").axisMax)
		.toBe(5);
});

test("cost quality reports the share of spend behind each authority", () => {
	const rows = costQuality([
		breakdown({ billedCostUsd: 6, costSource: "provider_reported" }),
		breakdown({ billedCostUsd: 3, costSource: "synth_cloud" }),
		breakdown({ provider: "synth-cloud", estimatedCostUsd: 1, costSource: "synth_cloud" }),
		breakdown({ estimatedCostUsd: 99, costSource: "tariff_estimate" }),
		breakdown({ provider: "local-laguna", costSource: "none" })
	]);
	const byKey = Object.fromEntries(rows.map((row) => [row.key, row]));
	expect(byKey.provider_reported.share).toBeCloseTo(0.6);
	expect(byKey.synth_cloud.share).toBeCloseTo(0.3);
	expect(byKey.backend_estimate.share).toBeCloseTo(0.1);
	expect(byKey.none.share).toBe(0);
	// Every authority is always listed, so a 0% row is a statement, not a gap.
	expect(rows.map((row) => row.label)).toEqual([
		"Provider reported",
		"Synth Cloud actual",
		"Backend estimate",
		"Unpriced"
	]);
});

// ── The rendered page ──

async function stubUsage(page: import("@playwright/test").Page) {
	await page.addInitScript(() => {
		type Row = Record<string, unknown>;
		const row = (overrides: Row): Row => ({
			provider: "openrouter",
			modelId: "openai/gpt-5.6-luna",
			requests: 1,
			inputTokens: 0,
			cachedInputTokens: null,
			nonCachedInputTokens: null,
			cacheWriteTokens: null,
			reasoningTokens: null,
			outputTokens: 0,
			totalTokens: 0,
			cacheHitRate: null,
			billedCostUsd: null,
			estimatedCostUsd: null,
			costSource: "none",
			decodeTpsP50: null,
			decodeTpsP95: null,
			endToEndTpsP50: null,
			endToEndTpsP95: null,
			ttftMsP50: null,
			ttftMsP95: null,
			perfSampleCount: 0,
			...overrides
		});
		const days: Row[] = [];
		for (let index = 0; index < 12; index += 1) {
			const stamp = `2026-08-${`${index + 1}`.padStart(2, "0")}`;
			days.push({
				day: stamp,
				totals: row({
					provider: "openrouter",
					modelId: "all",
					requests: 4,
					billedCostUsd: 1 + index * 0.4,
					totalTokens: 2_000_000 + index * 400_000
				})
			});
			days.push({
				day: stamp,
				totals: row({
					provider: "synth-cloud",
					modelId: "all",
					requests: 2,
					billedCostUsd: 0.5 + index * 0.1,
					totalTokens: 600_000
				})
			});
		}
		(window as unknown as { synthUsage: unknown }).synthUsage = {
			summary: async (usageWindow: string) => ({
				window: usageWindow,
				totals: row({
					provider: "all",
					modelId: "all",
					requests: 72,
					inputTokens: 34_000_000,
					cachedInputTokens: 30_000_000,
					nonCachedInputTokens: 4_000_000,
					cacheWriteTokens: 900_000,
					reasoningTokens: 1_200_000,
					outputTokens: 5_000_000,
					totalTokens: 39_000_000,
					cacheHitRate: 30 / 34,
					billedCostUsd: 42.6,
					estimatedCostUsd: 3.4,
					costSource: "provider_reported",
					decodeTpsP50: 26
				}),
				models: [
					row({
						modelId: "openai/gpt-5.6-luna",
						requests: 48,
						totalTokens: 30_000_000,
						billedCostUsd: 33.6,
						costSource: "provider_reported"
					}),
					row({
						provider: "synth-cloud",
						modelId: "poolside/laguna-s-2.1",
						requests: 20,
						totalTokens: 7_200_000,
						billedCostUsd: 9.0,
						costSource: "synth_cloud"
					}),
					row({
						provider: "local-laguna",
						modelId: "poolside/Laguna-XS-2.1-NVFP4-mlx",
						requests: 4,
						totalTokens: 1_800_000
					})
				],
				days,
				generatedAt: "2026-08-12T12:00:00+00:00"
			})
		};
	});
}

test("the usage dashboard leads with spend, a daily chart, and a labelled breakdown", async ({ page }) => {
	await stubUsage(page);
	// addInitScript lands on the next navigation; the fixture already opened the app.
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-inventory").click();
	await page.getByTestId("inventory-tab-usage").click();

	const panel = page.getByTestId("usage-panel");
	await expect(panel).toBeVisible();

	// The hero includes only authoritative settled receipts.
	await expect(page.getByTestId("usage-hero-value")).toHaveText("$42.60");
	await expect(page.getByTestId("usage-hero-note")).toContainText("$42.60 settled");
	await expect(page.getByTestId("usage-hero-note")).not.toContainText("estimated");

	// Identity is never colour-alone: every provider is named in the legend…
	const legend = page.getByTestId("usage-legend");
	await expect(legend).toContainText("OpenRouter");
	await expect(legend).toContainText("Synth Cloud");
	await expect(legend).toContainText("On-device");

	// …and on-device work says it has no charge rather than showing $0.00.
	await expect(page.getByTestId("usage-provider-local-laguna")).toContainText("On-device · no provider charge");

	await expect(page.getByTestId("usage-chart")).toBeVisible();
	await expect(page.getByTestId("usage-stat-total")).toContainText("39M");
	await expect(page.getByTestId("usage-stat-cached")).toContainText("30M");
	await expect(page.getByTestId("usage-stat-output")).toContainText("includes 1.2M reasoning");
	// $33.60 of the $42.60 model rows attribute to provider-reported receipts.
	await expect(page.getByTestId("usage-quality-provider_reported")).toContainText("78.9%");

	const table = page.getByTestId("usage-breakdown-table");
	await expect(table).toContainText("openai/gpt-5.6-luna");
	await expect(table).toContainText("$33.60");

	// The Day view is the chart's table equivalent, so the series is readable
	// without relying on colour or hover.
	await page.getByTestId("usage-grouping-day").click();
	await expect(page.getByTestId("usage-breakdown-table")).toContainText("Aug 12, 2026");
});

test("hovering the chart names every band and its value for that day", async ({ page }) => {
	await stubUsage(page);
	// addInitScript lands on the next navigation; the fixture already opened the app.
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-inventory").click();
	await page.getByTestId("inventory-tab-usage").click();

	const chart = page.getByTestId("usage-chart");
	await expect(chart).toBeVisible();
	await chart.locator(".usage-chart-plot").hover();

	const tooltip = page.getByTestId("usage-chart-tooltip");
	await expect(tooltip).toBeVisible();
	await expect(tooltip).toContainText("OpenRouter");
	await expect(tooltip).toContainText("Synth Cloud");
});

test("a device with no usage says so instead of drawing an empty chart", async ({ page }) => {
	await page.addInitScript(() => {
		(window as unknown as { synthUsage: unknown }).synthUsage = {
			summary: async (usageWindow: string) => ({
				window: usageWindow,
				totals: {
					provider: "all",
					modelId: "all",
					requests: 0,
					inputTokens: 0,
					cachedInputTokens: null,
					nonCachedInputTokens: null,
					cacheWriteTokens: null,
					reasoningTokens: null,
					outputTokens: 0,
					totalTokens: 0,
					cacheHitRate: null,
					billedCostUsd: null,
					estimatedCostUsd: null,
					costSource: "none",
					decodeTpsP50: null,
					decodeTpsP95: null,
					endToEndTpsP50: null,
					endToEndTpsP95: null,
					ttftMsP50: null,
					ttftMsP95: null,
					perfSampleCount: 0
				},
				models: [],
				days: [],
				generatedAt: "2026-08-12T12:00:00+00:00"
			})
		};
	});
	// addInitScript lands on the next navigation; the fixture already opened the app.
	await page.reload();
	await page.getByTestId("titlebar").waitFor();
	await page.getByTestId("open-inventory").click();
	await page.getByTestId("inventory-tab-usage").click();

	await expect(page.getByTestId("usage-hero-value")).toHaveText("Unavailable");
	await expect(page.getByTestId("usage-hero-note")).toContainText("No request in this window carried a price");
	await expect(page.getByTestId("usage-chart-empty")).toBeVisible();
	// Unreported cache traffic is named, never rendered as a confident zero.
	await expect(page.getByTestId("usage-stat-cached")).toContainText("Unavailable");
});
