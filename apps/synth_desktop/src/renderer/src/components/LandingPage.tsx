import { useEffect, useRef, useState } from "react";
import { EXECUTION_TARGETS, TARGET_GROUP_LABEL } from "../types/landing";
import type { ExecutionTargetOption, LandingState } from "../types/landing";
import { SynthLogo } from "./SynthLogo";

type Props = {
	state: LandingState;
	selectedTargetId: string;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	onSetupAgent: () => void;
};

function IconAgents() {
	return (
		<span className="quick-card-icon-stack" aria-hidden>
			<svg className="quick-card-icon quick-card-icon-back" viewBox="0 0 24 24" fill="none">
				<circle cx="11" cy="12" r="6.5" stroke="currentColor" strokeWidth="1.4" />
				<path
					d="M11 8.2c2.2 0 4 1.7 4 4s-1.8 4-4 4"
					stroke="currentColor"
					strokeWidth="1.4"
					strokeLinecap="round"
				/>
			</svg>
			<svg className="quick-card-icon quick-card-icon-accent" viewBox="0 0 24 24" fill="none">
				<path
					d="M16.5 7.2l.7 1.7 1.7.7-1.7.7-.7 1.7-.7-1.7-1.7-.7 1.7-.7.7-1.7z"
					fill="#f05f22"
				/>
			</svg>
		</span>
	);
}

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

	useEffect(() => {
		if (!open) return;
		const onDocClick = (e: MouseEvent) => {
			if (!ref.current?.contains(e.target as Node)) setOpen(false);
		};
		document.addEventListener("mousedown", onDocClick);
		return () => document.removeEventListener("mousedown", onDocClick);
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
				<SynthLogo className="model-pill-logo" compact />
				<span className="model-pill-label">{selected.label}</span>
				<svg className="model-pill-chevron" width="12" height="12" viewBox="0 0 12 12" aria-hidden>
					<path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" fill="none" />
				</svg>
			</button>
			{open ? (
				<div id="model-dropdown" className="model-dropdown" role="listbox" data-testid="model-dropdown">
					{(["local", "remote", "cloud"] as const).map((group) => {
						const items = EXECUTION_TARGETS.filter((t) => t.group === group);
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
	onConfigureAccount,
	onSetupAgent
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
							<span><strong>Sign in to Synth</strong><small>Connect cloud models and Intern</small></span>
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
				<div className="quick-actions">
					<button
						type="button"
						className="quick-card"
						onClick={onSetupAgent}
						data-testid="quick-setup-agent"
					>
						<span className="quick-card-icon-wrap"><IconAgents /></span>
						<span><strong>Set up an agent</strong><small>Schedule recurring work</small></span>
						<span className="quick-card-arrow" aria-hidden>→</span>
					</button>
				</div>
			</div>
		</div>
	);
}
