import { actions, always, eventually, extract } from "@antithesishq/bombadil";

/**
 * Muse / local residency honesty — CUA 2026-08-10 1:20 PM.
 *
 * Screenshot showed a green-dot residency card:
 *   Muse-Glimmer-30B-GGUF
 *   Memory unavailable
 *   Last prompt 25m ago
 *   Memory · Memory unavailable
 *   Next free · Free scheduled for 1:20 PM · awaiting unload
 * with Laguna-XS-2.1 ready underneath.
 *
 * That is theater: a "loaded" model with a ready chrome that cannot
 * report how much memory it holds, while promising an unload of unknown
 * weights. Fixture is injected by tests/bombadil/run.mjs. Expected RED
 * until the product stops painting this state.
 *
 * BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/muse-residency-honesty.spec.ts \
 *   npm run test:bombadil:muse-residency --workspace @synth/synth-desktop
 */

function textOf(node: HTMLElement | null | undefined): string {
	return (node?.textContent ?? "").replace(/\s+/g, " ").trim();
}

const residency = extract((state: any) => {
	const document = state.document;
	const card = document.querySelector<HTMLElement>('[data-testid="model-residency"]');
	const summary = card?.querySelector<HTMLButtonElement>("button.model-residency-summary, button[aria-expanded]");
	const details = document.querySelector<HTMLElement>('[data-testid="model-residency-details"]');
	const readyBar = document.querySelector<HTMLElement>('[data-testid="model-status-ready"]');
	const inference = document.querySelector<HTMLElement>('[data-testid="inference-panel"]');
	const summaryText = textOf(summary ?? card);
	const detailsText = textOf(details);
	const cardText = textOf(card);
	const aria = summary?.getAttribute("aria-label") ?? "";
	const title = summary?.getAttribute("title") ?? "";
	const expanded = summary?.getAttribute("aria-expanded") === "true" || Boolean(details);
	const memoryUnavailable = /Memory unavailable/i.test(cardText)
		|| /Memory unavailable/i.test(aria)
		|| /Memory unavailable/i.test(title);
	const awaitingUnload = /awaiting unload/i.test(cardText)
		|| /awaiting unload/i.test(aria)
		|| /awaiting unload/i.test(title)
		|| /awaiting unload/i.test(detailsText);
	const hasRealResidentGb = /\d+(\.\d+)?\s*GB\s+resident/i.test(cardText)
		|| /\d+(\.\d+)?\s*GB\s+resident/i.test(aria);
	const museNamed = /Muse-Glimmer-30B-GGUF|Muse Glimmer/i.test(cardText)
		|| /Muse-Glimmer-30B-GGUF|Muse Glimmer/i.test(aria);
	const readyDot = Boolean(card?.querySelector(".model-residency-dot"));
	const summarySubtitle = textOf(card?.querySelector(".model-residency-copy span"));
	const nextFree = details
		? textOf(
			[...details.querySelectorAll("div")].find((row) => /Next free/i.test(row.textContent ?? ""))
				?.querySelector("strong")
		)
		: "";
	const memoryRow = details
		? textOf(
			[...details.querySelectorAll("div")].find((row) => /^Memory\b/i.test((row.textContent ?? "").trim()) || /Memory/.test(row.querySelector("span")?.textContent ?? ""))
				?.querySelector("strong")
		)
		: "";
	const rect = summary?.getBoundingClientRect()
		?? card?.getBoundingClientRect()
		?? null;
	const inferenceText = textOf(inference);
	return {
		shellReady: Boolean(document.querySelector(".app-shell")),
		cardVisible: Boolean(card) && (card?.getBoundingClientRect().height ?? 0) > 0,
		summaryPoint: rect && rect.width > 0 && rect.height > 0
			? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
			: null,
		expanded,
		memoryUnavailable,
		awaitingUnload,
		hasRealResidentGb,
		museNamed,
		readyDot,
		summaryLeadsWithUnavailable: /Memory unavailable/i.test(summarySubtitle),
		detailsMemoryUnavailable: /Memory unavailable/i.test(memoryRow) || (/Memory unavailable/i.test(detailsText) && expanded),
		detailsAwaitingUnload: /awaiting unload/i.test(nextFree) || (/awaiting unload/i.test(detailsText) && expanded),
		ariaClaimsUnavailable: /Memory unavailable/i.test(aria),
		ariaClaimsAwaitingUnload: /awaiting unload/i.test(aria),
		readySidecarUnderCard: Boolean(readyBar),
		readySidecarText: textOf(readyBar),
		monitorPausedWhileCardVisible: Boolean(inference)
			&& inference?.getAttribute("data-state") === "off"
			&& /Monitor paused/i.test(inferenceText),
		unknownMemoryPlusAwaitingUnload: memoryUnavailable && awaitingUnload,
		readyChromePlusUnknownMemory: readyDot && memoryUnavailable,
		musePlusUnknownMemory: museNamed && memoryUnavailable,
		musePlusAwaitingUnknownUnload: museNamed && memoryUnavailable && awaitingUnload
	};
});

/** Expand the CUA residency card so details rows are observable. */
export const open_muse_residency_honesty_fixture = actions(() => {
	if (!residency.current.cardVisible && residency.current.summaryPoint) {
		return [{ Click: { name: "Reveal Muse residency card", point: residency.current.summaryPoint } }];
	}
	if (residency.current.cardVisible && !residency.current.expanded && residency.current.summaryPoint) {
		return [
			{ Click: { name: "Expand Muse residency details", point: residency.current.summaryPoint } },
			"Wait",
			"Wait"
		];
	}
	return ["Wait"];
});

/** Fixture must paint the dishonest Muse residency state from the CUA shot. */
export const muse_residency_dishonesty_fixture_is_reachable = eventually(() =>
	residency.current.cardVisible
	&& residency.current.museNamed
	&& residency.current.memoryUnavailable
	&& residency.current.awaitingUnload
	&& residency.current.expanded
).within(8, "seconds");

/** Primary subtitle under a loaded model must never be "Memory unavailable". */
export const residency_summary_never_leads_with_memory_unavailable = always(() =>
	!residency.current.cardVisible || !residency.current.summaryLeadsWithUnavailable
);

/** A ready-green residency card cannot advertise unknown memory. */
export const residency_ready_dot_never_pairs_with_memory_unavailable = always(() =>
	!residency.current.cardVisible || !residency.current.readyChromePlusUnknownMemory
);

/** "Awaiting unload" is nonsense when resident bytes are unknown. */
export const residency_never_awaits_unload_of_unknown_memory = always(() =>
	!residency.current.cardVisible || !residency.current.unknownMemoryPlusAwaitingUnload
);

/** Expanded Memory row must not print "Memory unavailable". */
export const residency_details_never_list_memory_unavailable = always(() =>
	!residency.current.expanded || !residency.current.detailsMemoryUnavailable
);

/** Expanded Next free must not promise unload when memory is unknown. */
export const residency_details_never_await_unload_without_known_bytes = always(() =>
	!residency.current.expanded
	|| !(residency.current.detailsAwaitingUnload && residency.current.memoryUnavailable)
);

/** Accessible name must not describe a ready load as Memory unavailable. */
export const residency_accessible_name_never_claims_memory_unavailable = always(() =>
	!residency.current.cardVisible || !residency.current.ariaClaimsUnavailable
);

/** Accessible name must not claim awaiting unload of unknown memory. */
export const residency_accessible_name_never_awaits_unknown_unload = always(() =>
	!residency.current.cardVisible
	|| !(residency.current.ariaClaimsAwaitingUnload && residency.current.memoryUnavailable)
);

/** Muse GGUF specifically must not ship the CUA Memory unavailable card. */
export const muse_glimmer_never_reports_memory_unavailable_while_loaded = always(() =>
	!residency.current.cardVisible || !residency.current.musePlusUnknownMemory
);

/** Muse GGUF must not combine Memory unavailable with awaiting unload. */
export const muse_glimmer_never_awaits_unload_of_unknown_memory = always(() =>
	!residency.current.cardVisible || !residency.current.musePlusAwaitingUnknownUnload
);

/** If residency claims a load, it must show a real GB figure — not Unavailable. */
export const loaded_residency_always_reports_concrete_resident_gb = always(() =>
	!residency.current.cardVisible
	|| residency.current.hasRealResidentGb
	|| !residency.current.museNamed
);

/**
 * Dual-chrome confusion from the shot: a green ready bar under a resident
 * card that cannot report memory. Naming may say Muse or Laguna; either way
 * the ready affirmation is dishonest while memory is unknown.
 */
export const ready_sidecar_never_affirms_ready_under_unknown_memory = always(() =>
	!residency.current.cardVisible
	|| !residency.current.memoryUnavailable
	|| !residency.current.readySidecarUnderCard
);

/** Extra lock for the exact CUA pair: Muse card + "Laguna-… ready" bar. */
export const muse_unknown_memory_never_sits_above_laguna_ready = always(() =>
	!residency.current.cardVisible
	|| !residency.current.musePlusUnknownMemory
	|| !residency.current.readySidecarUnderCard
	|| !/Laguna/i.test(residency.current.readySidecarText)
);

/** Inference rail "Monitor paused" while a dishonest resident card is up. */
export const paused_monitor_never_covers_for_unknown_resident_memory = always(() =>
	!residency.current.cardVisible
	|| !residency.current.memoryUnavailable
	|| !residency.current.monitorPausedWhileCardVisible
);
