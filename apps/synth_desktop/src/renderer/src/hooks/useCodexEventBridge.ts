import { useEffect, type MutableRefObject } from "react";
import type { Session } from "@synth/runtime-protocol";
import {
	codexResumeRequest,
	isCodexCompactionEvent
} from "../runtime/codexTurn";
import { codexEventToRuntime } from "../runtime/nativeCodex";
import { dispatchRuntimeEvent } from "../stores/sessionStore";
import type { CodexBridge } from "../env";
import type { DesktopPreferences } from "../preferences";

export function useCodexEventBridge(args: {
	nativeCodex: CodexBridge | undefined;
	allocateNativeSequence: (sessionId: string) => number;
	sessionsRef: MutableRefObject<Session[]>;
	manualCompactionPendingRef: MutableRefObject<Set<string>>;
	queuedCompactionRef: MutableRefObject<Set<string>>;
	staleRunFenceRef: MutableRefObject<Set<string>>;
	autoCompactTokenLimits: DesktopPreferences["agentContext"]["autoCompactTokenLimits"];
	localBaseUrl: string | undefined;
	showToast: (message: string) => void;
}): void {
	const {
		nativeCodex,
		allocateNativeSequence,
		sessionsRef,
		manualCompactionPendingRef,
		queuedCompactionRef,
		staleRunFenceRef,
		autoCompactTokenLimits,
		localBaseUrl,
		showToast
	} = args;

	useEffect(() => {
		if (!nativeCodex) return;
		return nativeCodex.onEvent((event) => {
			const manualCompaction =
				isCodexCompactionEvent(event) &&
				manualCompactionPendingRef.current.delete(event.sessionId);
			const normalizedEvent = manualCompaction
				? { ...event, params: { ...event.params, source: "manual" } }
				: event;
			const sequence = allocateNativeSequence(event.sessionId);
			const runtimeEvent = codexEventToRuntime(normalizedEvent, sequence);
			const updatedThreadName =
				event.method === "thread/name/updated" && typeof event.params.threadName === "string"
					? event.params.threadName.trim()
					: null;
			const fenced =
				runtimeEvent.eventKind === "run.started" &&
				staleRunFenceRef.current.has(event.sessionId);
			if (runtimeEvent.eventKind === "run.failed" || runtimeEvent.eventKind === "run.cancelled") {
				manualCompactionPendingRef.current.delete(event.sessionId);
			}
			if (
				(runtimeEvent.eventKind === "run.completed" ||
					runtimeEvent.eventKind === "run.failed" ||
					runtimeEvent.eventKind === "run.cancelled") &&
				queuedCompactionRef.current.delete(event.sessionId) &&
				nativeCodex.compact
			) {
				manualCompactionPendingRef.current.add(event.sessionId);
				const session = sessionsRef.current.find((candidate) => candidate.id === event.sessionId);
				if (!session) return;
				void codexResumeRequest(nativeCodex, session, autoCompactTokenLimits, localBaseUrl)
					.then((request) => nativeCodex.compact!(request))
					.then(() => showToast("Compacting context…"))
					.catch((reason) => {
						manualCompactionPendingRef.current.delete(event.sessionId);
						showToast(reason instanceof Error ? reason.message : String(reason));
					});
			}
			dispatchRuntimeEvent(runtimeEvent, {
				fenced,
				title: updatedThreadName
			});
		});
	}, [
		allocateNativeSequence,
		autoCompactTokenLimits,
		localBaseUrl,
		manualCompactionPendingRef,
		nativeCodex,
		queuedCompactionRef,
		sessionsRef,
		showToast,
		staleRunFenceRef
	]);
}
