/**
 * Session orchestration seam (Wave 3 stub).
 *
 * Hosts will grow prompt-queue drain, compaction scheduling, and ensure-active
 * session flows here. For now it re-exports the session store writers so App
 * can migrate call sites without inventing a second status path.
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
	upsertSession
} from "../stores/sessionStore";
