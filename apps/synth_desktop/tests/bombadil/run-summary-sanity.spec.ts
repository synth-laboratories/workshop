import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const summary = extract((state: any) => {
	const document = state.document;
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const transcript = document.querySelector<HTMLElement>('[data-testid="chat-transcript"]');
	const summaries = transcript
		? [...transcript.querySelectorAll<HTMLElement>(".run-summary")].map((node) => (node.textContent ?? "").replace(/\s+/g, " ").trim())
		: [];
	const absurdDuration = summaries.some((text) => {
		const minutes = /\bWorked\s+(\d+)m/i.exec(text);
		const hours = /\bWorked\s+(\d+)h/i.exec(text);
		return (minutes ? Number(minutes[1]) >= 180 : false) || (hours ? Number(hours[1]) >= 3 : false);
	});
	const rect = chat?.getBoundingClientRect() ?? null;
	return {
		chatPoint: chat && !chat.classList.contains("active") && rect
			? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
			: null,
		hasWorkedSummary: summaries.some((text) => /\bWorked\b/i.test(text)),
		absurdDuration
	};
});

export const open_old_run_summary_fixture = actions(() =>
	summary.current.chatPoint
		? [{ Click: { name: "Open old run-summary fixture", point: summary.current.chatPoint } }]
		: ["Wait"]
);

export const old_run_summary_fixture_is_exercised = eventually(() =>
	summary.current.hasWorkedSummary
).within(8, "seconds");

/** Historic/replayed timestamps must not turn into multi-hour work claims. */
export const worked_duration_never_displays_an_absurd_multi_hour_value = always(() =>
	!summary.current.absurdDuration
);
