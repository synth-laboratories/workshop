import type { LocalActivityLine, LocalChat } from "../../types/landing";
import { MANDER_STATES, type ManderState } from "./Mander.types";

export function isManderState(value: unknown): value is ManderState {
	return typeof value === "string" && (MANDER_STATES as readonly string[]).includes(value);
}

export function sessionHasOpenTools(chat: LocalChat | null | undefined): boolean {
	if (!chat) return false;
	return Object.values(chat.activityByMessageId ?? {}).flat().some(lineIsOpenTool);
}

function lineIsOpenTool(line: LocalActivityLine): boolean {
	if (line.toolStatus === "running") return true;
	if (line.toolStatus === "completed" || line.toolStatus === "failed") return false;
	return line.kind === "command"
		|| line.kind === "file_read"
		|| line.kind === "file_write"
		|| line.kind === "search"
		|| line.kind === "visual"
		|| line.kind === "working";
}

/**
 * Host default wins while a turn is running. Otherwise the MCP overlay sticks,
 * including `success` after the turn completes. Idle is the rest state.
 */
export function resolveManderEmotion(input: {
	running: boolean;
	toolsOpen?: boolean;
	overlay?: unknown;
}): ManderState {
	if (input.running) return input.toolsOpen ? "working" : "thinking";
	if (isManderState(input.overlay)) return input.overlay;
	return "idle";
}

export function presentationSummary(metadata: Record<string, unknown> | undefined): string | null {
	const value = metadata?.presentationSummary;
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	return trimmed || null;
}
