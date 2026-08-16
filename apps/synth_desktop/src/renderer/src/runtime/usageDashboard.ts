import type { UsageBreakdown, UsageDayPoint } from "@synth/runtime-protocol";

export type { UsageBreakdown, UsageDayPoint };

/**
 * The reductions behind Data → Usage.
 *
 * Everything here is pure: one `UsageSummary` in, display-ready shapes out. It
 * lives apart from the component so the numbers can be tested without a
 * webview, and so the rules they encode are written down in one place:
 *
 *   · A dollar figure always says who vouches for it. Settled charges and
 *     tariff estimates are added — the ledger drops an estimate the moment its
 *     request settles, so the sum cannot double-count — but the split is never
 *     hidden.
 *   · Missing telemetry is `null`, and renders "Unavailable". A real zero and
 *     an unreported field must not look alike.
 *   · On-device runs have no provider charge, so they carry tokens and no
 *     dollars, and say so rather than showing $0.00.
 */

export const UNAVAILABLE = "Unavailable";

/** The chart never draws more buckets than this; the excess is announced, not dropped in silence. */
export const MAX_CHART_DAYS = 90;

/**
 * Categorical slots, assigned to a provider in fixed order and never cycled —
 * a provider keeps its hue when a filter changes the series count. The steps
 * themselves live in `styles/usage.css`, validated against both the light and
 * dark chart surfaces. A fifth provider folds into `other` rather than
 * inventing a hue nobody checked.
 */
export const SERIES_SLOTS = ["one", "two", "three", "four"] as const;
export const OTHER_SLOT = "other";
const MAX_SERIES = SERIES_SLOTS.length;

export function providerLabel(provider: string): string {
	if (provider === "local-laguna") return "On-device";
	if (provider === "openrouter") return "OpenRouter";
	if (provider === "synth-cloud") return "Synth Cloud";
	return provider;
}

// =============================================================================
// Formatting
// =============================================================================

const USD_CENTS = new Intl.NumberFormat(undefined, {
	style: "currency",
	currency: "USD",
	minimumFractionDigits: 2,
	maximumFractionDigits: 2
});
const USD_PRECISE = new Intl.NumberFormat(undefined, {
	style: "currency",
	currency: "USD",
	minimumFractionDigits: 2,
	maximumFractionDigits: 4
});

/** Dollars, keeping sub-cent amounts legible instead of rounding them away to $0.00. */
export function usd(value: number | null | undefined): string {
	if (typeof value !== "number" || !Number.isFinite(value)) return UNAVAILABLE;
	if (value !== 0 && Math.abs(value) < 0.01) return USD_PRECISE.format(value);
	return USD_CENTS.format(value);
}

/** Token counts at a glance: 48B, 1.27B, 142M, 12.4K, 940. */
export function compactTokens(value: number | null | undefined): string {
	if (typeof value !== "number" || !Number.isFinite(value)) return UNAVAILABLE;
	const abs = Math.abs(value);
	const unit = (divisor: number, suffix: string) => {
		const scaled = value / divisor;
		// Three significant figures reads better than a fixed decimal here:
		// 48B, not 48.0B; 1.27B, not 1.3B.
		const digits = Math.abs(scaled) >= 100 ? 0 : Math.abs(scaled) >= 10 ? 1 : 2;
		return `${Number(scaled.toFixed(digits))}${suffix}`;
	};
	if (abs >= 1e9) return unit(1e9, "B");
	if (abs >= 1e6) return unit(1e6, "M");
	if (abs >= 1e3) return unit(1e3, "K");
	return value.toLocaleString();
}

export function percent(rate: number | null | undefined): string {
	if (typeof rate !== "number" || !Number.isFinite(rate)) return UNAVAILABLE;
	return `${(rate * 100).toFixed(rate !== 0 && Math.abs(rate) < 0.001 ? 2 : 1)}%`;
}

/** `2026-08-10` → `AUG 10`, in the local calendar the Rust side bucketed on. */
export function axisDay(day: string): string {
	const [year, month, date] = day.split("-").map(Number);
	if (!year || !month || !date) return day;
	return new Date(year, month - 1, date)
		.toLocaleDateString(undefined, { month: "short", day: "numeric" })
		.toUpperCase();
}

export function longDay(day: string): string {
	const [year, month, date] = day.split("-").map(Number);
	if (!year || !month || !date) return day;
	return new Date(year, month - 1, date).toLocaleDateString(undefined, {
		month: "short",
		day: "numeric",
		year: "numeric"
	});
}

// =============================================================================
// Reductions
// =============================================================================

/**
 * What one slice cost, in mixed authority. `null` when nothing was ever priced,
 * which is different from a priced zero (an on-device run).
 */
export function spendUsd(row: UsageBreakdown): number | null {
	if (row.billedCostUsd == null && row.estimatedCostUsd == null) return null;
	return (row.billedCostUsd ?? 0) + (row.estimatedCostUsd ?? 0);
}

export type ProviderRoll = {
	provider: string;
	label: string;
	spendUsd: number | null;
	billedUsd: number | null;
	estimatedUsd: number | null;
	totalTokens: number;
	requests: number;
	share: number;
};

function addNullable(a: number | null, b: number | null | undefined): number | null {
	if (a == null && b == null) return null;
	return (a ?? 0) + (b ?? 0);
}

/** Per-provider rollup, ordered by spend then tokens — the order the bars and the chart share. */
export function providerRollup(models: UsageBreakdown[]): ProviderRoll[] {
	const byProvider = new Map<string, ProviderRoll>();
	for (const row of models) {
		const next: ProviderRoll = byProvider.get(row.provider) ?? {
			provider: row.provider,
			label: providerLabel(row.provider),
			spendUsd: null,
			billedUsd: null,
			estimatedUsd: null,
			totalTokens: 0,
			requests: 0,
			share: 0
		};
		next.billedUsd = addNullable(next.billedUsd, row.billedCostUsd);
		next.estimatedUsd = addNullable(next.estimatedUsd, row.estimatedCostUsd);
		next.spendUsd = addNullable(next.billedUsd, next.estimatedUsd);
		next.totalTokens += row.totalTokens;
		next.requests += row.requests;
		byProvider.set(row.provider, next);
	}
	const rolls = [...byProvider.values()].sort(
		(a, b) => (b.spendUsd ?? 0) - (a.spendUsd ?? 0) || b.totalTokens - a.totalTokens
	);
	const total = rolls.reduce((sum, roll) => sum + (roll.spendUsd ?? 0), 0);
	for (const roll of rolls) {
		roll.share = total > 0 ? (roll.spendUsd ?? 0) / total : 0;
	}
	return rolls;
}

/**
 * Which colour slot a provider gets — fixed by its rank in this window's
 * rollup, so the assignment is stable for as long as the window is.
 */
export function seriesSlots(rolls: ProviderRoll[]): Map<string, string> {
	const slots = new Map<string, string>();
	rolls.forEach((roll, index) => {
		slots.set(roll.provider, index < MAX_SERIES ? SERIES_SLOTS[index] : OTHER_SLOT);
	});
	return slots;
}

export type ChartSeries = {
	days: string[];
	providers: string[];
	/** `values[provider][day]`, zero-filled across the whole span. */
	values: number[][];
	/** The true stacked peak. */
	max: number;
	/** The peak rounded up to a readable tick, which is what the axis is drawn to. */
	axisMax: number;
	truncatedFrom: number | null;
};

/**
 * The next readable tick at or above `value` — 1, 2, 2.5, or 5 times a power of
 * ten. An axis labelled $111.54 is a number nobody asked for; $125.00 is a
 * scale you can read a bar against.
 */
export function niceCeil(value: number): number {
	if (!Number.isFinite(value) || value <= 0) return 0;
	const magnitude = 10 ** Math.floor(Math.log10(value));
	const normalized = value / magnitude;
	const step = [1, 2, 2.5, 5, 10].find((candidate) => normalized <= candidate + 1e-9) ?? 10;
	return step * magnitude;
}

/**
 * The day series, zero-filled into a continuous calendar and stacked in the
 * provider order the rest of the page uses. Rust deliberately omits empty
 * days — a chart needs them, a table does not — so the filling happens here.
 */
export function chartSeries(
	points: UsageDayPoint[],
	providers: string[],
	metric: "cost" | "tokens"
): ChartSeries {
	if (points.length === 0) {
		return { days: [], providers, values: [], max: 0, axisMax: 0, truncatedFrom: null };
	}
	const byDay = new Map<string, Map<string, number>>();
	for (const point of points) {
		const value = metric === "cost" ? spendUsd(point.totals) ?? 0 : point.totals.totalTokens;
		const day = byDay.get(point.day) ?? new Map<string, number>();
		day.set(point.totals.provider, (day.get(point.totals.provider) ?? 0) + value);
		byDay.set(point.day, day);
	}
	const stamps = [...byDay.keys()].sort();
	const first = new Date(`${stamps[0]}T00:00:00`);
	const last = new Date(`${stamps[stamps.length - 1]}T00:00:00`);
	const allDays: string[] = [];
	for (const cursor = new Date(first); cursor <= last; cursor.setDate(cursor.getDate() + 1)) {
		const year = cursor.getFullYear();
		const month = `${cursor.getMonth() + 1}`.padStart(2, "0");
		const date = `${cursor.getDate()}`.padStart(2, "0");
		allDays.push(`${year}-${month}-${date}`);
	}
	const truncatedFrom = allDays.length > MAX_CHART_DAYS ? allDays.length : null;
	const days = truncatedFrom ? allDays.slice(-MAX_CHART_DAYS) : allDays;
	const values = providers.map((provider) => days.map((day) => byDay.get(day)?.get(provider) ?? 0));
	const max = days.reduce((peak, _day, index) => {
		const stacked = values.reduce((sum, row) => sum + row[index], 0);
		return Math.max(peak, stacked);
	}, 0);
	return { days, providers, values, max, axisMax: niceCeil(max), truncatedFrom };
}

export type CostQualityRow = { key: string; label: string; amountUsd: number; share: number };

const COST_AUTHORITY: Array<[string, string]> = [
	["provider_reported", "Provider reported"],
	["synth_cloud", "Synth Cloud"],
	["tariff_estimate", "Tariff estimate"],
	["none", "Unpriced"]
];

/**
 * Who vouches for the money on this page, by share of spend — the one panel
 * that answers "how much of this number should I trust". Every authority is
 * always listed, so a 0% row is a statement rather than a gap.
 */
export function costQuality(models: UsageBreakdown[]): CostQualityRow[] {
	const totals = new Map<string, number>();
	for (const row of models) {
		totals.set(row.costSource, (totals.get(row.costSource) ?? 0) + (spendUsd(row) ?? 0));
	}
	const total = [...totals.values()].reduce((sum, value) => sum + value, 0);
	return COST_AUTHORITY.map(([key, label]) => {
		const amountUsd = totals.get(key) ?? 0;
		return { key, label, amountUsd, share: total > 0 ? amountUsd / total : 0 };
	});
}
