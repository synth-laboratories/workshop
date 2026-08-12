import type { SynthAccountSummary, SynthBackendSettings } from "../env";
import {
	type AccountViewModel,
	formatDate,
	formatTimestamp,
	formatTokens,
	formatUsd
} from "../runtime/accountView";
import { AccountSignIn, BackendSettings } from "./BackendSettings";
import type { DeviceUsageSummary } from "./UsageSheet";

/**
 * Settings → Account, consolidated.
 *
 * Before this page, "Account" *was* the connection editor: profile, backend URL,
 * env file, key variable. Those are deployment knobs, not an account. The four
 * user-facing sections from `AUTH_BILLING_FLOW.md` come first —
 *
 *   Profile & organization · Plan & allowances · Usage · Devices & security
 *
 * — and the endpoint/key editor is demoted into a collapsed Advanced connection
 * disclosure. Every number states its scope: Synth Cloud figures come from the
 * Account Snapshot, device figures from the local ledger, and neither is ever
 * added to the other.
 */

type Props = {
	view: AccountViewModel;
	summary: SynthAccountSummary | null;
	deviceUsage: DeviceUsageSummary | null;
	connection: SynthBackendSettings | null;
	onBilling: (action: "upgrade" | "manage") => void;
	onRefresh: () => void;
	onOpenDeviceUsage: () => void;
};

function Row({ label, value, testId }: { label: string; value: string; testId?: string }) {
	return (
		<div className="account-page-row">
			<span>{label}</span>
			<strong data-testid={testId}>{value}</strong>
		</div>
	);
}

function Section({
	title,
	description,
	testId,
	children
}: {
	title: string;
	description: string;
	testId: string;
	children: React.ReactNode;
}) {
	return (
		<section className="account-page-section" data-testid={testId}>
			<header>
				<h3>{title}</h3>
				<p>{description}</p>
			</header>
			{children}
		</section>
	);
}

export function AccountPage({
	view,
	summary,
	deviceUsage,
	connection,
	onBilling,
	onRefresh,
	onOpenDeviceUsage
}: Props) {
	const plan = view.plan;
	const cloudUsage = summary?.cloudUsage ?? null;
	const lastUpdated = formatTimestamp(summary?.lastUpdated);
	const environment = summary?.environment ?? null;
	const showDollarFigures = view.planHasDollars;

	return (
		<div className="settings-finetunes account-page" data-testid="settings-account">
			<header className="settings-section-head">
				<div>
					<h2>Account</h2>
				</div>
				<span className="finetune-badge" data-testid="account-page-state">{view.subtitle}</span>
			</header>

			<Section
				title="Profile & organization"
				description="Who this device is signed in as."
				testId="account-page-profile"
			>
				<div className="account-page-identity">
					<span className="account-avatar" aria-hidden>{view.initial}</span>
					<span>
						<strong data-testid="account-page-name">{view.title}</strong>
						<small>{summary?.email ?? (view.signedIn ? "No email reported" : "Not signed in")}</small>
					</span>
				</div>
				{summary?.organization ? (
					<>
						<Row
							label="Organization"
							value={summary.organization.displayName ?? summary.organization.id}
							testId="account-page-org"
						/>
						{summary.organization.role ? <Row label="Role" value={summary.organization.role} /> : null}
					</>
				) : view.signedIn ? (
					<p className="account-page-note">No organization is reported for this account.</p>
				) : (
					<p className="account-page-note">
						Local models work without an account. Sign in to use Synth Cloud and see a plan.
					</p>
				)}
				{environment && environment !== "prod" ? (
					<Row label="Environment" value={environment} testId="account-page-environment" />
				) : null}
			</Section>

			<Section
				title="Plan & allowances"
				description="Synth Cloud dollars for the current period."
				testId="account-page-plan"
			>
				{view.planIsDevSeed ? (
					<p className="account-page-warning" data-testid="account-page-dev-seed">
						Dev stand-in — this allowance is seeded locally and charged from this device's
						ledger. It is not a Synth Cloud plan.
					</p>
				) : null}
				{view.statusNote ? (
					<p className="account-page-warning" data-testid="account-page-status-note">{view.statusNote}</p>
				) : null}
				{view.cloudBlockedReason ? (
					<p className="account-page-warning" data-testid="account-page-blocked">{view.cloudBlockedReason}</p>
				) : null}

				{plan ? (
					<>
						<Row label="Plan" value={plan.name} testId="account-page-plan-name" />
						{view.planHasDollars ? (
							<>
								<Row label="Monthly allowance" value={formatUsd(plan.monthlyAllowanceUsd)} testId="account-page-allowance" />
								<Row label="Used this period" value={formatUsd(plan.usedUsd)} testId="account-page-used" />
								<Row label="Remaining" value={formatUsd(plan.remainingUsd)} testId="account-page-remaining" />
							</>
						) : (
							<p className="account-page-note">This account is not metered in monthly dollars.</p>
						)}
						{formatDate(plan.resetsAt) ? <Row label="Resets" value={formatDate(plan.resetsAt) as string} testId="account-page-resets" /> : null}
						{formatDate(plan.renewsAt) ? <Row label="Renews" value={formatDate(plan.renewsAt) as string} /> : null}
					</>
				) : view.signedIn ? (
					<p className="account-page-note" data-testid="account-page-no-plan">
						No Synth Cloud plan is reported for this account yet.
					</p>
				) : (
					<p className="account-page-note">Sign in to see your plan and allowance.</p>
				)}

				{showDollarFigures && summary?.catalog?.length ? (
					<div className="account-page-catalog" data-testid="account-page-catalog">
						{summary.catalog.map((option) => (
							<div key={option.tier} className={option.tier === plan?.tier ? "is-current" : undefined}>
								<span>{option.displayName}</span>
								<strong>{formatUsd(option.priceUsd)}/mo</strong>
								<small>{formatUsd(option.monthlyAllowanceUsd)} cloud allowance</small>
							</div>
						))}
					</div>
				) : null}

				<div className="account-page-actions">
					{view.primaryAction && view.primaryAction.kind !== "sign_in" ? (
						<button
							type="button"
							className="settings-secondary-btn"
							data-testid="account-page-primary-action"
							onClick={() => {
								if (view.primaryAction?.kind === "retry") onRefresh();
								else onBilling(view.primaryAction?.kind === "upgrade" ? "upgrade" : "manage");
							}}
						>
							{view.primaryAction.label}
						</button>
					) : null}
					<button type="button" className="settings-secondary-btn" data-testid="account-page-refresh" onClick={onRefresh}>
						Refresh
					</button>
					{lastUpdated ? (
						<span className="finetune-meta" data-testid="account-page-last-updated">Last updated {lastUpdated}</span>
					) : null}
				</div>
			</Section>

			<Section
				title="Usage"
				description="Synth Cloud spend and this device's local runs, kept separate."
				testId="account-page-usage"
			>
				<h4 className="account-page-subhead">Synth Cloud</h4>
				{cloudUsage ? (
					<div className="account-page-windows">
						<div><span>Today</span>{showDollarFigures ? <strong data-testid="account-page-today">{formatUsd(cloudUsage.today.costUsd)}</strong> : null}<small>{cloudUsage.today.events} events</small></div>
						<div><span>7 days</span>{showDollarFigures ? <strong data-testid="account-page-7d">{formatUsd(cloudUsage.sevenDays.costUsd)}</strong> : null}<small>{cloudUsage.sevenDays.events} events</small></div>
						<div><span>30 days</span>{showDollarFigures ? <strong data-testid="account-page-30d">{formatUsd(cloudUsage.thirtyDays.costUsd)}</strong> : null}<small>{cloudUsage.thirtyDays.events} events</small></div>
					</div>
				) : (
					<p className="account-page-note" data-testid="account-page-no-cloud-usage">
						{view.signedIn ? "Synth Cloud has not reported usage for this account." : "Sign in to see Synth Cloud usage."}
					</p>
				)}

				<h4 className="account-page-subhead">This device</h4>
				<Row label="Tokens this week" value={formatTokens(deviceUsage?.weeklyTokens)} testId="account-page-device-weekly" />
				{showDollarFigures ? <Row label="Estimated cost this week" value={formatUsd(deviceUsage?.weeklyCostUsd)} /> : null}
				<Row label="All tracked tokens" value={formatTokens(deviceUsage?.totalTokens)} />
				<Row label="Tracked runs" value={formatTokens(deviceUsage?.entries)} />
				<p className="account-page-note">Device totals are local runs on this Mac — not your Synth Cloud allowance.</p>
				<div className="account-page-actions">
					<button type="button" className="settings-secondary-btn" data-testid="account-page-open-inventory" onClick={onOpenDeviceUsage}>
						Open Data → Usage
					</button>
				</div>
			</Section>

			<Section
				title="Devices & security"
				description="This device's Synth Cloud session."
				testId="account-page-devices"
			>
				<AccountSignIn />
				<Row
					label="Credential"
					value={connection?.apiKeyConfigured
						? `${connection.apiKeyFingerprint ?? "configured"} · ${connection.apiKeySource ?? "env file"}`
						: "Not configured"}
					testId="account-page-credential"
				/>
				<Row label="Backend" value={connection?.backendUrl ?? "Loading…"} testId="account-page-backend" />
				<p className="account-page-note">
					The key is held by the native host and never reaches this window. Signing out clears
					it; local history and the device ledger stay.
				</p>
			</Section>

			<details className="account-page-advanced" data-testid="account-page-advanced">
				<summary>Advanced connection</summary>
				<p className="account-page-note">
					Endpoint and native-host credential references for development profiles. Changing
					these reconnects the runtime.
				</p>
				<BackendSettings />
			</details>
		</div>
	);
}
