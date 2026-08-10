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

export function LocalModelResidency({
	status,
	onFreeMemory
}: {
	status: LagunaStatus | null;
	onFreeMemory?: () => Promise<void>;
}) {
	const [expanded, setExpanded] = useState(false);
	const [now, setNow] = useState(Date.now());
	const [freeing, setFreeing] = useState(false);
	const [freeError, setFreeError] = useState<string | null>(null);
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
		// `freeAt` is the daemon's answer to "when do these weights go away",
		// and a null answer means "no eviction is scheduled" — the case for a
		// model whose weights live in an engine this daemon cannot unload.
		// Deriving a countdown from the idle setting instead promised a free
		// that never arrives: the sidebar showed Muse Glimmer freeing in 14
		// minutes while it stays resident for as long as it is selected.
		const remaining = status?.freeAt != null
			? Math.max(0, Math.ceil((status.freeAt - now) / 1_000))
			: null;
		return { idle, remaining, scheduledAt: status?.freeAt ?? null };
	}, [now, status?.freeAt, status?.idleSeconds, status?.lastUsedAt, status?.updatedAt]);

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
					{freeError ? <p className="model-residency-error" role="alert">{freeError}</p> : null}
					<button
						type="button"
						className="model-residency-free"
						data-testid="free-local-model-memory"
						disabled={freeing || !onFreeMemory}
						onClick={async (event) => {
							event.stopPropagation();
							if (!onFreeMemory) return;
							setFreeing(true);
							setFreeError(null);
							try {
								await onFreeMemory();
							} catch (reason) {
								setFreeError(reason instanceof Error ? reason.message : String(reason));
							} finally {
								setFreeing(false);
							}
						}}
					>
						{freeing ? "Freeing memory…" : "Free memory"}
					</button>
				</div>
			) : null}
		</div>
	);
}
