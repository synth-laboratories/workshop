/**
 * The renderer half of the build-maturity envelope
 * (contracts/release-tiers-v1.toml).
 *
 * Vite injects `__WORKSHOP_TIER__` and the `__TIER_HAS_*__` booleans as
 * compile-time literals (vite.config.ts `define`). To structurally exclude
 * code from narrower bundles, gate it on the raw `__TIER_HAS_BETA__` /
 * `__TIER_HAS_ALPHA__` / `__TIER_HAS_DEV__` globals directly — the constants
 * are replaced before tree-shaking, so a statically-false branch and the
 * modules only it imports are eliminated from the production bundle. The
 * exports here are for display logic and tests, where elimination is not the
 * point.
 *
 * The host binary carries its own tier (release_tier_get); a bundle/host
 * mismatch is a packaging defect, reported by verifyHostTier below.
 */

import type { ReleaseTier, ReleaseTierReport } from "../bridge/types";

export const TIER_ORDER: readonly ReleaseTier[] = ["core", "stable", "beta", "alpha", "dev"];

/** The tier this renderer bundle was built at. */
export const BUILD_TIER: ReleaseTier = __WORKSHOP_TIER__;

/** True when the bundle envelope is at least `min`. */
export function tierIncludes(min: ReleaseTier): boolean {
	return TIER_ORDER.indexOf(BUILD_TIER) >= TIER_ORDER.indexOf(min);
}

/**
 * Cross-check the host's compiled tier against this bundle's. Returns the
 * mismatch message (and logs it) so callers can surface it; null when aligned.
 */
export function verifyHostTier(report: ReleaseTierReport): string | null {
	if (report.tier === BUILD_TIER) return null;
	const message =
		`packaging defect: host binary is tier "${report.tier}" but the renderer bundle is tier "${BUILD_TIER}" — ` +
		"rebuild with matching tier-* cargo features and WORKSHOP_TIER";
	console.error(message);
	return message;
}
