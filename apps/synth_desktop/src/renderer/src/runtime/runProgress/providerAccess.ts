import type { RunProgressProjection } from "./types";

/**
 * Secrets are advisory display data, never a substitute for the durable run
 * record. In particular, a lease normally disappears when a terminal run is
 * cleaned up; that must not turn a completed card into a fabricated missing
 * credential warning on its next polling interval.
 */
export function providerAccessFromSecrets({
	terminal,
	capability,
	grant,
	proxyRunning
}: {
	terminal: boolean;
	capability?: {
		provider: string;
		status: string;
		displaySuffix?: string | null;
		usedCalls: number;
		maxCalls: number;
		usedCostUsd: number;
		maxCostUsd: number;
	};
	grant?: {
		provider?: string | null;
		maxCalls: number;
		maxCostUsd: number;
	};
	proxyRunning: boolean;
}): RunProgressProjection["providerAccess"] | undefined {
	if (terminal) return undefined;
	if (capability) {
		const status = capability.status === "exhausted"
			? "exhausted"
			: capability.status === "expired"
				? "expired"
				: proxyRunning
					? "healthy"
					: "proxy_down";
		return {
			provider: capability.provider,
			status,
			suffix: capability.displaySuffix ?? undefined,
			usedCalls: capability.usedCalls,
			maxCalls: capability.maxCalls,
			usedCostUsd: capability.usedCostUsd,
			maxCostUsd: capability.maxCostUsd,
			note: status === "proxy_down"
				? "Provider proxy is not running."
				: status === "exhausted"
					? "Call or spend ceiling reached."
					: "Via Workshop proxy"
		};
	}
	if (grant) {
		return {
			provider: grant.provider ?? "openai",
			status: "approval_required",
			usedCalls: 0,
			maxCalls: grant.maxCalls,
			usedCostUsd: 0,
			maxCostUsd: grant.maxCostUsd,
			note: "Allow this in Settings → Secrets"
		};
	}
	if (!proxyRunning) {
		return {
			provider: "openai",
			status: "proxy_down",
			usedCalls: 0,
			maxCalls: 0,
			usedCostUsd: 0,
			maxCostUsd: 0,
			note: "Provider proxy is not running."
		};
	}
	// A lease absent from this optional snapshot is not evidence that a
	// credential is absent. The optimizer's durable terminal state owns that.
	return undefined;
}
