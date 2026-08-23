import type { Session } from "@synth/runtime-protocol";
import type { LocalChat } from "../../types/landing";
import { Mander } from "./Mander";
import { presentationSummary, resolveManderEmotion, sessionHasOpenTools } from "./Mander.presence";

type Props = {
	session?: Session;
	chat?: LocalChat;
	running?: boolean;
};

export function ManderPresence({ session, chat, running = false }: Props) {
	const state = resolveManderEmotion({
		running,
		toolsOpen: sessionHasOpenTools(chat),
		overlay: session?.metadata.presentationEmotion
	});
	const summary = presentationSummary(session?.metadata);

	return (
		<div className="mander-presence" data-testid="mander-presence" data-mander-emotion={state}>
			<Mander state={state} size={64} label="Session mascot" />
			{summary ? (
				<p className="mander-presence-summary" data-testid="mander-presence-summary">{summary}</p>
			) : null}
		</div>
	);
}
