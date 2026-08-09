import { useEffect, useMemo, useState } from "react";
import type { LagunaStatus } from "../env";

function compactModelName(value: string): string {
	return value.split("/").at(-1)?.replace(/-mlx$/i, "") ?? value;
}

function formatMemory(bytes: number | null): string {
	if (bytes == null || bytes <= 0) return "Memory unavailable";
	return `${(bytes / 1024 ** 3).toFixed(1)} GB resident`;
}

function formatAge(seconds: number): string {
	if (seconds < 5) return "just now";
	if (seconds < 60) return `${seconds}s ago`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
	return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m ago`;
}

function formatDuration(seconds: number): string {
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	const remainder = seconds % 60;
	return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function formatScheduledTime(timestamp: number): string {
	return new Intl.DateTimeFormat(undefined, {
		hour: "numeric",
		minute: "2-digit"
	}).format(new Date(timestamp));
}

export function LocalModelResidency({ status }: { status: LagunaStatus | null }) {
	const [expanded, setExpanded] = useState(false);
	const [now, setNow] = useState(Date.now());
	useEffect(() => {
		if (status?.phase !== "ready" || !status.loadedModel) return;
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, [status?.loadedModel, status?.phase]);

	const timing = useMemo(() => {
		const reportedIdle = Math.max(0, status?.idleSeconds ?? 0);
		const elapsed = status?.updatedAt ? Math.max(0, Math.floor((now - status.updatedAt) / 1_000)) : 0;
		const idle = status?.lastUsedAt != null
			? Math.max(0, Math.floor((now - status.lastUsedAt) / 1_000))
			: reportedIdle + elapsed;
		const unloadAfter = status?.idleUnloadAfterSeconds ?? null;
		const remaining = status?.freeAt != null
			? Math.max(0, Math.ceil((status.freeAt - now) / 1_000))
			: unloadAfter == null ? null : Math.max(0, unloadAfter - idle);
		const scheduledAt = status?.freeAt ?? (remaining == null ? null : now + remaining * 1_000);
		return { idle, remaining, scheduledAt };
	}, [now, status?.freeAt, status?.idleSeconds, status?.idleUnloadAfterSeconds, status?.lastUsedAt, status?.updatedAt]);

	if (status?.phase !== "ready" || !status.loadedModel) return null;
	const model = compactModelName(status.loadedModel);
	const countdown = timing.remaining == null
		? "Automatic freeing disabled"
		: timing.remaining <= 0
			? `Free scheduled for ${formatScheduledTime(timing.scheduledAt!)} · awaiting unload`
			: `Frees at ${formatScheduledTime(timing.scheduledAt!)} · in ${formatDuration(timing.remaining)}`;
	const detailsId = "local-model-residency-details";

	return (
		<div className="model-residency" data-testid="model-residency">
			<button
				type="button"
				className="model-residency-summary"
				aria-expanded={expanded}
				aria-controls={detailsId}
				aria-label={`${model} loaded, ${formatMemory(status.memoryBytes)}, last prompt ${formatAge(timing.idle)}, ${countdown}`}
				title={`${model}\n${formatMemory(status.memoryBytes)}\nLast prompt ${formatAge(timing.idle)}\n${countdown}`}
				onClick={() => setExpanded((value) => !value)}
			>
				<span className="model-residency-dot" aria-hidden />
				<span className="model-residency-copy">
					<strong>{model}</strong>
					<span>{formatMemory(status.memoryBytes)}</span>
				</span>
				<span className="model-residency-chevron" aria-hidden>{expanded ? "⌄" : "›"}</span>
			</button>
			{expanded ? (
				<div className="model-residency-details" id={detailsId} data-testid="model-residency-details">
					<div><span>Last prompt</span><strong>{formatAge(timing.idle)}</strong></div>
					<div><span>Memory</span><strong>{formatMemory(status.memoryBytes)}</strong></div>
					<div><span>Next free</span><strong aria-live="polite">{countdown}</strong></div>
				</div>
			) : null}
		</div>
	);
}
