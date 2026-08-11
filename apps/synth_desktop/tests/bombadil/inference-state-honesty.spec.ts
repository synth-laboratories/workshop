import { actions, always, eventually, extract } from "@antithesishq/bombadil";

const inference = extract((state: any) => {
	const document = state.document;
	const toggle = document.querySelector<HTMLElement>('[data-testid="toggle-inference-rail"]');
	const chat = document.querySelector<HTMLElement>('[data-testid="local-chat-blank-worked-turn"]');
	const panel = document.querySelector<HTMLElement>('[data-testid="inference-panel"][data-state="ready"]');
	const residency = document.querySelector<HTMLElement>('[data-testid="inference-residency"]');
	const chips = document.querySelector<HTMLElement>('[data-testid="inference-chips"]');
	const rect = toggle?.getBoundingClientRect() ?? null;
	const residencyText = (residency?.textContent ?? "").replace(/\s+/g, " ").trim();
	const panelText = (panel?.textContent ?? "").replace(/\s+/g, " ").trim();
	return {
		chatPoint: chat && !chat.classList.contains("active") ? (() => { const value = chat.getBoundingClientRect(); return { x: value.left + value.width / 2, y: value.top + value.height / 2 }; })() : null,
		togglePoint: rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null,
		ready: Boolean(panel),
		idle: panel?.dataset.phase === "idle",
		residentUnavailable: /RESIDENT.*Unavailable/i.test(residencyText),
		idleShowsActiveTokenPlaceholders: panel?.dataset.phase === "idle" && Boolean(chips) && /prompt\s+Unavailable.*cached\s+Unavailable.*output\s+Unavailable/i.test(panelText),
		unavailableRateAdvertised: /Unavailable\s+tok\/s/i.test(panelText)
	};
});

export const open_inference_honesty_fixture = actions(() =>
	inference.current.chatPoint
		? [{ Click: { name: "Open inference fixture chat", point: inference.current.chatPoint } }]
		: !inference.current.ready && inference.current.togglePoint
		? [{ Click: { name: "Open local inference monitor", point: inference.current.togglePoint } }]
		: ["Wait"]
);

export const inference_honesty_fixture_is_exercised = eventually(() =>
	inference.current.ready && inference.current.idle
).within(8, "seconds");

/** A resident model cannot simultaneously present its residency as unavailable. */
export const resident_inference_state_never_labels_itself_unavailable = always(() =>
	!inference.current.ready || !inference.current.residentUnavailable
);

/** Idle means there is no active request; active-token placeholders should disappear. */
export const idle_inference_state_hides_active_request_placeholders = always(() =>
	!inference.current.ready || !inference.current.idleShowsActiveTokenPlaceholders
);

export const inference_monitor_never_advertises_unavailable_tok_s = always(() =>
	!inference.current.ready || !inference.current.unavailableRateAdvertised
);

