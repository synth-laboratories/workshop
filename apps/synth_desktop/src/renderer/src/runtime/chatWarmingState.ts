export type ChatWarmingState = {
	running: boolean;
	targetKind: string | null;
	targetModel: string | null;
	lastMessageRole: string | null;
	localPhase: string | null;
	localLoadedModel: string | null;
	/** Weight residency from the inference daemon, when observed. */
	localResident?: boolean | null;
	hostedPhase?: string | null;
};

export type ChatInferencePhase = "idle" | "warming" | "working";

function isHostedLaguna(state: ChatWarmingState): boolean {
	return state.targetKind === "cloud"
		&& Boolean(state.targetModel?.startsWith("synth_internal/laguna-"));
}

/**
 * One chat-facing lifecycle for local and hosted Laguna. Both targets enter
 * `warming`, advance to `working` only when their runtime supplies readiness
 * evidence, and return to `idle` when the turn ends. Local readiness comes
 * from LagunaStatus; hosted readiness is the first streamed model output,
 * which follows Shoal admission and scale-from-zero warmup.
 */
export function chatInferencePhase(state: ChatWarmingState): ChatInferencePhase {
	if (!state.running) return "idle";
	if (state.targetKind === "local") {
		return state.localPhase === "loading" || state.localResident === false || !state.localLoadedModel
			? "warming"
			: "working";
	}
	if (isHostedLaguna(state)) {
		if (["queued", "provisioning", "warming", "scaled_down", "saturated"].includes(state.hostedPhase ?? "")) {
			return "warming";
		}
		if (["ready", "running"].includes(state.hostedPhase ?? "")) return "working";
		return state.lastMessageRole === "assistant" ? "working" : "warming";
	}
	return "working";
}

export function chatIsWarmingUp(state: ChatWarmingState): boolean {
	return chatInferencePhase(state) === "warming";
}
