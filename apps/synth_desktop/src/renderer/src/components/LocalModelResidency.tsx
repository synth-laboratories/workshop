import { useEffect, useId, useMemo, useState } from "react";
import type { LagunaStatus } from "../env";

function compactModelName(value: string): string {
	const name = value.split("/").at(-1)?.replace(/-(?:mlx|gguf)$/i, "") ?? value;
	if (/^muse-glimmer-30b$/i.test(name)) return "Muse Glimmer 30B";
	if (/^laguna-xs-2\.1-nvfp4$/i.test(name)) return "Laguna XS 2.1 NVFP4";
	return name.replace(/[-_]+/g, " ");
}

function formatMemory(bytes: number | null): string {
	if (bytes == null || bytes <= 0) return "Unavailable";
	const gibibytes = bytes / 1024 ** 3;
	return `${Number.isInteger(gibibytes) ? gibibytes.toFixed(0) : gibibytes.toFixed(1)} GB`;
}

function formatAge(seconds: number): string {
	if (seconds < 5) return "just now";
	if (seconds < 60) return `${seconds}s ago`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
	return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m ago`;
}

function formatApproxDuration(seconds: number): string {
	if (seconds <= 5) return "now";
	if (seconds < 60) return `~${seconds}s`;
	const minutes = Math.max(1, Math.round(seconds / 60));
	return `~${minutes} min`;
}

function ModelMemoryIcon() {
	return (
		<svg className="model-residency-icon" viewBox="0 0 20 20" fill="none" aria-hidden>
			<path d="M6.2 4.5h8.3v10.2H6.2z" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" />
			<path d="M3.2 6.5H6m-2.8 3H6m-2.8 3H6M9 2v2.5m3 0V2m-3 12.7V18m3-3.3V18" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
			<path d="M9 7.4h2.8v3H9z" stroke="currentColor" strokeWidth="1.15" />
		</svg>
	);
}

function FreeMemoryIcon() {
	return (
		<svg viewBox="0 0 18 18" fill="none" aria-hidden>
			<circle cx="9" cy="9" r="6.5" stroke="currentColor" strokeWidth="1.35" />
			<rect x="6.6" y="6.6" width="4.8" height="4.8" rx="0.8" fill="currentColor" />
		</svg>
	);
}

export function LocalModelResidency({
	status,
	onFreeMemory
}: {
	status: LagunaStatus | null;
	onFreeMemory?: () => Promise<void>;
}) {
	const tooltipId = useId();
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
		const remaining = status?.freeAt != null
			? Math.max(0, Math.ceil((status.freeAt - now) / 1_000))
			: null;
		return { idle, remaining };
	}, [now, status?.freeAt, status?.idleSeconds, status?.lastUsedAt, status?.updatedAt]);

	if (status?.phase !== "ready" || !status.loadedModel) return null;
	const model = compactModelName(status.loadedModel);
	const memory = formatMemory(status.memoryBytes);
	const automaticFree = timing.remaining == null
		? "Stays loaded until freed"
		: `Frees automatically in ${formatApproxDuration(timing.remaining)}`;

	return (
		<div className="model-residency" data-testid="model-residency">
			<div
				className="model-residency-summary"
				tabIndex={0}
				aria-describedby={tooltipId}
				aria-label={`${model} is loaded. Memory: ${memory}. Last prompt: ${formatAge(timing.idle)}. ${automaticFree}.`}
			>
				<ModelMemoryIcon />
				<span className="model-residency-label">
					<span className="model-residency-name">{model}</span>
					{memory !== "Unavailable" ? <span className="model-residency-memory"> · {memory}</span> : null}
				</span>
				<button
					type="button"
					className="model-residency-free"
					data-testid="free-local-model-memory"
					disabled={freeing || !onFreeMemory}
					aria-label={freeing ? `Freeing ${model} memory` : `Free ${model} memory`}
					onClick={async () => {
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
					{freeing ? <span className="model-residency-spinner" aria-hidden /> : <FreeMemoryIcon />}
				</button>
				<div className="model-residency-tooltip" id={tooltipId} role="tooltip" data-testid="model-residency-tooltip">
					<strong>{model} is loaded</strong>
					<span>Memory: {memory}</span>
					<span>Last prompt: {formatAge(timing.idle)}</span>
					<span>{automaticFree}</span>
				</div>
			</div>
			{freeError ? <p className="model-residency-error" role="alert">{freeError}</p> : null}
		</div>
	);
}
