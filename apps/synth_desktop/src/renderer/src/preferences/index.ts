export {
	PREFERENCES_STORAGE_KEY,
	PREFERENCES_SCHEMA_VERSION,
	DEFAULT_PREFERENCES,
	DEFAULT_LAYOUT,
	normalizePreferences,
	normalizeLayoutSnapshot,
	migrateLegacyPreferences,
	clampNumber,
	type DesktopPreferences,
	type LayoutSnapshot,
	type ToolActivityMode,
	type ActiveEnterAction,
	type ThemePreference,
	type ConversationMeta,
	type QueuedPrompt
} from "./schema";

export {
	loadPreferences,
	getPreferences,
	updatePreferences,
	resetPreferences,
	subscribePreferences,
	setTheme,
	setToolActivityMode,
	setActiveEnterAction,
	setAppearanceFonts,
	saveLayout,
	saveLayoutAsDefault,
	resetLayoutToDefault,
	applyDefaultLayout,
	setUnreadCompletedChats,
	setApprovalModePreference,
	renameConversation,
	pinConversation,
	archiveConversation,
	listArchivedConversationIds,
	enqueuePrompt,
	updateQueuedPrompt,
	removeQueuedPrompt,
	promptsForConversation,
	nextQueuedPrompt,
	applyPreferencesToDocument,
	preferencesAdapter
} from "./store";

export {
	presentActivityLines,
	activityStatusAnnouncement,
	type ActivityPresentationItem
} from "./activityPresentation";
