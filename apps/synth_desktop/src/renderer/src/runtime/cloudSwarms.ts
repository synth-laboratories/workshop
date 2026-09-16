import { listen } from "@tauri-apps/api/event";
import { fromGenerated } from "../bridge";
import { commands } from "../generated/protocol";

export type SwarmEventEnvelope = {
	schema_version: "research.swarm_event.v1";
	sequence: string;
	run_id: string;
	ts: string;
	kind: "snapshot" | "transcript" | "heartbeat";
	payload: Record<string, unknown>;
	state_version: string;
	transcript_cursor: string | null;
};

export async function swarmSdkCall(operation: string, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
	return await fromGenerated(commands.cloudSwarmsSdkCall({ operation, payload })) as Record<string, unknown>;
}

export async function watchSwarm(
	swarmId: string,
	onEvent: (event: SwarmEventEnvelope) => void,
	onState: (state: "connected" | "reconnecting" | "closed" | "error", detail?: string) => void,
	resume?: { lastEventId?: string; transcriptCursor?: string }
): Promise<() => void> {
	let disposed = false;
	let terminal = false;
	let subscriptionId: string | null = null;
	let lastEventId = resume?.lastEventId;
	let transcriptCursor = resume?.transcriptCursor;
	let reconnectAttempt = 0;
	let reconnectTimer: number | null = null;
	const terminalStates = new Set(["completed", "failed", "cancelled", "canceled", "succeeded", "timed_out"]);
	const scheduleReconnect = () => {
		if (disposed || terminal || reconnectTimer !== null) return;
		onState("reconnecting");
		const delay = Math.min(5_000, 250 * 2 ** reconnectAttempt++);
		reconnectTimer = window.setTimeout(() => {
			reconnectTimer = null;
			void start(false).catch((reason) => {
				onState("error", reason instanceof Error ? reason.message : "Swarm stream reconnect failed");
				scheduleReconnect();
			});
		}, delay);
	};
	const unlisten = await listen<Record<string, unknown>>("cloud:swarm-event", ({ payload }) => {
		if (payload.subscriptionId !== subscriptionId) return;
		if (payload.kind === "event" && payload.event && typeof payload.event === "object") {
			const event = payload.event as SwarmEventEnvelope;
			lastEventId = event.sequence || lastEventId;
			transcriptCursor = event.transcript_cursor ?? transcriptCursor;
			const state = String(event.payload.status ?? event.payload.state ?? event.payload.swarm_status ?? "").toLowerCase();
			terminal = terminalStates.has(state);
			onEvent(event);
			return;
		}
		if (payload.kind === "closed") {
			if (terminal) onState("closed");
			else scheduleReconnect();
		} else if (payload.kind === "error") {
			onState("error", typeof payload.message === "string" ? payload.message : "Swarm stream failed");
		}
	});
	async function start(initial: boolean) {
		// Pick the correlation id before starting the native task so an
		// immediate first SSE event cannot race the invoke response.
		subscriptionId = `swarm-watch-${crypto.randomUUID()}`;
		const started = await fromGenerated(commands.cloudSwarmsWatchStart({
			subscriptionId,
			swarmId,
			lastEventId: lastEventId ?? null,
			transcriptCursor: transcriptCursor ?? null,
			timeoutSeconds: null
		}));
		subscriptionId = started.subscriptionId;
		reconnectAttempt = 0;
		onState("connected");
		if (!initial && disposed) void fromGenerated(commands.cloudSwarmsWatchStop(subscriptionId));
	}
	try {
		await start(true);
	} catch (error) {
		unlisten();
		throw error;
	}
	return () => {
		disposed = true;
		if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
		unlisten();
		if (subscriptionId) void fromGenerated(commands.cloudSwarmsWatchStop(subscriptionId));
	};
}
