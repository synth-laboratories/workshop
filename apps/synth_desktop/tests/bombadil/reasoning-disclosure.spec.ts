import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * Reasoning is a deliberate disclosure, not ambient tool activity. This
 * exercises the seeded local chat through the normal renderer and guards the
 * geometry that most easily regresses when a disclosure grows while the dock
 * is visible.
 */
const reasoning = extract((state: any) => {
	const document = state.document;
	const viewport = state.window;
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
	const disclosure = document.querySelector<HTMLElement>(".reasoning-disclosure");
	const toggle = disclosure?.querySelector<HTMLButtonElement>("button") ?? null;
	const detail = disclosure?.querySelector<HTMLElement>(".local-activity-detail") ?? null;
	const disclosureRect = disclosure?.getBoundingClientRect() ?? null;
	const composerRect = composer?.getBoundingClientRect() ?? null;
	const style = disclosure ? getComputedStyle(disclosure) : null;
	const inTranscript = !disclosureRect || !transcript || (() => {
		const rect = transcript.getBoundingClientRect();
		return disclosureRect.left >= rect.left && disclosureRect.right <= rect.right + 1;
	})();
	return {
		hasDisclosure: Boolean(disclosure),
		disclosureIsAButton: Boolean(toggle && toggle.getAttribute("aria-expanded") !== null),
		inTranscript,
		collapsedDisclosureIsCompact: !disclosure || toggle?.getAttribute("aria-expanded") === "true" || disclosureRect!.height <= 48,
		disclosureIsNotACard: !style || (style.borderTopWidth === "0px" && style.paddingTop === "0px" && style.backgroundImage === "none"),
		detailClearsComposer: !detail || !composerRect || detail.getBoundingClientRect().bottom <= composerRect.top - 12,
		noHorizontalOverflow: document.documentElement.scrollWidth <= viewport.innerWidth + 1
	};
});

export const exercise_reasoning_disclosure = actions(() => {
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1280, height: 840 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

// The normal Bombadil fixture has no model generation, so this records the
// invariant shape whenever one is present without pretending that a thought
// exists in every conversation.
export const reasoning_disclosure_is_semantic = always(() =>
	!reasoning.current.hasDisclosure || reasoning.current.disclosureIsAButton
);

export const reasoning_disclosure_stays_in_the_transcript = always(() =>
	!reasoning.current.hasDisclosure || reasoning.current.inTranscript
);

export const collapsed_reasoning_is_a_compact_dropdown_not_a_card = always(() =>
	!reasoning.current.hasDisclosure || reasoning.current.collapsedDisclosureIsCompact
);

export const reasoning_disclosure_has_no_card_chrome = always(() =>
	!reasoning.current.hasDisclosure || reasoning.current.disclosureIsNotACard
);

export const expanded_reasoning_never_covers_the_composer = always(() =>
	!reasoning.current.hasDisclosure || reasoning.current.detailClearsComposer
);

export const reasoning_surfaces_never_create_horizontal_overflow = always(() =>
	reasoning.current.noHorizontalOverflow
);
