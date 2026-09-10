import type { ExecutionTarget } from "@synth/runtime-protocol";
import type { ModelCatalog, ModelCatalogEntry } from "../generated/protocol";
import {
	EXECUTION_TARGETS,
	replaceOpenRouterExecutionTargets,
	type ExecutionTargetOption,
	isOpenRouterTargetId
} from "../types/landing";
import { installModelCatalogCapabilities } from "./modelCapabilities";

const EMPTY_CATALOG: ModelCatalog = { entries: [], diagnostics: [], generatedAt: "" };

/** Browser/Playwright fixture only. Packaged Desktop replaces this with the
 * typed Rust projection before it resolves the default target. */
export const BROWSER_MODEL_CATALOG: ModelCatalog = {
	generatedAt: "browser-fixture",
	diagnostics: [],
	entries: [
		browserFixtureEntry("openrouter-luna", "openai/gpt-5.6-luna", "GPT 5.6 Luna", ["text", "image"], "effort", 272000),
		browserFixtureEntry("openrouter-laguna-s", "poolside/laguna-s-2.1", "Laguna S 2.1", ["text"], "binary", 262144),
		browserFixtureEntry("openrouter-muse-spark", "meta/muse-spark-1.2", "Muse Spark 1.2", ["text", "image"], "effort", 1048576),
		browserFixtureEntry("openrouter-gemini-flash", "google/gemini-3.7-flash", "Gemini 3.7 Flash", ["text", "image"], "effort", 1048576)
	]
};

let currentCatalog: ModelCatalog = EMPTY_CATALOG;
const historicalTargets = new Map<string, ExecutionTargetOption>();

export function installModelCatalog(catalog: ModelCatalog): void {
	currentCatalog = catalog;
	const pickerEntries = catalog.entries
		.filter((entry) => entry.enabled)
		.map(entryToTargetOption);
	replaceOpenRouterExecutionTargets(pickerEntries);
	installModelCatalogCapabilities(catalog.entries);
}

export function modelCatalog(): ModelCatalog {
	return currentCatalog;
}

export function modelCatalogEntry(targetId: string): ModelCatalogEntry | undefined {
	return currentCatalog.entries.find((entry) => entry.targetId === targetId);
}

export function modelCatalogEntryForModel(modelId: string): ModelCatalogEntry | undefined {
	return currentCatalog.entries.find((entry) => entry.modelId === modelId);
}

export function targetOptionForId(targetId: string): ExecutionTargetOption | undefined {
	return EXECUTION_TARGETS.find((target) => target.id === targetId) ?? historicalTargets.get(targetId);
}

export function isOpenRouterCatalogTarget(targetId: string): boolean {
	return isOpenRouterTargetId(targetId);
}

export function canStartNewTurn(targetId: string): boolean {
	const entry = modelCatalogEntry(targetId);
	if (!entry) return !targetId.startsWith("openrouter:");
	return entry.enabled && entry.availability !== "unavailable" && entry.availability !== "expired";
}

export function catalogStatusLabel(entry: ModelCatalogEntry | undefined): string | null {
	if (!entry) return null;
	switch (entry.availability) {
		case "ready": return entry.metadataObservedAt ? `Metadata checked ${formatObservation(entry.metadataObservedAt)}` : "Ready";
		case "credential_required": return "OpenRouter API key required";
		case "unverified": return "Unverified metadata";
		case "unavailable": return "Unavailable";
		case "expired": return "Expired";
	}
}

export function rememberHistoricalOpenRouterTarget(target: Extract<ExecutionTarget, { kind: "remote" }>, targetId: string): void {
	if (!isOpenRouterTargetId(targetId) || historicalTargets.has(targetId)) return;
	historicalTargets.set(targetId, {
		id: targetId,
		label: target.model,
		description: `OpenRouter · ${target.model} · retained session`,
		modelId: target.model,
		selectable: false,
		availability: "unavailable",
		group: "remote",
		diagnostic: "This configured target is no longer available for new sessions."
	});
}

export function unknownOpenRouterTargetId(model: string): string {
	let value = 2166136261;
	for (let index = 0; index < model.length; index += 1) {
		value ^= model.charCodeAt(index);
		value = Math.imul(value, 16777619);
	}
	return `openrouter:unknown:${(value >>> 0).toString(16).padStart(8, "0")}`;
}

function entryToTargetOption(entry: ModelCatalogEntry): ExecutionTargetOption {
	const status = catalogStatusLabel(entry);
	return {
		id: entry.targetId,
		label: entry.displayName,
		description: ["OpenRouter", entry.modelId, status].filter(Boolean).join(" · "),
		modelId: entry.modelId,
		selectable: entry.availability !== "unavailable" && entry.availability !== "expired",
		availability: entry.availability,
		source: entry.source,
		diagnostic: entry.diagnostic,
		group: "remote"
	};
}

function formatObservation(value: string): string {
	const parsed = Date.parse(value);
	return Number.isNaN(parsed) ? value : new Date(parsed).toLocaleDateString();
}

function browserFixtureEntry(
	targetId: string,
	modelId: string,
	displayName: string,
	inputModalities: string[],
	reasoningControl: ModelCatalogEntry["capabilities"]["reasoningControl"],
	maxContextTokens: number
): ModelCatalogEntry {
	return {
		targetId,
		provider: "openrouter",
		modelId,
		displayName,
		source: "builtin",
		enabled: true,
		availability: "credential_required",
		capabilities: {
			inputModalities,
			outputModalities: ["text"],
			tools: true,
			reasoningControl,
			defaultReasoning: reasoningControl === "binary" ? "max" : "high",
			maxContextTokens,
			maxCompletionTokens: null
		},
		metadataObservedAt: "browser fixture",
		metadataSource: "fixture",
		diagnostic: "OpenRouter API key required for new turns."
	};
}
