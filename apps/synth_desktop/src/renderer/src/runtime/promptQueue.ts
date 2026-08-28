import type { Session, SessionStatus } from "@synth/runtime-protocol";
import {
	nextQueuedPrompt,
	removeQueuedPrompt
} from "../preferences";

const FINISHED_STATUSES = new Set<SessionStatus>([
	"ready",
	"interrupted",
	"completed",
	"failed"
]);

export type PromptQueueDrainRefs = {
	statuses: Map<string, SessionStatus>;
	draining: Set<string>;
};

export type PromptQueueDrainDeps = {
	refs: PromptQueueDrainRefs;
	send: (sessionId: string, text: string) => Promise<boolean>;
	onAccepted: (promptId: string) => void;
};

/**
 * When a session leaves `running`, drain the next queued prompt (if any).
 * Idempotent across React effect re-runs via status/draining refs.
 */
export function drainPromptQueues(
	sessions: ReadonlyArray<Session>,
	deps: PromptQueueDrainDeps
): void {
	for (const session of sessions) {
		const previous = deps.refs.statuses.get(session.id);
		const finished = FINISHED_STATUSES.has(session.status);
		if (previous === "running" && finished) {
			const next = nextQueuedPrompt(session.id);
			if (next && !deps.refs.draining.has(session.id)) {
				deps.refs.draining.add(session.id);
				void deps
					.send(session.id, next.text)
					.then((accepted) => {
						if (accepted) deps.onAccepted(next.id);
					})
					.finally(() => deps.refs.draining.delete(session.id));
			}
		}
		deps.refs.statuses.set(session.id, session.status);
	}
}

export { nextQueuedPrompt, removeQueuedPrompt };
