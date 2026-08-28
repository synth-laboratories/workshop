import type { UsageBreakdown } from "@synth/runtime-protocol";
import type { DeviceUsageSummary } from "../components/UsageSheet";
import { bridges } from "./desktopBridge";

/**
 * Compact device rollup for the Settings/Account pages, derived from the
 * native `usage_summary` aggregation — never by reducing raw ledger rows in
 * the renderer. Dollar totals include only settled billed charges.
 */
export async function loadDeviceUsage(): Promise<DeviceUsageSummary | null> {
	const bridge = bridges.usage;
	if (!bridge) return null;
	const [sevenDays, allTime] = await Promise.all([bridge.summary("7d"), bridge.summary("all")]);
	const cost = (totals: UsageBreakdown) => {
		const estimate = totals.costSource === "synth_cloud" ? totals.estimatedCostUsd : null;
		if (totals.billedCostUsd == null && estimate == null) return null;
		return (totals.billedCostUsd ?? 0) + (estimate ?? 0);
	};
	return {
		weeklyTokens: sevenDays.totals.totalTokens,
		weeklyCostUsd: cost(sevenDays.totals),
		totalTokens: allTime.totals.totalTokens,
		totalCostUsd: cost(allTime.totals),
		entries: allTime.totals.requests
	};
}
