import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const failedTurn = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const retry = document.querySelector<HTMLElement>('[data-testid="send-retry-button"]')
		?? [...(transcript?.querySelectorAll<HTMLElement>("button") ?? [])].find((button) =>
			/retry|try again/i.test(`${button.textContent ?? ""} ${button.getAttribute("aria-label") ?? ""}`)
		) ?? null;
	const chatRect = chat?.getBoundingClientRect() ?? null;
	const retryRect = retry?.getBoundingClientRect() ?? null;
	const transcriptRect = transcript?.getBoundingClientRect() ?? null;
	const text = (transcript?.textContent ?? "").replace(/\s+/g, " ").trim();
	return {
		chatPoint: chat && !chat.classList.contains("active") && chatRect
			? { x: chatRect.left + chatRect.width / 2, y: chatRect.top + chatRect.height / 2 }
			: null,
		failed: /provider could not produce a response|stopped with an error/i.test(text),
		asksToTryAgain: /Try again/i.test(text),
		retryVisible: Boolean(retryRect && retryRect.width >= 40 && retryRect.height >= 24),
		retryOwnedByTranscript: Boolean(retryRect && transcriptRect
			&& retryRect.left >= transcriptRect.left && retryRect.right <= transcriptRect.right
			&& retryRect.top >= transcriptRect.top && retryRect.bottom <= transcriptRect.bottom)
	};
});

export const open_failed_turn_fixture = actions(() =>
	failedTurn.current.chatPoint
		? [{ Click: { name: "Open failed-turn fixture", point: failedTurn.current.chatPoint } }]
		: ["Wait"]
);

export const failed_turn_fixture_is_exercised = eventually(() =>
	failedTurn.current.failed && failedTurn.current.asksToTryAgain
).within(8, "seconds");

/** If the transcript tells the user to retry, the recovery action must exist beside it. */
export const failed_turn_try_again_copy_always_has_a_visible_owned_retry_action = always(() =>
	!failedTurn.current.failed
	|| !failedTurn.current.asksToTryAgain
	|| (failedTurn.current.retryVisible && failedTurn.current.retryOwnedByTranscript)
);
