/**
 * Where a run-progress card belongs in a transcript.
 *
 * The rule is one card per run per conversation, at the earliest point the run
 * was referenced. A conversation that starts a run, polls it four times, and
 * cancels it shows one card — not six — and the card stays anchored to the turn
 * that started the run rather than jumping to the newest poll.
 */

import type { LocalActivityLine, LocalChat } from "../../types/landing";
import type { RunKind, RunProgressTranscriptItem } from "./types";
import { isRunKind } from "./types";

/**
 * Items for one message's activity, ordered by first appearance and
 * de-duplicated by run id.
 */
export function runProgressItemsForLines(
	lines: LocalActivityLine[],
	createdAt = ""
): RunProgressTranscriptItem[] {
	const seen = new Set<string>();
	const items: RunProgressTranscriptItem[] = [];
	for (const line of lines) {
		const runId = line.optimizerRunId;
		if (!runId || seen.has(runId)) continue;
		seen.add(runId);
		items.push({
			kind: "run_progress",
			runId,
			// A tool result that did not name its algorithm still gets a card; the
			// durable record names the workflow, and the adapter dispatch reads it.
			runKind: (isRunKind(line.runKind) ? line.runKind : "gepa") as RunKind,
			createdAt
		});
	}
	return items;
}

/**
 * Every run referenced anywhere in a chat, keyed by the message that first
 * referenced it. A run mentioned only by the active turn is keyed under
 * `activeKey`, so a live run gets a card before its turn has an assistant
 * message to hang from.
 */
export function runProgressItemsByMessage(
	chat: LocalChat,
	activeKey = "__active__"
): Record<string, RunProgressTranscriptItem[]> {
	const claimed = new Set<string>();
	const byMessage: Record<string, RunProgressTranscriptItem[]> = {};
	const activity = chat.activityByMessageId ?? {};
	// Message order is the transcript's order; the active bucket is last.
	const keys = [
		...chat.messages.map((message) => message.id).filter((id) => activity[id]),
		...Object.keys(activity).filter(
			(key) => key !== activeKey && !chat.messages.some((message) => message.id === key)
		),
		...(activity[activeKey] ? [activeKey] : [])
	];
	for (const key of keys) {
		const items = runProgressItemsForLines(activity[key] ?? []).filter(
			(item) => !claimed.has(item.runId)
		);
		for (const item of items) claimed.add(item.runId);
		if (items.length > 0) byMessage[key] = items;
	}
	return byMessage;
}
