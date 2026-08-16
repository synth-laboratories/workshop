import { useEffect, useMemo, useState } from "react";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import type { ModelPerformanceTurnSample } from "../bridge";
import type { LocalChat } from "../types/landing";
import { formatTps } from "../components/InferencePanel";
import { bridges } from "../runtime/desktopBridge";

const GENERATION_TPS_UNAVAILABLE = "Generation TPS unavailable";

function timestamp(value: string): number | null {
	const parsed = Date.parse(value);
	return Number.isFinite(parsed) ? parsed : null;
}

function median(values: number[]): number | null {
	const sorted = values.filter((value) => Number.isFinite(value) && value > 0).sort((a, b) => a - b);
	if (sorted.length === 0) return null;
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 1 ? sorted[middle]! : (sorted[middle - 1]! + sorted[middle]!) / 2;
}

function label(samples: ModelPerformanceTurnSample[]): string | null {
	const value = median(samples.map((sample) => sample.outputTps));
	if (value == null) return null;
	return `${formatTps(value)} tok/s generation median`;
}

function record(value: unknown): Record<string, unknown> | null {
	return value && typeof value === "object" ? value as Record<string, unknown> : null;
}

function outputTokens(payload: Record<string, unknown>): number | null {
	const usage = record(payload.tokenUsage) ?? record(payload.usage) ?? payload;
	const turn = record(payload.turn);
	const turnUsage = record(turn?.tokenUsage) ?? record(turn?.usage);
	const candidates = [
		record(usage.last),
		record(usage.lastUsage),
		record(usage.lastTokenUsage),
		record(turnUsage?.last),
		record(turnUsage?.lastUsage),
		record(turnUsage?.lastTokenUsage),
		turnUsage,
		usage
	];
	for (const candidate of candidates) {
		if (!candidate) continue;
		const value = candidate.outputTokens ?? candidate.output_tokens ?? candidate.completionTokens ?? candidate.completion_tokens;
		if (typeof value === "number" && Number.isFinite(value) && value > 0) return value;
	}
	return null;
}

/** Live equivalent of the native turn tracker, using the same first/last
 * visible output boundary and the latest explicitly per-turn token count. */
export function liveTurnPerformanceLabel(events: RuntimeEvent[], lastUserAt: number | null): string | null {
	if (lastUserAt == null) return null;
	let firstOutputAt: number | null = null;
	let lastOutputAt: number | null = null;
	let generationActiveMs = 0;
	let latestOutputTokens: number | null = null;
	for (const event of events) {
		const at = timestamp(event.createdAt);
		if (at == null || at < lastUserAt) continue;
		if (event.eventKind === "message.delta" && typeof event.payload.delta === "string" && event.payload.delta.length > 0) {
			firstOutputAt ??= at;
			if (lastOutputAt != null) {
				const gap = at - lastOutputAt;
				if (gap > 0 && gap <= 2_000) generationActiveMs += gap;
			}
			lastOutputAt = at;
		}
		if (event.eventKind.toLowerCase().includes("usage") || event.eventKind.startsWith("run.")) {
			latestOutputTokens = outputTokens(event.payload) ?? latestOutputTokens;
		}
	}
	if (firstOutputAt == null || lastOutputAt == null || latestOutputTokens == null) return null;
	const seconds = generationActiveMs / 1_000;
	if (seconds <= 0) return null;
	const tps = latestOutputTokens / seconds;
	return Number.isFinite(tps) && tps > 0 ? `${formatTps(tps)} tok/s generation median` : null;
}

export function turnPerformanceLabels(chat: LocalChat, samples: ModelPerformanceTurnSample[]) {
	const byMessageId: Record<string, string> = {};
	let live: string | null = null;
	let userStartedAt: number | null = null;
	for (let index = 0; index < chat.messages.length; index += 1) {
		const message = chat.messages[index]!;
		if (message.role === "user") {
			userStartedAt = timestamp(message.at);
			continue;
		}
		if (message.role !== "assistant" || userStartedAt == null) continue;
		let nextUserAt: number | null = null;
		for (let cursor = index + 1; cursor < chat.messages.length; cursor += 1) {
			const candidate = chat.messages[cursor]!;
			if (candidate.role !== "user") continue;
			nextUserAt = timestamp(candidate.at);
			break;
		}
		const turnSamples = samples.filter((sample) =>
			sample.startedAtMs >= userStartedAt! && (nextUserAt == null || sample.startedAtMs < nextUserAt)
		);
		const turnLabel = label(turnSamples) ?? GENERATION_TPS_UNAVAILABLE;
		byMessageId[message.id] = turnLabel;
		if (nextUserAt == null) live = turnLabel;
	}
	return { byMessageId, live };
}

export function useTurnPerformanceLabels(chat: LocalChat, events: RuntimeEvent[], running: boolean) {
	const [samples, setSamples] = useState<ModelPerformanceTurnSample[]>([]);

	useEffect(() => {
		let disposed = false;
		const refresh = async () => {
			try {
				const next = await bridges.modelPerformance?.turnSamples(chat.id);
				if (!disposed && next) setSamples(next);
			} catch {
				// Throughput is optional; a missing sample stays absent rather than stale.
			}
		};
		void refresh();
		if (!running) return () => { disposed = true; };
		const timer = window.setInterval(() => void refresh(), 2_000);
		return () => {
			disposed = true;
			window.clearInterval(timer);
		};
	}, [chat.id, running]);

	return useMemo(() => {
		const persisted = turnPerformanceLabels(chat, samples);
		if (!running) return persisted;
		let lastUserAt: number | null = null;
		for (const message of chat.messages) {
			if (message.role === "user") lastUserAt = timestamp(message.at);
		}
		const live = liveTurnPerformanceLabel(events, lastUserAt);
		if (!live) return persisted;
		const byMessageId = { ...persisted.byMessageId };
		let inCurrentTurn = false;
		for (const message of chat.messages) {
			if (message.role === "user") inCurrentTurn = timestamp(message.at) === lastUserAt;
			else if (inCurrentTurn && message.role === "assistant") byMessageId[message.id] = live;
		}
		return { byMessageId, live };
	}, [chat, events, running, samples]);
}
