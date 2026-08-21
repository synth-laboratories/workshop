import { useEffect, useMemo, useState } from "react";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import type { LocalChat } from "../types/landing";
import type { ModelPerformanceTurnSample } from "../bridge";

const GENERATION_TPS_UNAVAILABLE = "Generation speed unavailable";
const GENERATION_SPEED_EVENT = "turn/generationSpeed";
const MEASUREMENT_SCHEMA = "synth.generation-speed.v1";

/**
 * One backend measurement of one output-text segment, as it arrives on the
 * journal. The renderer does not compute rates: it renders what the transport
 * measured at the stream frame, which is the only place the timing is honest.
 */
type GenerationSpeedMeasurement = {
	schemaVersion: string;
	measurementKind: string;
	itemId: string;
	responseId: string | null;
	outputIndex: number;
	contentIndex: number;
	phase: "commentary" | "final_answer" | "other";
	status: "completed" | "partial" | "unavailable";
	tps: number | null;
	exactTokensAfterFirstSample: number;
	durationMs: number;
	sampleCount: number;
	tokenCountSource: string;
	tokenizerId?: string | null;
	clockSource: string;
	unavailableReason: string | null;
	qualityFlags: string[];
};

function formatTps(value: number): string {
	return value >= 10 ? value.toFixed(1) : value.toFixed(2);
}

function timestamp(value: string): number | null {
	const parsed = Date.parse(value);
	return Number.isFinite(parsed) ? parsed : null;
}

function compactDuration(milliseconds: number): string {
	const seconds = Math.max(0, Math.round(milliseconds / 1_000));
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	const remainder = seconds % 60;
	return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function isTerminal(event: RuntimeEvent): boolean {
	return ["run.completed", "run.failed", "run.cancelled", "turn/completed", "turn/failed", "turn/interrupted"].includes(event.eventKind);
}

/**
 * Read a measurement off an event, or `null` when the payload is not one.
 *
 * The schema version is checked, not assumed: nothing recorded before segment
 * measurement existed may be rendered as if it were a measurement.
 */
function measurement(event: RuntimeEvent): GenerationSpeedMeasurement | null {
	if (event.eventKind !== GENERATION_SPEED_EVENT) return null;
	const payload = event.payload as Partial<GenerationSpeedMeasurement> | undefined;
	if (!payload || payload.schemaVersion !== MEASUREMENT_SCHEMA) return null;
	if (typeof payload.itemId !== "string" || payload.itemId.length === 0) return null;
	return payload as GenerationSpeedMeasurement;
}

/** Whether a rate may be shown as this segment's speed. */
function isPublishable(value: GenerationSpeedMeasurement): boolean {
	return typeof value.tps === "number" && Number.isFinite(value.tps) && value.tps > 0
		&& (value.status === "completed" || value.status === "partial");
}

/**
 * The audit trail behind one displayed value, for the Advanced view's tooltip.
 * Data only — every field is something the measurement recorded about itself.
 */
function detail(value: GenerationSpeedMeasurement): string {
	const segment = [value.responseId, value.itemId, value.outputIndex, value.contentIndex]
		.filter((part) => part !== null && part !== undefined)
		.join(":");
	const fields = [
		`kind ${value.measurementKind}`,
		`tokens ${value.exactTokensAfterFirstSample}`,
		`duration ${(value.durationMs / 1_000).toFixed(2)}s`,
		`samples ${value.sampleCount}`,
		`token source ${value.tokenCountSource}`,
		value.tokenizerId ? `tokenizer ${value.tokenizerId}` : null,
		`clock ${value.clockSource}`,
		`segment ${segment}`,
		value.status === "partial" ? "partial" : null,
		value.unavailableReason ? `reason ${value.unavailableReason}` : null,
		value.qualityFlags.length ? `flags ${value.qualityFlags.join(", ")}` : null
	].filter((field): field is string => field !== null);
	return `Client-observed text delivery; excludes tools and reasoning. ${fields.join(" · ")}`;
}

function generationLabel(value: GenerationSpeedMeasurement | undefined): string {
	if (!value || !isPublishable(value)) return GENERATION_TPS_UNAVAILABLE;
	const rate = `Observed generation: ${formatTps(value.tps!)} tok/s`;
	return value.status === "partial" ? `${rate} (partial)` : rate;
}

function endToEndSample(
	samples: ModelPerformanceTurnSample[],
	messageAt: number,
	terminalAt: number | null
): ModelPerformanceTurnSample | null {
	const matches = samples.filter((sample) =>
		sample.measurementKind === "end_to_end"
		&& sample.startedAtMs <= messageAt
		&& sample.completedAtMs >= messageAt
		&& (terminalAt == null || sample.completedAtMs <= terminalAt + 1_000)
	);
	return matches.sort((a, b) => a.completedAtMs - b.completedAtMs).at(0) ?? null;
}

function endToEndLabel(sample: ModelPerformanceTurnSample | null): TurnPerformanceLabel | null {
	if (!sample || !Number.isFinite(sample.outputTps) || sample.outputTps <= 0) return null;
	return {
		generation: `End-to-end output: ${formatTps(sample.outputTps)} tok/s`,
		worked: null,
		detail: "Authoritative provider output tokens divided by turn acceptance-to-completion time. Includes first-token latency; this is not decoder-only TPS."
	};
}

export type TurnPerformanceLabel = {
	generation: string;
	worked: string | null;
	detail: string | null;
};

/**
 * Build immutable temporal snapshots from the durable event journal.
 *
 * Generation speed is not computed here. Each label renders one backend
 * measurement of one output-text segment, matched to the assistant message that
 * segment produced — the transport's `itemId` is that message's id. A turn that
 * calls tools produces several assistant messages and therefore several
 * measurements, each with its own tokens and its own elapsed time; they are
 * shown separately rather than blended into one turn-wide figure, and tool
 * execution appears in none of them.
 *
 * A message with no measurement, or whose measurement did not clear its
 * eligibility thresholds, reads `Generation speed unavailable`. Nothing is
 * carried forward from an earlier segment to fill the gap.
 *
 * "Worked" is terminal time minus the latest persisted turn/accepted boundary,
 * never render time. It is elapsed wall time for the whole turn — deliberately
 * a different quantity from generation speed, and never divided into one.
 */
export function turnPerformanceLabels(
	chat: LocalChat,
	events: RuntimeEvent[],
	running = false,
	turnSamples: ModelPerformanceTurnSample[] = []
) {
	const byMessageId: Record<string, TurnPerformanceLabel> = {};
	const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
	const messages = chat.messages;

	// Last measurement wins per segment: a replayed journal may deliver the same
	// event twice, and a segment is measured once when it ends.
	const measurements = new Map<string, GenerationSpeedMeasurement>();
	for (const event of ordered) {
		const value = measurement(event);
		if (value) measurements.set(value.itemId, value);
	}

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
		for (const event of ordered) {
			const at = timestamp(event.createdAt);
			if (at == null || at < turnStartAt || (cutoff != null && at >= cutoff && event !== terminal)) continue;
			if (event.eventKind === "turn/accepted") acceptedAt = at;
		}
		const value = measurements.get(message.id);
		const worked = isFinal && terminalAt != null && acceptedAt != null && terminalAt >= acceptedAt
			? `Worked ${compactDuration(terminalAt - acceptedAt)}`
			: null;
		const fallback = (!value || !isPublishable(value)) && isFinal && terminalAt != null
			? endToEndLabel(endToEndSample(turnSamples, messageAt, terminalAt))
			: null;
		byMessageId[message.id] = fallback ?? {
			generation: generationLabel(value),
			worked,
			detail: value ? detail(value) : null
		};
	}
	// No live figure while a segment is still streaming. A measurement exists
	// only once its segment has ended, and showing the previous segment's rate
	// here would be a number about text the model is not generating.
	return { byMessageId, live: null as string | null };
}

export type TurnSamplesLoader = (sessionId: string) => Promise<ModelPerformanceTurnSample[]>;

export function useTurnPerformanceLabels(
	chat: LocalChat,
	events: RuntimeEvent[],
	running: boolean,
	loadTurnSamples?: TurnSamplesLoader
) {
	const [turnSamples, setTurnSamples] = useState<ModelPerformanceTurnSample[]>([]);
	const terminalCursor = useMemo(
		() => events.filter(isTerminal).map((event) => event.sequence).join(","),
		[events]
	);

	useEffect(() => {
		let disposed = false;
		// A provider may settle and persist its authoritative usage before the UI
		// clears its last activity line.  The terminal journal event is the source
		// of truth for whether a settled sample is eligible to display.
		if ((!terminalCursor && running) || !loadTurnSamples) return () => { disposed = true; };
		void loadTurnSamples(chat.id)
			.then((samples) => { if (!disposed) setTurnSamples(samples); })
			.catch(() => { if (!disposed) setTurnSamples([]); });
		return () => { disposed = true; };
	}, [chat.id, loadTurnSamples, running, terminalCursor]);

	return useMemo(
		() => turnPerformanceLabels(chat, events, running, turnSamples),
		[chat, events, running, turnSamples]
	);
}
