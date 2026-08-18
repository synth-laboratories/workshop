/**
 * Usage with coverage.
 *
 * Displaying zero for missing telemetry is forbidden, and a total that only
 * 3 of 40 rollouts reported is not the run's cost. Every figure therefore
 * carries who vouched for it and how much of the run it covers, and the
 * formatters below refuse to print a number the coverage does not support.
 */

import type { CoveredMetric, CoveredMetricSource, RunUsageProjection } from "./types";

export const UNAVAILABLE = "Unavailable";

/** Nothing was reported. Distinct from a reported 0, which is a real value. */
export function unavailableMetric(expectedUnits?: number): CoveredMetric {
	return {
		observedUnits: 0,
		...(expectedUnits != null ? { expectedUnits, coverage: 0 } : {}),
		source: "unavailable"
	};
}

export function coveredMetric(
	value: number | null | undefined,
	source: CoveredMetricSource,
	observedUnits: number,
	expectedUnits?: number
): CoveredMetric {
	if (value == null || !Number.isFinite(value)) return unavailableMetric(expectedUnits);
	return {
		value,
		observedUnits,
		...(expectedUnits != null && expectedUnits > 0
			? { expectedUnits, coverage: Math.min(1, observedUnits / expectedUnits) }
			: expectedUnits != null
				? { expectedUnits }
				: {}),
		source
	};
}

export function emptyUsage(): RunUsageProjection {
	return {
		costUsd: unavailableMetric(),
		promptTokens: unavailableMetric(),
		completionTokens: unavailableMetric(),
		rollouts: unavailableMetric()
	};
}

export function formatUsd(value: number): string {
	const absolute = Math.abs(value);
	if (absolute > 0 && absolute < 0.01) return `$${value.toFixed(4)}`;
	return `$${value.toFixed(2)}`;
}

export function formatCount(value: number): string {
	return value.toLocaleString("en-US", { maximumFractionDigits: 0 });
}

/** Coverage as a percentage, or null when no denominator was declared. */
export function coverageLabel(metric: CoveredMetric): string | null {
	if (metric.coverage == null) return null;
	const percent = metric.coverage * 100;
	return `${percent >= 10 || percent === 0 ? percent.toFixed(0) : percent.toFixed(1)}%`;
}

const SOURCE_WORDS: Record<CoveredMetricSource, string> = {
	provider: "provider reported",
	container: "container reported",
	derived: "derived from events",
	unavailable: "not reported"
};

/**
 * The compact line: value, then the shortest true thing about its coverage.
 * `unit` names what coverage is measured over ("rollout", "trial").
 */
export function metricSummary(
	metric: CoveredMetric,
	format: (value: number) => string,
	unit = "unit"
): string {
	if (metric.value == null) return `${UNAVAILABLE} · ${SOURCE_WORDS[metric.source]}`;
	const coverage = coverageLabel(metric);
	if (coverage == null) return `${format(metric.value)} · ${SOURCE_WORDS[metric.source]}`;
	return `${format(metric.value)} reported · ${coverage} ${unit} coverage`;
}

/** The dialog's fuller line: adds the observed/expected counts behind the share. */
export function metricExplanation(metric: CoveredMetric, unit = "unit"): string {
	const source = SOURCE_WORDS[metric.source];
	if (metric.expectedUnits == null) {
		return metric.observedUnits > 0
			? `${source} by ${formatCount(metric.observedUnits)} ${unit}${metric.observedUnits === 1 ? "" : "s"}; no denominator declared`
			: `${source}; no denominator declared`;
	}
	return `${formatCount(metric.observedUnits)} of ${formatCount(metric.expectedUnits)} ${unit}${
		metric.expectedUnits === 1 ? "" : "s"
	} reported it · ${source}`;
}

/**
 * Cost for the compact card. A partially covered total says so rather than
 * passing itself off as the bill.
 */
export function costSummary(metric: CoveredMetric, unit = "rollout"): string {
	if (metric.value == null) {
		return `Cost unavailable · ${metric.source === "unavailable" ? "producer emitted no cost telemetry" : SOURCE_WORDS[metric.source]}`;
	}
	return metricSummary(metric, formatUsd, unit);
}
