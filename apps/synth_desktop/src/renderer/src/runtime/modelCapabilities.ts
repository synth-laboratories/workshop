import type { ExecutionTarget } from "@synth/runtime-protocol";
import { MUSE_GLIMMER_MODEL, OPENROUTER_LAGUNA_S_MODEL, OPENROUTER_LUNA_MODEL, SYNTH_CLOUD_LAGUNA_S_MODEL } from "../types/landing";

/**
 * Declarative registry for model-specific composer controls.
 *
 * Register a model knob here instead of branching on model ids in App or Composer.
 * The same entry owns rendering, validation, persistence, defaults, and turn transport.
 */
export type ModelKnobValue = "none" | "low" | "medium" | "high" | "xhigh" | "max";

export type ModelKnobOption = {
	id: ModelKnobValue;
	label: string;
};

export type ModelKnobSpec = {
	id: string;
	label: string;
	testId: string;
	storageKey: string;
	legacyStorageKeys?: string[];
	defaultValue: ModelKnobValue;
	options: ModelKnobOption[];
	turnStartField: "effort";
};

export type ModelCapabilitySpec = {
	targetId: string;
	target: { kind: "local"; models?: string[] } | { kind: "remote"; models: string[] };
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
	{ id: "low", label: "Low" },
	{ id: "medium", label: "Medium" },
	{ id: "high", label: "High" },
	{ id: "xhigh", label: "XHigh" },
	{ id: "max", label: "Max" }
];

const BINARY_THINKING_OPTIONS: ModelKnobOption[] = [
	{ id: "none", label: "None" },
	{ id: "max", label: "Max" }
];

const LOCAL_THINKING_OPTIONS: ModelKnobOption[] = [
	{ id: "none", label: "Off" },
	{ id: "high", label: "On" }
];

export const MODEL_CAPABILITY_REGISTRY: ModelCapabilitySpec[] = [
	{
		targetId: "local-muse-glimmer",
		target: { kind: "local", models: [MUSE_GLIMMER_MODEL] },
		knobs: [],
		reasoningDisplay: "full",
		// llama.cpp has the projector loaded, but Workshop's local Responses
		// transport does not yet forward image parts to the Muse engine.
		inputModalities: ["text"],
		maxContextTokens: 131_072
	},
	{
		targetId: "local-laguna",
		target: { kind: "local", models: ["laguna-xs-2.1"] },
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
		targetId: "synth-cloud-laguna-s",
		target: { kind: "remote", models: [SYNTH_CLOUD_LAGUNA_S_MODEL] },
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
	}
];

export type ModelKnobValues = Record<string, ModelKnobValue>;

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
		entry.target.models?.includes(target.model)
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
				saved && knob.options.some((option) => option.id === saved)
					? saved as ModelKnobValue
					: knob.defaultValue;
		}
	}
	return values;
}

export function modelKnobValue(
	values: ModelKnobValues,
	targetId: string,
	knob: ModelKnobSpec
): ModelKnobValue {
	const value = values[modelKnobKey(targetId, knob.id)];
	return knob.options.some((option) => option.id === value) ? value : knob.defaultValue;
}

export function turnStartEffortForExecutionTarget(
	target: ExecutionTarget,
	values: ModelKnobValues
): ModelKnobValue | undefined {
	const capability = modelCapabilitiesForExecutionTarget(target);
	const knob = capability?.knobs.find(
		(candidate) => candidate.turnStartField === "effort"
	);
	return capability && knob ? modelKnobValue(values, capability.targetId, knob) : undefined;
}
