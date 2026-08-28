import type { RecoveryNotice } from "@synth/runtime-protocol";

/**
 * A recovered Codex chat already contains the abandoned user prompt in its
 * durable thread. Replaying that prompt can duplicate tools, paid work, and
 * long-running evaluations. The next turn must instead reconcile the existing
 * thread and continue only what remains unfinished.
 */
export function restartContinuationPrompt(notice: RecoveryNotice): string {
	const activity = notice.lastActivity?.label?.trim();
	const checkpoint = activity
		? ` The last durably recorded activity was ${activity}.`
		: "";
	return `Continue from where this thread stopped when Workshop restarted.${checkpoint} Inspect the existing thread and durable outputs first. Do not repeat completed work or relaunch an external action unless its previous outcome is known not to have succeeded.`;
}
