import type { ChatMessage } from "../types/landing";

const ROLE_LABELS: Record<ChatMessage["role"], string> = {
	user: "User",
	assistant: "Assistant",
	system: "System"
};

/** Serialize the visible conversation without inventing unavailable metadata. */
export function conversationMarkdown(title: string, messages: ChatMessage[]): string {
	const heading = `# ${title.trim() || "Untitled chat"}`;
	const turns = messages
		.filter((message) => message.body.trim().length > 0)
		.map((message) => `## ${ROLE_LABELS[message.role]}\n\n${message.body.trim()}`);
	return [heading, ...turns].join("\n\n");
}
