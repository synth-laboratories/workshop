import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const alignment = extract((state: any) => {
	const document = state.document;
	const rect = (selector: string) =>
		document.querySelector<HTMLElement>(selector)?.getBoundingClientRect() ?? null;
	const point = (value: DOMRect | null) => value
		? { x: value.left + value.width / 2, y: value.top + value.height / 2 }
		: null;
	const centerX = (value: DOMRect | null) => value ? value.left + value.width / 2 : null;
	const fixtureChat = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const fixtureChatRect = fixtureChat?.getBoundingClientRect() ?? null;
	const chat = rect('[data-testid="chat-transcript"]');
	const transcript = rect(".chat-transcript-inner");
	const composer = rect('[data-testid="composer"]');
	const trigger = rect('[data-testid="resource-shelf-trigger"]');
	const activity = rect('[data-testid="activity-mode-menu-trigger"]');
	const toolbar = rect('[data-testid="transcript-toolbar"]');
	const shelf = rect('[data-testid="resource-shelf"]');
	const triggerElement = document.querySelector<HTMLElement>('[data-testid="resource-shelf-trigger"]');
	const tolerance = 3;
	const transcriptCenter = centerX(transcript);
	const composerCenter = centerX(composer);
	return {
		fixturePoint: point(fixtureChatRect),
		triggerPoint: point(trigger),
		shelfOpen: Boolean(shelf),
		triggerStateMatchesPanel:
			!triggerElement || triggerElement.getAttribute("aria-expanded") === String(Boolean(shelf)),
		triggerInsideChat: !trigger || !chat || (
			trigger.left >= chat.left && trigger.right <= chat.right &&
			trigger.top >= chat.top && trigger.bottom <= chat.bottom &&
			chat.right - trigger.right >= 8 && chat.right - trigger.right <= 24
		),
		toolbarControlsDoNotOverlap: !trigger || !activity || !toolbar || (
			activity.right <= trigger.left - 4 &&
			activity.top >= toolbar.top && activity.bottom <= toolbar.bottom &&
			trigger.top >= toolbar.top && trigger.bottom <= toolbar.bottom
		),
		shelfInsideChat: !shelf || !chat || (
			shelf.left >= chat.left && shelf.right <= chat.right &&
			shelf.top >= chat.top && shelf.bottom <= chat.bottom
		),
		shelfAlignedToTrigger: !shelf || !trigger || (
			Math.abs(shelf.right - trigger.right) <= tolerance &&
			shelf.top >= trigger.bottom + 4
		),
		shelfClearsComposer: !shelf || !composer || shelf.bottom <= composer.top - 12,
		composerAlignedToTranscript:
			composerCenter === null || transcriptCenter === null ||
			Math.abs(composerCenter - transcriptCenter) <= tolerance,
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

/** Reach the seeded chat, expand Outputs, then hold it open while resizing. */
export const open_outputs_and_exercise_supported_viewports = actions(() => {
	if (!alignment.current.triggerPoint && alignment.current.fixturePoint) {
		return [{ Click: { name: "Open visual alignment fixture", point: alignment.current.fixturePoint } }];
	}
	if (alignment.current.triggerPoint && !alignment.current.shelfOpen) {
		return [{ Click: { name: "Open Outputs for alignment", point: alignment.current.triggerPoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1280, height: 840 } },
		{ SetViewport: { width: 1440, height: 900 } }
	];
});

export const outputs_panel_is_exercised = eventually(() =>
	alignment.current.shelfOpen
).within(8, "seconds");

export const outputs_expanded_state_matches_the_panel = always(() =>
	alignment.current.triggerStateMatchesPanel
);

export const outputs_trigger_stays_aligned_inside_chat = always(() =>
	alignment.current.triggerInsideChat
);

export const transcript_toolbar_controls_never_overlap = always(() =>
	alignment.current.toolbarControlsDoNotOverlap
);

export const outputs_panel_stays_aligned_inside_chat = always(() =>
	alignment.current.shelfInsideChat && alignment.current.shelfAlignedToTrigger
);

export const outputs_panel_never_covers_the_composer = always(() =>
	alignment.current.shelfClearsComposer
);

export const transcript_and_composer_keep_the_same_centerline = always(() =>
	alignment.current.composerAlignedToTranscript
);

export const aligned_surfaces_never_create_horizontal_overflow = always(() =>
	alignment.current.noHorizontalOverflow
);
