import { useEffect, useRef } from "react";
import type { SynthAccountSummary } from "../env";
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
 *   SYNTH CLOUD  — the Account Snapshot. Authoritative for plan dollars.
 *   THIS DEVICE  — the local usage ledger. Device facts, not an allowance.
 *
 * Signed-out and local-only users see the sign-in invitation instead of empty
 * plan chrome, and production never renders a dollar figure the backend did
 * not report.
 */

export type DeviceUsageSummary = {
	weeklyTokens: number;
	weeklyCostUsd: number;
	totalTokens: number;
	totalCostUsd: number;
	entries: number;
};

type Props = {
	open: boolean;
	view: AccountViewModel;
	summary: SynthAccountSummary | null;
	deviceUsage: DeviceUsageSummary | null;
	onClose: () => void;
	onSignIn: () => void;
	onBilling: (action: "upgrade" | "manage") => void;
	onRetry: () => void;
	onOpenDeviceUsage: () => void;
};

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

export function UsageSheet({
	open,
	view,
	summary,
	deviceUsage,
	onClose,
	onSignIn,
	onBilling,
	onRetry,
	onOpenDeviceUsage
}: Props) {
	const closeRef = useRef<HTMLButtonElement>(null);

	useEffect(() => {
		if (!open) return;
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				onClose();
			}
		};
		document.addEventListener("keydown", onKey);
		requestAnimationFrame(() => closeRef.current?.focus());
		return () => document.removeEventListener("keydown", onKey);
	}, [onClose, open]);

	if (!open) return null;

	const plan = view.plan;
	const cloudUsage = summary?.cloudUsage ?? null;
	const lastUpdated = formatTimestamp(summary?.lastUpdated);

	return (
		<div
			className="usage-sheet"
			role="dialog"
			aria-modal="true"
			aria-labelledby="usage-sheet-title"
			data-testid="usage-sheet"
			onMouseDown={(event) => {
				if (event.target === event.currentTarget) onClose();
			}}
		>
			<div className="usage-sheet-card">
				<div className="usage-sheet-head">
					<h2 id="usage-sheet-title">Usage</h2>
					<button
						ref={closeRef}
						type="button"
						className="ghost-button"
						onClick={onClose}
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
									Dev stand-in — this allowance is seeded locally and charged from this
									device, not from Synth Cloud.
								</p>
							) : null}
							{view.statusNote ? (
								<p className="usage-sheet-warning" data-testid="usage-sheet-status-note">
									{view.statusNote}
								</p>
							) : null}

							{plan ? (
								<>
									<UsageRow
										label="Plan"
										value={plan.name}
										testId="usage-sheet-plan-name"
									/>
									{view.planHasDollars ? (
										<>
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
										</>
									) : (
										<p className="usage-sheet-note">
											This account is not metered in monthly dollars.
										</p>
									)}
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
							) : (
								<p className="usage-sheet-note" data-testid="usage-sheet-no-plan">
									No Synth Cloud plan is reported for this account yet.
								</p>
							)}

							{cloudUsage ? (
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
						<p>Local runs on this Mac — not your Synth Cloud allowance</p>
					</header>
					<UsageRow
						label="Tokens this week"
						value={formatTokens(deviceUsage?.weeklyTokens)}
						testId="usage-sheet-device-weekly-tokens"
					/>
					<UsageRow
						label="Estimated cost this week"
						value={formatUsd(deviceUsage?.weeklyCostUsd)}
					/>
					<UsageRow
						label="All tracked tokens"
						value={formatTokens(deviceUsage?.totalTokens)}
						testId="usage-sheet-device-total-tokens"
					/>
					<UsageRow label="Tracked runs" value={formatTokens(deviceUsage?.entries)} />
					<div className="usage-sheet-actions">
						<button
							type="button"
							className="ghost-button"
							onClick={onOpenDeviceUsage}
							data-testid="usage-sheet-open-inventory"
						>
							Open Inventory → Usage
						</button>
					</div>
				</section>
			</div>
		</div>
	);
}
