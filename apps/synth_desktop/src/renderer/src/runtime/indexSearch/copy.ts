/**
 * Every price, limit, retention and privacy sentence Workshop shows for Index
 * search is derived from `GET /api/v1/index/capabilities`. Nothing here knows
 * what the Index costs; a missing capability yields `null`, never a default.
 */
import type { IndexSearchCapabilities, IndexSearchMode } from "./types.ts";

export type IndexSearchCopy = {
	modeLabel(mode: IndexSearchMode): string;
	priceLabel(mode: IndexSearchMode): string | null;
	limitsLabel(mode: IndexSearchMode): string | null;
	retentionLabel(): string | null;
	privacyCopy(): string | null;
};

export function formatCents(cents: number): string {
	if (cents === 0) return `${cents}¢`;
	if (cents % 100 === 0) return `$${(cents / 100).toFixed(0)}`;
	return cents < 100 ? `${cents}¢` : `$${(cents / 100).toFixed(2)}`;
}

export function indexSearchCopy(capabilities: IndexSearchCapabilities | null): IndexSearchCopy {
	const search = capabilities?.publicSearch ?? null;
	return {
		modeLabel(mode) {
			return mode === "fast" ? "Fast" : "Deep";
		},
		priceLabel(mode) {
			const cents = search?.priceCents[mode];
			if (cents === undefined) return null;
			return `${formatCents(cents)} per search`;
		},
		limitsLabel(mode) {
			const limits = search?.limits[mode];
			if (!limits) return null;
			return `${limits.peerMinute}/min · ${limits.peerDay}/day per device`;
		},
		retentionLabel() {
			const retention = search?.retention;
			if (!retention) return null;
			const parts: string[] = [];
			if (retention.publicQueryDays !== null) parts.push(`public queries kept ${retention.publicQueryDays} days`);
			if (retention.privateProcessingMinutes !== null) parts.push(`private processing ${retention.privateProcessingMinutes} minutes`);
			return parts.length ? parts.join(" · ") : null;
		},
		privacyCopy() {
			return search?.privacyCopy ?? null;
		}
	};
}
