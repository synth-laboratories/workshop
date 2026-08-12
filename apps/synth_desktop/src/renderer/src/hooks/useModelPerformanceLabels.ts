import { useEffect, useMemo, useState } from "react";
import { formatTps } from "../components/InferencePanel";
import type { ModelPerformanceSummary } from "../bridge";
import {
	performanceKindLabel,
	performancePreference,
	performanceTargetId
} from "../runtime/modelPerformanceLabels";
import type { InferenceMonitor } from "../components/InferencePanel";

/**
 * Aggregates native model-performance summaries for composer/sidebar labels.
 */
export function useModelPerformanceLabels(
	selectedTargetId: string,
	inferenceMonitor: InferenceMonitor
) {
	const [modelPerformance, setModelPerformance] = useState<ModelPerformanceSummary[]>([]);

	useEffect(() => {
		let disposed = false;
		const refresh = async () => {
			try {
				const summaries = await window.synthModelPerformance?.summaries();
				if (!disposed && summaries) setModelPerformance(summaries);
			} catch {
				// Optional telemetry must never block chat.
			}
		};
		void refresh();
		const timer = window.setInterval(() => void refresh(), 10_000);
		return () => {
			disposed = true;
			window.clearInterval(timer);
		};
	}, []);

	const persistedPerformanceByTarget = useMemo(() => {
		const chosen = new Map<string, ModelPerformanceSummary>();
		for (const summary of modelPerformance) {
			if (summary.tpsP50 == null || summary.sampleCount < 1) continue;
			const targetId = performanceTargetId(summary);
			if (!targetId) continue;
			const current = chosen.get(targetId);
			if (
				!current ||
				performancePreference(summary, targetId) > performancePreference(current, targetId)
			) {
				chosen.set(targetId, summary);
			}
		}
		return chosen;
	}, [modelPerformance]);

	const selectedModelMedianTps =
		selectedTargetId === "local-laguna"
			? inferenceMonitor.snapshot?.rolling.decodeTpsP50 ?? null
			: null;
	const selectedPersistedPerformance = persistedPerformanceByTarget.get(selectedTargetId);
	const selectedModelMedianTpsLabel =
		selectedModelMedianTps == null
			? selectedPersistedPerformance?.tpsP50 == null
				? null
				: `${formatTps(selectedPersistedPerformance.tpsP50)} tok/s ${performanceKindLabel(selectedPersistedPerformance.measurementKind)} p50`
			: `${formatTps(selectedModelMedianTps)} tok/s p50`;

	const aggregateModelTpsLabels = useMemo(() => {
		const labels: Record<string, string> = {};
		for (const [targetId, summary] of persistedPerformanceByTarget) {
			if (summary.tpsP50 == null) continue;
			labels[targetId] =
				`${formatTps(summary.tpsP50)} tok/s ${performanceKindLabel(summary.measurementKind)} p50 · ${summary.sampleCount} ${summary.sampleCount === 1 ? "request" : "requests"} · all sessions`;
		}
		if (!labels["local-laguna"] && selectedModelMedianTpsLabel) {
			labels["local-laguna"] = `${selectedModelMedianTpsLabel} · daemon lifetime`;
		}
		return labels;
	}, [persistedPerformanceByTarget, selectedModelMedianTpsLabel]);

	return {
		persistedPerformanceByTarget,
		selectedModelMedianTpsLabel,
		aggregateModelTpsLabels
	};
}
