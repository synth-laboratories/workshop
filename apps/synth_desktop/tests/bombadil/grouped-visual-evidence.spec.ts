import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * CUA 2026-08-13: W1 Craftax turn collapsed to
 *   "> Ran commands, used tools 4 calls"
 * with the assistant preamble truncated and no named container/visual
 * evidence. Grouped activity must keep synth_containers / synth_visuals
 * calls inspectable and surface the created visual chip.
 *
 * Fixture is injected by tests/bombadil/run.mjs.
 */
const evidence = extract((state: any) => {
	const document = state.document;
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const assistant = document.querySelector<HTMLElement>(".local-assistant p");
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-v02-grouped-visual-session"]');
	const chatRect = chat?.getBoundingClientRect();
	const firstRun = document.querySelector<HTMLElement>('[data-testid="first-run-account-choice"] button');
	const firstRunRect = firstRun?.getBoundingClientRect();
	const text = transcript?.textContent ?? "";
	const groupLabels = [...document.querySelectorAll<HTMLElement>(".activity-group-toggle")]
		.map((node) => (node.textContent ?? "").replace(/\s+/g, " ").trim());
	const assistantTruncated = Boolean(
		assistant
		&& (assistant.scrollWidth > assistant.clientWidth + 1
			|| assistant.scrollHeight > assistant.clientHeight + 2)
	);
	return {
		firstRunPoint: firstRunRect && firstRunRect.width > 0
			? { x: firstRunRect.left + firstRunRect.width / 2, y: firstRunRect.top + Math.min(firstRunRect.height / 2, 24) }
			: null,
		chatPoint: chatRect && chatRect.width > 0
			? { x: chatRect.left + chatRect.width / 2, y: chatRect.top + chatRect.height / 2 }
			: null,
		transcriptReady: Boolean(transcript),
		containerToolVisible: /synth_containers\.container_(?:list|register)/.test(text),
		visualToolVisible: /synth_visuals\.visual_create/.test(text),
		visualChipVisible: Boolean(document.querySelector('[data-testid="artifact-chip-vis_w1_craftax"]')),
		opaqueUsedToolsGroup: groupLabels.some((label) => /used tools/i.test(label)),
		assistantFullText: Boolean(assistant && /trace and reward evidence/.test(assistant.textContent ?? "")),
		assistantTruncated,
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_the_w1_visual_turn = actions(() => {
	if (evidence.current.firstRunPoint) {
		return [{ Click: { name: "Continue locally", point: evidence.current.firstRunPoint } }];
	}
	if (evidence.current.chatPoint && !evidence.current.transcriptReady) {
		return [{ Click: { name: "Open the W1 Craftax visual turn", point: evidence.current.chatPoint } }];
	}
	if (evidence.current.chatPoint && !evidence.current.containerToolVisible) {
		return [{ Click: { name: "Open the W1 Craftax visual turn", point: evidence.current.chatPoint } }];
	}
	return ["Wait"];
});

export const container_and_visual_mcp_calls_stay_named_in_the_transcript = eventually(() =>
	evidence.current.containerToolVisible && evidence.current.visualToolVisible
).within(8, "seconds");

export const created_visual_has_a_transcript_chip = eventually(() =>
	evidence.current.visualChipVisible
).within(8, "seconds");

export const grouped_activity_never_hides_w1_evidence_as_used_tools = always(() =>
	!evidence.current.transcriptReady || !evidence.current.opaqueUsedToolsGroup
);

export const assistant_preamble_is_fully_readable = eventually(() =>
	evidence.current.assistantFullText && !evidence.current.assistantTruncated
).within(8, "seconds");

export const w1_turn_does_not_overflow_the_page = always(() =>
	evidence.current.noHorizontalOverflow
);
