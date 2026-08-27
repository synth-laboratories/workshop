import type { VisualRecord } from "@synth/runtime-protocol";

/**
 * Resolve a catalog row through the authoritative visual registry before it is
 * mounted. Catalog rows are discovery data; the record returned by `get` owns
 * the saved binding envelope that restores live and sealed evidence.
 */
export async function hydrateVisualRecord(
	listed: Pick<VisualRecord, "id">,
	get: (visualId: string) => Promise<VisualRecord>
): Promise<VisualRecord> {
	const visual = await get(listed.id);
	if (visual.id !== listed.id) {
		throw new Error(`Visual registry returned ${visual.id} for ${listed.id}`);
	}
	return visual;
}
