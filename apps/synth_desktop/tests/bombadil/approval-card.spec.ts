import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const approval = extract((state: any) => {
	const document = state.document;
	const point = (selector: string) => {
		const element = document.querySelector<HTMLElement>(selector);
		const rect = element?.getBoundingClientRect();
		return rect && rect.width > 0 && rect.height > 0
			? { x: rect.left + rect.width / 2, y: rect.top + Math.min(rect.height / 2, 24) }
			: null;
	};
	const card = document.querySelector<HTMLElement>(".approval-card");
	const working = document.querySelector<HTMLElement>('[data-testid="model-working"]');
	const reject = card?.querySelector<HTMLElement>(".approval-reject");
	const approve = card?.querySelector<HTMLElement>(".approval-approve");
	const cardRect = card?.getBoundingClientRect();
	const workingRect = working?.getBoundingClientRect();
	return {
		firstRunPoint: point('[data-testid="first-run-account-choice"] button'),
		chatPoint: point('[data-testid="local-chat-v02-approval-session"]'),
		cardVisible: Boolean(card),
		workingVisible: Boolean(working),
		rejectVisible: Boolean(reject),
		approveVisible: Boolean(approve),
		approveOnce: (approve?.textContent ?? "").includes("Approve once"),
		cardAboveWorking: !cardRect || !workingRect || cardRect.bottom <= workingRect.top + 1,
		noHorizontalOverflow: document.documentElement.scrollWidth <= state.window.innerWidth + 1
	};
});

export const open_the_waiting_turn = actions(() => {
	if (approval.current.firstRunPoint) {
		return [{ Click: { name: "Continue locally", point: approval.current.firstRunPoint } }];
	}
	if (!approval.current.cardVisible && approval.current.chatPoint) {
		return [{ Click: { name: "Open the waiting approval turn", point: approval.current.chatPoint } }];
	}
	return ["Wait"];
});

export const approval_card_pins_above_working_with_reject_and_approve_once = eventually(() =>
	approval.current.cardVisible
	&& approval.current.workingVisible
	&& approval.current.rejectVisible
	&& approval.current.approveVisible
	&& approval.current.approveOnce
	&& approval.current.cardAboveWorking
	&& approval.current.noHorizontalOverflow
).within(8, "seconds");

export const waiting_turn_does_not_overflow_the_page = always(() =>
	approval.current.noHorizontalOverflow
);
