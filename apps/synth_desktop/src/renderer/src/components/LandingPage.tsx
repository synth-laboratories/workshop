import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { apiProviderForTarget, EXECUTION_TARGETS, isOpenRouterTargetId, LAUNCH_PICKER_TARGETS, MODEL_ACCESS_LABEL, MODEL_ACCESS_ORDER, modelAccessForTarget, TARGET_GROUP_LABEL } from "../types/landing";
import { targetOptionForId } from "../runtime/modelCatalog";
import type { ExecutionTargetOption, LandingState, ModelAccessKind } from "../types/landing";
import { SynthLogo } from "./SynthLogo";
import type { LagunaPolicy } from "../bridge/types";
import { policyLabel } from "../runtime/lagunaPolicies";
import { ComposerLayoutHost } from "./ComposerLayout";

type Props = {
	state: LandingState;
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	lagunaAdapters?: LagunaPolicy[];
	selectedLagunaAdapterId?: string | null;
	onSelectLagunaAdapter?: (checkpointId: string | null) => void;
	onConfigureAccount?: () => void;
	onConfigureModels?: () => void;
	onResolveBilling?: () => void;
};

export function ModelPicker({
	selectedTargetId,
	apiKeyConfigured,
	openrouterApiKeyConfigured,
	codexOauthConfigured,
	cloudBlockedReason = null,
	onSelectTarget,
	onConfigureAccount,
	onConfigureModels,
	onResolveBilling,
	lagunaPolicies = [],
	selectedLagunaPolicyId = null,
	onSelectLagunaPolicy
}: {
	selectedTargetId: string;
	apiKeyConfigured?: boolean;
	openrouterApiKeyConfigured?: boolean;
	codexOauthConfigured?: boolean;
	/** Backend-authored reason billable cloud actions are blocked; local is unaffected. */
	cloudBlockedReason?: string | null;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	onConfigureModels?: () => void;
	onResolveBilling?: () => void;
	lagunaPolicies?: LagunaPolicy[];
	selectedLagunaPolicyId?: string | null;
	onSelectLagunaPolicy?: (modelId: string | null) => void;
}) {
	const [open, setOpen] = useState(false);
	const [activeAccess, setActiveAccess] = useState<ModelAccessKind | null>(null);
	const ref = useRef<HTMLDivElement>(null);
	const selected = targetOptionForId(selectedTargetId) ?? EXECUTION_TARGETS[0];
	const selectedLagunaPolicy = lagunaPolicies.find((policy) =>
		policy.isBase ? selectedLagunaPolicyId === null : policy.modelId === selectedLagunaPolicyId
	);
	const selectedLabel = selectedTargetId === "local-laguna" && selectedLagunaPolicy
		? policyLabel(selectedLagunaPolicy)
		: selected.label;
	// The dropdown must stay inside the viewport with an 8px inset, never cover
	// the composer, and flip above the trigger when the space below is tighter
	// than the space above. Content taller than the slot scrolls internally.
	const [placement, setPlacement] = useState<{ direction: "down" | "up"; maxHeight: number } | null>(null);
	useLayoutEffect(() => {
		// No setState in the closed branch: a state update in a mount-time layout
		// effect perturbs StrictMode's effect replay and double-registers the
		// app-level keydown listeners (Cmd+J toggled twice = no-op).
		if (!open) return;
		const compute = () => {
			const trigger = ref.current?.querySelector<HTMLElement>('[data-testid="model-picker"]');
			if (!trigger) return;
			const rect = trigger.getBoundingClientRect();
			const inset = 8;
			const gap = 8;
			const composer = document.querySelector<HTMLElement>('[data-testid="composer"]');
			const composerTop = composer ? composer.getBoundingClientRect().top : Number.POSITIVE_INFINITY;
			const bottomLimit = Math.min(window.innerHeight - inset, composerTop - gap);
			const spaceBelow = Math.max(0, bottomLimit - (rect.bottom + gap));
			const spaceAbove = Math.max(0, rect.top - gap - inset);
			const direction = spaceBelow >= 240 || spaceBelow >= spaceAbove ? "down" : "up";
			/*
			 * Clamp to the space actually available on the chosen side. A floor
			 * here (previously `Math.max(120, …)`) silently re-crossed the very
			 * boundary bottomLimit exists to enforce, so a trigger sitting close
			 * to the composer opened a dropdown straight over it. The panel
			 * scrolls internally, so a tight slot degrades to scrolling, not
			 * overlap.
			 */
			const maxHeight = Math.floor(direction === "down" ? spaceBelow : spaceAbove);
			setPlacement({ direction, maxHeight });
		};
		compute();
		window.addEventListener("resize", compute);
		return () => window.removeEventListener("resize", compute);
	}, [open]);
	useEffect(() => {
		if (!open) return;
		ref.current
			?.querySelector(".model-option.selected")
			?.scrollIntoView({ block: "nearest" });
	}, [open, placement]);

	useEffect(() => {
		if (!open) return;
		const onDocClick = (e: MouseEvent) => {
			if (!ref.current?.contains(e.target as Node)) setOpen(false);
		};
		const onKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape") setOpen(false);
		};
		document.addEventListener("mousedown", onDocClick);
		document.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("mousedown", onDocClick);
			document.removeEventListener("keydown", onKeyDown);
		};
	}, [open]);

	return (
		<div className="model-picker-wrap" ref={ref}>
			<button
				type="button"
				className="model-pill"
				onClick={() => setOpen((v) => {
					if (!v) setActiveAccess(null);
					return !v;
				})}
				data-testid="model-picker"
				aria-label="Select execution target"
				aria-expanded={open}
				aria-controls="model-dropdown"
				aria-haspopup="listbox"
			>
				<span className="model-pill-label">{selectedLabel}</span>
				<svg className="model-pill-chevron" width="12" height="12" viewBox="0 0 12 12" aria-hidden>
					<path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" fill="none" />
				</svg>
			</button>
			{open ? (
				<div
					id="model-dropdown"
					className="model-dropdown"
					role="listbox"
					data-testid="model-dropdown"
					data-placement={placement?.direction ?? "down"}
					style={placement
						? {
							maxHeight: placement.maxHeight,
							...(placement.direction === "up" ? { top: "auto", bottom: "calc(100% + 8px)" } : {})
						}
						: undefined}
				>
					{activeAccess === null ? MODEL_ACCESS_ORDER.map((access) => (
						<button
							key={access}
							type="button"
							role="option"
							aria-selected={modelAccessForTarget(selected) === access}
							className={`model-option model-access-option${modelAccessForTarget(selected) === access ? " selected" : ""}`}
							data-testid={`model-access-${access}`}
							onClick={() => setActiveAccess(access)}
						>
							<span className="model-option-label">{MODEL_ACCESS_LABEL[access]}</span>
							<span className="model-option-desc">{access === "local" ? "Models on this Mac" : access === "api" ? "Synth and third-party providers" : "Your ChatGPT subscription"} ›</span>
						</button>
					)) : <>
						<button type="button" className="model-option model-access-back" data-testid="model-access-back" onClick={() => setActiveAccess(null)}>
							<span className="model-option-label">‹ {MODEL_ACCESS_LABEL[activeAccess]}</span>
							<span className="model-option-desc">All access methods</span>
						</button>
					{(["local", "cloud", "remote", "subscription"] as const).filter((group) => {
						const sample = LAUNCH_PICKER_TARGETS.find((target) => target.group === group);
						return sample ? modelAccessForTarget(sample) === activeAccess : false;
					}).map((group) => {
						const items = LAUNCH_PICKER_TARGETS.filter((t) => t.group === group);
						if (!items.length) return null;
						return (
							<div key={group} className="model-dropdown-group">
								<div className="model-dropdown-group-label">{activeAccess === "api" ? apiProviderForTarget(items[0]) : TARGET_GROUP_LABEL[group]}</div>
								{items.map((target: ExecutionTargetOption) => {
									if (target.id === "local-laguna" && lagunaPolicies.length) {
										return lagunaPolicies.map((policy) => {
											const policyId = policy.isBase ? null : policy.modelId;
											const selectedHere = selectedTargetId === target.id && selectedLagunaPolicyId === policyId;
											return (
												<button
													key={policy.modelId}
													type="button"
													role="option"
													aria-selected={selectedHere}
													data-testid={`model-option-local-laguna-${policy.isBase ? "base" : policy.modelId}`}
													className={`model-option${selectedHere ? " selected" : ""}`}
													onClick={() => {
														onSelectTarget(target.id);
														onSelectLagunaPolicy?.(policyId);
														setOpen(false);
													}}
												>
													<span className="model-option-label">{policyLabel(policy)}</span>
													<span className="model-option-desc">{policy.isBase ? "Base model · This Mac" : "Fine-tuned model · This Mac"}</span>
												</button>
											);
										});
									}
									const needsSynthKey =
										target.id.startsWith("synth-cloud-") && apiKeyConfigured !== true;
									const needsOpenRouterKey =
										isOpenRouterTargetId(target.id) && openrouterApiKeyConfigured !== true;
									const needsCodexOauth =
										target.id.startsWith("chatgpt-") && codexOauthConfigured !== true;
									const allowanceBlocked =
										target.id.startsWith("synth-cloud-") && !needsSynthKey && Boolean(cloudBlockedReason);
									if (target.selectable === false) {
										return (
											<div key={target.id} className="model-option is-disabled" data-testid={`model-option-${target.id}`}>
												<span className="model-option-copy" role="option" aria-selected={false} aria-disabled="true">
													<span className="model-option-label">{target.label}</span>
													<span className="model-option-desc">{target.diagnostic ?? target.availability ?? "Unavailable"}</span>
												</span>
											</div>
										);
									}
									if (allowanceBlocked) {
										return (
											<div
												key={target.id}
												className="model-option is-disabled"
												data-testid={`model-option-${target.id}`}
											>
												<span
													className="model-option-copy"
													role="option"
													aria-selected={false}
													aria-disabled="true"
												>
													<span className="model-option-label">{target.label}</span>
													<span className="model-option-desc" data-testid="model-option-allowance-blocked">{cloudBlockedReason}</span>
												</span>
												<button
													type="button"
													className="model-option-configure"
													data-testid="model-resolve-synth-billing"
													onClick={() => {
														onResolveBilling?.();
														setOpen(false);
													}}
												>
													Manage plan
												</button>
											</div>
										);
									}
									if (needsSynthKey || needsOpenRouterKey || needsCodexOauth) {
										const providerName = needsCodexOauth ? "ChatGPT subscription" : needsOpenRouterKey ? "OpenRouter" : "Synth";
										return (
											<div
												key={target.id}
												className="model-option is-disabled"
												data-testid={`model-option-${target.id}`}
											>
												<span
													className="model-option-copy"
													role="option"
													aria-selected={false}
													aria-disabled="true"
												>
													<span className="model-option-label">{target.label}</span>
											<span className="model-option-desc">{needsCodexOauth ? "Connect in Settings → Models" : `${providerName} API key required`}</span>
												</span>
										<button
											type="button"
											className="model-option-configure"
											data-testid={needsCodexOauth ? "model-configure-chatgpt-subscription" : `model-configure-${providerName.toLowerCase()}-api-key`}
													onClick={() => {
														(needsCodexOauth ? onConfigureModels : onConfigureAccount)?.();
														setOpen(false);
													}}
												>
											{needsCodexOauth ? "Connect ChatGPT subscription" : `Configure ${providerName} API key`}
												</button>
											</div>
										);
									}
									return (
										<button
											key={target.id}
											type="button"
											role="option"
											aria-selected={target.id === selectedTargetId}
											data-testid={`model-option-${target.id}`}
											className={`model-option${target.id === selectedTargetId ? " selected" : ""}`}
											onClick={() => {
												onSelectTarget(target.id);
												setOpen(false);
											}}
										>
											<span className="model-option-label">{target.label}</span>
											<span className="model-option-desc">{target.description}</span>
										</button>
									);
								})}
							</div>
						);
					})}</>}
				</div>
			) : null}
		</div>
	);
}

export function LandingPage({
	state,
	selectedTargetId,
	onSelectTarget,
	lagunaAdapters = [],
	selectedLagunaAdapterId = null,
	onSelectLagunaAdapter,
	onConfigureAccount,
	onConfigureModels,
	onResolveBilling
}: Props) {
	const [accountChoiceMade, setAccountChoiceMade] = useState(
		() => window.localStorage.getItem("synth.accountChoiceMade") === "1"
	);
	return (
		<div className="landing" data-testid="landing-page">
			<div className="landing-hero">
				<div className="synth-logo-wrap">
					<SynthLogo className="synth-logo" />
				</div>
				<div className="landing-title-row">
					<p className="landing-title">Start a new conversation using</p>
					<ModelPicker
						selectedTargetId={selectedTargetId}
						apiKeyConfigured={state.apiKeyConfigured}
						openrouterApiKeyConfigured={state.openrouterApiKeyConfigured}
						codexOauthConfigured={state.codexOauthConfigured}
						cloudBlockedReason={state.cloudBlockedReason}
						onSelectTarget={onSelectTarget}
						onConfigureAccount={onConfigureAccount}
						onConfigureModels={onConfigureModels}
							onResolveBilling={onResolveBilling}
							lagunaPolicies={lagunaAdapters}
							selectedLagunaPolicyId={selectedLagunaAdapterId}
							onSelectLagunaPolicy={onSelectLagunaAdapter}
						/>
				</div>
				{!state.apiKeyConfigured && !accountChoiceMade ? (
					<div className="quick-actions" data-testid="first-run-account-choice">
						<button type="button" className="quick-card" onClick={() => {
							window.localStorage.setItem("synth.accountChoiceMade", "1");
							setAccountChoiceMade(true);
						}}>
							<span><strong>Continue locally</strong><small>No account required</small></span>
						</button>
						<button type="button" className="quick-card" onClick={onConfigureAccount}>
							<span><strong>Sign in to Synth</strong><small>Connect cloud models</small></span>
						</button>
					</div>
				) : null}
			</div>
			<ComposerLayoutHost />
		</div>
	);
}
