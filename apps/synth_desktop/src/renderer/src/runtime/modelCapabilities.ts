import type { ExecutionTarget } from "@synth/runtime-protocol";
import type { ModelCatalogEntry } from "../generated/protocol";
import { isOpenRouterTargetId } from "../types/landing";
import { SYNTH_CLOUD_LAGUNA_S_MODEL, SYNTH_CLOUD_LAGUNA_XS_B200_MODEL, SYNTH_CLOUD_LAGUNA_XS_H100_MODEL, SYNTH_CLOUD_MUSE_SPARK_MODEL } from "../types/landing";

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
	maxContextTokens?: number;
	/** Provider-discovered tool availability. No catalog entry invents this. */
	supportsTools?: boolean;
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

const BUILTIN_MODEL_CAPABILITY_REGISTRY: ModelCapabilitySpec[] = [
	{
		targetId: "chatgpt-luna",
		target: { kind: "remote", models: ["gpt-5.6-luna"] },
		knobs: [
			{ id: "reasoning", label: "Thinking", testId: "reasoning-effort", storageKey: "synth.models.chatgpt-luna.reasoning", defaultValue: "xhigh", options: LUNA_EFFORT_OPTIONS, turnStartField: "effort" },
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
	...[
		["synth-cloud-laguna-xs-b200", SYNTH_CLOUD_LAGUNA_XS_B200_MODEL],
		["synth-cloud-laguna-xs-h100", SYNTH_CLOUD_LAGUNA_XS_H100_MODEL]
	].map(([targetId, model]) => ({
		targetId,
		target: { kind: "cloud" as const, models: [model] },
		knobs: [{
			id: "reasoning",
			label: "Thinking",
			testId: "reasoning-effort",
			storageKey: `synth.models.${targetId}.reasoning`,
			defaultValue: "max" as const,
			options: BINARY_THINKING_OPTIONS,
			turnStartField: "effort" as const
		}],
		reasoningDisplay: "summary" as const,
		inputModalities: ["text" as const],
		maxContextTokens: 262_144
	})),
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

/** Replaced atomically when Rust's catalog changes; built-ins and configured
 * OpenRouter targets consequently use the same capability projection path. */
export let MODEL_CAPABILITY_REGISTRY: ModelCapabilitySpec[] = [...BUILTIN_MODEL_CAPABILITY_REGISTRY];

export function installModelCatalogCapabilities(entries: ModelCatalogEntry[]): void {
	const nonOpenRouter = BUILTIN_MODEL_CAPABILITY_REGISTRY.filter((entry) => !isOpenRouterTargetId(entry.targetId));
	MODEL_CAPABILITY_REGISTRY = [
		...nonOpenRouter,
		...entries.map((entry) => catalogCapability(entry))
	];
}

function catalogCapability(entry: ModelCatalogEntry): ModelCapabilitySpec {
	const reasoning = entry.capabilities.reasoningControl;
	const options = reasoning === "binary" ? BINARY_THINKING_OPTIONS : LUNA_EFFORT_OPTIONS;
	const defaultValue = entry.capabilities.defaultReasoning;
	const admittedDefault = defaultValue && options.some((option) => option.transportValue === defaultValue)
		? defaultValue as ModelKnobTransportValue
		: reasoning === "binary" ? "max" : "high";
	return {
		targetId: entry.targetId,
		target: { kind: "remote", models: [entry.modelId] },
		knobs: reasoning === "none" ? [] : [{
			id: "reasoning",
			label: reasoning === "binary" ? "Thinking" : "Reasoning effort",
			testId: "reasoning-effort",
			storageKey: `synth.models.${entry.targetId}.reasoning`,
			defaultValue: admittedDefault,
			options,
			turnStartField: "effort"
		}],
		reasoningDisplay: reasoning === "none" ? "none" : "summary",
		inputModalities: entry.capabilities.inputModalities.includes("image") ? ["text", "image"] : ["text"],
		maxContextTokens: entry.capabilities.maxContextTokens ?? undefined,
		supportsTools: entry.capabilities.tools
	};
}

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
	if (target.kind === "remote" && target.targetId) {
		const byId = modelCapabilitiesForTarget(target.targetId);
		if (byId) return byId;
	}
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
