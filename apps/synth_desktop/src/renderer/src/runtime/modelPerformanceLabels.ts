import {
	OPENROUTER_LAGUNA_S_MODEL,
	OPENROUTER_LUNA_MODEL,
	OPENROUTER_MUSE_SPARK_MODEL,
	SYNTH_CLOUD_LAGUNA_S_MODEL,
	SYNTH_CLOUD_MUSE_SPARK_MODEL
} from "../types/landing";
import type { ModelPerformanceSummary } from "../env";

export function performanceTargetId(summary: ModelPerformanceSummary): string | null {
	if (summary.provider === "local-laguna") return "local-laguna";
	if (summary.provider === "synth-cloud" && summary.modelId === SYNTH_CLOUD_LAGUNA_S_MODEL) {
		return "synth-cloud-laguna-s";
	}
	if (summary.provider === "synth-cloud" && summary.modelId === SYNTH_CLOUD_MUSE_SPARK_MODEL) {
		return "synth-cloud-muse-spark";
	}
	if (summary.provider !== "openrouter") return null;
	if (summary.modelId === OPENROUTER_LUNA_MODEL) return "openrouter-luna";
	if (summary.modelId === OPENROUTER_LAGUNA_S_MODEL) return "openrouter-laguna-s";
	if (summary.modelId === OPENROUTER_MUSE_SPARK_MODEL) return "openrouter-muse-spark";
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
	if (summary.measurementKind === "observed_stream") return 3;
	if (summary.measurementKind === "provider_reported") return 2;
	return 1;
}
