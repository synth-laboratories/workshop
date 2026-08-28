import { useEffect, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { appEventToRuntimeEvent } from "@synth/runtime-protocol";
import type { CodexActivityEvent, RuntimeEvent, Session } from "@synth/runtime-protocol";
import { browserRuntimeClient } from "../runtime/browserRuntimeClient";
import { appendCodexActivity } from "../runtime/codexTurn";
import {
	dispatchRuntimeEvent,
	replaceSessionEvents
} from "../stores/sessionStore";
import { publicError } from "../runtime/publicError";
import type { InternBridge } from "../bridge";

/**
 * Subscribe to non-Codex session event streams (Intern / browser fixture).
 * Codex app-server sessions are owned by the native onEvent bridge instead.
 */
export function useForeignSessionEventBridge(args: {
	activeSessionId: string | null;
	sessions: Session[];
	nativeIntern: InternBridge | undefined;
	refreshSessions: () => Promise<unknown>;
	showToast: (message: string) => void;
	setCodexActivityBySession: Dispatch<SetStateAction<Record<string, CodexActivityEvent[]>>>;
}): void {
	const {
		activeSessionId,
		sessions,
		nativeIntern,
		refreshSessions,
		showToast,
		setCodexActivityBySession
	} = args;

	useEffect(() => {
		let disposed = false;
		let subscription: { close(): void } | null = null;
		if (!activeSessionId) return () => undefined;
		const sessionId = activeSessionId;
		const selected = sessions.find((session) => session.id === sessionId);
		if (selected?.metadata.runtime === "codex-app-server") return () => undefined;

		async function connect() {
			try {
				if (selected?.target.kind === "intern" && nativeIntern) {
					const rows = await nativeIntern.eventsAfter(sessionId, 0, 500);
					if (disposed) return;
					replaceSessionEvents(
						sessionId,
						rows.map(appEventToRuntimeEvent).filter((event): event is RuntimeEvent => event !== null)
					);
					const unlisten = nativeIntern.onEvent((appEvent) => {
						if (disposed || appEvent.sessionId !== sessionId) return;
						const event = appEventToRuntimeEvent(appEvent);
						if (!event) return;
						dispatchRuntimeEvent(event);
						if (
							event.eventKind.startsWith("run.") ||
							event.eventKind === "command.receipt" ||
							event.eventKind === "command.resolved" ||
							event.eventKind === "session.updated" ||
							event.eventKind === "intern.projection_updated"
						) {
							void refreshSessions().catch(() => undefined);
						}
					});
					subscription = { close: unlisten };
					return;
				}
				const page = await browserRuntimeClient.events(sessionId, 0, 500);
				if (disposed) return;
				replaceSessionEvents(sessionId, page.events);
				subscription = await browserRuntimeClient.subscribe(
					sessionId,
					page.nextSequence,
					(event) => {
						if (disposed) return;
						dispatchRuntimeEvent(event);
						if (
							event.eventKind.startsWith("run.") ||
							event.eventKind === "usage.recorded" ||
							event.eventKind === "command.receipt" ||
							event.eventKind === "command.resolved" ||
							event.eventKind === "session.updated" ||
							event.eventKind === "intern.projection_updated"
						) {
							void refreshSessions().catch(() => undefined);
						}
					},
					undefined,
					(event) => {
						if (disposed) return;
						setCodexActivityBySession((current) => ({
							...current,
							[sessionId]: appendCodexActivity(current[sessionId] ?? [], event)
						}));
					}
				);
			} catch (reason) {
				if (!disposed) {
					showToast(publicError(reason));
				}
			}
		}

		void connect();
		return () => {
			disposed = true;
			subscription?.close();
		};
	}, [
		activeSessionId,
		nativeIntern,
		refreshSessions,
		sessions,
		setCodexActivityBySession,
		showToast
	]);
}

export type CodexEventBridgeRefs = {
	sessionsRef: MutableRefObject<Session[]>;
	manualCompactionPendingRef: MutableRefObject<Set<string>>;
	queuedCompactionRef: MutableRefObject<Set<string>>;
	staleRunFenceRef: MutableRefObject<Set<string>>;
	allocateNativeSequence: (sessionId: string) => number;
};
