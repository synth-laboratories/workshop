import { useMemo, useState } from "react";
import {
	INDEX_ASSET_ROLES,
	indexExport,
	parseSelection,
	type IndexExportMetadata,
	type IndexExportOutcome,
	type IndexExportPreview,
	type IndexRevisionStatus
} from "../runtime/indexExport";

const EMPTY: IndexExportMetadata = {
	title: "",
	abstractText: "",
	kind: "research_report",
	researchAreas: ["evaluation"],
	workflowStages: ["evaluation"],
	tagIds: [],
	reusable: "",
	howToUse: "",
	observed: "",
	limitations: "",
	resources: "",
	claimKind: "diagnostic",
	claimStatement: "",
	claimScope: "",
	claimAssessment: "supported",
	missingEvidenceReason: null,
	reproductionLevel: "inspectable",
	reproductionInstructions: "",
	expectedOutputs: "",
	restrictions: "",
	license: "CC-BY-4.0",
	rightsAttested: false,
	sensitiveData: "none_declared",
	requestedAudience: "private",
	confirmPublicRelease: false,
	contributorPrincipalId: "",
	contributorRoles: ["research"],
	upstream: []
};

const TEXT_FIELDS: Array<[keyof IndexExportMetadata, string]> = [
	["title", "Title"],
	["abstractText", "Abstract"],
	["claimStatement", "Main claim"],
	["claimScope", "Claim scope (where it applies)"],
	["reusable", "What is reusable"],
	["howToUse", "How to use it"],
	["observed", "What was observed"],
	["limitations", "Limitations"],
	["resources", "Resources required"],
	["reproductionInstructions", "Reproduction instructions"],
	["expectedOutputs", "Expected outputs"],
	["restrictions", "Restrictions"],
	["contributorPrincipalId", "Your Synth user id"]
];

function newExportId(): string {
	return `exp_${crypto.randomUUID().replace(/-/g, "").slice(0, 24)}`;
}

function describe(error: unknown): string {
	if (error && typeof error === "object" && "message" in error) return String((error as { message: unknown }).message);
	return String(error);
}

/**
 * Selected files → manifest/provenance/rights/visibility preview → private draft
 * upload → explicit submit → QA status. Public release is only a request that the
 * owner confirms and an independent reviewer decides; this panel never publishes.
 */
export function IndexContributePanel({ onClose }: { onClose: () => void }) {
	const [exportId] = useState(newExportId);
	const [root, setRoot] = useState("");
	const [selection, setSelection] = useState("report report.md");
	const [meta, setMeta] = useState<IndexExportMetadata>(EMPTY);
	const [preview, setPreview] = useState<IndexExportPreview | null>(null);
	const [outcome, setOutcome] = useState<IndexExportOutcome | null>(null);
	const [status, setStatus] = useState<IndexRevisionStatus | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const files = useMemo(() => parseSelection(selection), [selection]);
	const request = { exportId, root, files, metadata: meta };
	const set = <K extends keyof IndexExportMetadata>(key: K, value: IndexExportMetadata[K]) => {
		setMeta((current) => ({ ...current, [key]: value }));
		setPreview(null);
	};

	async function run<T>(action: () => Promise<T>, apply: (value: T) => void) {
		setBusy(true);
		setError(null);
		try {
			apply(await action());
		} catch (caught) {
			setError(describe(caught));
		} finally {
			setBusy(false);
		}
	}

	return (
		<section className="index-contribute" aria-label="Prepare Contribution">
			<header>
				<h2>Prepare Contribution</h2>
				<p>Only the files you list are read. The draft stays private until you submit it and an independent reviewer approves it.</p>
				<button type="button" onClick={onClose}>Close</button>
			</header>

			<label>
				Export root folder
				<input value={root} onChange={(event) => { setRoot(event.target.value); setPreview(null); }} placeholder="/path/to/run/output" />
			</label>
			<label>
				Selected files, one per line as “role relative/path” (roles: {INDEX_ASSET_ROLES.join(", ")})
				<textarea rows={4} value={selection} onChange={(event) => { setSelection(event.target.value); setPreview(null); }} />
			</label>

			{TEXT_FIELDS.map(([key, label]) => (
				<label key={key}>
					{label}
					<input value={String(meta[key] ?? "")} onChange={(event) => set(key, event.target.value as never)} />
				</label>
			))}
			<label>
				Why is the claim untested? (required when no evidence file is selected)
				<input value={meta.missingEvidenceReason ?? ""} onChange={(event) => set("missingEvidenceReason", event.target.value || null)} />
			</label>
			<label>
				Requested audience
				<select value={meta.requestedAudience} onChange={(event) => { set("requestedAudience", event.target.value as IndexExportMetadata["requestedAudience"]); set("confirmPublicRelease", false); }}>
					<option value="private">Private (only you)</option>
					<option value="org">Organization</option>
					<option value="public">Public (after review)</option>
				</select>
			</label>
			{meta.requestedAudience === "public" ? (
				<label>
					<input type="checkbox" checked={meta.confirmPublicRelease} onChange={(event) => set("confirmPublicRelease", event.target.checked)} />
					I request public release of exactly these files once a reviewer approves them.
				</label>
			) : null}
			<label>
				<input type="checkbox" checked={meta.rightsAttested} onChange={(event) => set("rightsAttested", event.target.checked)} />
				I have the rights to share every selected file under {meta.license}, and it contains no secrets or private data I may not share.
			</label>

			<div className="index-contribute-actions">
				<button type="button" disabled={busy || !root} onClick={() => run(() => indexExport.preview(request), setPreview)}>Preview</button>
				<button type="button" disabled={busy || !preview || outcome != null} onClick={() => run(() => indexExport.upload(request), setOutcome)}>Upload private draft</button>
				<button type="button" disabled={busy || !outcome} onClick={() => outcome && run(() => indexExport.submit(outcome.reference, exportId), () => undefined)}>Submit for review</button>
				<button type="button" disabled={busy || !outcome} onClick={() => outcome && run(() => indexExport.status(outcome.reference), setStatus)}>Refresh review status</button>
			</div>

			{error ? <p role="alert">{error}</p> : null}
			{preview ? (
				<div className="index-contribute-preview">
					<h3>Manifest ({preview.assets.length} files, {preview.totalBytes} bytes) · audience: {preview.requestedAudience}</h3>
					<table>
						<thead><tr><th>Role</th><th>Path</th><th>Bytes</th><th>SHA-256</th></tr></thead>
						<tbody>
							{preview.assets.map((asset) => (
								<tr key={asset.assetId}><td>{asset.role}</td><td>{asset.logicalPath}</td><td>{asset.sizeBytes}</td><td><code>{asset.digestSha256.slice(0, 12)}…</code></td></tr>
							))}
						</tbody>
					</table>
					{preview.warnings.map((warning) => <p key={warning}>{warning}</p>)}
				</div>
			) : null}
			{outcome ? <p>Private draft {outcome.reference.contributionId} @ {outcome.reference.revisionId} uploaded and finalized. Submit when ready.</p> : null}
			{status ? (
				<div>
					<h3>Review status: {status.status ?? "unknown"}</h3>
					{(status.assessments ?? []).map((item, index) => <p key={index}><strong>{item.decision}</strong>: {item.comments}</p>)}
				</div>
			) : null}
		</section>
	);
}
