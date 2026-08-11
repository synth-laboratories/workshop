import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const outputs = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const trigger = document.querySelector<HTMLElement>('[data-testid="resource-shelf-trigger"]');
	const shelf = document.querySelector<HTMLElement>('[data-testid="resource-shelf"]');
	const empty = document.querySelector<HTMLElement>('[data-testid="resource-shelf-empty"]');
	const emptyTitle = empty?.querySelector<HTMLElement>("strong") ?? null;
	const emptyCopy = empty?.querySelector<HTMLElement>("span") ?? null;
	const transcript = document.querySelector<HTMLElement>(".chat-transcript-inner");
	const rect = (element: HTMLElement | null) => element?.getBoundingClientRect() ?? null;
	const point = (element: HTMLElement | null) => {
		const value = rect(element);
		return value ? { x: value.left + value.width / 2, y: value.top + value.height / 2 } : null;
	};
	const shelfRect = rect(shelf);
	const transcriptRect = rect(transcript);
	return {
		chatPoint: point(chat),
		fixtureSelected: Boolean(chat?.classList.contains("active")),
		triggerPoint: point(trigger),
		transcriptVisible: Boolean(transcript),
		shelfOpen: Boolean(shelf),
		emptyShelfOpen: Boolean(shelf && empty),
		emptyCopyReadable: !emptyTitle || !emptyCopy || (() => {
			const titleRect = emptyTitle.getBoundingClientRect();
			const copyRect = emptyCopy.getBoundingClientRect();
			return titleRect.bottom <= copyRect.top - 4 || titleRect.right <= copyRect.left - 6;
		})(),
		shelfAvoidsTranscript: !shelfRect || !transcriptRect || (
			shelfRect.right <= transcriptRect.left || shelfRect.left >= transcriptRect.right
			|| shelfRect.bottom <= transcriptRect.top || shelfRect.top >= transcriptRect.bottom
		)
	};
});

export const open_empty_outputs_fixture = actions(() => {
	if (!outputs.current.fixtureSelected && outputs.current.chatPoint) {
		return [{ Click: { name: "Open empty-output fixture chat", point: outputs.current.chatPoint } }];
	}
	if (!outputs.current.shelfOpen && outputs.current.triggerPoint) {
		return [{ Click: { name: "Open empty Outputs shelf", point: outputs.current.triggerPoint } }];
	}
	return ["Wait"];
});

export const empty_outputs_state_is_exercised = eventually(() =>
	outputs.current.emptyShelfOpen
).within(8, "seconds");

/** CUA: an empty Outputs popover obscured readable transcript content. */
export const empty_outputs_never_opens_a_transcript_obscuring_popover = always(() =>
	!outputs.current.emptyShelfOpen && outputs.current.shelfAvoidsTranscript
);

/** Empty-state title and explanatory copy must remain visibly separated. */
export const empty_outputs_copy_never_collides = always(() =>
	!outputs.current.emptyShelfOpen || outputs.current.emptyCopyReadable
);
