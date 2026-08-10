import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { EXECUTION_TARGETS, LAUNCH_PICKER_TARGETS, TARGET_GROUP_LABEL } from "../types/landing";
import type { ExecutionTargetOption, LandingState } from "../types/landing";
import { SynthLogo } from "./SynthLogo";
import { ProviderMark, providerMarkForTarget } from "./ProviderMark";

type Props = {
	state: LandingState;
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
};

export function ModelPicker({
	selectedTargetId,
	apiKeyConfigured,
	onSelectTarget,
	onConfigureAccount
}: {
	selectedTargetId: string;
	apiKeyConfigured?: boolean;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
}) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const selected = EXECUTION_TARGETS.find((t) => t.id === selectedTargetId) ?? EXECUTION_TARGETS[0];
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
			const spaceBelow = bottomLimit - (rect.bottom + gap);
			const spaceAbove = rect.top - gap - inset;
			const direction = spaceBelow >= 240 || spaceBelow >= spaceAbove ? "down" : "up";
			const maxHeight = Math.max(120, Math.floor(direction === "down" ? spaceBelow : spaceAbove));
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
				onClick={() => setOpen((v) => !v)}
				data-testid="model-picker"
				aria-label="Select execution target"
				aria-expanded={open}
				aria-controls="model-dropdown"
				aria-haspopup="listbox"
			>
				<ProviderMark
					kind={providerMarkForTarget(selectedTargetId)}
					className={`model-pill-logo model-pill-logo-${providerMarkForTarget(selectedTargetId)}`}
				/>
				<span className="model-pill-label">{selected.label}</span>
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
					{(["local", "remote", "cloud"] as const).map((group) => {
						const items = LAUNCH_PICKER_TARGETS.filter((t) => t.group === group);
						if (!items.length) return null;
						return (
							<div key={group} className="model-dropdown-group">
								<div className="model-dropdown-group-label">{TARGET_GROUP_LABEL[group]}</div>
								{items.map((target: ExecutionTargetOption) => {
									const needsSynthKey =
										target.id === "synth-cloud-laguna-s" && apiKeyConfigured !== true;
									if (needsSynthKey) {
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
													<span className="model-option-desc">Synth API key required</span>
												</span>
												<button
													type="button"
													className="model-option-configure"
													data-testid="model-configure-synth-api-key"
													onClick={() => {
														onConfigureAccount?.();
														setOpen(false);
													}}
												>
													Configure Synth API key
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
					})}
				</div>
			) : null}
		</div>
	);
}

export function LandingPage({
	state,
	selectedTargetId,
	onSelectTarget,
	onConfigureAccount
}: Props) {
	const [accountChoiceMade, setAccountChoiceMade] = useState(
		() => window.localStorage.getItem("synth.accountChoiceMade") === "1"
	);
	return (
		<div className="landing" data-testid="landing-page">
			<div className="landing-hero">
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
				<div className="synth-logo-wrap">
					<SynthLogo className="synth-logo" />
				</div>
				<div className="landing-title-row">
					<p className="landing-title">Start a new conversation using</p>
					<ModelPicker
						selectedTargetId={selectedTargetId}
						apiKeyConfigured={state.apiKeyConfigured}
						onSelectTarget={onSelectTarget}
						onConfigureAccount={onConfigureAccount}
					/>
				</div>
			</div>
		</div>
	);
}
