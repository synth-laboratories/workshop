/**
 * Boundary event channel constants.
 * Command names live in `src/renderer/src/generated/protocol.ts` (`commands.*`).
 */

export const EVENT_CHANNELS = {
	/** Single origin-tagged session/runtime stream (Provider | Desktop). */
	RUNTIME: "runtime:event",
	/**
	 * @deprecated Producers no longer emit here. Compat listen only — remove
	 * once no leftover emitters remain.
	 */
	CODEX: "codex:event",
	VISUAL_SHOW: "visual:show",
	TERMINAL: "terminal:event",
	LAGUNA_STATUS: "laguna:status",
	LAGUNA_DOWNLOAD: "laguna:download",
	LAGUNA_INFERENCE: "laguna:inference",
	TRAINING_MODELS_DOWNLOAD: "training-models:download",
	WHISPER_RUNTIME: "whisper:runtime",
	WHISPER_DOWNLOAD: "whisper:download",
	OPTIMIZER_STATUS: "optimizer:status"
} as const;

export type EventChannelName = (typeof EVENT_CHANNELS)[keyof typeof EVENT_CHANNELS];

/** Matches Rust `contract::events::EventOrigin`. */
export const EVENT_ORIGINS = {
	PROVIDER: "provider",
	DESKTOP: "desktop"
} as const;

export type EventOrigin = (typeof EVENT_ORIGINS)[keyof typeof EVENT_ORIGINS];
