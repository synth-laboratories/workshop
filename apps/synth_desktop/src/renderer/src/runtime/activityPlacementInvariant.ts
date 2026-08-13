import type { ChatMessage, LocalActivityLine } from "../types/landing";

/**
 * Activity marked `placement: "after"` hangs below the owning assistant bubble.
 * Those lines must chronologically follow the bubble's text. When post-tool
 * final text is merged into the same message that already owns earlier tools,
 * tools render under the answer that used them — this invariant fails closed.
 */
export function assertLocalActivityPlacementInvariant(
	messages: ChatMessage[],
	activityByMessageId: Record<string, LocalActivityLine[]>,
	lastContentSequenceByMessageId: ReadonlyMap<string, number>
): void {
	for (const message of messages) {
		if (message.role !== "assistant") continue;
		const contentSeq = lastContentSequenceByMessageId.get(message.id);
		if (contentSeq === undefined) continue;
		for (const line of activityByMessageId[message.id] ?? []) {
			if (line.placement !== "after") continue;
			const lineSeq = activityLineSequence(line);
			if (lineSeq === undefined) continue;
			if (lineSeq < contentSeq) {
				throw new Error(
					`Activity placement invariant violated: ${line.kind ?? "activity"} ${line.id} ` +
						`(seq ${lineSeq}) is placement "after" on message ${message.id} whose content ` +
						`was last written at seq ${contentSeq}. Tools/thoughts that ran before the ` +
						`final answer must not render below it — keep them on a preamble bubble or ` +
						`place them before the merged answer.`
				);
			}
		}
	}
}

/** Parse the event sequence encoded in activity line ids (`activity-12`, …). */
export function activityLineSequence(line: LocalActivityLine): number | undefined {
	if (typeof line.sequence === "number" && Number.isFinite(line.sequence)) return line.sequence;
	const match = /^(?:activity|context-compaction|run-summary|session-health)-(\d+)$/.exec(line.id);
	if (!match) return undefined;
	return Number(match[1]);
}
