"use client";

import {
	useEffect,
	useState
} from "react";

import {
	acquireOrRenewInternSyncPresence,
	createInternIdempotencyKey,
	type InternSyncPresenceLease,
	type InternSyncPresenceLeaseRequest,
	releaseInternSyncPresence,
	ResearchInternApiError
} from "@/lib/researchIntern";

import {
	allocateSyncPresenceGeneration,
	retainSyncPresenceGeneration
} from "./syncPresenceGeneration";

const RENEW_RETRY_MS = 5_000;
const MAX_RENEW_DELAY_MS = 20_000;
const EXPIRY_SAFETY_MS = 15_000;

export type SyncPresenceState = {
	status: "disabled" | "connecting" | "present" | "reconnecting" | "conflict";
	lease: InternSyncPresenceLease | null;
	error: string | null;
};

function renewDelay(lease: InternSyncPresenceLease): number {
	const remaining = Date.parse(lease.expires_at) - Date.now() - EXPIRY_SAFETY_MS;

	return Math.max(
		1_000,
		Math.min(
			MAX_RENEW_DELAY_MS,
			remaining
		)
	);
}

/** Owns the selected live Sync session's authenticated browser-presence lease. */
export function useSyncPresence(syncSessionId: string | null): SyncPresenceState {
	const [state, setState] = useState<SyncPresenceState>({
		status: syncSessionId
			? "connecting"
			: "disabled",
		lease: null,
		error: null
	});

	useEffect(
		() => {
			if (!syncSessionId) {
				setState({ status: "disabled",lease: null,error: null });

				return;
			}
			let stopped = false;
			let released = false;
			let timer: ReturnType<typeof setTimeout> | null = null;
			let request: InternSyncPresenceLeaseRequest;
			try {
				request = {
					connection_id: createInternIdempotencyKey("sync-presence"),
					connection_generation: allocateSyncPresenceGeneration(
						syncSessionId,
						window.localStorage
					)
				};
			} catch (error) {
				setState({
					status: "reconnecting",
					lease: null,
					error: error instanceof Error
						? error.message
						: "Sync presence connection identity could not be created."
				});

				return;
			}

			setState({ status: "connecting",lease: null,error: null });

			const renew = async () => {
				try {
					const lease = await acquireOrRenewInternSyncPresence(
						syncSessionId,
						request
					);
					if (stopped) return;
					request.connection_generation = lease.connection_generation;
					retainSyncPresenceGeneration(
						syncSessionId,
						lease.connection_generation,
						window.localStorage
					);
					setState({ status: "present",lease,error: null });
					timer = setTimeout(
						() => void renew(),
						renewDelay(lease)
					);
				} catch (error) {
					if (stopped) return;
					const conflict = error instanceof ResearchInternApiError && error.kind === "conflict";
					setState({
						status: conflict
							? "conflict"
							: "reconnecting",
						lease: null,
						error: conflict
							? "This Sync session is operator-present in another browser connection."
							: error instanceof Error
								? error.message
								: "Sync operator presence could not be renewed."
					});
					if (!conflict) {
						timer = setTimeout(
							() => void renew(),
							RENEW_RETRY_MS
						);
					}
				}
			};

			const release = () => {
				if (released) return;
				released = true;
				void releaseInternSyncPresence(
					syncSessionId,
					request,
					{ keepalive: true }
				)
					.catch(() => undefined);
			};

			window.addEventListener(
				"pagehide",
				release
			);
			void renew();

			return () => {
				stopped = true;
				if (timer) clearTimeout(timer);
				window.removeEventListener(
					"pagehide",
					release
				);
				release();
			};
		},
		[syncSessionId]
	);

	return state;
}
