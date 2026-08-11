import { actions, always, eventually, extract } from "@antithesishq/bombadil";

function overlaps(a: DOMRect, b: DOMRect) {
	return !(a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top);
}

const transcriptOrder = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const turn = document.querySelector<HTMLElement>(".local-turn-assistant");
	const message = turn?.querySelector<HTMLElement>(".local-assistant") ?? null;
	const firstTool = turn?.querySelector<HTMLElement>('[data-testid^="activity-"].tool-activity') ?? null;
	const groupToggle = turn?.querySelector<HTMLElement>('[data-testid^="activity-group-toggle-"]') ?? null;
	const group = turn?.querySelector<HTMLElement>('[data-testid^="activity-group-"]') ?? null;
	const steps = group ? [...group.querySelectorAll<HTMLElement>(".activity-group-step")] : [];
	const rowGeometrySound = steps.every((step, index) => {
		const action = step.querySelector<HTMLElement>(".activity-group-action");
		const context = step.querySelector<HTMLElement>(".activity-group-context");
		if (!action || !context) return true;
		const stepRect = step.getBoundingClientRect();
		const actionRect = action.getBoundingClientRect();
		const contextRect = context.getBoundingClientRect();
		const commandText = action.querySelector<HTMLElement>(".tool-activity-body code");
		const commandRange = commandText && commandText.firstChild ? document.createRange() : null;
		if (commandRange && commandText?.firstChild) commandRange.selectNodeContents(commandText);
		const commandTextRect = commandRange?.getBoundingClientRect() ?? actionRect;
		const nextRect = steps[index + 1]?.getBoundingClientRect() ?? null;
		return contextRect.top >= stepRect.top - 1
			&& contextRect.bottom <= stepRect.bottom + 1
			&& Math.abs((actionRect.top + actionRect.bottom) / 2 - (contextRect.top + contextRect.bottom) / 2) <= 12
			&& contextRect.left - commandTextRect.right <= 32
			&& (!nextRect || !overlaps(contextRect, nextRect));
	});
	const chatRect = chat?.getBoundingClientRect() ?? null;
	const messageRect = message?.getBoundingClientRect() ?? null;
	const toolRect = firstTool?.getBoundingClientRect() ?? null;
	const toggleRect = groupToggle?.getBoundingClientRect() ?? null;
	return {
		chatPoint: chat && !chat.classList.contains("active") && chatRect
			? { x: chatRect.left + chatRect.width / 2, y: chatRect.top + chatRect.height / 2 }
			: null,
		groupTogglePoint: toggleRect ? { x: toggleRect.left + toggleRect.width / 2, y: toggleRect.top + toggleRect.height / 2 } : null,
		hasPreambleAndTools: Boolean(message && (firstTool || group)),
		preambleBeforeTools: !messageRect || (!toolRect && !group) || messageRect.bottom <= (toolRect ?? group!.getBoundingClientRect()).top,
		groupExpanded: Boolean(group?.querySelector(".activity-group-body")),
		rowGeometrySound
	};
});

export const open_chronology_fixture_and_expand_activity = actions(() => {
	if (transcriptOrder.current.chatPoint) return [{ Click: { name: "Open chronology fixture", point: transcriptOrder.current.chatPoint } }];
	if (!transcriptOrder.current.groupExpanded && transcriptOrder.current.groupTogglePoint) {
		return [{ Click: { name: "Expand grouped tool activity", point: transcriptOrder.current.groupTogglePoint } }];
	}
	return [
		{ SetViewport: { width: 960, height: 640 } },
		{ SetViewport: { width: 1172, height: 768 } },
		{ SetViewport: { width: 1280, height: 840 } }
	];
});

export const chronology_fixture_is_exercised = eventually(() =>
	transcriptOrder.current.hasPreambleAndTools && transcriptOrder.current.groupExpanded
).within(8, "seconds");

/** Earlier assistant text must render above the later tool calls it introduced. */
export const transcript_respects_event_timestamp_order = always(() =>
	!transcriptOrder.current.hasPreambleAndTools
	|| !transcriptOrder.current.groupExpanded
	|| transcriptOrder.current.preambleBeforeTools
);

/** Compact reasoning controls must remain attached to their corresponding tool row. */
export const grouped_activity_context_controls_stay_in_their_rows = always(() =>
	!transcriptOrder.current.groupExpanded || transcriptOrder.current.rowGeometrySound
);
