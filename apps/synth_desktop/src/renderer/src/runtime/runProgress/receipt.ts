/**
 * Render receipts — proof that a visual revision rendered from complete local
 * evidence, and the check that says whether it still can.
 *
 * The evidence needs no client-side copy. The kernel projection is durable in
 * SQLite and, now that reads take a WAL snapshot instead of the write lock, is
 * readable in about a millisecond with no producer involved — so a reopened
 * terminal visual already renders offline. Copying the projection into a
 * second store would create a second authority for product truth, which is
 * what the kernel invariants exist to prevent.
 *
 * What a copy *would* have provided incidentally, and what is provided here
 * directly, is the ability to notice when local evidence no longer supports
 * what was already shown. A revision that advances is normal. A revision that
 * goes backwards, or that stays the same while its content changes, is a
 * regression — and rendering it silently is worse than saying so.
 */

import type { OptimizerRunViewV2, VisualRenderReceipt } from "../../bridge";

/**
 * A stable digest of the rendered projection.
 *
 * Keys are sorted so two structurally identical projections digest alike
 * regardless of serialization order; without that, an incidental key reorder
 * would read as content corruption on every reopen.
 */
export function visualDataDigest(view: OptimizerRunViewV2): string {
	const canonical = (value: unknown): string => {
		if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
		if (value && typeof value === "object") {
			const record = value as Record<string, unknown>;
			return `{${Object.keys(record).sort()
				.map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`)
				.join(",")}}`;
		}
		return JSON.stringify(value) ?? "null";
	};
	const text = canonical(view);
	// FNV-1a, 64-bit, as two 32-bit halves. Not a security digest — this only
	// has to detect drift between two things Workshop itself wrote, and a
	// cryptographic hash on every first paint would cost more than it proves.
	let high = 0x811c9dc5;
	let low = 0x811c9dc5;
	for (let index = 0; index < text.length; index += 1) {
		const code = text.charCodeAt(index);
		low = Math.imul(low ^ (code & 0xff), 0x01000193) >>> 0;
		high = Math.imul(high ^ ((code >>> 8) & 0xff), 0x01000193) >>> 0;
	}
	return `fnv1a64:${high.toString(16).padStart(8, "0")}${low.toString(16).padStart(8, "0")}`;
}

export type ReceiptVerdict =
	/** No prior render, or a template change makes comparison meaningless. */
	| { kind: "unverified"; reason?: "no_receipt" | "template_changed" | "different_run" }
	/** Local evidence is at or ahead of what already rendered. */
	| { kind: "current"; renderedAt: string }
	/** Same revision, different content. */
	| { kind: "content_changed"; renderedAt: string; projectionRevision: number }
	/** Local evidence is older than what already rendered. */
	| { kind: "regressed"; renderedAt: string; renderedRevision: number; localRevision: number };

/**
 * Compare local evidence against what this visual revision already rendered.
 *
 * A template change yields `unverified` rather than a failure: different code
 * legitimately renders the same projection differently, so its digest is not
 * comparable across versions.
 */
export function verifyAgainstReceipt(
	receipt: VisualRenderReceipt | null | undefined,
	local: { optimizerRunId: string; projectionRevision: number; dataDigest: string; templateVersion?: string }
): ReceiptVerdict {
	if (!receipt) return { kind: "unverified", reason: "no_receipt" };
	if (receipt.optimizerRunId !== local.optimizerRunId) {
		return { kind: "unverified", reason: "different_run" };
	}
	if ((receipt.templateVersion ?? "") !== (local.templateVersion ?? "")) {
		return { kind: "unverified", reason: "template_changed" };
	}
	if (local.projectionRevision < receipt.projectionRevision) {
		return {
			kind: "regressed",
			renderedAt: receipt.renderedAt,
			renderedRevision: receipt.projectionRevision,
			localRevision: local.projectionRevision
		};
	}
	if (
		local.projectionRevision === receipt.projectionRevision &&
		receipt.dataDigest !== local.dataDigest
	) {
		return {
			kind: "content_changed",
			renderedAt: receipt.renderedAt,
			projectionRevision: receipt.projectionRevision
		};
	}
	return { kind: "current", renderedAt: receipt.renderedAt };
}
