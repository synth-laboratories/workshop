/**
 * `EvidenceClient` — lazy, range-addressed access to a run's raw journal.
 *
 * Aggregate surfaces read the durable projection and are authoritative the
 * moment a visual mounts. Raw events are for the surfaces that genuinely need
 * them — Replay, the agent transcript, frame drill-down — and those open on
 * intent, not on mount. This is what they read through.
 *
 * The client owns one thing the caller should not have to: *coverage*. It
 * remembers the spans it already holds and sends them with each request, so
 * the backend answers with the complement. A tab reopened after a restart, or
 * a reader who jumped to the end of a run and then scrolled back, transfers
 * only what is genuinely missing.
 *
 * Coverage is a set of ranges rather than a cursor on purpose. A cursor can
 * only say "after N", which is right for a live tail and wrong for browsing: a
 * reader holding `[1..500]` and `[2000..2259]` who asks "after 2259" fetches
 * nothing and keeps the hole in the middle forever.
 */

import type { EvidencePage, EvidenceRange } from "../../bridge";

export type EvidenceWindow = { from: number; to: number };

export type EvidenceClient = {
	/**
	 * Load `window`, fetching only the spans not already held. Returns every
	 * event the client holds inside that window, in sequence order.
	 *
	 * Repeats until the window is covered or the backend stops making
	 * progress, so one call is one complete answer rather than a page the
	 * caller has to drive.
	 */
	load(window: EvidenceWindow): Promise<unknown[]>;
	/** Spans currently held, newest normalization. */
	coverage(): EvidenceRange[];
	/** The run's durable tail as of the last answer, or 0 before the first. */
	tail(): number;
};

type Transport = {
	evidencePage(
		optimizerRunId: string,
		window: EvidenceRange,
		held?: EvidenceRange[] | null,
		limit?: number | null
	): Promise<EvidencePage>;
};

function sequenceOf(event: unknown): number {
	if (!event || typeof event !== "object") return 0;
	const record = event as Record<string, unknown>;
	const raw = record.sequenceNumber ?? record.sequence_number;
	const value = Number(raw);
	return Number.isSafeInteger(value) && value > 0 ? value : 0;
}

export function createEvidenceClient(
	optimizerRunId: string,
	transport: Transport,
	pageLimit = 200
): EvidenceClient {
	// Sequence-keyed, so a span fetched twice — which the coverage protocol is
	// designed to prevent but a caller can still force by racing two loads —
	// stores one copy rather than two.
	const held = new Map<number, unknown>();
	let coverage: EvidenceRange[] = [];
	let tailCursor = 0;

	return {
		coverage: () => coverage.map((range) => ({ ...range })),
		tail: () => tailCursor,
		async load(window) {
			const from = Math.max(1, Math.floor(window.from));
			const to = Math.floor(window.to);
			if (to >= from) {
				// Bounded rather than `while (!complete)`: a backend that stops
				// making progress must end the loop, not spin against it.
				for (let attempt = 0; attempt < 512; attempt += 1) {
					const page = await transport.evidencePage(
						optimizerRunId,
						{ from, to },
						coverage,
						pageLimit
					);
					tailCursor = page.tailCursor;
					coverage = page.coverage;
					for (const event of page.events) {
						const sequence = sequenceOf(event);
						if (sequence > 0) held.set(sequence, event);
					}
					if (page.complete || !page.range || page.events.length === 0) break;
				}
			}
			return [...held.entries()]
				.filter(([sequence]) => sequence >= from && sequence <= to)
				.sort(([left], [right]) => left - right)
				.map(([, event]) => event);
		}
	};
}
