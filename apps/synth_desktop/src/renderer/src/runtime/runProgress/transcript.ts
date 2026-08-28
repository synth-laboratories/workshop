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
import { isRunKind, isTerminalRunStatus, normalizeRunStatus } from "./types";

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


/**
 * Activity lines whose run has since stopped, and what it stopped as.
 *
 * A tool line is a record of what was true when the call returned. It is never
 * rewritten, because it was not wrong — but a conversation that polled a run
 * four times keeps four sentences saying "running" long after the run was
 * cancelled, and a reader scrolling back has no way to tell which of them still
 * holds. The v9 rollout was cancelled and the transcript went on describing an
 * active rollout and live credentials.
 *
 * The run record is the authority, so supersession is derived from it at render
 * time rather than baked into the line: a run that settles while the transcript
 * is open marks its lines without the journal being rewritten.
 *
 * A status this build does not recognise is not terminal — see
 * {@link isTerminalRunStatus}. Marking an unreadable status as finished is the
 * same failure in the other direction.
 */
export function supersededRunActivity(
	lines: readonly LocalActivityLine[],
	runs: readonly { id: string; status: string }[]
): Map<string, string> {
	const terminal = new Map<string, string>();
	for (const run of runs) {
		if (isTerminalRunStatus(run.status)) terminal.set(run.id, normalizeRunStatus(run.status));
	}
	const superseded = new Map<string, string>();
	if (terminal.size === 0) return superseded;
	for (const line of lines) {
		const status = line.optimizerRunId ? terminal.get(line.optimizerRunId) : undefined;
		if (status) superseded.set(line.id, status);
	}
	return superseded;
}

/** Every activity line in a chat, in transcript order. */
export function chatActivityLines(chat: LocalChat): LocalActivityLine[] {
	const activity = chat.activityByMessageId ?? {};
	return Object.values(activity).flat();
}
