/**
 * Session orchestration seam (Wave 3).
 *
 * Owns the prompt-queue drain helper and re-exports session store writers so
 * App can migrate call sites without inventing a second status path. Compaction
 * scheduling and ensure-active session flows continue to land here.
 */

export {
	dispatchLocalSessionStatus,
	dispatchRuntimeEvent,
	dispatchTurnAccepted,
	getEventsBySession,
	getSessions,
	getSessionStoreSnapshot,
	mergeInternSessions,
	mergeSessionReplay,
	patchSessionMetadata,
	replaceSessionEvents,
	replaceSessions,
	resetSessionStore,
	upsertSession,
	useSession,
	useSessionEvents,
	useSessionRunning,
	useSessions,
	useWorkingChatIds
} from "../stores/sessionStore";

export {
	drainPromptQueues,
	nextQueuedPrompt,
	removeQueuedPrompt,
	type PromptQueueDrainDeps,
	type PromptQueueDrainRefs
} from "./promptQueue";

export {
	buildLandingState,
	buildSessionViewSlice,
	sessionIsAsync,
	sessionIsLocalChat,
	sessionIsSync
} from "./sessionView";
