/**
 * Synth Index "Prepare Contribution" bridge. Rust owns credentials, file reads
 * and the canonical intake; the renderer only sends explicit selections and
 * metadata and receives manifests and statuses. Nothing here publishes.
 */
import { invoke } from "@tauri-apps/api/core";

export const INDEX_ASSET_ROLES = [
	"report",
	"summary",
	"code",
	"data",
	"model",
	"environment",
	"evidence",
	"reproduce"
] as const;

export type IndexSelectedFile = { path: string; role: string; logicalPath?: string | null };

export type IndexReference = { contributionId: string; revisionId: string };

export type IndexExportMetadata = {
	title: string;
	abstractText: string;
	kind: string;
	researchAreas: string[];
	workflowStages: string[];
	tagIds: string[];
	reusable: string;
	howToUse: string;
	observed: string;
	limitations: string;
	resources: string;
	claimKind: string;
	claimStatement: string;
	claimScope: string;
	claimAssessment: string;
	missingEvidenceReason: string | null;
	reproductionLevel: string;
	reproductionInstructions: string;
	expectedOutputs: string;
	restrictions: string;
	license: string;
	rightsAttested: boolean;
	sensitiveData: string;
	requestedAudience: "private" | "org" | "public";
	confirmPublicRelease: boolean;
	contributorPrincipalId: string;
	contributorRoles: string[];
	upstream: IndexReference[];
};

export type IndexExportRequest = {
	exportId: string;
	root: string;
	files: IndexSelectedFile[];
	metadata: IndexExportMetadata;
};

export type IndexExportAsset = {
	assetId: string;
	role: string;
	logicalPath: string;
	mediaType: string;
	sizeBytes: number;
	digestSha256: string;
};

export type IndexExportPreview = {
	assets: IndexExportAsset[];
	totalBytes: number;
	requestedAudience: string;
	package: unknown;
	warnings: string[];
};

export type IndexExportOutcome = {
	reference: IndexReference;
	publicationId: string;
	uploadedPaths: string[];
	finalized: unknown;
	status: "finalized";
};

export type IndexRevisionStatus = {
	status?: string;
	assessments?: Array<{ decision: string; comments: string; created_at?: string }>;
	[key: string]: unknown;
};

export const indexExport = {
	preview: (request: IndexExportRequest) =>
		invoke<IndexExportPreview>("index_export_preview", { request }),
	upload: (request: IndexExportRequest) =>
		invoke<IndexExportOutcome>("index_export_upload", { request }),
	submit: (reference: IndexReference, exportId: string) =>
		invoke<unknown>("index_export_submit", { request: { reference, exportId } }),
	status: (reference: IndexReference) =>
		invoke<IndexRevisionStatus>("index_revision_status", { request: reference })
};

/** Parse "role path" lines typed or pasted by the user; nothing is discovered. */
export function parseSelection(text: string): IndexSelectedFile[] {
	return text
		.split("\n")
		.map((line) => line.trim())
		.filter(Boolean)
		.map((line) => {
			const [role, ...rest] = line.split(/\s+/);
			return { path: rest.join(" "), role: role ?? "", logicalPath: null };
		});
}
