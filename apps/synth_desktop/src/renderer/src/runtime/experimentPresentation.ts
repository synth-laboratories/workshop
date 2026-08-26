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

const FAILURE_STATUSES = new Set(["failed", "error", "aborted", "interrupted"]);
const REASON_KEYS = ["error", "errorMessage", "error_message", "message", "reason", "failureReason", "failure_reason"];
const RECEIPT_KEYS = ["terminalReceipt", "terminal_receipt", "receipt"];

function oneLine(value: string): string {
	return value.trim().replace(/\s+/g, " ");
}

function textsFrom(source: unknown, keys: string[]): string[] {
	const rec = record(source);
	if (!rec) return [];
	const out: string[] = [];
	for (const key of keys) {
		const value = rec[key];
		if (typeof value === "string" && value.trim()) out.push(oneLine(value));
		const nested = record(value);
		if (!nested) continue;
		for (const inner of ["message", "reason", "error", "summary", "text", "detail"]) {
			const text = nested[inner];
			if (typeof text === "string" && text.trim()) out.push(oneLine(text));
		}
	}
	return out;
}

function firstText(sources: unknown[], keys: string[]): string | null {
	for (const source of sources) {
		const rec = record(source);
		const nested = rec ? [source, rec.assessment, rec.error, rec.receipt] : [source];
		for (const item of nested) {
			const found = textsFrom(item, keys);
			if (found[0]) return found[0];
		}
	}
	return null;
}

/** One-line failure reason for a DAG node. Null when the node is not failed. */
export function formatNodeFailureReason(node: {
	status?: string;
	provenance?: unknown;
	metrics?: unknown;
	config?: unknown;
}): string | null {
	if (!node.status || !FAILURE_STATUSES.has(node.status.toLowerCase())) return null;
	return firstText([node.provenance, node.metrics, node.config], REASON_KEYS)
		?? firstText([node.provenance, node.metrics, node.config], RECEIPT_KEYS)
		?? "Reason unavailable";
}
