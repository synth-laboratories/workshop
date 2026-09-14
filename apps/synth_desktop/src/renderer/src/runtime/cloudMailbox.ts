import { fromGenerated } from "../bridge/invoke";
import {
	commands,
	type MailboxConnectionView,
	type MailboxOutboxRowView,
	type MailboxSignOutView,
	type MailboxStatusView,
	type ScopeView
} from "../generated/protocol";

/**
 * Native mailbox bridge. Every host command refuses while the cloud host is
 * qualification-gated; the renderer never receives a credential.
 */
export const cloudMailbox = {
	view: (): Promise<ScopeView> => fromGenerated(commands.cloudScopeView()),
	connections: (): Promise<MailboxConnectionView[]> => fromGenerated(commands.cloudMailboxConnections()),
	status: (threadId: string): Promise<MailboxStatusView> => fromGenerated(commands.cloudMailboxStatus(threadId)),
	answer: (threadId: string, messageId: string, body: string): Promise<MailboxOutboxRowView> =>
		fromGenerated(commands.cloudMailboxAnswer(threadId, messageId, body)),
	decline: (threadId: string, messageId: string, reason: string): Promise<MailboxOutboxRowView> =>
		fromGenerated(commands.cloudMailboxDecline(threadId, messageId, reason)),
	signOut: (): Promise<MailboxSignOutView> => fromGenerated(commands.cloudMailboxSignOut())
};
