export type HostedInferenceLifecycle = {
	protocolVersion: string | null;
	phase: string | null;
	reason: string | null;
	observedAt: number | null;
	warmOperationId: string | null;
	coldStartDeadlineSeconds: number | null;
	cooldown: {
		policy: string | null;
		idleTimeoutSeconds: number | null;
		lastActivityAt: number | null;
		warmUntil: number | null;
	};
};

const EMPTY_COOLDOWN = { policy: null, idleTimeoutSeconds: null, lastActivityAt: null, warmUntil: null };
const numberOrNull = (value: unknown): number | null =>
	typeof value === "number" && Number.isFinite(value) ? value : null;

export function parseHostedInferenceLifecycle(value: unknown): HostedInferenceLifecycle | null {
	if (!value || typeof value !== "object") return null;
	const raw = (value as Record<string, unknown>).inference_lifecycle;
	if (!raw || typeof raw !== "object") return null;
	const lifecycle = raw as Record<string, unknown>;
	const rawCooldown = lifecycle.cooldown;
	const cooldown = rawCooldown && typeof rawCooldown === "object" ? rawCooldown as Record<string, unknown> : null;
	return {
		protocolVersion: typeof lifecycle.protocol_version === "string" ? lifecycle.protocol_version : null,
		phase: typeof lifecycle.phase === "string" ? lifecycle.phase : null,
		reason: typeof lifecycle.reason === "string" ? lifecycle.reason : null,
		observedAt: numberOrNull(lifecycle.observed_at),
		warmOperationId: typeof lifecycle.warm_operation_id === "string" ? lifecycle.warm_operation_id : null,
		coldStartDeadlineSeconds: numberOrNull(lifecycle.cold_start_deadline_seconds),
		cooldown: cooldown ? {
			policy: typeof cooldown.policy === "string" ? cooldown.policy : null,
			idleTimeoutSeconds: numberOrNull(cooldown.idle_timeout_seconds),
			lastActivityAt: numberOrNull(cooldown.last_activity_at),
			warmUntil: numberOrNull(cooldown.warm_until)
		} : EMPTY_COOLDOWN
	};
}

export function hostedLifecycleLabel(phase: string | null | undefined): string | null {
	switch (phase) {
		case "queued": return "Waiting for capacity…";
		case "provisioning": return "Starting cloud GPU…";
		case "warming": return "Warming model…";
		case "ready": return "Model ready…";
		case "running": return "Generating…";
		case "scaled_down": return "Starting hosted model…";
		case "saturated": return "Waiting for capacity…";
		default: return null;
	}
}

export function hostedCooldownLabel(lifecycle: HostedInferenceLifecycle | null, nowMs = Date.now()): string | null {
	if (!lifecycle || !["ready", "running"].includes(lifecycle.phase ?? "")) return null;
	const warmUntil = lifecycle.cooldown.warmUntil;
	if (warmUntil == null) return lifecycle.cooldown.policy === "provider_managed" ? "Hosted model ready" : null;
	const remainingSeconds = Math.max(0, Math.ceil(warmUntil - nowMs / 1_000));
	if (remainingSeconds === 0) return "Hosted model may scale down now";
	const minutes = Math.ceil(remainingSeconds / 60);
	return `Hosted model warm · scales down in ${minutes}m`;
}
