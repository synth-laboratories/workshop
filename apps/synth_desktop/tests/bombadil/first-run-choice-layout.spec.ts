import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const firstRunChoice = extract((state: any) => {
	const document = state.document;
	const choice = document.querySelector<HTMLElement>('[data-testid="first-run-account-choice"]');
	const cards = choice ? [...choice.querySelectorAll<HTMLElement>(".quick-card")] : [];
	const cardsReadable = cards.every((card) => {
		const copy = card.querySelector<HTMLElement>(":scope > span");
		const title = copy?.querySelector<HTMLElement>("strong") ?? null;
		const caption = copy?.querySelector<HTMLElement>("small") ?? null;
		if (!copy || !title || !caption) return false;
		const cardRect = card.getBoundingClientRect();
		const copyRect = copy.getBoundingClientRect();
		const titleRect = title.getBoundingClientRect();
		const captionRect = caption.getBoundingClientRect();
		return copyRect.width >= Math.min(170, cardRect.width * 0.62)
			&& titleRect.height <= 24
			&& captionRect.height <= 22
			&& copyRect.left >= cardRect.left
			&& copyRect.right <= cardRect.right;
	});
	return {
		visible: Boolean(choice),
		hasBothCards: cards.length === 2,
		cardsReadable,
		contained: !choice || choice.getBoundingClientRect().right <= state.window.innerWidth
	};
});

export const fuzz_first_run_choice_widths = actions(() => [
	{ SetViewport: { width: 960, height: 640 } },
	{ SetViewport: { width: 1172, height: 768 } },
	{ SetViewport: { width: 1280, height: 840 } }
]);

export const first_run_account_choice_is_exercised = eventually(() =>
	firstRunChoice.current.visible && firstRunChoice.current.hasBothCards
).within(8, "seconds");

/** First-run choices must reserve readable copy width rather than a 34px icon column. */
export const first_run_choice_copy_never_collapses_into_vertical_fragments = always(() =>
	!firstRunChoice.current.visible || (firstRunChoice.current.cardsReadable && firstRunChoice.current.contained)
);
