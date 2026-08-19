/**
 * Account view model.
 *
 * The Rust host owns the facts; this module owns only their presentation. Two
 * invariants come from `synth_cloud_api_usage.md` and are enforced here rather
 * than in JSX, so every surface obeys them:
 *
 *  1. Cloud dollars and device usage are never blended. Cloud figures come from
 *     the Account Snapshot; device figures come from the local usage ledger and
 *     are always labelled `This device`.
 *  2. Production never shows an invented plan. When the snapshot is missing, or
 *     the account is not metered in dollars, there is no dollar figure at all —
 *     a local/dev stand-in is shown only with its own label.
 */

import type {
	SynthAccountPlan,
	SynthAccountState,
	SynthAccountSummary
} from "../bridge";

export type AccountViewModel = {
	state: SynthAccountState;
	signedIn: boolean;
	/** Identity line: display name, or the honest fallback for this state. */
	title: string;
	subtitle: string;
	initial: string;
	organizationLabel: string | null;
	plan: SynthAccountPlan | null;
	/** True when `plan` is the local/dev stand-in and must be labelled as such. */
	planIsDevSeed: boolean;
	/** True when the plan carries real dollar figures worth rendering. */
	planHasDollars: boolean;
	/** One backend-authored recovery action, or none. */
	primaryAction: { kind: "sign_in" | "upgrade" | "manage" | "retry"; label: string } | null;
	/** Non-null when billable Synth Cloud actions are blocked for this account. */
	cloudBlockedReason: string | null;
	statusNote: string | null;
	/** The expandable `Usage remaining` summary in the account menu. */
	allowance: AllowanceSummary;
};

/**
 * The cloud allowance, resolved to exactly what the menu should show.
 *
 * Cloud only, by construction: device totals live in Data → Usage and are
 * never summed into these rows (invariant 1). `rows` is empty whenever there is
 * no metered dollar figure to show, so no surface can render an invented
 * `$0.00` (invariant 2) — `note` says why instead.
 */
export type AllowanceSummary = {
	/** Collapsed-row trailing value, or null when there is no honest figure. */
	headline: string | null;
	rows: { label: string; value: string }[];
	/** Shown in place of rows when there is nothing metered to show. */
	note: string | null;
	/** True when `rows` came from the labelled local/dev stand-in. */
	isDevSeed: boolean;
};

const CURRENCY = new Intl.NumberFormat("en-US", {
	style: "currency",
	currency: "USD",
	minimumFractionDigits: 2,
	maximumFractionDigits: 2
});

export function formatUsd(value: number | null | undefined): string {
	return typeof value === "number" && Number.isFinite(value)
		? CURRENCY.format(value)
		: "UNKNOWN";
}

export function formatTokens(value: number | null | undefined): string {
	return (typeof value === "number" && Number.isFinite(value) ? value : 0).toLocaleString();
}

export function formatDate(value: string | null | undefined): string | null {
	if (!value) return null;
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? null : parsed.toLocaleDateString();
}

export function formatTimestamp(value: string | null | undefined): string | null {
	if (!value) return null;
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? null : parsed.toLocaleString();
}

/**
 * Resolve the shell state. `pairing` wins because only the renderer knows a
 * browser sign-in is in flight; otherwise the host's state is authoritative and
 * we fall back to key presence only for hosts that predate `state`.
 */
export function resolveAccountState(
	summary: SynthAccountSummary | null,
	apiKeyConfigured: boolean,
	pairing: boolean
): SynthAccountState {
	if (pairing) return "pairing";
	if (summary?.state) return summary.state;
	if (summary?.signedIn ?? apiKeyConfigured) return "active";
	return "local_only";
}

/** Dollar figures are only real when the backend metered them. */
function planHasDollars(plan: SynthAccountPlan | null | undefined): boolean {
	if (!plan) return false;
	if (plan.metered === false) return false;
	return typeof plan.monthlyAllowanceUsd === "number";
}

function blockedReason(state: SynthAccountState, plan: SynthAccountPlan | null): string | null {
	switch (state) {
		case "limited":
			return plan
				? `${plan.name} monthly allowance is used up. Local models keep working.`
				: "Your Synth Cloud allowance is used up. Local models keep working.";
		case "past_due":
			return "Synth Cloud billing needs attention. Local models keep working.";
		case "canceled":
			return "This Synth Cloud plan is no longer active. Local models keep working.";
		default:
			return null;
	}
}

export function buildAccountView(
	summary: SynthAccountSummary | null,
	apiKeyConfigured: boolean,
	pairing = false
): AccountViewModel {
	const state = resolveAccountState(summary, apiKeyConfigured, pairing);
	const plan = summary?.plan ?? null;
	const isDevSeed = (plan?.source ?? summary?.source) === "dev_seed";
	const signedIn = state !== "local_only" && state !== "signed_out" && state !== "pairing";
	const displayName = summary?.displayName?.trim() || null;

	// Signed-out chrome is a call to action, not an empty account row: the
	// title invites sign-in and the subtitle says what mode you are in today.
	const title = displayName
		?? (state === "local_only" || state === "signed_out"
			? "Sign in to Synth"
			: state === "pairing"
				? "Finishing sign-in…"
				: "Synth account");

	const subtitle = (() => {
		switch (state) {
			case "local_only":
				return "Local mode";
			case "signed_out":
				return summary?.sessionHealth === "revoked" ? "Sign in again" : "Signed out";
			case "pairing":
				return "Approve this device in your browser";
			case "limited":
				return "Allowance used";
			case "past_due":
				return "Billing needs attention";
			case "canceled":
				return "Plan inactive";
			case "error":
				return "Synth Cloud unavailable";
			case "unknown":
				return "Plan unavailable";
			default:
				return summary?.organization?.displayName ?? summary?.email ?? "Signed in";
		}
	})();

	const primaryAction = (() => {
		switch (state) {
			case "local_only":
			case "signed_out":
				return { kind: "sign_in" as const, label: "Sign in to Synth" };
			case "limited":
				return summary?.billing?.checkoutUrl || summary?.billing?.upgradeTier
					? { kind: "upgrade" as const, label: "Upgrade" }
					: { kind: "manage" as const, label: "Manage billing" };
			case "past_due":
			case "canceled":
				return { kind: "manage" as const, label: "Manage billing" };
			case "error":
			case "unknown":
				return { kind: "retry" as const, label: "Retry" };
			default: {
				const tierCanUpgrade = summary?.plan?.tier === "free" || summary?.plan?.tier === "starter";
				if (tierCanUpgrade && (summary?.billing?.checkoutUrl || summary?.billing?.upgradeTier)) {
					return { kind: "upgrade" as const, label: "Upgrade" };
				}
				return summary?.billing?.portalUrl
					? { kind: "manage" as const, label: "Manage billing" }
					: null;
			}
		}
	})();

	const allowance = buildAllowance(state, plan, isDevSeed);

	const statusNote = summary?.stale
		? `Showing the last known plan${summary.error ? ` — ${summary.error}` : ""}`
		: summary?.error ?? null;

	return {
		state,
		signedIn,
		title,
		subtitle,
		initial: (displayName ?? "S").slice(0, 1).toUpperCase(),
		organizationLabel: summary?.organization?.displayName ?? null,
		plan,
		planIsDevSeed: isDevSeed,
		planHasDollars: planHasDollars(plan),
		primaryAction,
		cloudBlockedReason: blockedReason(state, plan),
		statusNote,
		allowance
	};
}

/** Signed-out copy is fixed by contract: an invitation, never a zero dollar figure. */
const SIGNED_OUT_ALLOWANCE_NOTE = "Sign in to Synth to see a cloud allowance";

function buildAllowance(
	state: SynthAccountState,
	plan: SynthAccountPlan | null,
	isDevSeed: boolean
): AllowanceSummary {
	if (state === "local_only" || state === "signed_out" || state === "pairing") {
		return { headline: null, rows: [], note: SIGNED_OUT_ALLOWANCE_NOTE, isDevSeed: false };
	}
	if (!plan) {
		return {
			headline: null,
			rows: [],
			note: "Synth Cloud has not reported a plan for this account yet",
			isDevSeed: false
		};
	}
	if (!planHasDollars(plan)) {
		// An unmetered account has a real plan but no dollar allowance. Naming
		// the plan is honest; inventing an allowance for it is not.
		return {
			headline: null,
			rows: [{ label: "Plan", value: plan.name }],
			note: "This account is not metered in dollars",
			isDevSeed
		};
	}
	const resets = formatDate(plan.resetsAt);
	return {
		headline: typeof plan.remainingUsd === "number" ? formatUsd(plan.remainingUsd) : null,
		rows: [
			{ label: "Plan", value: plan.name },
			{ label: "Monthly allowance", value: formatUsd(plan.monthlyAllowanceUsd) },
			{ label: "Used this period", value: formatUsd(plan.usedUsd) },
			...(typeof plan.remainingUsd === "number"
				? [{ label: "Remaining", value: formatUsd(plan.remainingUsd) }]
				: []),
			...(resets ? [{ label: "Resets", value: resets }] : [])
		],
		note: null,
		isDevSeed
	};
}
