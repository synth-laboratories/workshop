import { useEffect, useRef, type MutableRefObject } from "react";
import type { Session } from "@synth/runtime-protocol";
import {
	codexResumeRequest,
	isCodexCompactionEvent
} from "../runtime/codexTurn";
import { codexEventToRuntime } from "../runtime/nativeCodex";
import { dispatchRuntimeEvent } from "../stores/sessionStore";
import type { CodexBridge, CodexEvent } from "../bridge";
import type { DesktopPreferences } from "../preferences";

export type CodexUsageSnapshot = {
	usedPercent: number;
	resetsAt: number;
	windowMinutes?: number;
	planType?: string;
};

function codexUsageFromEvent(event: { method: string; params: Record<string, unknown> }): CodexUsageSnapshot | null {
	if (event.method !== "token_count") return null;
	const info = event.params.info;
	if (!info || typeof info !== "object") return null;
	const rateLimits = (info as Record<string, unknown>).rate_limits;
	if (!rateLimits || typeof rateLimits !== "object") return null;
	const limits = rateLimits as Record<string, unknown>;
	const primary = limits.primary;
	if (!primary || typeof primary !== "object") return null;
	const window = primary as Record<string, unknown>;
	const usedPercent = window.used_percent;
	const resetsAt = window.resets_at;
	if (typeof usedPercent !== "number" || !Number.isFinite(usedPercent) || typeof resetsAt !== "number" || !Number.isFinite(resetsAt)) return null;
	return {
		usedPercent: Math.max(0, Math.min(100, usedPercent)),
		resetsAt,
		windowMinutes: typeof window.window_minutes === "number" ? window.window_minutes : undefined,
		planType: typeof limits.plan_type === "string" ? limits.plan_type : undefined
	};
}

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
	/** A native event proves that an accepted turn actually began making progress. */
	onTurnActivity?: (sessionId: string) => void;
	onRawEvent?: (event: CodexEvent) => void;
	onOauthReauthRequired?: () => void;
	onCodexUsage?: (usage: CodexUsageSnapshot) => void;
}): void {
	const { nativeCodex } = args;
	// The Tauri listener is installed asynchronously. Reinstalling it whenever an
	// inline callback changes creates a real event-loss window after every event:
	// dispatch -> React render -> unlisten -> async listen. Keep one transport
	// subscription and let it read the current handlers/configuration instead.
	const currentRef = useRef(args);
	currentRef.current = args;

	useEffect(() => {
		if (!nativeCodex) return;
		return nativeCodex.onEvent((event) => {
			const {
				allocateNativeSequence,
				sessionsRef,
				manualCompactionPendingRef,
				queuedCompactionRef,
				staleRunFenceRef,
				autoCompactTokenLimits,
				localBaseUrl,
				showToast,
				onTurnActivity,
				onRawEvent,
				onOauthReauthRequired,
				onCodexUsage
			} = currentRef.current;
			onTurnActivity?.(event.sessionId);
			onRawEvent?.(event);
			const usage = codexUsageFromEvent(event);
			if (usage) onCodexUsage?.(usage);
			if (event.method === "turn/failed" && event.params.code === "codex_oauth_reauth_required") {
				onOauthReauthRequired?.();
				showToast("Reconnect ChatGPT subscription in Settings → Models");
			}
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
	}, [nativeCodex]);
}
