import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * Honest completed turns. The 2026-08-10 CUA screenshot showed:
 *   user "hello" → "Worked 11s" → blank area → "Reasoned"
 *   composer chip: "Unavailable tok/s observed p50"
 *
 * A turn that claims Worked must leave a visible assistant answer or an
 * explicit failure/system explanation — never a successful-looking blank.
 * Throughput chips must never print the literal Unavailable token rate.
 *
 * Fixture is injected by tests/bombadil/run.mjs when this spec is selected.
 * This run is expected to FAIL until the product stops rendering that state.
 *
 * BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/empty-completed-turn.spec.ts \
 *   npm run test:bombadil:empty-turn --workspace @synth/synth-desktop
 */
const honesty = extract((state: any) => {
	const document = state.document;
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const chatRow = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const chatRect = chatRow?.getBoundingClientRect();
	const workedSummaries = transcript
		? [...transcript.querySelectorAll<HTMLElement>(".run-summary")].filter((node) =>
			/\bWorked\b/i.test(node.textContent ?? "")
		)
		: [];
	const assistantBodies = transcript
		? [...transcript.querySelectorAll<HTMLElement>(".local-assistant")].map((node) => {
			const clone = node.cloneNode(true) as HTMLElement;
			clone.querySelector(".message-actions")?.remove();
			return (clone.textContent ?? "").replace(/\s+/g, " ").trim();
		})
		: [];
	const hasVisibleAssistantAnswer = assistantBodies.some((body) => body.length > 0);
	const hasHonestFailureCopy = Boolean(
		transcript && /could not produce a response|without a response|Try again|Stopped with an error/i
			.test(transcript.textContent ?? "")
	);
	const blankSuccessfulTurn = workedSummaries.length > 0
		&& !hasVisibleAssistantAnswer
		&& !hasHonestFailureCopy;
	const composerText = composer?.textContent ?? "";
	const unavailableThroughputChip = /Unavailable\s+tok\/s/i.test(composerText);
	return {
		shellReady: Boolean(document.querySelector(".app-shell")),
		transcriptReady: Boolean(transcript),
		chatPoint: chatRect && chatRect.width > 0 && chatRect.height > 0
			? { x: chatRect.left + chatRect.width / 2, y: chatRect.top + chatRect.height / 2 }
			: null,
		blankSuccessfulTurn,
		unavailableThroughputChip,
		workedCount: workedSummaries.length,
		reasonedVisible: Boolean(
			transcript && /\bReasoned\b/i.test(transcript.textContent ?? "")
		)
	};
});

export const open_blank_worked_fixture_chat = actions(() =>
	honesty.current.chatPoint
		? [
			{ Click: { name: "Open blank Worked fixture chat", point: honesty.current.chatPoint } },
			"Wait",
			"Wait"
		]
		: ["Wait"]
);

/** Fixture must actually paint the dishonest completed turn. */
export const blank_worked_fixture_is_reachable = eventually(() =>
	honesty.current.workedCount > 0 && honesty.current.blankSuccessfulTurn
);

/**
 * Core honesty lock — fails while Worked+blank(+Reasoned) is on screen.
 * Expected RED against the injected CUA fixture until product is fixed.
 */
export const completed_turns_never_look_successful_when_blank = always(() =>
	honesty.current.workedCount === 0 || !honesty.current.blankSuccessfulTurn
);

export const composer_never_advertises_unavailable_tok_s = always(() =>
	!honesty.current.unavailableThroughputChip
);
