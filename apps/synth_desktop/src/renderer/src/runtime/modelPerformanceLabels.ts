import {
	CHATGPT_LUNA_MODEL,
	CHATGPT_SOL_MODEL,
	CHATGPT_TERRA_MODEL,
	SYNTH_CLOUD_LAGUNA_S_MODEL,
	SYNTH_CLOUD_MUSE_SPARK_MODEL
} from "../types/landing";
import type { ModelPerformanceSummary } from "../bridge";
import { modelCatalogEntryForModel } from "./modelCatalog";

export function performanceTargetId(summary: ModelPerformanceSummary): string | null {
	if (summary.provider === "local-laguna") return "local-laguna";
	if (summary.provider === "openai-codex-oauth") {
		if (summary.modelId === CHATGPT_LUNA_MODEL) return "chatgpt-luna";
		if (summary.modelId === CHATGPT_SOL_MODEL) return "chatgpt-sol";
		if (summary.modelId === CHATGPT_TERRA_MODEL) return "chatgpt-terra";
		return null;
	}
	if (summary.provider === "synth-cloud" && summary.modelId === SYNTH_CLOUD_LAGUNA_S_MODEL) {
		return "synth-cloud-laguna-s";
	}
	if (summary.provider === "synth-cloud" && summary.modelId === SYNTH_CLOUD_MUSE_SPARK_MODEL) {
		return "synth-cloud-muse-spark";
	}
	if (summary.provider !== "openrouter") return null;
	const catalogEntry = modelCatalogEntryForModel(summary.modelId);
	if (catalogEntry) return catalogEntry.targetId;
	return null;
}

export function performanceKindLabel(kind: ModelPerformanceSummary["measurementKind"]): string {
	if (kind === "decode") return "decode";
	if (kind === "provider_reported") return "provider";
	if (kind === "end_to_end") return "end-to-end";
	return "observed";
}

export function performancePreference(summary: ModelPerformanceSummary, targetId: string): number {
	if (targetId === "local-laguna" && summary.measurementKind === "decode") return 4;
	if (summary.measurementKind === "observed_stream_segment") return 3;
	if (summary.measurementKind === "provider_reported") return 2;
	return 1;
}
