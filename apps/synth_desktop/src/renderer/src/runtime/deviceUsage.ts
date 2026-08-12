import type { UsageBreakdown } from "@synth/runtime-protocol";
import type { DeviceUsageSummary } from "../components/UsageSheet";

/**
 * Compact device rollup for the Settings/Account pages, derived from the
 * native `usage_summary` aggregation — never by reducing raw ledger rows in
 * the renderer. Billed money and unbilled estimates are combined here only
 * because these pages show a single indicative figure; the Usage sheet keeps
 * them separate and labeled.
 */
export async function loadDeviceUsage(): Promise<DeviceUsageSummary | null> {
	const bridge = window.synthUsage;
	if (!bridge) return null;
	const [sevenDays, allTime] = await Promise.all([bridge.summary("7d"), bridge.summary("all")]);
	const cost = (totals: UsageBreakdown) =>
		(totals.billedCostUsd ?? 0) + (totals.estimatedCostUsd ?? 0);
	return {
		weeklyTokens: sevenDays.totals.totalTokens,
		weeklyCostUsd: cost(sevenDays.totals),
		totalTokens: allTime.totals.totalTokens,
		totalCostUsd: cost(allTime.totals),
		entries: allTime.totals.requests
	};
}
