import type { SynthAccountSummary } from "../bridge";

export const BILLING_RETURN_KEY = "synth.billing-return.v1";
export const BILLING_RETURN_TTL_MS = 30 * 60 * 1000;
export type BillingReturn = { identity: string; tier: string | null; startedAt: number };

export function billingIdentity(summary: SynthAccountSummary | null): string | null {
	if (!summary?.signedIn || !summary.accountId || !summary.organization?.id) return null;
	return JSON.stringify([summary.environment, summary.accountId, summary.organization.id]);
}

export function parseBillingReturn(raw: string | null, now: number): BillingReturn | null {
	try {
		const value = raw ? JSON.parse(raw) : null;
		if (!value || typeof value.identity !== "string" ||
			(value.tier !== null && !["starter", "pro"].includes(value.tier)) ||
			!Number.isFinite(value.startedAt) || value.startedAt > now ||
			now - value.startedAt >= BILLING_RETURN_TTL_MS) return null;
		return value;
	} catch { return null; }
}

export function billingReturnState(pending: BillingReturn, summary: SynthAccountSummary): "pending" | "changed_identity" | "confirmed" {
	if (summary.stale || summary.error) return "pending";
	if (billingIdentity(summary) !== pending.identity) return "changed_identity";
	return pending.tier && summary.plan?.tier === pending.tier ? "confirmed" : "pending";
}
