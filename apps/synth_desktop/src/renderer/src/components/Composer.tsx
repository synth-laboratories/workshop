import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
	EXECUTION_TARGETS,
	TARGET_GROUP_LABEL,
	type ExecutionTargetOption,
	type LandingState
} from "../types/landing";
import { SynthLogo } from "./SynthLogo";
import type { ApprovalMode } from "../runtime/nativeCodex";
import {
	modelCapabilitiesForTarget,
	modelKnobValue,
	type ModelKnobSpec,
	type ModelKnobValue,
	type ModelKnobValues
} from "../runtime/modelCapabilities";

type Props = {
	state: LandingState;
	onSend: (text: string) => void;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	approvalMode: ApprovalMode;
	onSelectApprovalMode: (mode: ApprovalMode) => void;
	modelKnobValues: ModelKnobValues;
	onSelectModelKnob: (targetId: string, knobId: string, value: ModelKnobValue) => void;
	/** True while the active conversation has a non-terminal run. */
	agentWorking?: boolean;
	/** Preferred Enter action while working. */
	activeEnterAction?: "steer" | "enqueue";
	onSteer?: (text: string) => void | Promise<void>;
	onEnqueue?: (text: string) => void;
	queuedPrompts?: Array<{ id: string; text: string }>;
	onEditQueuedPrompt?: (id: string, text: string) => void;
	onRemoveQueuedPrompt?: (id: string) => void;
	/** After stop, offer send-next / keep / remove for leftover queue items. */
	queueAfterStop?: boolean;
	onSendNextQueued?: () => void;
	onKeepQueued?: () => void;
	steerSupported?: boolean;
	steerError?: string | null;
};

const APPROVAL_OPTIONS: Array<{ id: ApprovalMode; label: string; description: string }> = [
	{ id: "ask", label: "Always ask", description: "Ask before commands or protected actions." },
	{ id: "accept-edits", label: "Accept edits", description: "Allow workspace edits; ask for risky commands." },
	{ id: "plan", label: "Plan", description: "Read-only exploration; no file changes." },
	{ id: "allow-all", label: "Allow all", description: "Full system access without prompts." }
];

function PermissionMenu({ mode, onSelect, disabled }: { mode: ApprovalMode; onSelect: (mode: ApprovalMode) => void; disabled: boolean }) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const selected = APPROVAL_OPTIONS.find((option) => option.id === mode)!;
	useEffect(() => {
		if (!open) return;
		const close = (event: MouseEvent) => { if (!ref.current?.contains(event.target as Node)) setOpen(false); };
		document.addEventListener("mousedown", close);
		return () => document.removeEventListener("mousedown", close);
	}, [open]);
	return <div className="permission-wrap" ref={ref}>
		<button type="button" className="permission-select" disabled={disabled} onClick={() => setOpen((value) => !value)} aria-expanded={open} aria-controls="approval-mode-menu" aria-haspopup="listbox" data-testid="approval-mode-select">
			<IconAsk />{selected.label}<IconChevron />
		</button>
		{open ? <div id="approval-mode-menu" className="permission-menu" role="listbox" aria-label="Approval mode" data-testid="approval-mode-menu">
			{APPROVAL_OPTIONS.map((option) => <button key={option.id} type="button" role="option" aria-selected={option.id === mode} className={`permission-option${option.id === mode ? " selected" : ""}`} onClick={() => { onSelect(option.id); setOpen(false); }}>
				<span><strong>{option.label}</strong><small>{option.description}</small></span>{option.id === mode ? <b aria-hidden>✓</b> : null}
			</button>)}
		</div> : null}
	</div>;
}

function ModelKnobMenu({ value, onSelect, knob }: {
	value: ModelKnobValue;
	onSelect: (value: ModelKnobValue) => void;
	knob: ModelKnobSpec;
}) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const selected = knob.options.find((option) => option.id === value) ?? knob.options[0];
	useEffect(() => {
		if (!open) return;
		const close = (event: MouseEvent) => { if (!ref.current?.contains(event.target as Node)) setOpen(false); };
		document.addEventListener("mousedown", close);
		return () => document.removeEventListener("mousedown", close);
	}, [open]);
	if (!selected) return null;
	return <div className="reasoning-effort-wrap" ref={ref}>
		<button
			type="button"
			className={`reasoning-effort-chip${open ? " open" : ""}`}
			onClick={() => setOpen((value) => !value)}
			aria-label={`${knob.label}: ${selected.label}`}
			aria-expanded={open}
			aria-controls={`${knob.testId}-menu`}
			aria-haspopup="listbox"
			data-testid={`${knob.testId}-select`}
		>
			<span>{selected.label}</span><IconChevron />
		</button>
		{open ? <div id={`${knob.testId}-menu`} className="reasoning-effort-menu" role="listbox" aria-label={knob.label} data-testid={`${knob.testId}-menu`}>
			{knob.options.map((option) => <button
				key={option.id}
				type="button"
				role="option"
				aria-selected={option.id === value}
				className={option.id === value ? "selected" : ""}
				onClick={() => { onSelect(option.id); setOpen(false); }}
			>
				<span>{option.label}</span>{option.id === value ? <b aria-hidden>✓</b> : null}
			</button>)}
		</div> : null}
	</div>;
}

function IconEdit() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="3.25" y="3.25" width="9.5" height="9.5" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path
				d="M5.6 10.4l4.7-4.7M10.3 5.7l.7.7"
				stroke="currentColor"
				strokeWidth="1.25"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconAsk() {
	return (
		<svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden>
			<circle cx="7" cy="7" r="5.25" stroke="currentColor" strokeWidth="1.25" />
			<path
				d="M5.55 5.45a1.45 1.45 0 112.1 1.3c-.4.24-.65.55-.65 1.05"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinecap="round"
			/>
			<circle cx="7" cy="10.05" r="0.7" fill="currentColor" />
		</svg>
	);
}

function IconMic() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="6" y="2.25" width="4" height="7" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path
				d="M4.25 8a3.75 3.75 0 007.5 0M8 11.75v2"
				stroke="currentColor"
				strokeWidth="1.3"
				strokeLinecap="round"
			/>
		</svg>
	);
}

function IconSend() {
	return (
		<svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden>
			<path
				d="M7 11V3M7 3L4 6M7 3l3 3"
				stroke="currentColor"
				strokeWidth="1.55"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconChevron() {
	return (
		<svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden>
			<path d="M2 3.5L5 6.5L8 3.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
		</svg>
	);
}

function modelChipLabel(state: LandingState): string {
	const target = EXECUTION_TARGETS.find((t) => t.id === state.selectedTargetId);
	if (state.selectedTargetId === "local-laguna") {
		if (state.model.status === "not_installed" || state.model.status === "error") {
			return "Laguna offline";
		}
		if (state.model.status === "starting" || state.model.status === "loading") {
			return "Laguna starting…";
		}
		return target?.label ?? `synth/${state.model.name}`;
	}
	return target?.label ?? "Select model";
}

function composerPlaceholder(state: LandingState): string {
	if (state.selectedTargetId === "local-laguna") {
		return state.composerPlaceholder;
	}
	if (state.selectedTargetId === "synth-cloud-laguna-s") {
		return state.apiKeyConfigured
			? "Ask via Synth Cloud (usage tracked)…"
			: "Configure Synth API key in Settings → Account";
	}
	if (state.selectedTargetId.startsWith("openrouter-")) {
		return "Ask via OpenRouter (usage tracked)…";
	}
	if (
		(state.selectedTargetId === "intern-sync" || state.selectedTargetId === "intern-async") &&
		state.internMode === "unconfigured"
	) {
		return "Configure Synth Cloud in Settings → Account";
	}
	if (state.selectedTargetId === "intern-sync") return "Message live Intern…";
	if (state.selectedTargetId === "intern-async") return "Message background Intern…";
	return state.composerPlaceholder;
}

function composerEnabled(state: LandingState): boolean {
	if (state.selectedTargetId === "synth-cloud-laguna-s") {
		return state.apiKeyConfigured === true;
	}
	if (state.selectedTargetId.startsWith("openrouter-")) return true;
	if (state.selectedTargetId === "intern-sync" || state.selectedTargetId === "intern-async") {
		return state.internMode !== "unconfigured";
	}
	return state.composerEnabled;
}

const GROUP_ORDER: ExecutionTargetOption["group"][] = ["local", "remote", "cloud"];

function ModelMenu({
	state,
	onSelectTarget,
	onConfigureAccount
}: {
	state: LandingState;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
}) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const modelLabel = modelChipLabel(state);
	const modelReady = !(
		state.selectedTargetId === "local-laguna" && state.model.status === "not_installed"
	);
	const selected = EXECUTION_TARGETS.find((t) => t.id === state.selectedTargetId);

	useEffect(() => {
		if (!open) return;
		const onDocClick = (e: MouseEvent) => {
			if (!ref.current?.contains(e.target as Node)) setOpen(false);
		};
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") setOpen(false);
		};
		document.addEventListener("mousedown", onDocClick);
		document.addEventListener("keydown", onKey);
		return () => {
			document.removeEventListener("mousedown", onDocClick);
			document.removeEventListener("keydown", onKey);
		};
	}, [open]);

	const pickTarget = (targetId: string) => {
		onSelectTarget(targetId);
		setOpen(false);
	};

	return (
		<div className="composer-model-wrap" ref={ref}>
			<button
				type="button"
				className={`model-chip${modelReady ? "" : " is-empty"}${open ? " open" : ""}`}
				onClick={() => setOpen((v) => !v)}
				aria-label={`Model: ${modelLabel}`}
				aria-expanded={open}
				aria-controls="composer-model-menu"
				aria-haspopup="listbox"
				data-testid="composer-model"
			>
				<SynthLogo className="model-chip-logo" compact />
				<span className="model-chip-label">{modelLabel}</span>
				<IconChevron />
			</button>
			{open ? (
				<div id="composer-model-menu" className="composer-model-menu" role="listbox" data-testid="composer-model-menu">
					{GROUP_ORDER.map((group) => {
						const items = EXECUTION_TARGETS.filter((t) => t.group === group);
						if (!items.length) return null;
						return (
							<div key={group} className="composer-model-group">
								<div className="composer-model-group-label">{TARGET_GROUP_LABEL[group]}</div>
								{items.map((target) => {
									const localBlocked =
										target.id === "local-laguna" &&
										(state.model.status === "not_installed" ||
											state.model.status === "error" ||
											state.model.status === "starting" ||
											state.model.status === "loading");
									const needsSynthKey =
										target.id === "synth-cloud-laguna-s" && state.apiKeyConfigured !== true;
									const localProgress =
										target.id === "local-laguna"
											? state.model.status === "downloading"
												? `Downloading… ${state.model.downloadProgress ?? 0}%`
												: state.model.status === "loading"
													? "Loading local weights…"
													: state.model.status === "starting"
														? "Connecting to local runtime…"
														: null
											: null;
									const selectedHere = target.id === state.selectedTargetId;
									if (needsSynthKey) {
										return (
											<div
												key={target.id}
												className="composer-model-option is-disabled"
												data-testid={`composer-model-option-${target.id}`}
											>
												<span
													className="composer-model-option-main"
													role="option"
													aria-selected={false}
													aria-disabled="true"
												>
													<span className="composer-model-option-label">{target.label}</span>
													<span className="composer-model-option-desc">Synth API key required</span>
												</span>
												<button
													type="button"
													className="composer-model-configure"
													data-testid="composer-model-configure-synth-api-key"
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
											data-testid={`composer-model-option-${target.id}`}
											aria-selected={selectedHere}
											disabled={localBlocked}
											className={`composer-model-option${selectedHere ? " selected" : ""}`}
											onClick={() => pickTarget(target.id)}
										>
											<span className="composer-model-option-main">
												<span className="composer-model-option-label">{target.label}</span>
												<span className="composer-model-option-desc">
													{localProgress ?? target.description}
												</span>
											</span>
											{selectedHere ? (
												<span className="composer-model-check" aria-hidden>
													✓
												</span>
											) : null}
										</button>
									);
								})}
							</div>
						);
					})}
					<p className="composer-model-footnote">
						{selected?.group === "local"
							? "Local Laguna XS · usage on daemon ledger"
							: selected?.group === "remote"
								? "Remote via Codex/ACP · usage tracked locally"
								: selected?.id === "synth-cloud-laguna-s"
									? "Synth Cloud · usage tracked"
									: "Cloud Intern · mailbox authority"}
					</p>
				</div>
			) : null}
		</div>
	);
}

export function Composer({
	state,
	onSend,
	onSelectTarget,
	onConfigureAccount,
	approvalMode,
	onSelectApprovalMode,
	modelKnobValues,
	onSelectModelKnob,
	agentWorking = false,
	activeEnterAction = "enqueue",
	onSteer,
	onEnqueue,
	queuedPrompts = [],
	onEditQueuedPrompt,
	onRemoveQueuedPrompt,
	queueAfterStop = false,
	onSendNextQueued,
	onKeepQueued,
	steerSupported = false,
	steerError = null
}: Props) {
	const [value, setValue] = useState("");
	const [submitting, setSubmitting] = useState(false);
	const dockRef = useRef<HTMLDivElement>(null);
	const enabled = composerEnabled(state);
	const placeholder = composerPlaceholder(state);
	const modelCapabilities = modelCapabilitiesForTarget(state.selectedTargetId);
	const enterAction = agentWorking ? activeEnterAction : "submit";
	const alternateAction = agentWorking
		? (activeEnterAction === "enqueue" ? "steer" : "enqueue")
		: null;

	useEffect(() => {
		setValue("");
	}, [state.id]);

	useLayoutEffect(() => {
		const dock = dockRef.current;
		const mainPane = dock?.closest<HTMLElement>(".main-pane");
		if (!dock || !mainPane) return;

		let frame = 0;
		const updateClearance = () => {
			frame = 0;
			const clearance = Math.ceil(window.innerHeight - dock.getBoundingClientRect().top + 16);
			mainPane.style.setProperty("--composer-clearance", `${clearance}px`);
		};
		const scheduleClearanceUpdate = () => {
			if (frame) cancelAnimationFrame(frame);
			frame = requestAnimationFrame(updateClearance);
		};
		const resizeObserver = new ResizeObserver(scheduleClearanceUpdate);
		resizeObserver.observe(dock);
		const mutationObserver = new MutationObserver(scheduleClearanceUpdate);
		mutationObserver.observe(mainPane, { childList: true, subtree: true });
		window.addEventListener("resize", scheduleClearanceUpdate);
		updateClearance();

		return () => {
			if (frame) cancelAnimationFrame(frame);
			resizeObserver.disconnect();
			mutationObserver.disconnect();
			window.removeEventListener("resize", scheduleClearanceUpdate);
			mainPane.style.removeProperty("--composer-clearance");
		};
	}, []);

	const perform = async (intent: "submit" | "steer" | "enqueue") => {
		if (!enabled || !value.trim() || submitting) return;
		const text = value.trim();
		setSubmitting(true);
		try {
			if (intent === "enqueue") {
				onEnqueue?.(text);
				setValue("");
				return;
			}
			if (intent === "steer") {
				if (!steerSupported) {
					// Honest failure: keep the text and surface the contract gap.
					await onSteer?.(text);
					return;
				}
				await onSteer?.(text);
				setValue("");
				return;
			}
			onSend(text);
			setValue("");
		} finally {
			setSubmitting(false);
		}
	};

	const submit = () => {
		void perform(agentWorking ? activeEnterAction : "submit");
	};

	const submitAlternate = () => {
		if (!alternateAction) return;
		void perform(alternateAction);
	};

	const sendLabel = !agentWorking
		? "Send message"
		: activeEnterAction === "enqueue"
			? "Enqueue message"
			: steerSupported
				? "Steer active turn"
				: "Steer unavailable";

	return (
		<div className="composer-dock" data-testid="composer-dock" ref={dockRef}>
			{queuedPrompts.length > 0 ? (
				<div className="prompt-queue" data-testid="prompt-queue" aria-label="Queued prompts">
					<div className="prompt-queue-head">
						<div><strong>Next turns</strong><span>{queuedPrompts.length} {queuedPrompts.length === 1 ? "prompt" : "prompts"}</span></div>
						<span>Sent in order</span>
					</div>
					{queuedPrompts.map((item, index) => (
						<div key={item.id} className="prompt-queue-item" data-testid={`queued-prompt-${item.id}`}>
							<span className="prompt-queue-index" aria-hidden>{index + 1}</span>
							<input
								className="prompt-queue-text"
								aria-label={`Queued prompt ${index + 1}`}
								value={item.text}
								onChange={(event) => onEditQueuedPrompt?.(item.id, event.target.value)}
							/>
							<button
								type="button"
								className="prompt-queue-remove"
								aria-label={`Remove queued prompt ${index + 1}`}
								data-testid={`remove-queued-${item.id}`}
								onClick={() => onRemoveQueuedPrompt?.(item.id)}
							>
								<span aria-hidden>×</span>
							</button>
						</div>
					))}
				</div>
			) : null}
			{queueAfterStop && queuedPrompts.length > 0 ? (
				<div className="prompt-queue-after-stop" role="region" aria-label="Queued prompts after stop" data-testid="prompt-queue-after-stop">
					<p>Queued prompts were kept after stop.</p>
					<button type="button" data-testid="send-next-queued" onClick={onSendNextQueued}>Send next</button>
					<button type="button" data-testid="keep-queued" onClick={onKeepQueued}>Keep</button>
				</div>
			) : null}
			{steerError ? (
				<p className="composer-steer-error" role="alert" data-testid="steer-error">{steerError}</p>
			) : null}
			<div className={`composer${enabled ? "" : " is-disabled"}`} data-testid="composer" data-enter-action={enterAction}>
				{agentWorking ? (
					<p className="composer-intent-hint" data-testid="composer-intent-hint">
						{activeEnterAction === "enqueue"
							? "Enter enqueues · ⌘Enter steers"
							: "Enter steers · ⌘Enter enqueues"}
						{!steerSupported ? " · Steer needs a runtime primitive" : ""}
					</p>
				) : null}
				<textarea
					className="composer-input"
					rows={2}
					disabled={!enabled || submitting}
					placeholder={placeholder}
					value={value}
					onChange={(e) => setValue(e.target.value)}
					onKeyDown={(e) => {
						if (e.key !== "Enter" || e.shiftKey) return;
						e.preventDefault();
						if (e.metaKey || e.ctrlKey) submitAlternate();
						else submit();
					}}
					aria-label="Message composer"
					data-testid="composer-input"
				/>
				<div className="composer-toolbar">
					<div className="composer-left">
						<button type="button" className="composer-icon-btn" disabled={!enabled} aria-label="Edit context">
							<IconEdit />
						</button>
						<PermissionMenu mode={approvalMode} onSelect={onSelectApprovalMode} disabled={!enabled} />
					</div>
					<div className="composer-right">
						<ModelMenu state={state} onSelectTarget={onSelectTarget} onConfigureAccount={onConfigureAccount} />
						{modelCapabilities?.knobs.map((knob) => (
							<ModelKnobMenu
								key={knob.id}
								knob={knob}
								value={modelKnobValue(modelKnobValues, state.selectedTargetId, knob)}
								onSelect={(value) => onSelectModelKnob(state.selectedTargetId, knob.id, value)}
							/>
						))}
						<button type="button" className="composer-icon-btn" disabled={!enabled} aria-label="Voice input">
							<IconMic />
						</button>
						<button
							type="button"
							className="send-btn"
							disabled={!enabled || !value.trim() || submitting}
							onClick={submit}
							aria-label={sendLabel}
							data-testid="composer-send"
							data-intent={enterAction}
						>
							<IconSend />
						</button>
					</div>
				</div>
			</div>
		</div>
	);
}
