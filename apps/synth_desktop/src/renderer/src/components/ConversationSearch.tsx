import { useEffect, useMemo, useRef, useState } from "react";
import type { LandingState } from "../types/landing";

/*
 * v0.1 removal contract: Intern sync/async sessions are de-scoped, so search
 * indexes local conversations only. Re-adding a kind here re-exposes Intern.
 */
type Result = { id: string; title: string; detail: string; haystack: string; kind: "chat" };

type Props = {
	state: LandingState;
	onClose: (options?: { restoreFocus?: boolean }) => void;
	onOpenChat: (id: string) => void;
};

export function ConversationSearch({ state, onClose, onOpenChat }: Props) {
	const [query, setQuery] = useState("");
	const inputRef = useRef<HTMLInputElement>(null);
	useEffect(() => inputRef.current?.focus(), []);

	const results = useMemo<Result[]>(() => {
		const all: Result[] = state.chats.map((chat) => ({
			id: chat.id, title: chat.title, detail: "Local chat", kind: "chat" as const,
			haystack: `${chat.title} ${chat.messages.map((message) => message.body).join(" ")}`
		}));
		const normalized = query.trim().toLowerCase();
		return normalized ? all.filter((result) => result.haystack.toLowerCase().includes(normalized)) : all;
	}, [query, state]);

	const open = (result: Result) => {
		onClose({ restoreFocus: false });
		onOpenChat(result.id);
	};

	return (
		<div
			className="search-scrim"
			role="presentation"
			data-testid="search-scrim"
			onClick={(event) => {
				if (event.target === event.currentTarget) onClose();
			}}
		>
			<div className="conversation-search" role="dialog" aria-modal="true" aria-label="Search conversations" aria-keyshortcuts="Escape Meta+K" data-testid="conversation-search">
				<label className="conversation-search-input">
					<span aria-hidden>⌕</span>
					<input
						ref={inputRef}
						type="search"
						value={query}
						onChange={(event) => setQuery(event.target.value)}
						onKeyDown={(event) => {
							if (event.key === "Escape") onClose();
							if (event.key === "Enter" && results[0]) open(results[0]);
						}}
						placeholder="Search conversations…"
						aria-label="Search conversations"
					/>
					<kbd>⌘K</kbd>
					<button type="button" className="conversation-search-close" onClick={() => onClose()} aria-label="Close search">
						<span aria-hidden>×</span>
					</button>
				</label>
				<div className="conversation-results" role="listbox" aria-label="Conversations">
					{results.map((result, index) => (
						<button type="button" role="option" aria-selected={index === 0} key={result.id} onClick={() => open(result)}>
							<span className="search-result-icon" aria-hidden>◎</span>
							<span><strong>{result.title}</strong><small>{result.detail}</small></span>
						</button>
					))}
					{results.length === 0 ? <p>No conversations found.</p> : null}
				</div>
			</div>
		</div>
	);
}
