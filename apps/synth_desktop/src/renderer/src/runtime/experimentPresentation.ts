type UnknownRecord = Record<string, unknown>;

const record = (value: unknown): UnknownRecord | null => value != null && typeof value === "object" && !Array.isArray(value) ? value as UnknownRecord : null;
const first = (source: UnknownRecord | null, keys: string[]): unknown => keys.map((key) => source?.[key]).find((value) => value != null);
const number = (value: unknown): number | null => typeof value === "number" && Number.isFinite(value) ? value : null;
const metric = (value: number | null): string => value == null ? "—" : new Intl.NumberFormat("en-US", { maximumFractionDigits: 4 }).format(value);

export function formatExperimentResult(value: unknown): string {
	if (value == null || value === "") return "—";
	if (typeof value === "number") return metric(number(value));
	if (typeof value === "string" || typeof value === "boolean") return String(value);

	const result = record(value);
	if (!result) return "Result recorded";
	const baseline = record(first(result, ["baseline", "control"]));
	const variant = record(first(result, ["variant", "candidate", "best"]));
	const baselineReward = number(first(baseline, ["reward", "score", "value"]) ?? first(result, ["baselineReward", "baseline_reward", "controlReward", "control_reward"]));
	const variantReward = number(first(variant, ["reward", "score", "value"]) ?? first(result, ["variantReward", "variant_reward", "candidateReward", "candidate_reward", "bestReward", "best_reward"]));
	const delta = number(first(result, ["rewardDelta", "reward_delta", "delta", "uplift"]));

	if (baselineReward != null || variantReward != null) {
		return `${metric(baselineReward)} → ${metric(variantReward)}${delta == null ? "" : ` · Δ ${delta > 0 ? "+" : ""}${metric(delta)}`}`;
	}
	if (delta != null) return `Δ ${delta > 0 ? "+" : ""}${metric(delta)}`;
	const verdict = first(result, ["verdict", "status", "summary"]);
	return typeof verdict === "string" && verdict.trim() ? verdict : "Result recorded";
}
