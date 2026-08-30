import type { RefObject } from "react";
import type { VisualSeal, VisualSealBundle, VisualUpload } from "../bridge";
import { formatVisualAdmissionIdentity, type ArtifactRef } from "../types/landing";
import { VisualOpsLine } from "./VisualOpsLine";

export type VisualPaneDebugState = {
	connectionState: string | null;
	transportState: string | null;
	projectionSource: string | null;
	stale: boolean;
};

type VisualPaneChromeProps = {
	artifact: ArtifactRef;
	expanded: boolean;
	inspectorOpen: boolean;
	overflowRef: RefObject<HTMLDivElement | null>;
	moreButtonRef: RefObject<HTMLButtonElement | null>;
	busy: boolean;
	artifactOperationsEnabled: boolean;
	annotationsCount: number;
	sealEligible: boolean;
	sealDisabledReason: string | null;
	seals: VisualSeal[];
	sealedBundle: VisualSealBundle | null;
	compareBundle: VisualSealBundle | null;
	shareUpload: VisualUpload | null;
	sharedUrl: string;
	sharedUrlValid: boolean;
	sharedUrlError: string | null;
	debugState: VisualPaneDebugState;
	onToggleInspector: () => void;
	onBeginLabeling: () => void;
	onSeal: () => void;
	onLiveRevision: () => void;
	onCloseComparison: () => void;
	onShare: () => void;
	onReopenSeal: (receiptDigest: string) => void;
	onCompareSeal: (receiptDigest: string) => void;
	onSharedUrlChange: (value: string) => void;
	onOpenShared: () => void;
	onCopySharedUrl: () => void;
	onToggleExpanded: () => void;
	onClose: () => void;
};

function DebugValue({ label, value }: { label: string; value: string | null }) {
	return <><dt>{label}</dt><dd>{value ?? "—"}</dd></>;
}

export function VisualPaneChrome({
	artifact,
	expanded,
	inspectorOpen,
	overflowRef,
	moreButtonRef,
	busy,
	artifactOperationsEnabled,
	annotationsCount,
	sealEligible,
	sealDisabledReason,
	seals,
	sealedBundle,
	compareBundle,
	shareUpload,
	sharedUrl,
	sharedUrlValid,
	sharedUrlError,
	debugState,
	onToggleInspector,
	onBeginLabeling,
	onSeal,
	onLiveRevision,
	onCloseComparison,
	onShare,
	onReopenSeal,
	onCompareSeal,
	onSharedUrlChange,
	onOpenShared,
	onCopySharedUrl,
	onToggleExpanded,
	onClose
}: VisualPaneChromeProps) {
	const revision = artifact.revision;
	const visualId = artifact.visualId;
	return (
		<header className="visual-pane-head">
			<div className="visual-pane-head-text">
				<span className="visual-pane-title" title={artifact.title}>{artifact.title}</span>
			</div>
			<div className="visual-pane-head-actions">
				<button
					type="button"
					className="visual-expand"
					onClick={onToggleExpanded}
					aria-pressed={expanded}
					aria-label={expanded ? "Restore split view" : "Expand visual"}
					data-testid="toggle-visual-expand"
				>
					{expanded ? "Restore" : "Expand"}
				</button>
				<div className="visual-pane-overflow" ref={overflowRef}>
					<button
						ref={moreButtonRef}
						type="button"
						className="visual-pane-more"
						onClick={onToggleInspector}
						aria-haspopup="dialog"
						aria-expanded={inspectorOpen}
						aria-controls="visual-artifact-inspector"
						aria-label="Visual details and actions"
						title="Visual details and actions"
					>
						<span aria-hidden>•••</span>
					</button>
					{inspectorOpen ? (
						<div id="visual-artifact-inspector" className="visual-artifact-inspector" role="dialog" aria-label="Visual details and actions" data-testid="visual-artifact-inspector">
							{artifactOperationsEnabled ? <section className="visual-inspector-section" aria-labelledby="visual-inspector-actions">
								<h3 id="visual-inspector-actions">Actions</h3>
								<div className="visual-inspector-actions">
									{sealedBundle ? <button type="button" onClick={onLiveRevision}>Live revision</button> : null}
									{compareBundle ? <button type="button" onClick={onCloseComparison}>Close comparison</button> : null}
									<button type="button" onClick={onBeginLabeling} disabled={!visualId || !revision || busy}>Label{annotationsCount ? ` · ${annotationsCount}` : ""}</button>
									<button type="button" onClick={onSeal} disabled={!sealEligible || busy}>{busy ? "Working…" : "Seal revision"}</button>
									{sealedBundle ? <button type="button" onClick={onShare} disabled={busy}>{shareUpload?.state === "committed" ? "Shared privately" : "Share privately"}</button> : null}
								</div>
								{!sealEligible && sealDisabledReason ? <p className="visual-inspector-note">{sealDisabledReason}</p> : null}
							</section> : null}

							{artifactOperationsEnabled && seals.length ? (
								<section className="visual-inspector-section" aria-labelledby="visual-inspector-revisions">
									<h3 id="visual-inspector-revisions">Offline revisions</h3>
									<div className="visual-inspector-revisions">
										{seals.map((seal) => (
											<div key={seal.receiptDigest}>
												<button type="button" onClick={() => onReopenSeal(seal.receiptDigest)}>rev {seal.visualRevision} · {seal.receiptDigest.slice(0, 8)}</button>
												{sealedBundle?.seal.receiptDigest !== seal.receiptDigest ? <button type="button" onClick={() => onCompareSeal(seal.receiptDigest)}>Compare</button> : null}
											</div>
										))}
									</div>
								</section>
							) : null}

							<section className="visual-inspector-section" aria-labelledby="visual-inspector-details">
								<h3 id="visual-inspector-details">Details</h3>
								<p className="visual-pane-identity" data-testid="visual-pane-identity">
									{formatVisualAdmissionIdentity({
										visualId: visualId ?? artifact.id,
										revision,
										receiptDigest: seals.find((seal) => seal.visualRevision === revision)?.receiptDigest ?? artifact.receiptDigest,
										contentDigest: artifact.contentDigest
									})}
								</p>
								<VisualOpsLine sessionId={artifact.sessionId ?? artifact.ownerSessionId} runId={artifact.runId} traceId={artifact.traceId} testId="visual-pane-ops" probe />
							</section>

							{artifactOperationsEnabled ? <section className="visual-inspector-section" aria-labelledby="visual-inspector-shared">
								<h3 id="visual-inspector-shared">Open shared visual</h3>
								<form className="visual-shared-open" onSubmit={(event) => { event.preventDefault(); onOpenShared(); }}>
									<input value={sharedUrl} onChange={(event) => onSharedUrlChange(event.target.value)} placeholder="Paste private artifact URL" aria-label="Private artifact URL" aria-invalid={Boolean(sharedUrlError)} />
									<button type="submit" disabled={!sharedUrlValid || busy}>Open</button>
								</form>
								{sharedUrlError ? <p className="visual-inspector-error" role="alert">{sharedUrlError}</p> : null}
								{shareUpload?.committedUrl ? <div className="visual-share-url"><a href={shareUpload.committedUrl} target="_blank" rel="noreferrer">Private permalink</a><button type="button" onClick={onCopySharedUrl}>Copy</button></div> : null}
							</section> : null}

							<details className="visual-inspector-debug">
								<summary>Debug</summary>
								<dl>
									<DebugValue label="Connection" value={debugState.connectionState} />
									<DebugValue label="Transport" value={debugState.transportState} />
									<DebugValue label="Projection" value={debugState.projectionSource} />
									<DebugValue label="Freshness" value={debugState.stale ? "stale" : "current"} />
								</dl>
							</details>
						</div>
					) : null}
				</div>
				<button type="button" className="visual-close" onClick={onClose} aria-label="Close visual">×</button>
			</div>
		</header>
	);
}
