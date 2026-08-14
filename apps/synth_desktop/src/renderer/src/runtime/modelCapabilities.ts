import type { ExecutionTarget } from "@synth/runtime-protocol";
import { OPENROUTER_GEMINI_FLASH_MODEL, OPENROUTER_LAGUNA_S_MODEL, OPENROUTER_LUNA_MODEL, OPENROUTER_MUSE_SPARK_MODEL, SYNTH_CLOUD_LAGUNA_S_MODEL, SYNTH_CLOUD_MUSE_SPARK_MODEL } from "../types/landing";

/**
 * Declarative registry for model-specific composer controls.
 *
 * Register a model knob here instead of branching on model ids in App or Composer.
 * The same entry owns rendering, validation, persistence, defaults, and turn transport.
 */
export type ModelKnobTransportValue = "none" | "low" | "medium" | "high" | "xhigh" | "max" | "default" | "fast";
export type ModelKnobDisplayValue = "Minimal" | "None" | "Low" | "Medium" | "High" | "XHigh" | "Max" | "Standard" | "Fast";

export type ModelKnobOption = {
	displayValue: ModelKnobDisplayValue;
	transportValue: ModelKnobTransportValue;
};

export type ModelKnobSpec = {
	id: string;
	label: string;
	testId: string;
	storageKey: string;
	legacyStorageKeys?: string[];
	defaultValue: ModelKnobTransportValue;
	options: ModelKnobOption[];
	turnStartField: "effort" | "serviceTier";
};

export type ModelCapabilitySpec = {
	targetId: string;
	target: { kind: "local" } | { kind: "remote" | "cloud"; models: string[] };
	knobs: ModelKnobSpec[];
	/**
	 * How the provider's reasoning payload may be shown in the transcript.
	 * This is deliberately separate from the request knob: a model can accept
	 * a reasoning effort without ever returning displayable reasoning.
	 */
	reasoningDisplay: "none" | "full" | "summary";
	/** Input kinds accepted by the provider/model before a turn is attempted. */
	inputModalities: readonly ("text" | "image")[];
	/** Provider-advertised maximum combined input/output context window. */
	maxContextTokens: number;
};

const LUNA_EFFORT_OPTIONS: ModelKnobOption[] = [
	{ displayValue: "Low", transportValue: "low" },
	{ displayValue: "Medium", transportValue: "medium" },
	{ displayValue: "High", transportValue: "high" },
	{ displayValue: "XHigh", transportValue: "xhigh" },
	{ displayValue: "Max", transportValue: "max" }
];

const SPARK_EFFORT_OPTIONS: ModelKnobOption[] = [
	{ displayValue: "Low", transportValue: "low" },
	{ displayValue: "Medium", transportValue: "medium" },
	{ displayValue: "High", transportValue: "high" },
	{ displayValue: "XHigh", transportValue: "xhigh" }
];

const BINARY_THINKING_OPTIONS: ModelKnobOption[] = [
	{ displayValue: "None", transportValue: "none" },
	{ displayValue: "Max", transportValue: "max" }
];

const LOCAL_THINKING_OPTIONS: ModelKnobOption[] = [
	{ displayValue: "Minimal", transportValue: "none" },
	{ displayValue: "Max", transportValue: "high" }
];

const SERVICE_TIER_OPTIONS: ModelKnobOption[] = [
	{ displayValue: "Standard", transportValue: "default" },
	{ displayValue: "Fast", transportValue: "fast" }
];

export const MODEL_CAPABILITY_REGISTRY: ModelCapabilitySpec[] = [
	{
		targetId: "chatgpt-luna",
		target: { kind: "remote", models: ["gpt-5.6-luna"] },
		knobs: [
			{ id: "reasoning", label: "Thinking", testId: "reasoning-effort", storageKey: "synth.models.chatgpt-luna.reasoning", defaultValue: "medium", options: LUNA_EFFORT_OPTIONS, turnStartField: "effort" },
			{ id: "service-tier", label: "Speed", testId: "service-tier", storageKey: "synth.models.chatgpt-luna.service-tier", defaultValue: "default", options: SERVICE_TIER_OPTIONS, turnStartField: "serviceTier" }
		],
		reasoningDisplay: "summary", inputModalities: ["text", "image"], maxContextTokens: 272_000
	},
	{
		targetId: "chatgpt-sol",
		target: { kind: "remote", models: ["gpt-5.6-sol"] },
		knobs: [
			{ id: "reasoning", label: "Thinking", testId: "reasoning-effort", storageKey: "synth.models.chatgpt-sol.reasoning", defaultValue: "medium", options: LUNA_EFFORT_OPTIONS, turnStartField: "effort" },
			{ id: "service-tier", label: "Speed", testId: "service-tier", storageKey: "synth.models.chatgpt-sol.service-tier", defaultValue: "default", options: SERVICE_TIER_OPTIONS, turnStartField: "serviceTier" }
		],
		reasoningDisplay: "summary", inputModalities: ["text", "image"], maxContextTokens: 272_000
	},
	{
		targetId: "chatgpt-terra",
		target: { kind: "remote", models: ["gpt-5.6-terra"] },
		knobs: [
			{ id: "reasoning", label: "Thinking", testId: "reasoning-effort", storageKey: "synth.models.chatgpt-terra.reasoning", defaultValue: "medium", options: LUNA_EFFORT_OPTIONS, turnStartField: "effort" },
			{ id: "service-tier", label: "Speed", testId: "service-tier", storageKey: "synth.models.chatgpt-terra.service-tier", defaultValue: "default", options: SERVICE_TIER_OPTIONS, turnStartField: "serviceTier" }
		],
		reasoningDisplay: "summary", inputModalities: ["text", "image"], maxContextTokens: 272_000
	},
	{
		targetId: "local-laguna",
		target: { kind: "local" },
		knobs: [{
			id: "reasoning",
			label: "Thinking",
			testId: "reasoning-effort",
			storageKey: "synth.models.local-laguna.reasoning",
			legacyStorageKeys: ["synth.lagunaThinking"],
			defaultValue: "high",
			options: LOCAL_THINKING_OPTIONS,
			turnStartField: "effort"
		}],
		// The owned MLX Responses bridge separates its <think> span from the
		// answer stream, so this is the one target allowed to show that text.
		reasoningDisplay: "full",
		inputModalities: ["text"],
		maxContextTokens: 262_144
	},
	{
		targetId: "openrouter-luna",
		target: { kind: "remote", models: [OPENROUTER_LUNA_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Reasoning effort",
			testId: "reasoning-effort",
			storageKey: "synth.reasoningEffort",
			defaultValue: "medium",
			options: LUNA_EFFORT_OPTIONS,
			turnStartField: "effort"
		}],
		// Closed/remote providers expose a provider-authored summary, not their
		// private chain of thought. Render only that safe payload when present.
		reasoningDisplay: "summary",
		inputModalities: ["text", "image"],
		maxContextTokens: 272_000
	},
	{
		targetId: "openrouter-laguna-s",
		target: { kind: "remote", models: [OPENROUTER_LAGUNA_S_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Thinking",
			testId: "reasoning-effort",
			storageKey: "synth.models.openrouter-laguna-s.reasoning",
			legacyStorageKeys: ["synth.lagunaThinking"],
			defaultValue: "max",
			options: BINARY_THINKING_OPTIONS,
			turnStartField: "effort"
		}],
		// Poolside's S 2.1 model card documents a per-request
		// `enable_thinking` switch rather than graded low/max budgets. Our
		// Responses adapter carries this binary choice as none/max, so present
		// the exact provider vocabulary to people: None / Max.
		reasoningDisplay: "summary",
		inputModalities: ["text"],
		maxContextTokens: 262_144
	},
	{
		targetId: "openrouter-muse-spark",
		target: { kind: "remote", models: [OPENROUTER_MUSE_SPARK_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Reasoning effort",
			testId: "reasoning-effort",
			storageKey: "synth.models.openrouter-muse-spark.reasoning",
			defaultValue: "medium",
			options: SPARK_EFFORT_OPTIONS,
			turnStartField: "effort"
		}],
		reasoningDisplay: "summary",
		inputModalities: ["text", "image"],
		maxContextTokens: 1_048_576
	},
	{
		targetId: "openrouter-gemini-flash",
		target: { kind: "remote", models: [OPENROUTER_GEMINI_FLASH_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Reasoning effort",
			testId: "reasoning-effort",
			storageKey: "synth.models.openrouter-gemini-flash.reasoning",
			defaultValue: "medium",
			options: LUNA_EFFORT_OPTIONS,
			turnStartField: "effort"
		}],
		reasoningDisplay: "summary",
		inputModalities: ["text", "image"],
		maxContextTokens: 1_048_576
	},
	{
		targetId: "synth-cloud-laguna-s",
		target: { kind: "cloud", models: [SYNTH_CLOUD_LAGUNA_S_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Thinking",
			testId: "reasoning-effort",
			storageKey: "synth.models.synth-cloud-laguna-s.reasoning",
			legacyStorageKeys: ["synth.lagunaThinking"],
			defaultValue: "max",
			options: BINARY_THINKING_OPTIONS,
			turnStartField: "effort"
		}],
		reasoningDisplay: "summary",
		inputModalities: ["text"],
		maxContextTokens: 262_144
	},
	{
		targetId: "synth-cloud-muse-spark",
		target: { kind: "remote", models: [SYNTH_CLOUD_MUSE_SPARK_MODEL] },
		knobs: [{
			id: "reasoning",
			label: "Reasoning effort",
			testId: "reasoning-effort",
			storageKey: "synth.models.synth-cloud-muse-spark.reasoning",
			defaultValue: "medium",
			options: SPARK_EFFORT_OPTIONS,
			turnStartField: "effort"
		}],
		reasoningDisplay: "summary",
		inputModalities: ["text", "image"],
		maxContextTokens: 1_048_576
	}
];

export type ModelKnobValues = Record<string, ModelKnobTransportValue>;

export function modelKnobKey(targetId: string, knobId: string): string {
	return `${targetId}:${knobId}`;
}

export function modelSupportsImageInput(targetId: string): boolean {
	return modelCapabilitiesForTarget(targetId)?.inputModalities.includes("image") ?? false;
}

export function modelCapabilitiesForTarget(targetId: string): ModelCapabilitySpec | undefined {
	return MODEL_CAPABILITY_REGISTRY.find((entry) => entry.targetId === targetId);
}

export function modelCapabilitiesForExecutionTarget(target: ExecutionTarget): ModelCapabilitySpec | undefined {
	if (target.kind === "intern") return undefined;
	return MODEL_CAPABILITY_REGISTRY.find((entry) =>
		entry.target.kind === target.kind &&
		(entry.target.kind === "local" || entry.target.models.includes(target.model))
	);
}

export function modelKnobForTarget(targetId: string, knobId: string): ModelKnobSpec | undefined {
	return modelCapabilitiesForTarget(targetId)?.knobs.find((knob) => knob.id === knobId);
}

export function loadModelKnobValues(storage: Pick<Storage, "getItem">): ModelKnobValues {
	const values: ModelKnobValues = {};
	for (const capability of MODEL_CAPABILITY_REGISTRY) {
		for (const knob of capability.knobs) {
			const candidates = [knob.storageKey, ...(knob.legacyStorageKeys ?? [])];
			const saved = candidates.map((key) => storage.getItem(key)).find((value) => value !== null);
			values[modelKnobKey(capability.targetId, knob.id)] =
				saved && knob.options.some((option) => option.transportValue === saved)
					? saved as ModelKnobTransportValue
					: knob.defaultValue;
		}
	}
	return values;
}

export function modelKnobValue(
	values: ModelKnobValues,
	targetId: string,
	knob: ModelKnobSpec
): ModelKnobTransportValue {
	const value = values[modelKnobKey(targetId, knob.id)];
	return knob.options.some((option) => option.transportValue === value) ? value : knob.defaultValue;
}

export function turnStartEffortForExecutionTarget(
	target: ExecutionTarget,
	values: ModelKnobValues
): ModelKnobTransportValue | undefined {
	const capability = modelCapabilitiesForExecutionTarget(target);
	const knob = capability?.knobs.find(
		(candidate) => candidate.turnStartField === "effort"
	);
	return capability && knob ? modelKnobValue(values, capability.targetId, knob) : undefined;
}

export function serviceTierForExecutionTarget(target: ExecutionTarget, values: ModelKnobValues): "default" | "fast" | undefined {
	const capability = modelCapabilitiesForExecutionTarget(target);
	const knob = capability?.knobs.find((candidate) => candidate.turnStartField === "serviceTier");
	const value = capability && knob ? modelKnobValue(values, capability.targetId, knob) : undefined;
	return value === "fast" || value === "default" ? value : undefined;
}
