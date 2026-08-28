/**
 * `useRunProgress` — the one hook every run-progress surface reads.
 *
 * It owns three things the components must not each reinvent:
 *
 *   · The ownership gate. A card subscribes only after the durable record
 *     confirms the run belongs to this conversation (or to no conversation).
 *     Switching chats unmounts the card, which parks the subscription.
 *   · The elapsed-time clock. One 1s tick per mounted card while the run is
 *     live, stopped the moment the run is terminal, so a finished card is inert.
 *   · Control intent. Pause, resume, and cancel are *requests*. The intent is
 *     held separately from observed state and cleared when the durable record
 *     catches up; nothing optimistically rewrites the run's status.
 *   · Experience-budget telemetry. Time to first progress, update latency, and
 *     estimate coverage are recorded per run and flushed once when it ends.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { projectRunProgress } from "../runtime/runProgress/project";
import { providerAccessFromSecrets } from "../runtime/runProgress/providerAccess";
import {
	resolveOwnedRun,
	subscribeToRun,
	type RunProgressConnectionState,
	type RunProgressSnapshot
} from "../runtime/runProgress/subscription";
import {
	flushRunTelemetry,
	recordSample,
	recordSubscribed
} from "../runtime/runProgress/telemetry";
import type { RunControlIntent, RunProgressProjection } from "../runtime/runProgress/types";

const CLOCK_INTERVAL_MS = 1_000;

export type RunProgressState = {
	projection: RunProgressProjection | null;
	connection: RunProgressConnectionState;
	error?: string;
	/** The run is not this conversation's to watch, or no longer exists. */
	unavailableReason?: string;
	intent: RunControlIntent | null;
	requestControl: (action: RunControlIntent["action"]) => void;
};

export function useRunProgress(runId: string, sessionRef?: string): RunProgressState {
	const [snapshot, setSnapshot] = useState<RunProgressSnapshot | null>(null);
	const [unavailableReason, setUnavailableReason] = useState<string | undefined>();
	const [intent, setIntent] = useState<RunControlIntent | null>(null);
	const [clock, setClock] = useState(() => Date.now());
	const [providerAccess, setProviderAccess] = useState<RunProgressProjection["providerAccess"]>();
	const intentRef = useRef<RunControlIntent | null>(null);
	intentRef.current = intent;

	useEffect(() => {
		let cancelled = false;
		let unsubscribe: (() => void) | undefined;
		setSnapshot(null);
		setUnavailableReason(undefined);
		void resolveOwnedRun(runId, sessionRef).then((run) => {
			if (cancelled) return;
			if (!run) {
				setUnavailableReason(
					"This run is not available in this conversation. It may have been removed, or it belongs to another chat."
				);
				return;
			}
			recordSubscribed(runId, run.algorithmId, Date.now());
			unsubscribe = subscribeToRun(runId, (next) => {
				if (!cancelled) setSnapshot(next);
			});
		});
		return () => {
			cancelled = true;
			unsubscribe?.();
		};
	}, [runId, sessionRef]);

	const terminal = snapshot?.state === "terminal";
	const live = snapshot !== null && !terminal;

	useEffect(() => {
		if (!live) return;
		const timer = window.setInterval(() => setClock(Date.now()), CLOCK_INTERVAL_MS);
		return () => window.clearInterval(timer);
	}, [live]);

	useEffect(() => {
		const secrets = bridges.secrets;
		if (!secrets) return;
		let cancelled = false;
		const load = async () => {
			try {
				const [caps, inbox] = await Promise.all([secrets.capabilities(), secrets.pending()]);
				if (cancelled) return;
				const match = caps.find((cap) => cap.runId === runId);
				const grant = inbox.grants.find((item) => item.runId === runId);
				setProviderAccess(providerAccessFromSecrets({
					terminal,
					capability: match,
					grant,
					proxyRunning: inbox.proxy.running
				}));
			} catch {
				/* Secrets are optional on this surface. */
			}
		};
		void load();
		const timer = window.setInterval(() => void load(), 2500);
		return () => {
			cancelled = true;
			window.clearInterval(timer);
		};
	}, [runId, live, snapshot?.run?.algorithmId]);

	// A terminal run's elapsed time comes from its own timestamps, so freezing
	// the clock cannot change what it shows.
	const projection = useMemo(() => {
		const next = snapshot ? projectRunProgress(snapshot, clock) : null;
		if (!next || !providerAccess) return next;
		return { ...next, providerAccess };
	}, [snapshot, clock, providerAccess]);

	// One measurement per published snapshot, not per clock tick: a 1s elapsed
	// re-render is not new evidence about latency or estimate coverage.
	const measuredRevisionRef = useRef(-1);
	useEffect(() => {
		if (!projection || !snapshot || snapshot.revision === measuredRevisionRef.current) return;
		measuredRevisionRef.current = snapshot.revision;
		const lastEventAt = projection.timing.lastEventAt
			? Date.parse(projection.timing.lastEventAt)
			: Number.NaN;
		recordSample(projection.runId, projection.runKind, {
			etaState: projection.timing.eta?.state,
			stale: projection.stale,
			...(Number.isFinite(lastEventAt) ? { latencyMs: Date.now() - lastEventAt } : {}),
			now: Date.now()
		});
		if (projection.terminal) flushRunTelemetry(projection.runId);
	}, [projection, snapshot]);

	// Clear an intent once the durable record reflects it. A pause the producer
	// declined stays visible as `failed` until the user acts again.
	useEffect(() => {
		const current = intentRef.current;
		if (!current || !projection) return;
		const satisfied =
			(current.action === "pause" && projection.status === "paused") ||
			(current.action === "resume" && projection.status === "running") ||
			(current.action === "cancel" && projection.status === "cancelled");
		if (satisfied) setIntent(null);
	}, [projection?.status]);

	const requestControl = useCallback(
		(action: RunControlIntent["action"]) => {
			const bridge = bridges.optimizers;
			const requested: RunControlIntent = {
				runId,
				action,
				state: "requested",
				requestedAt: Date.now()
			};
			setIntent(requested);
			if (!bridge) {
				setIntent({ ...requested, state: "failed", error: "Optimizer bridge is unavailable" });
				return;
			}
			const call =
				action === "pause" ? bridge.pause(runId)
					: action === "resume" ? bridge.resume(runId)
						: bridge.cancel(runId);
			void call
				.then(() => setIntent((current) =>
					current && current.requestedAt === requested.requestedAt
						? { ...current, state: "acknowledged" }
						: current
				))
				.catch((reason) => setIntent((current) =>
					current && current.requestedAt === requested.requestedAt
						? { ...current, state: "failed", error: publicError(reason) }
						: current
				));
		},
		[runId]
	);

	return {
		projection,
		connection: unavailableReason ? "unavailable" : snapshot?.state ?? "loading",
		error: snapshot?.error,
		unavailableReason,
		intent,
		requestControl
	};
}
