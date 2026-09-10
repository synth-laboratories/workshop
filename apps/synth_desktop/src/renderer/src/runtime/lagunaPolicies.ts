import type { LagunaPolicy, SavedLoraCheckpoint } from "../bridge/types";

/** The daemon's base policy: Laguna XS with nothing attached. */
export const LOCAL_BASE_POLICY = "poolside/Laguna-XS-2.1-NVFP4-mlx";
/** The model id a registered Laguna finetune is served under. */
export const LOCAL_FT_POLICY = "synth/Laguna-XS-2.1-ft";

function modelName(modelId: string): string {
	if (modelId === LOCAL_BASE_POLICY) return "Laguna XS 2.1";
	return modelId
		.split("/").pop()!
		.replace(/-NVFP4-mlx$/i, "")
		.replace(/-mlx$/i, "")
		.replace(/-/g, " ")
		.replace(/\bxs\b/gi, "XS")
		.replace(/\s+/g, " ")
		.trim();
}

export function policyLabel(policy: LagunaPolicy): string {
	if (policy.isBase) return modelName(policy.modelId);
	return policy.title ?? policy.modelId.split("/").pop() ?? policy.modelId;
}

/** The actual model leads, followed by every registered SFT variant. */
export function orderedLagunaPolicies(policies: readonly LagunaPolicy[]): LagunaPolicy[] {
	return [...policies].sort((left, right) => {
		if (left.isBase !== right.isBase) return left.isBase ? -1 : 1;
		return policyLabel(left).localeCompare(policyLabel(right));
	});
}

/**
 * What to render for a policy's speed.
 *
 * A blank is a real answer here. The daemon reports `null` until it has enough
 * samples, and marks a delta unresolvable when it is smaller than that
 * policy's own measurement noise — on a busy Mac that is most of the time.
 * Rendering a number anyway would dress up noise as a measurement.
 */
export function policySpeed(policy: LagunaPolicy): { rate: string; delta: string | null } {
	const rate = policy.tokensPerSecondP10 == null
		? "—"
		: `${policy.tokensPerSecondP10.toFixed(1)} tok/s`;
	const delta = policy.deltaIsResolvable && policy.deltaVsBasePct != null
		? `${policy.deltaVsBasePct > 0 ? "+" : ""}${policy.deltaVsBasePct.toFixed(1)}%`
		: null;
	return { rate, delta };
}

/**
 * Whether a catalog row can be served by the Laguna daemon.
 *
 * Qwen Optimizers adapters are trained against the local SFT student, not
 * Laguna, so they stay on the catalog's own Chat Completions / Responses
 * buttons. The host enforces this too; this only keeps the button hidden.
 */
export function isLagunaCompatibleAdapter(checkpoint: SavedLoraCheckpoint): boolean {
	const base = checkpoint.baseModel.toLowerCase();
	return checkpoint.placement === "this_mac"
		&& checkpoint.checkpointKind === "inference"
		&& checkpoint.status === "ready"
		&& (base.includes("laguna") || base.includes("poolside"));
}
