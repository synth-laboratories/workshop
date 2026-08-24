import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UsageSummary, UsageWindow } from "@synth/runtime-protocol";
import { bridges } from "../runtime/desktopBridge";
import {
	axisDay,
	type ChartSeries,
	chartSeries,
	compactTokens,
	costQuality,
	longDay,
	MAX_CHART_DAYS,
	OTHER_SLOT,
	percent,
	providerLabel,
	providerRollup,
	seriesSlots,
	spendUsd,
	usd
} from "../runtime/usageDashboard";

/**
 * The device usage dashboard.
 *
 * Every number here is reduced in Rust over the one `usage_records` ledger and
 * arrives in a single `UsageSummary`; this file only lays it out. The rules the
 * numbers obey — mixed authority is added but never hidden, missing telemetry
 * reads "Unavailable" rather than zero, on-device work carries tokens and no
 * dollars — live with the reductions in `runtime/usageDashboard.ts`.
 *
 * The chart is a partition of the same rows as the headline (the Rust tests
 * assert the day series adds back up to the totals), so the two cannot drift.
 */

const WINDOWS: Array<{ id: UsageWindow; label: string }> = [
	{ id: "today", label: "Today" },
	{ id: "7d", label: "7 days" },
	{ id: "30d", label: "30 days" },
	{ id: "all", label: "All time" }
];

// =============================================================================
// Chart
// =============================================================================

/** Plot geometry in user units; the SVG stretches to its container width. */
const PLOT_W = 1000;
const PLOT_H = 220;

function stackedPaths(series: ChartSeries): Array<{ area: string; line: string }> {
	const { days, values, axisMax: max } = series;
	if (days.length === 0 || max <= 0) return [];
	const x = (index: number) =>
		days.length === 1 ? PLOT_W / 2 : (index / (days.length - 1)) * PLOT_W;
	const y = (value: number) => PLOT_H - (value / max) * PLOT_H;
	const cumulative = days.map(() => 0);
	return values.map((row) => {
		const lower = [...cumulative];
		row.forEach((value, index) => {
			cumulative[index] += value;
		});
		const upper = [...cumulative];
		const top = upper.map((value, index) => `${x(index)},${y(value)}`);
		const bottom = lower
			.map((value, index) => `${x(index)},${y(value)}`)
			.reverse();
		return {
			area: `M ${top.join(" L ")} L ${bottom.join(" L ")} Z`,
			line: `M ${top.join(" L ")}`
		};
	});
}

function DailyChart({
	series,
	metric,
	slots,
	labelFor
}: {
	series: ChartSeries;
	metric: "cost" | "tokens";
	slots: Map<string, string>;
	labelFor: (provider: string) => string;
}) {
	const [hover, setHover] = useState<number | null>(null);
	const plotRef = useRef<HTMLDivElement>(null);
	const paths = useMemo(() => stackedPaths(series), [series]);
	const format = metric === "cost" ? usd : compactTokens;

	const onMove = useCallback(
		(event: React.MouseEvent<HTMLDivElement>) => {
			const box = plotRef.current?.getBoundingClientRect();
			if (!box || series.days.length === 0) return;
			const ratio = Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1);
			setHover(Math.round(ratio * (series.days.length - 1)));
		},
		[series.days.length]
	);

	if (series.days.length === 0 || series.max <= 0) {
		return (
			<div className="usage-chart-empty" data-testid="usage-chart-empty">
				<p>Nothing to plot in this window yet.</p>
			</div>
		);
	}

	const gridlines = [1, 0.5, 0];
	const hoveredTotals = hover == null
		? null
		: series.providers
			.map((provider, index) => ({ provider, value: series.values[index][hover] }))
			.filter((entry) => entry.value > 0);

	return (
		<div className="usage-chart" data-testid="usage-chart">
			<div className="usage-chart-frame">
				<div className="usage-chart-axis" aria-hidden="true">
					{gridlines.map((fraction) => (
						<span key={fraction} style={{ top: `${(1 - fraction) * 100}%` }}>
							{format(series.axisMax * fraction)}
						</span>
					))}
				</div>
				<div
					className="usage-chart-plot"
					ref={plotRef}
					onMouseMove={onMove}
					onMouseLeave={() => setHover(null)}
				>
					{gridlines.map((fraction) => (
						<div
							key={fraction}
							className="usage-chart-gridline"
							style={{ top: `${(1 - fraction) * 100}%` }}
							aria-hidden="true"
						/>
					))}
					<svg
						viewBox={`0 0 ${PLOT_W} ${PLOT_H}`}
						preserveAspectRatio="none"
						role="img"
						aria-label={`Daily ${metric === "cost" ? "spend" : "tokens"} by provider`}
					>
						{/* Fills first, edges second: a later band's fill would
						    otherwise paint over the seam that separates it from
						    the band below. */}
						{paths.map((path, index) => (
							<path
								key={`fill-${series.providers[index]}`}
								className={`usage-chart-area usage-series-${slots.get(series.providers[index]) ?? OTHER_SLOT}`}
								d={path.area}
							/>
						))}
						{paths.map((path, index) => (
							<g
								key={`edge-${series.providers[index]}`}
								className={`usage-series-${slots.get(series.providers[index]) ?? OTHER_SLOT}`}
							>
								{/* A surface-coloured seam keeps stacked fills from
								    touching, so adjacent bands stay readable without
								    relying on hue alone. */}
								<path
									className="usage-chart-seam"
									d={path.line}
									vectorEffect="non-scaling-stroke"
								/>
								<path
									className="usage-chart-line"
									d={path.line}
									vectorEffect="non-scaling-stroke"
								/>
							</g>
						))}
					</svg>
					{hover != null ? (
						<div
							className="usage-chart-crosshair"
							style={{
								left: `${series.days.length === 1 ? 50 : (hover / (series.days.length - 1)) * 100}%`
							}}
							aria-hidden="true"
						/>
					) : null}
					{hover != null && hoveredTotals ? (
						<div
							className="usage-chart-tooltip"
							data-testid="usage-chart-tooltip"
							style={{
								left: `${series.days.length === 1 ? 50 : (hover / (series.days.length - 1)) * 100}%`
							}}
						>
							<strong>{longDay(series.days[hover])}</strong>
							{hoveredTotals.length === 0 ? (
								<span className="usage-chart-tooltip-empty">No requests</span>
							) : (
								hoveredTotals.map((entry) => (
									<span key={entry.provider}>
										<i className={`usage-swatch usage-series-${slots.get(entry.provider) ?? OTHER_SLOT}`} />
										{labelFor(entry.provider)}
										<b>{format(entry.value)}</b>
									</span>
								))
							)}
						</div>
					) : null}
				</div>
			</div>
			<div className="usage-chart-days" aria-hidden="true">
				<span>{axisDay(series.days[0])}</span>
				{series.days.length > 2 ? (
					<span>{axisDay(series.days[Math.floor((series.days.length - 1) / 2)])}</span>
				) : null}
				{series.days.length > 1 ? (
					<span>{axisDay(series.days[series.days.length - 1])}</span>
				) : null}
			</div>
			{series.truncatedFrom ? (
				<p className="usage-chart-note" data-testid="usage-chart-truncated">
					Showing the most recent {MAX_CHART_DAYS} of {series.truncatedFrom} days. The
					totals above cover the whole window.
				</p>
			) : null}
		</div>
	);
}

// =============================================================================
// Panel
// =============================================================================

function Stat({
	label,
	value,
	note,
	testId
}: {
	label: string;
	value: string;
	note?: string | null;
	testId?: string;
}) {
	return (
		<div className="usage-stat" data-testid={testId}>
			<span className="usage-stat-label">{label}</span>
			<strong className="usage-stat-value">{value}</strong>
			{note ? <span className="usage-stat-note">{note}</span> : null}
		</div>
	);
}

function Segmented<T extends string>({
	label,
	options,
	value,
	onChange,
	testId,
	size = "md"
}: {
	label: string;
	options: Array<{ id: T; label: string }>;
	value: T;
	onChange: (next: T) => void;
	testId?: string;
	size?: "sm" | "md";
}) {
	return (
		<div
			className={size === "sm" ? "usage-segmented usage-segmented-sm" : "usage-segmented"}
			role="group"
			aria-label={label}
			data-testid={testId}
		>
			{options.map((option) => (
				<button
					key={option.id}
					type="button"
					className={option.id === value ? "usage-segmented-button is-active" : "usage-segmented-button"}
					aria-pressed={option.id === value}
					onClick={() => onChange(option.id)}
					data-testid={testId ? `${testId}-${option.id}` : undefined}
				>
					{option.label}
				</button>
			))}
		</div>
	);
}

export function UsagePanel() {
	const [usageWindow, setUsageWindow] = useState<UsageWindow>("30d");
	const [metric, setMetric] = useState<"cost" | "tokens">("cost");
	const [grouping, setGrouping] = useState<"model" | "day">("model");
	const [summary, setSummary] = useState<UsageSummary | null>(null);
	const [failed, setFailed] = useState(false);
	const [reloadToken, setReloadToken] = useState(0);

	useEffect(() => {
		const bridge = bridges.usage;
		if (!bridge) {
			setSummary(null);
			setFailed(true);
			return;
		}
		let disposed = false;
		setFailed(false);
		bridge
			.summary(usageWindow)
			.then((next) => {
				if (!disposed) setSummary(next);
			})
			.catch(() => {
				if (!disposed) {
					setSummary(null);
					setFailed(true);
				}
			});
		return () => {
			disposed = true;
		};
	}, [usageWindow, reloadToken]);

	const models = useMemo(() => summary?.models ?? [], [summary]);
	const rolls = useMemo(() => providerRollup(models), [models]);
	const slots = useMemo(() => seriesSlots(rolls), [rolls]);
	const series = useMemo(
		() => chartSeries(summary?.days ?? [], rolls.map((roll) => roll.provider), metric),
		[summary, rolls, metric]
	);
	const quality = useMemo(() => costQuality(models), [models]);

	const totals = summary?.totals ?? null;
	const totalSpend = totals ? spendUsd(totals) : null;
	const totalBilled = totals?.billedCostUsd ?? null;
	const totalEstimated = totals?.costSource === "synth_cloud" ? totals.estimatedCostUsd : null;
	const activeDays = new Set((summary?.days ?? []).map((point) => point.day)).size;

	return (
		<div className="usage-panel ws-stack" data-testid="usage-panel">
			<div className="usage-panel-head">
				<div>
					<h2 className="usage-panel-title">Usage</h2>
					<p className="usage-panel-lede">
						Every model request from this Mac, reduced from the on-device ledger.
					</p>
				</div>
				<div className="usage-panel-controls">
					<Segmented
						label="Usage window"
						options={WINDOWS.map((window) => ({ id: window.id, label: window.label }))}
						value={usageWindow}
						onChange={setUsageWindow}
						testId="usage-window"
					/>
					<button
						type="button"
						className="ws-btn ws-btn-secondary"
						onClick={() => setReloadToken((token) => token + 1)}
						data-testid="usage-refresh"
					>
						Refresh
					</button>
				</div>
			</div>

			{failed ? (
				<div className="ws-note ws-note-danger" role="alert" data-testid="usage-failed">
					Device usage is unavailable right now.
				</div>
			) : null}

			{!summary && !failed ? (
				<div className="ws-empty" data-testid="usage-loading">
					<p>Loading device usage…</p>
				</div>
			) : null}

			{summary && totals ? (
				<>
					<div className="usage-hero">
						<section className="usage-hero-spend" data-testid="usage-hero">
							<span className="usage-eyebrow">Device spend</span>
							<strong className="usage-hero-value" data-testid="usage-hero-value">
								{usd(totalSpend)}
							</strong>
							<p className="usage-hero-note" data-testid="usage-hero-note">
								{totalSpend == null
									? "No request in this window carried a price."
									: totalBilled != null && totalEstimated != null
										? `${usd(totalBilled)} settled · ${usd(totalEstimated)} estimated by Backend`
										: totalBilled != null
											? `${usd(totalBilled)} settled with the provider`
											: totalEstimated != null
												? `${usd(totalEstimated)} estimated by Backend · not settled`
												: "Cost unavailable — Backend reported no estimate or actual receipt."}
							</p>

							<div className="usage-provider-bars" data-testid="usage-provider-bars">
								{rolls.length === 0 ? (
									<p className="usage-muted">No provider activity in this window.</p>
								) : (
									rolls.map((roll) => (
										<div
											className="usage-provider"
											key={roll.provider}
											data-testid={`usage-provider-${roll.provider}`}
										>
											<div className="usage-provider-head">
												<span>
													<i className={`usage-swatch usage-series-${slots.get(roll.provider) ?? OTHER_SLOT}`} />
													{roll.label}
												</span>
											<strong>
												{roll.spendUsd == null
													? roll.provider === "local-laguna" ? "No provider charge" : "Unavailable"
													: usd(roll.spendUsd)}
											</strong>
											</div>
											{/* A provider with no charge has no share to draw; an
											    empty track would read as "spent zero" rather
											    than "not a billed provider". */}
											{roll.spendUsd == null ? null : (
												<div className="usage-provider-track">
													<div
														className={`usage-provider-fill usage-series-${slots.get(roll.provider) ?? OTHER_SLOT}`}
														style={{ width: `${Math.max(roll.share * 100, 1.5)}%` }}
													/>
												</div>
											)}
											<span className="usage-provider-meta">
											{roll.spendUsd == null
												? roll.provider === "local-laguna"
													? "On-device · no provider charge"
													: "Cost unavailable"
												: `${percent(roll.share)} of spend`}
												{" · "}
												{compactTokens(roll.totalTokens)} tokens
											</span>
										</div>
									))
								)}
							</div>
						</section>

						<section className="usage-hero-chart">
							<div className="usage-section-head">
								<h3>Daily {metric === "cost" ? "spend" : "tokens"}</h3>
								<div className="usage-section-controls">
									<Segmented
										label="Chart metric"
										size="sm"
										options={[
											{ id: "cost" as const, label: "Cost" },
											{ id: "tokens" as const, label: "Tokens" }
										]}
										value={metric}
										onChange={setMetric}
										testId="usage-metric"
									/>
								</div>
							</div>
							{rolls.length > 0 ? (
								<div className="usage-legend" data-testid="usage-legend">
									{rolls.map((roll) => (
										<span key={roll.provider}>
											<i className={`usage-swatch usage-series-${slots.get(roll.provider) ?? OTHER_SLOT}`} />
											{roll.label}
										</span>
									))}
								</div>
							) : null}
							<DailyChart
								series={series}
								metric={metric}
								slots={slots}
								labelFor={providerLabel}
							/>
						</section>
					</div>

					<div className="usage-stats" data-testid="usage-stats">
						<Stat
							label="Processed tokens"
							value={compactTokens(totals.totalTokens)}
							note={
								activeDays > 0
									? `${compactTokens(Math.round(totals.totalTokens / activeDays))} per active day`
									: null
							}
							testId="usage-stat-total"
						/>
						<Stat
							label="Cached input"
							value={compactTokens(totals.cachedInputTokens)}
							note={
								totals.cachedInputTokens == null
									? "No provider reported cache traffic"
									: `${percent(totals.cacheHitRate)} of observed input`
							}
							testId="usage-stat-cached"
						/>
						<Stat
							label="Uncached input"
							value={compactTokens(totals.nonCachedInputTokens)}
							note={
								totals.cacheWriteTokens == null
									? null
									: `${compactTokens(totals.cacheWriteTokens)} cache writes`
							}
							testId="usage-stat-uncached"
						/>
						<Stat
							label="Output"
							value={compactTokens(totals.outputTokens)}
							note={
								totals.reasoningTokens == null
									? null
									: `includes ${compactTokens(totals.reasoningTokens)} reasoning`
							}
							testId="usage-stat-output"
						/>
						<Stat
							label="Requests"
							value={compactTokens(totals.requests)}
							note={
								totals.decodeTpsP50 == null
									? "Throughput unavailable"
									: `${totals.decodeTpsP50.toFixed(totals.decodeTpsP50 >= 10 ? 0 : 1)} tok/s median decode`
							}
							testId="usage-stat-requests"
						/>
					</div>

					<div className="usage-lower">
						<section className="usage-breakdown">
							<div className="usage-section-head">
								<h3>Breakdown</h3>
								<Segmented
									label="Breakdown grouping"
									size="sm"
									options={[
										{ id: "model" as const, label: "Model" },
										{ id: "day" as const, label: "Day" }
									]}
									value={grouping}
									onChange={setGrouping}
									testId="usage-grouping"
								/>
							</div>
							{grouping === "model" ? (
								<BreakdownTable
									heading="Model"
									slots={slots}
									rows={models.map((row) => ({
										key: `${row.provider}:${row.modelId}`,
										name: row.modelId,
										provider: row.provider,
										spendUsd: spendUsd(row),
										tokens: row.totalTokens
									}))}
								/>
							) : (
								<BreakdownTable
									heading="Day"
									slots={slots}
									rows={(summary.days ?? [])
										.slice()
										.reverse()
										.map((point) => ({
											key: `${point.day}:${point.totals.provider}`,
											name: longDay(point.day),
											provider: point.totals.provider,
											spendUsd: spendUsd(point.totals),
											tokens: point.totals.totalTokens
										}))}
								/>
							)}
						</section>

						<section className="usage-quality" data-testid="usage-quality">
							<div className="usage-section-head">
								<h3>Cost quality</h3>
							</div>
							<p className="usage-muted">Who vouches for the dollars above.</p>
							{quality.map((row) => (
								<div className="usage-quality-row" key={row.key} data-testid={`usage-quality-${row.key}`}>
									<span>{row.label}</span>
									<strong>{percent(row.share)}</strong>
								</div>
							))}
							<div className="usage-quality-row usage-quality-total">
								<span>Cache hit rate</span>
								<strong>{percent(totals.cacheHitRate)}</strong>
							</div>
						</section>
					</div>
				</>
			) : null}
		</div>
	);
}

type BreakdownRow = {
	key: string;
	name: string;
	provider: string;
	spendUsd: number | null;
	tokens: number;
};

function BreakdownTable({
	heading,
	rows,
	slots
}: {
	heading: string;
	rows: BreakdownRow[];
	slots: Map<string, string>;
}) {
	const total = rows.reduce((sum, row) => sum + (row.spendUsd ?? 0), 0);
	if (rows.length === 0) {
		return (
			<div className="ws-empty">
				<p>No requests in this window yet.</p>
			</div>
		);
	}
	return (
		<div className="usage-table-scroll">
			<table className="usage-table" data-testid="usage-breakdown-table">
				<thead>
					<tr>
						<th scope="col">{heading}</th>
						<th scope="col" className="usage-num">Cost</th>
						<th scope="col" className="usage-num">Share</th>
						<th scope="col" className="usage-num">Tokens</th>
					</tr>
				</thead>
				<tbody>
					{rows.map((row) => (
						<tr key={row.key}>
							<th scope="row">
								<i className={`usage-swatch usage-series-${slots.get(row.provider) ?? OTHER_SLOT}`} />
								<span className="usage-table-name">{row.name}</span>
								<span className="usage-table-provider">{providerLabel(row.provider)}</span>
							</th>
							<td className="usage-num">
								{row.spendUsd == null ? "No charge" : usd(row.spendUsd)}
							</td>
							<td className="usage-num usage-faint">
								{total > 0 && row.spendUsd != null ? percent(row.spendUsd / total) : "—"}
							</td>
							<td className="usage-num">{compactTokens(row.tokens)}</td>
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}
