import { useCallback, useEffect, useRef, useState } from "react";
import type { UsageBreakdown, UsageSummary, UsageWindow } from "@synth/runtime-protocol";
import type { SynthAccountSummary } from "../bridge";
import {
	type AccountViewModel,
	formatDate,
	formatTimestamp,
	formatTokens,
	formatUsd
} from "../runtime/accountView";

/**
 * Usage sheet: two sections that are never blended.
 *
 *   SYNTH CLOUD  — the Account Snapshot. Authoritative for plan dollars, and
 *                  rendered only when the backend actually reported a plan;
 *                  the local dev stand-in never wears allowance chrome here.
 *   THIS DEVICE  — the per-request usage ledger, aggregated natively. Device
 *                  facts (tokens, cache traffic, throughput, provider bills),
 *                  not an allowance.
 *
 * Dollar rules: a settled provider charge renders as billed; a tariff figure
 * always says estimated; local runs say "On-device · no provider charge";
 * missing telemetry renders "Unavailable", never zero.
 */

/** Compact device rollup consumed by the Settings/Account pages. */
export type DeviceUsageSummary = {
	weeklyTokens: number;
	weeklyCostUsd: number;
	totalTokens: number;
	totalCostUsd: number;
	entries: number;
};

const WINDOWS: Array<{ id: UsageWindow; label: string }> = [
	{ id: "today", label: "Today" },
	{ id: "7d", label: "7 days" },
	{ id: "30d", label: "30 days" },
	{ id: "all", label: "All time" }
];

const UNAVAILABLE = "Unavailable";

function maybeTokens(value: number | null | undefined): string {
	return typeof value === "number" && Number.isFinite(value) ? formatTokens(value) : UNAVAILABLE;
}

function percent(rate: number | null): string {
	return typeof rate === "number" && Number.isFinite(rate)
		? `${(rate * 100).toFixed(rate >= 0.1 ? 0 : 1)}%`
		: UNAVAILABLE;
}

function tps(value: number | null): string | null {
	if (typeof value !== "number" || !Number.isFinite(value)) return null;
	return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} tok/s`;
}

function ttft(value: number | null): string | null {
	if (typeof value !== "number" || !Number.isFinite(value)) return null;
	return value >= 1000 ? `${(value / 1000).toFixed(1)} s` : `${Math.round(value)} ms`;
}

function providerLabel(provider: string): string {
	if (provider === "local-laguna") return "On-device";
	if (provider === "openrouter") return "OpenRouter";
	if (provider === "synth-cloud") return "Synth Cloud";
	return provider;
}

function rowTestId(row: UsageBreakdown): string {
	return `usage-model-${row.provider}-${row.modelId}`.replace(/[^a-zA-Z0-9_-]+/g, "-");
}

/**
 * The one place a dollar figure gets its authority label. Billed money and
 * unbilled estimates are shown side by side and never summed into one number.
 */
function costLine(row: UsageBreakdown): { text: string; kind: "local" | "billed" | "estimate" | "none" } {
	if (row.provider === "local-laguna") {
		return { text: "On-device · no provider charge", kind: "local" };
	}
	const parts: string[] = [];
	if (row.billedCostUsd != null) parts.push(`${formatUsd(row.billedCostUsd)} billed`);
	if (row.estimatedCostUsd != null) parts.push(`${formatUsd(row.estimatedCostUsd)} estimated`);
	if (parts.length === 0) {
		return { text: row.provider === "synth-cloud" ? "Billed by Synth Cloud" : "Cost unavailable", kind: "none" };
	}
	return { text: parts.join(" + "), kind: row.billedCostUsd != null ? "billed" : "estimate" };
}

function perfLine(row: UsageBreakdown): string {
	const parts: string[] = [];
	const decode = tps(row.decodeTpsP50);
	const decodeP95 = tps(row.decodeTpsP95);
	const endToEnd = tps(row.endToEndTpsP50);
	const endToEndP95 = tps(row.endToEndTpsP95);
	const firstToken = ttft(row.ttftMsP50);
	const firstTokenP95 = ttft(row.ttftMsP95);
	if (decode) parts.push(`decode ${decode}${decodeP95 ? ` (p95 ${decodeP95})` : ""}`);
	if (endToEnd) parts.push(`end-to-end ${endToEnd}${endToEndP95 ? ` (p95 ${endToEndP95})` : ""}`);
	if (firstToken) parts.push(`TTFT ${firstToken}${firstTokenP95 ? ` (p95 ${firstTokenP95})` : ""}`);
	if (parts.length === 0) return `Throughput ${UNAVAILABLE.toLowerCase()}`;
	return `${parts.join(" · ")} · ${row.perfSampleCount} sample${row.perfSampleCount === 1 ? "" : "s"}`;
}

function UsageRow({
	label,
	value,
	testId
}: {
	label: string;
	value: string;
	testId?: string;
}) {
	return (
		<div className="usage-sheet-row">
			<span>{label}</span>
			<strong data-testid={testId}>{value}</strong>
		</div>
	);
}

type Props = {
	open: boolean;
	view: AccountViewModel;
	summary: SynthAccountSummary | null;
	onClose: () => void;
	onSignIn: () => void;
	onBilling: (action: "upgrade" | "manage") => void;
	onRetry: () => void;
	onOpenDeviceUsage: () => void;
};

export function UsageSheet({
	open,
	view,
	summary,
	onClose,
	onSignIn,
	onBilling,
	onRetry,
	onOpenDeviceUsage
}: Props) {
	const closeRef = useRef<HTMLButtonElement>(null);
	const returnFocusRef = useRef<HTMLElement | null>(null);
	const [usageWindow, setUsageWindow] = useState<UsageWindow>("7d");
	const [usage, setUsage] = useState<UsageSummary | null>(null);
	const [usageFailed, setUsageFailed] = useState(false);
	const closeSheet = useCallback(() => {
		onClose();
		requestAnimationFrame(() => {
			const previous = returnFocusRef.current;
			if (previous?.isConnected && previous !== document.body) previous.focus();
			else document.querySelector<HTMLElement>('[data-testid="account-menu-trigger"]')?.focus();
		});
	}, [onClose]);

	useEffect(() => {
		if (!open) return;
		returnFocusRef.current = document.activeElement as HTMLElement | null;
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				closeSheet();
			}
		};
		document.addEventListener("keydown", onKey);
		requestAnimationFrame(() => closeRef.current?.focus());
		return () => document.removeEventListener("keydown", onKey);
	}, [closeSheet, open]);

	useEffect(() => {
		if (!open) return;
		const bridge = window.synthUsage;
		if (!bridge) {
			setUsage(null);
			setUsageFailed(true);
			return;
		}
		let disposed = false;
		setUsageFailed(false);
		bridge
			.summary(usageWindow)
			.then((next) => {
				if (!disposed) setUsage(next);
			})
			.catch(() => {
				if (!disposed) {
					setUsage(null);
					setUsageFailed(true);
				}
			});
		return () => {
			disposed = true;
		};
	}, [open, usageWindow]);

	if (!open) return null;

	const plan = view.plan;
	const cloudUsage = summary?.cloudUsage ?? null;
	const lastUpdated = formatTimestamp(summary?.lastUpdated);
	const showDollarFigures = view.planHasDollars && !view.planIsDevSeed;
	const totals = usage?.totals ?? null;

	return (
		<div
			className="usage-sheet"
			role="dialog"
			aria-modal="true"
			aria-labelledby="usage-sheet-title"
			data-testid="usage-sheet"
			onMouseDown={(event) => {
				if (event.target === event.currentTarget) closeSheet();
			}}
		>
			<div className="usage-sheet-card">
				<div className="usage-sheet-head">
					<h2 id="usage-sheet-title">Usage</h2>
					<button
						ref={closeRef}
						type="button"
						className="ghost-button"
						onClick={closeSheet}
						aria-label="Close usage"
						data-testid="usage-sheet-close"
					>
						×
					</button>
				</div>

				<section className="usage-sheet-section" data-testid="usage-sheet-cloud">
					<header>
						<h3>Synth Cloud</h3>
						<p>Billed to your Synth account</p>
					</header>

					{!view.signedIn ? (
						<div className="usage-sheet-empty" data-testid="usage-sheet-signed-out">
							<p>Sign in to use Synth Cloud models and see your plan allowance.</p>
							<button type="button" className="primary-button" onClick={onSignIn} data-testid="usage-sheet-sign-in">
								Sign in to Synth
							</button>
						</div>
					) : (
						<>
							{view.planIsDevSeed ? (
								<p className="usage-sheet-note" data-testid="usage-sheet-dev-seed">
									Dev stand-in account — no authoritative Synth Cloud plan exists,
									so no allowance is shown. Device usage below is real.
								</p>
							) : null}
							{view.statusNote ? (
								<p className="usage-sheet-warning" data-testid="usage-sheet-status-note">
									{view.statusNote}
								</p>
							) : null}

							{plan && showDollarFigures ? (
								<>
									<UsageRow
										label="Plan"
										value={plan.name}
										testId="usage-sheet-plan-name"
									/>
									<UsageRow
										label="Monthly allowance"
										value={formatUsd(plan.monthlyAllowanceUsd)}
										testId="usage-sheet-allowance"
									/>
									<UsageRow
										label="Used this period"
										value={formatUsd(plan.usedUsd)}
										testId="usage-sheet-used"
									/>
									<UsageRow
										label="Remaining"
										value={formatUsd(plan.remainingUsd)}
										testId="usage-sheet-remaining"
									/>
									{formatDate(plan.resetsAt) ? (
										<UsageRow
											label="Resets"
											value={formatDate(plan.resetsAt) as string}
											testId="usage-sheet-resets"
										/>
									) : null}
									{formatDate(plan.renewsAt) ? (
										<UsageRow label="Renews" value={formatDate(plan.renewsAt) as string} />
									) : null}
								</>
							) : plan && !view.planIsDevSeed ? (
								<p className="usage-sheet-note">
									This account is not metered in monthly dollars.
								</p>
							) : !view.planIsDevSeed ? (
								<p className="usage-sheet-note" data-testid="usage-sheet-no-plan">
									No Synth Cloud plan is reported for this account yet.
								</p>
							) : null}

							{cloudUsage && showDollarFigures ? (
								<div className="usage-sheet-windows" data-testid="usage-sheet-windows">
									<div>
										<span>Today</span>
										<strong data-testid="usage-sheet-today">{formatUsd(cloudUsage.today.costUsd)}</strong>
										<small>{cloudUsage.today.events} events</small>
									</div>
									<div>
										<span>7 days</span>
										<strong data-testid="usage-sheet-7d">{formatUsd(cloudUsage.sevenDays.costUsd)}</strong>
										<small>{cloudUsage.sevenDays.events} events</small>
									</div>
									<div>
										<span>30 days</span>
										<strong data-testid="usage-sheet-30d">{formatUsd(cloudUsage.thirtyDays.costUsd)}</strong>
										<small>{cloudUsage.thirtyDays.events} events</small>
									</div>
								</div>
							) : null}

							{view.cloudBlockedReason ? (
								<p className="usage-sheet-warning" data-testid="usage-sheet-blocked">
									{view.cloudBlockedReason}
								</p>
							) : null}

							<div className="usage-sheet-actions">
								{view.primaryAction && view.primaryAction.kind !== "sign_in" ? (
									<button
										type="button"
										className="primary-button"
										data-testid="usage-sheet-primary-action"
										onClick={() => {
											if (view.primaryAction?.kind === "retry") onRetry();
											else onBilling(view.primaryAction?.kind === "upgrade" ? "upgrade" : "manage");
										}}
									>
										{view.primaryAction.label}
									</button>
								) : null}
								{summary?.billing?.portalUrl && view.primaryAction?.kind !== "manage" ? (
									<button
										type="button"
										className="ghost-button"
										onClick={() => onBilling("manage")}
										data-testid="usage-sheet-manage-billing"
									>
										Manage billing
									</button>
								) : null}
							</div>
							{lastUpdated ? (
								<p className="usage-sheet-footnote" data-testid="usage-sheet-last-updated">
									Last updated {lastUpdated}
								</p>
							) : null}
						</>
					)}
				</section>

				<section className="usage-sheet-section" data-testid="usage-sheet-device">
					<header>
						<h3>This device</h3>
						<p>Every model request from this Mac — not your Synth Cloud allowance</p>
					</header>

					<div className="usage-window-control" role="group" aria-label="Usage window" data-testid="usage-window-control">
						{WINDOWS.map((candidate) => (
							<button
								key={candidate.id}
								type="button"
								className={candidate.id === usageWindow ? "usage-window-button is-active" : "usage-window-button"}
								aria-pressed={candidate.id === usageWindow}
								onClick={() => setUsageWindow(candidate.id)}
								data-testid={`usage-window-${candidate.id}`}
							>
								{candidate.label}
							</button>
						))}
					</div>

					{totals ? (
						<>
							<div className="usage-totals" data-testid="usage-totals">
								<UsageRow label="Total tokens" value={maybeTokens(totals.totalTokens)} testId="usage-total-tokens" />
								<UsageRow label="Input" value={maybeTokens(totals.inputTokens)} testId="usage-total-input" />
								<UsageRow
									label="Cached input"
									value={
										totals.cachedInputTokens == null
											? UNAVAILABLE
											: `${maybeTokens(totals.cachedInputTokens)} (${percent(totals.cacheHitRate)} hit)`
									}
									testId="usage-total-cached"
								/>
								<UsageRow label="Output" value={maybeTokens(totals.outputTokens)} testId="usage-total-output" />
								<UsageRow
									label="Billed"
									value={totals.billedCostUsd == null ? UNAVAILABLE : formatUsd(totals.billedCostUsd)}
									testId="usage-total-billed"
								/>
								{totals.estimatedCostUsd != null ? (
									<UsageRow
										label="Estimated (unbilled)"
										value={formatUsd(totals.estimatedCostUsd)}
										testId="usage-total-estimated"
									/>
								) : null}
								<UsageRow label="Requests" value={maybeTokens(totals.requests)} testId="usage-total-requests" />
							</div>

							{usage && usage.models.length > 0 ? (
								<div className="usage-model-rows" data-testid="usage-model-rows">
									{usage.models.map((row) => {
										const cost = costLine(row);
										return (
											<div className="usage-model-row" key={`${row.provider}:${row.modelId}`} data-testid={rowTestId(row)}>
												<div className="usage-model-title">
													<strong>{row.modelId}</strong>
													<span>{providerLabel(row.provider)} · {formatTokens(row.requests)} request{row.requests === 1 ? "" : "s"}</span>
												</div>
												<div className="usage-model-tokens">
													{`in ${maybeTokens(row.inputTokens)} · cached ${
														row.cachedInputTokens == null
															? UNAVAILABLE.toLowerCase()
															: `${formatTokens(row.cachedInputTokens)} (${percent(row.cacheHitRate)})`
													} · out ${maybeTokens(row.outputTokens)}`}
												</div>
												<div className={`usage-model-cost usage-model-cost-${cost.kind}`} data-testid={`${rowTestId(row)}-cost`}>
													{cost.text}
												</div>
												<div className="usage-model-perf" data-testid={`${rowTestId(row)}-perf`}>
													{perfLine(row)}
												</div>
											</div>
										);
									})}
								</div>
							) : (
								<p className="usage-sheet-note" data-testid="usage-empty">
									No model requests in this window yet.
								</p>
							)}
						</>
					) : (
						<p className="usage-sheet-note" data-testid="usage-loading">
							{usageFailed ? "Device usage is unavailable right now." : "Loading device usage…"}
						</p>
					)}

					<div className="usage-sheet-actions">
						<button
							type="button"
							className="ghost-button"
							onClick={onOpenDeviceUsage}
							data-testid="usage-sheet-open-inventory"
						>
							Open Data → Usage
						</button>
					</div>
				</section>
			</div>
		</div>
	);
}
