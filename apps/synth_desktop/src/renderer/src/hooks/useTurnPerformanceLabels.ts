import { useMemo } from "react";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import type { LocalChat } from "../types/landing";

const GENERATION_TPS_UNAVAILABLE = "Generation speed unavailable";
const MAX_GENERATION_DELTA_GAP_MS = 2_000;

function formatTps(value: number): string {
	return value >= 10 ? value.toFixed(1) : value.toFixed(2);
}

function timestamp(value: string): number | null {
	const parsed = Date.parse(value);
	return Number.isFinite(parsed) ? parsed : null;
}

function record(value: unknown): Record<string, unknown> | null {
	return value && typeof value === "object" ? value as Record<string, unknown> : null;
}

function outputTokens(payload: Record<string, unknown>): number | null {
	const usage = record(payload.tokenUsage) ?? record(payload.usage) ?? payload;
	const turn = record(payload.turn);
	const turnUsage = record(turn?.tokenUsage) ?? record(turn?.usage);
	for (const candidate of [
		record(usage.last), record(usage.lastUsage), record(usage.lastTokenUsage),
		record(turnUsage?.last), record(turnUsage?.lastUsage), record(turnUsage?.lastTokenUsage),
		turnUsage, usage
	]) {
		if (!candidate) continue;
		const value = candidate.outputTokens ?? candidate.output_tokens ?? candidate.completionTokens ?? candidate.completion_tokens;
		if (typeof value === "number" && Number.isFinite(value) && value > 0) return value;
	}
	return null;
}

function isOutputDelta(event: RuntimeEvent): boolean {
	return event.eventKind === "message.delta"
		&& typeof event.payload.delta === "string"
		&& event.payload.delta.length > 0;
}

function isUsage(event: RuntimeEvent): boolean {
	const kind = event.eventKind.toLowerCase();
	return kind.includes("usage") || kind.startsWith("run.") || kind.startsWith("turn/");
}

function isTerminal(event: RuntimeEvent): boolean {
	return ["run.completed", "run.failed", "run.cancelled", "turn/completed", "turn/failed", "turn/interrupted"].includes(event.eventKind);
}

function compactDuration(milliseconds: number): string {
	const seconds = Math.max(0, Math.round(milliseconds / 1_000));
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	const remainder = seconds % 60;
	return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

export type TurnPerformanceLabel = { generation: string; worked: string | null };

/**
 * Build immutable temporal snapshots from the durable event journal.
 *
 * Generation speed is cumulative generation-only output tokens divided by the
 * sum of positive, adjacent output-delta intervals no longer than two seconds.
 * Longer intervals are tool/orchestration/idle time and are excluded. Each
 * assistant segment is cut off when the next segment begins, or at the turn's
 * terminal event. Telemetry observed after that cutoff can never enter the
 * historical snapshot. "Worked" is terminal time minus the latest persisted
 * turn/accepted boundary, never render time.
 */
export function turnPerformanceLabels(chat: LocalChat, events: RuntimeEvent[], running = false) {
	const byMessageId: Record<string, TurnPerformanceLabel> = {};
	let live: string | null = null;
	const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
	const messages = chat.messages;

	for (let index = 0; index < messages.length; index += 1) {
		const message = messages[index]!;
		if (message.role !== "assistant") continue;
		const messageAt = timestamp(message.at);
		if (messageAt == null) continue;
		const priorTerminalAt = ordered.reduce<number | null>((latest, event) => {
			const at = timestamp(event.createdAt);
			return isTerminal(event) && at != null && at < messageAt ? Math.max(latest ?? Number.NEGATIVE_INFINITY, at) : latest;
		}, null);
		const firstAcceptedAt = ordered.reduce<number | null>((first, event) => {
			const at = timestamp(event.createdAt);
			if (event.eventKind !== "turn/accepted" || at == null || at <= (priorTerminalAt ?? Number.NEGATIVE_INFINITY) || at > messageAt) return first;
			return Math.min(first ?? Number.POSITIVE_INFINITY, at);
		}, null);
		let fallbackUserAt: number | null = null;
		for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
			if (messages[cursor]!.role === "user") { fallbackUserAt = timestamp(messages[cursor]!.at); break; }
		}
		const turnStartAt = firstAcceptedAt ?? fallbackUserAt ?? priorTerminalAt ?? Number.NEGATIVE_INFINITY;
		let nextAssistantAt: number | null = null;
		for (let cursor = index + 1; cursor < messages.length; cursor += 1) {
			const candidate = messages[cursor]!;
			if (candidate.role === "assistant") {
				nextAssistantAt = timestamp(candidate.at);
				break;
			}
		}
		const terminal = ordered.find((event) => {
			const at = timestamp(event.createdAt);
			return at != null && at >= messageAt && isTerminal(event);
		});
		const terminalAt = terminal ? timestamp(terminal.createdAt) : null;
		if (nextAssistantAt != null && terminalAt != null && nextAssistantAt >= terminalAt) nextAssistantAt = null;
		const isFinal = nextAssistantAt == null;
		const cutoff = nextAssistantAt ?? terminalAt ?? (running ? Number.POSITIVE_INFINITY : messageAt);
		let acceptedAt: number | null = null;
		let lastOutputAt: number | null = null;
		let generationActiveMs = 0;
		let tokens: number | null = null;
		for (const event of ordered) {
			const at = timestamp(event.createdAt);
			if (at == null || at < turnStartAt || (cutoff != null && at >= cutoff && event !== terminal)) continue;
			if (event.eventKind === "turn/accepted") acceptedAt = at;
			if (isOutputDelta(event)) {
				if (lastOutputAt != null) {
					const gap = at - lastOutputAt;
					if (gap > 0 && gap <= MAX_GENERATION_DELTA_GAP_MS) generationActiveMs += gap;
				}
				lastOutputAt = at;
			}
			if (isUsage(event)) {
				const observed = outputTokens(event.payload);
				if (observed != null) tokens = Math.max(tokens ?? 0, observed);
			}
		}
		const rate = tokens != null && generationActiveMs > 0 ? tokens / (generationActiveMs / 1_000) : null;
		const generation = rate != null && Number.isFinite(rate) && rate > 0
			? `${formatTps(rate)} tok/s generation speed`
			: GENERATION_TPS_UNAVAILABLE;
		const worked = isFinal && terminalAt != null && acceptedAt != null && terminalAt >= acceptedAt
			? `Worked ${compactDuration(terminalAt - acceptedAt)}`
			: null;
		byMessageId[message.id] = { generation, worked };
		if (isFinal && terminalAt == null) live = generation;
	}
	return { byMessageId, live };
}

export function useTurnPerformanceLabels(chat: LocalChat, events: RuntimeEvent[], running: boolean) {
	return useMemo(() => turnPerformanceLabels(chat, events, running), [chat, events, running]);
}
