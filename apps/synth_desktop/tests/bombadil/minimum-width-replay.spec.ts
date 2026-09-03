/**
 * The artifact pane stays usable at the narrowest supported width.
 *
 * Reviewing a Craftax replay at a narrow pane, agents repeatedly found the chat
 * composer sitting over the gameplay surface, and got a clean capture only by
 * widening to 1280 — which crosses the 900px breakpoint into the side-by-side
 * layout. Certification therefore never reviewed the compact layout at all; it
 * reviewed a different one and called it compact.
 *
 * `.composer-dock` is absolutely positioned against the bottom of the main
 * pane, so anything docked below the transcript sits underneath it.
 *
 * Fixture: the visual-alignment session and visual seeded by
 * tests/bombadil/run.mjs. (The grouped Craftax fixture would be a better
 * subject, but it renders a blank page on this base — see the return doc.)
 */

import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/** The stacked breakpoint, and a width comfortably inside it. Both are
 * supported sizes, so both are reviewed here rather than escaped from. */
const AT_BREAKPOINT = { width: 900, height: 700 };
const COMPACT = { width: 820, height: 640 };

const layout = extract((state: any) => {
	const document = state.document;
	const box = (element: Element | null | undefined) => element?.getBoundingClientRect() ?? null;
	const point = (value: DOMRect | null) => value && value.width > 0 && value.height > 0
		? { x: value.left + value.width / 2, y: value.top + value.height / 2 }
		: null;

	const firstRun = box(document.querySelector('[data-testid="first-run-account-choice"] button'));
	const fixtureChat = [...document.querySelectorAll<HTMLElement>('[data-testid^="local-chat-"]')]
		.find((element) => element.textContent?.includes("Bombadil visual alignment"));
	const chat = box(fixtureChat);
	const trigger = box(document.querySelector('[data-testid="resource-shelf-trigger"]'));
	const shelfOpen = Boolean(document.querySelector('[data-testid="resource-shelf"]'));
	const row = box(document.querySelector('[data-testid="resource-shelf"] .resource-shelf-row'));
	const pane = document.querySelector<HTMLElement>('[data-testid="visual-pane"]');
	const paneBox = box(pane);
	const bodyBox = box(pane?.querySelector(".visual-pane-body"));
	const canvasBox = box(pane?.querySelector('[data-testid="visual-live-craftax"]'));
	const composerBox = box(document.querySelector('[data-testid="composer"]'));
	const stacked = state.window.innerWidth <= 900;
	const paneVisible = Boolean(bodyBox && bodyBox.width > 0 && bodyBox.height > 0);

	return {
		firstRunPoint: point(firstRun),
		chatPoint: point(chat),
		triggerPoint: point(trigger),
		shelfOpen,
		rowPoint: point(row),
		paneVisible,
		reviewedCompact: paneVisible && stacked,
		// The defect: any part of the artifact surface underneath the composer.
		surfaceClearsComposer: !paneVisible || !bodyBox || !composerBox
			|| bodyBox.bottom <= composerBox.top,
		// The gameplay surface specifically, not just the pane chrome.
		replaySurfaceClearsComposer: !canvasBox || !composerBox
			|| canvasBox.bottom <= composerBox.top,
		// A pane squeezed to a sliver is not "visible" in any useful sense.
		surfaceHasUsableHeight: !paneVisible || !bodyBox || bodyBox.height >= 180,
		paneInsideWindow: !paneBox || (paneBox.top >= -1 && paneBox.bottom <= state.window.innerHeight + 1),
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_a_replay_at_the_narrowest_supported_widths = actions(() => {
	if (layout.current.firstRunPoint) {
		return [{ Click: { name: "Continue locally", point: layout.current.firstRunPoint } }];
	}
	if (layout.current.chatPoint && !layout.current.triggerPoint) {
		return [{ Click: { name: "Open the fixture chat", point: layout.current.chatPoint } }];
	}
	if (layout.current.triggerPoint && !layout.current.shelfOpen && !layout.current.paneVisible) {
		return [{ Click: { name: "Open Outputs", point: layout.current.triggerPoint } }];
	}
	if (layout.current.rowPoint && !layout.current.paneVisible) {
		return [{ Click: { name: "Open the artifact", point: layout.current.rowPoint } }];
	}
	return [
		{ SetViewport: AT_BREAKPOINT },
		{ SetViewport: COMPACT },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

/** Without this the rest is vacuous: a spec that never reaches the compact
 * layout passes every property in it by never testing anything. */
export const the_outputs_pane_actually_opened = eventually(() =>
	layout.current.paneVisible
).within(20, "seconds");

export const the_compact_layout_is_actually_reached = eventually(() =>
	layout.current.reviewedCompact
).within(20, "seconds");

export const the_composer_never_covers_the_artifact_surface = always(() =>
	layout.current.surfaceClearsComposer
);

export const the_composer_never_covers_the_replay_surface = always(() =>
	layout.current.replaySurfaceClearsComposer
);

export const the_artifact_surface_keeps_a_usable_height = always(() =>
	layout.current.surfaceHasUsableHeight
);

export const the_artifact_pane_stays_inside_the_window = always(() =>
	layout.current.paneInsideWindow
);

export const the_compact_layout_never_scrolls_sideways = always(() =>
	layout.current.noHorizontalOverflow
);
