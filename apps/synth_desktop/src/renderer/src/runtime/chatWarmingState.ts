export type ChatWarmingState = {
	running: boolean;
	targetKind: string | null;
	targetModel: string | null;
	lastMessageRole: string | null;
	localPhase: string | null;
	localLoadedModel: string | null;
};

/**
 * A hosted Laguna turn remains in its pre-output phase until the first model
 * text is observable. That interval includes Shoal admission and any required
 * scale-from-zero warmup; it is deliberately excluded from generation TPS.
 */
export function chatIsWarmingUp(state: ChatWarmingState): boolean {
	if (!state.running) return false;
	if (state.targetKind === "local") {
		return state.localPhase === "loading" || !state.localLoadedModel;
	}
	return state.targetKind === "cloud"
		&& Boolean(state.targetModel?.startsWith("synth_internal/laguna-"))
		&& state.lastMessageRole !== "assistant";
}
