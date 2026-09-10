import { useMemo, useState } from "react";
import type { WhisperRuntimeStatus } from "../bridge";

function compactModelName(value: string | null): string {
	if (!value) return "Whisper";
	return value.split("/").at(-1)?.replace(/[-_]mlx$/i, "") ?? value;
}

function phaseLabel(phase: string): string {
	switch (phase) {
		case "warming": return "Loading model…";
		case "transcribing": return "Transcribing…";
		case "ready": return "Ready · local";
		case "error": return "Needs attention";
		default: return phase;
	}
}

function formatRelease(seconds: number): string {
	if (seconds <= 0) return "Automatic release disabled";
	const minutes = Math.ceil(seconds / 60);
	return `Releases after ${minutes} min idle`;
}

export function WhisperResidency({ status }: { status: WhisperRuntimeStatus | null }) {
	const [expanded, setExpanded] = useState(false);
	const model = useMemo(() => compactModelName(status?.loadedModel ?? null), [status?.loadedModel]);
	if (!status || status.phase === "unloaded") return null;
	const detailsId = "whisper-residency-details";
	const state = phaseLabel(status.phase);
	const release = formatRelease(status.idleUnloadAfterSeconds);
	const summary = status.phase === "ready" ? `Ready · ${release.toLowerCase()}` : state;

	return (
		<div className="model-residency whisper-residency" data-phase={status.phase} data-testid="whisper-residency">
			<button
				type="button"
				className="model-residency-summary"
				aria-expanded={expanded}
				aria-controls={detailsId}
				aria-label={`${model}, ${state}, ${release}`}
				onClick={() => setExpanded((value) => !value)}
			>
				<span className="model-residency-dot" aria-hidden />
				<span className="model-residency-copy">
					<strong>{model}</strong>
					<span role="status" aria-live="polite">{summary}</span>
				</span>
				<span className="model-residency-chevron" aria-hidden>{expanded ? "⌄" : "›"}</span>
			</button>
			{expanded ? (
				<div className="model-residency-details" id={detailsId} data-testid="whisper-residency-details">
					<div><span>Runtime</span><strong>On-device Whisper</strong></div>
					<div><span>State</span><strong>{state}</strong></div>
					<div><span>Memory</span><strong>{release}</strong></div>
				</div>
			) : null}
		</div>
	);
}
