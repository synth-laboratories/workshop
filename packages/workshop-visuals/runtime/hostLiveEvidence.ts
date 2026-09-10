import type { HostLiveEvalProjection, ReplayPage, ReplayStream } from "./replayClient.ts";

export type HostLiveEvidence = {
  projection?: HostLiveEvalProjection;
  responseCount: number;
  recovered: number;
  ready: boolean;
  truncated: boolean;
  error: string | null;
};

/** A poll receipt describes the whole visual, not only the responding stream.
 * Its total response count orders concurrent snapshots without wall-clock time.
 * Never add together whole-visual projections from individual poll answers. */
export function acceptHostEvidence(
  previous: HostLiveEvidence | undefined,
  page: ReplayPage,
  streams: ReplayStream[],
  identity: { visualId?: string | null; revision?: number | null }
): HostLiveEvidence | undefined {
  if (page.receipt === undefined) {
    if (previous) throw new Error("Host replay stopped supplying its evidence receipt");
    return undefined; // Browser/fixture transport; never a silent host downgrade.
  }
  const receipt = page.receipt as Record<string, unknown> | null;
  if (!receipt || receipt.schemaVersion !== "synth.visual-stream-receipt.v1" ||
      receipt.visualId !== identity.visualId || receipt.revision !== identity.revision) {
    throw new Error("Host stream receipt does not match this visual revision");
  }
  const rows = receipt.streams as { streamId: string; declaredSource: string; pollResponses: number }[];
  if (!Array.isArray(rows) || rows.length !== streams.length ||
      streams.some(stream => !rows.some(row => row.streamId === stream.streamId && row.declaredSource === stream.pollUrl))) {
    throw new Error("Host stream receipt does not match the declared transports");
  }
  const count = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) >= 0;
  if (!rows.every(row => count(row.pollResponses)) || !count(receipt.recovered)) {
    throw new Error("Host stream receipt has invalid evidence accounting");
  }
  const responseCount = rows.reduce((total, row) => total + row.pollResponses, 0);
  if (previous && responseCount <= previous.responseCount) return previous;
  if (page.projection && page.projection.schema_version !== "synth.live-eval-projection.v1") {
    throw new Error("Unsupported host live projection schema");
  }
  const truncated = page.evidenceTruncated === true || receipt.trackingTruncated === true;
  const conflicts = Array.isArray(receipt.conflicts) ? receipt.conflicts.length : 0;
  const gaps = Array.isArray(receipt.gaps) ? receipt.gaps.length : 0;
  const missing = Array.isArray(receipt.streamsMissingTransport) ? receipt.streamsMissingTransport.length : 0;
  const error = truncated ? "Host evidence is truncated; displayed values cover only the retained prefix"
    : conflicts ? "Host observed conflicting replay envelopes"
    : gaps ? "Host observed an evidence gap"
    : missing ? "Declared streams are missing poll transport" : null;
  return { projection: page.projection, responseCount, recovered: receipt.recovered,
    ready: receipt.ready === true && receipt.respondingStreamCount === receipt.declaredStreamCount && !error,
    truncated, error };
}
