import { useEffect, useRef, useState } from "react";
import {
	AVAILABLE_LORAS,
	EXECUTION_TARGETS,
	LORA_NONE,
	TARGET_GROUP_LABEL,
	type ExecutionTargetOption,
	type LandingState
} from "../types/landing";
import { SynthLogo } from "./SynthLogo";

type Props = {
	state: LandingState;
	onSend: (text: string) => void;
	onSelectTarget: (id: string) => void;
	onSelectLora: (id: string) => void;
	onOpenFinetunes?: () => void;
};

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
		if (state.model.status === "not_installed") return "No model available";
		const base = "Laguna XS 2.1";
		if (state.selectedLoraId && state.selectedLoraId !== LORA_NONE) {
			const lora = AVAILABLE_LORAS.find((l) => l.id === state.selectedLoraId);
			if (lora) return `${base} · ${lora.displayName}`;
		}
		return target?.label ?? `synth/${state.model.name}`;
	}
	if (state.selectedLoraId && state.selectedLoraId !== LORA_NONE) {
		const lora = AVAILABLE_LORAS.find((l) => l.id === state.selectedLoraId);
		if (lora && lora.baseTargetId === state.selectedTargetId) {
			return `${target?.label ?? "model"} · ${lora.displayName}`;
		}
	}
	return target?.label ?? "Select model";
}

function composerPlaceholder(state: LandingState): string {
	if (state.selectedTargetId === "local-laguna") {
		if (state.model.status === "not_installed") return "No model available";
		return "Ask Laguna something…";
	}
	if (state.selectedTargetId.startsWith("openrouter-")) {
		return "Ask via OpenRouter (usage tracked)…";
	}
	if (state.selectedTargetId === "intern-sync") return "Message live Intern…";
	if (state.selectedTargetId === "intern-async") return "Message background Intern…";
	return state.composerPlaceholder;
}

function composerEnabled(state: LandingState): boolean {
	if (state.selectedTargetId.startsWith("openrouter-")) return true;
	if (state.selectedTargetId === "intern-sync" || state.selectedTargetId === "intern-async") {
		return true;
	}
	return state.composerEnabled;
}

const GROUP_ORDER: ExecutionTargetOption["group"][] = ["local", "remote", "cloud"];

function ModelMenu({
	state,
	onSelectTarget,
	onSelectLora,
	onOpenFinetunes
}: {
	state: LandingState;
	onSelectTarget: (id: string) => void;
	onSelectLora: (id: string) => void;
	onOpenFinetunes?: () => void;
}) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const modelLabel = modelChipLabel(state);
	const modelReady = !(
		state.selectedTargetId === "local-laguna" && state.model.status === "not_installed"
	);
	const selected = EXECUTION_TARGETS.find((t) => t.id === state.selectedTargetId);
	const localLoras = AVAILABLE_LORAS.filter(
		(l) => l.baseTargetId === "local-laguna" && l.status === "ready"
	);
	const remoteLoras = AVAILABLE_LORAS.filter(
		(l) => l.scope === "remote" && l.baseTargetId === state.selectedTargetId && l.status === "ready"
	);

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
		if (targetId === "local-laguna") {
			/* keep current LoRA if still local; else base */
			const current = AVAILABLE_LORAS.find((l) => l.id === state.selectedLoraId);
			if (!current || current.baseTargetId !== "local-laguna") onSelectLora(LORA_NONE);
		} else {
			const match = AVAILABLE_LORAS.find((l) => l.baseTargetId === targetId && l.status === "ready");
			onSelectLora(match && state.selectedLoraId === match.id ? match.id : LORA_NONE);
		}
		setOpen(false);
	};

	const pickLora = (loraId: string) => {
		onSelectLora(loraId);
		const lora = AVAILABLE_LORAS.find((l) => l.id === loraId);
		if (lora) onSelectTarget(lora.baseTargetId);
		else onSelectTarget("local-laguna");
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
				aria-haspopup="listbox"
				data-testid="composer-model"
			>
				<SynthLogo className="model-chip-logo" compact />
				<span className="model-chip-label">{modelLabel}</span>
				<IconChevron />
			</button>
			{open ? (
				<div className="composer-model-menu" role="listbox" data-testid="composer-model-menu">
					{GROUP_ORDER.map((group) => {
						const items = EXECUTION_TARGETS.filter((t) => t.group === group);
						if (!items.length) return null;
						return (
							<div key={group} className="composer-model-group">
								<div className="composer-model-group-label">{TARGET_GROUP_LABEL[group]}</div>
								{items.map((target) => {
									const localBlocked =
										target.id === "local-laguna" && state.model.status === "not_installed";
									const downloading =
										target.id === "local-laguna" && state.model.status === "downloading";
									const selectedHere =
										target.id === state.selectedTargetId &&
										(target.id !== "local-laguna" ||
											state.selectedLoraId === LORA_NONE ||
											!AVAILABLE_LORAS.some(
												(l) => l.id === state.selectedLoraId && l.baseTargetId === "local-laguna"
											));
									return (
										<button
											key={target.id}
											type="button"
											role="option"
											aria-selected={selectedHere}
											disabled={localBlocked}
											className={`composer-model-option${selectedHere ? " selected" : ""}`}
											onClick={() => pickTarget(target.id)}
										>
											<span className="composer-model-option-main">
												<span className="composer-model-option-label">{target.label}</span>
												<span className="composer-model-option-desc">
													{downloading
														? `Downloading… ${state.model.downloadProgress ?? 0}%`
														: target.description}
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

								{group === "local" && localLoras.length > 0 ? (
									<>
										<div className="composer-model-subgroup-label">Laguna LoRAs</div>
										<button
											type="button"
											role="option"
											aria-selected={
												state.selectedTargetId === "local-laguna" &&
												state.selectedLoraId === LORA_NONE
											}
											className={`composer-model-option is-lora${
												state.selectedTargetId === "local-laguna" &&
												state.selectedLoraId === LORA_NONE
													? " selected"
													: ""
											}`}
											onClick={() => pickLora(LORA_NONE)}
										>
											<span className="composer-model-option-main">
												<span className="composer-model-option-label">Base (no adapter)</span>
												<span className="composer-model-option-desc">Stock Laguna XS</span>
											</span>
											{state.selectedTargetId === "local-laguna" &&
											state.selectedLoraId === LORA_NONE ? (
												<span className="composer-model-check" aria-hidden>
													✓
												</span>
											) : null}
										</button>
										{localLoras.map((lora) => (
											<button
												key={lora.id}
												type="button"
												role="option"
												aria-selected={state.selectedLoraId === lora.id}
												className={`composer-model-option is-lora${
													state.selectedLoraId === lora.id ? " selected" : ""
												}`}
												onClick={() => pickLora(lora.id)}
											>
												<span className="composer-model-option-main">
													<span className="composer-model-option-label">{lora.displayName}</span>
													<span className="composer-model-option-desc">
														{lora.name} · {lora.revision}
													</span>
												</span>
												{state.selectedLoraId === lora.id ? (
													<span className="composer-model-check" aria-hidden>
														✓
													</span>
												) : null}
											</button>
										))}
									</>
								) : null}

								{group === "remote" && remoteLoras.length > 0 ? (
									<>
										<div className="composer-model-subgroup-label">Remote LoRAs</div>
										{remoteLoras.map((lora) => (
											<button
												key={lora.id}
												type="button"
												role="option"
												aria-selected={state.selectedLoraId === lora.id}
												className={`composer-model-option is-lora${
													state.selectedLoraId === lora.id ? " selected" : ""
												}`}
												onClick={() => pickLora(lora.id)}
											>
												<span className="composer-model-option-main">
													<span className="composer-model-option-label">{lora.displayName}</span>
													<span className="composer-model-option-desc">
														{lora.name} · {lora.revision}
													</span>
												</span>
												{state.selectedLoraId === lora.id ? (
													<span className="composer-model-check" aria-hidden>
														✓
													</span>
												) : null}
											</button>
										))}
									</>
								) : null}
							</div>
						);
					})}
					<p className="composer-model-footnote">
						{selected?.group === "local"
							? "Local · base + LoRA · usage on daemon ledger"
							: selected?.group === "remote"
								? "Remote via Codex/ACP · usage tracked locally"
								: "Cloud Intern · mailbox authority"}
					</p>
					{onOpenFinetunes ? (
						<button
							type="button"
							className="composer-model-finetunes-link"
							onClick={() => {
								setOpen(false);
								onOpenFinetunes();
							}}
							data-testid="open-finetunes-settings"
						>
							Manage finetunes in Settings…
						</button>
					) : null}
				</div>
			) : null}
		</div>
	);
}

export function Composer({ state, onSend, onSelectTarget, onSelectLora, onOpenFinetunes }: Props) {
	const [value, setValue] = useState("");
	const enabled = composerEnabled(state);
	const placeholder = composerPlaceholder(state);

	useEffect(() => {
		setValue("");
	}, [state.id]);

	const submit = () => {
		if (!enabled || !value.trim()) return;
		onSend(value.trim());
		setValue("");
	};

	return (
		<div className="composer-dock">
			<div className={`composer${enabled ? "" : " is-disabled"}`} data-testid="composer">
				<textarea
					className="composer-input"
					rows={2}
					disabled={!enabled}
					placeholder={placeholder}
					value={value}
					onChange={(e) => setValue(e.target.value)}
					onKeyDown={(e) => {
						if (e.key === "Enter" && !e.shiftKey) {
							e.preventDefault();
							submit();
						}
					}}
					aria-label="Message composer"
					data-testid="composer-input"
				/>
				<div className="composer-toolbar">
					<div className="composer-left">
						<button type="button" className="composer-icon-btn" disabled={!enabled} aria-label="Edit context">
							<IconEdit />
						</button>
						<button type="button" className="permission-select" disabled={!enabled}>
							<IconAsk />
							Always ask
							<IconChevron />
						</button>
					</div>
					<div className="composer-right">
						<ModelMenu
							state={state}
							onSelectTarget={onSelectTarget}
							onSelectLora={onSelectLora}
							onOpenFinetunes={onOpenFinetunes}
						/>
						<button type="button" className="composer-icon-btn" disabled={!enabled} aria-label="Voice input">
							<IconMic />
						</button>
						<button
							type="button"
							className="send-btn"
							disabled={!enabled || !value.trim()}
							onClick={submit}
							aria-label="Send message"
							data-testid="composer-send"
						>
							<IconSend />
						</button>
					</div>
				</div>
			</div>
		</div>
	);
}
