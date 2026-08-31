import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
	LAUNCH_PICKER_TARGETS,
	MODEL_ACCESS_LABEL,
	MODEL_ACCESS_ORDER,
	apiProviderForTarget,
	isOpenRouterTargetId,
	modelAccessForTarget,
	TARGET_GROUP_LABEL,
	type ExecutionTargetOption,
	type LandingState,
	type ModelAccessKind
} from "../types/landing";
import { targetOptionForId } from "../runtime/modelCatalog";
import { publicError } from "../runtime/publicError";
import { ProviderMark, providerMarkForTarget } from "./ProviderMark";
import type { ApprovalPolicy, SandboxMode } from "../runtime/nativeCodex";
import {
	modelCapabilitiesForTarget,
	modelKnobValue,
	modelSupportsImageInput,
	type ModelKnobSpec,
	type ModelKnobTransportValue,
	type ModelKnobValues
} from "../runtime/modelCapabilities";
import { IconSparkle, SlashCommandMenu, type SlashCommandId, type SlashCommandMenuHandle } from "./SlashCommandMenu";
import type { Skill } from "../runtime/skills";
import type { ComposerImageAttachment, ConversationWorkspaceScope, WhisperRuntimeStatus } from "../bridge";
import { WorkspaceScopeChip, workspaceLabel } from "./WorkspaceScopeChip";
import type { LagunaPolicy } from "../bridge/types";
import { policyLabel } from "../runtime/lagunaPolicies";
import { bridges } from "../runtime/desktopBridge";
import {
	armedPromptId,
	IDLE_STEER_STATE,
	normalizeSteerFailure,
	promotingPromptId,
	reduceSteer,
	steerFailure,
	type SteerEffect,
	type SteerEvent,
	type SteerState
} from "../runtime/steering";

/** Permission chip + menus — injectable like InferenceTransport. */
export type ComposerPermissions = {
	approvalPolicy: ApprovalPolicy;
	sandboxMode: SandboxMode;
	onSelect: (approvalPolicy: ApprovalPolicy, sandboxMode: SandboxMode) => void;
};

export type ComposerModelControls = {
	knobValues: ModelKnobValues;
	onSelectKnob: (targetId: string, knobId: string, value: ModelKnobTransportValue) => void;
	/** Rolling median decode speed for the currently selected model. */
	medianTpsLabel?: string | null;
	/** Cross-session aggregate speeds keyed by model target id. */
	aggregateTpsLabels?: Readonly<Record<string, string>>;
};

export type ComposerQueue = {
	prompts?: Array<{ id: string; text: string }>;
	onEnqueue?: (text: string) => string | undefined;
	onEdit?: (id: string, text: string) => void;
	onRemove?: (id: string) => void;
	onPromote?: (id: string, text: string) => void | Promise<void>;
	/** After stop, offer send-next / keep / remove for leftover queue items. */
	afterStop?: boolean;
	onSendNext?: () => void;
	onKeep?: () => void;
};

/** In-flight turn controls (steer / enter action / send failure). */
export type ComposerTurn = {
	agentWorking?: boolean;
	activeEnterAction?: "steer" | "enqueue";
	steerSupported?: boolean;
	steerError?: string | null;
	onSteer?: (text: string) => void | Promise<void>;
	onStop?: () => void;
	/**
	 * Recoverable turn-start failure, or a turn a dead process abandoned,
	 * rendered above the input inside its dock. `onRetry` is absent when
	 * retrying could duplicate work that may already have happened — the state
	 * is still shown, without an action that would be unsafe to take.
	 */
	sendFailure?: { message: string; onRetry?: () => void; actionLabel?: string } | null;
};

export type ComposerWorkspace = {
	sessionId?: string | null;
	onEnsureSession?: () => Promise<string | null>;
	fallback?: string | null;
	scope?: ConversationWorkspaceScope | null;
	onScopeChange?: (scope: ConversationWorkspaceScope) => void;
	onError?: (message: string) => void;
};

export type ComposerSlash = {
	/** Skills selectable from the "/" menu; each attaches as a removable chip. */
	skills?: Array<Skill>;
	onNew?: () => void;
	onMode?: () => void;
	onModel?: () => void;
	onMcp?: () => void;
	onRename?: () => void;
	onCompact?: () => void | Promise<void>;
};

export type ComposerAccountNav = {
	onConfigureAccount?: () => void;
	onConfigureModels?: () => void;
	/** Opens the plan/billing recovery path when cloud spend is blocked. */
	onResolveBilling?: () => void;
	/** Opens Settings → Voice so the user can pick/download a Whisper model. */
	onOpenVoiceSettings?: () => void;
};

type Props = {
	state: LandingState;
	/** User messages from the active chat, oldest first. */
	sentMessages?: string[];
	onSend: (text: string, images?: ComposerImageAttachment[]) => void | Promise<void>;
	onSelectTarget: (id: string) => void;
	lagunaAdapter?: {
		adapters: LagunaPolicy[];
		selectedId: string | null;
		onSelect: (checkpointId: string | null) => void;
	};
	permissions: ComposerPermissions;
	model: ComposerModelControls;
	queue: ComposerQueue;
	turn: ComposerTurn;
	workspace: ComposerWorkspace;
	slash: ComposerSlash;
	account: ComposerAccountNav;
};

const APPROVAL_OPTIONS: Array<{ id: ApprovalPolicy; label: string; description: string }> = [
	{ id: "untrusted", label: "Always ask", description: "Ask before commands or protected actions." },
	{ id: "on-request", label: "Ask for risky actions", description: "Let the agent request approval when needed." },
	{ id: "never", label: "Never ask", description: "Run commands without approval prompts." }
];
const SANDBOX_OPTIONS: Array<{ id: SandboxMode; label: string; description: string }> = [
	{ id: "read-only", label: "Read only", description: "Inspect files without changing them." },
	{ id: "workspace-write", label: "Workspace access", description: "Read and write inside the workspace." },
	{ id: "danger-full-access", label: "Full system access", description: "Allow unrestricted filesystem and network access." }
];
const APPROVAL_CHIP_LABEL: Record<ApprovalPolicy, string> = { untrusted: "Ask", "on-request": "Risky", never: "Auto" };
const SANDBOX_CHIP_LABEL: Record<SandboxMode, string> = { "read-only": "Read", "workspace-write": "Workspace", "danger-full-access": "Full" };

function PermissionMenu({ approvalPolicy, sandboxMode, onSelect, disabled, open, onOpenChange }: {
	approvalPolicy: ApprovalPolicy;
	sandboxMode: SandboxMode;
	onSelect: (approvalPolicy: ApprovalPolicy, sandboxMode: SandboxMode) => void;
	disabled: boolean;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const ref = useRef<HTMLDivElement>(null);
	const selectedApproval = APPROVAL_OPTIONS.find((option) => option.id === approvalPolicy)!;
	const selectedSandbox = SANDBOX_OPTIONS.find((option) => option.id === sandboxMode)!;
	useEffect(() => {
		if (!open) return;
		const close = (event: MouseEvent) => { if (!ref.current?.contains(event.target as Node)) onOpenChange(false); };
		document.addEventListener("mousedown", close);
		return () => document.removeEventListener("mousedown", close);
	}, [open, onOpenChange]);
	return <div className="permission-wrap" ref={ref}>
		<button type="button" className="permission-select" disabled={disabled} onClick={() => onOpenChange(!open)} aria-label={`Permissions: ${selectedApproval.label}; ${selectedSandbox.label}`} title={`${selectedApproval.label} · ${selectedSandbox.label}`} aria-expanded={open} aria-controls="approval-mode-menu" aria-haspopup="listbox" data-testid="approval-mode-select">
			<span className="permission-select-part"><IconAsk /><span>{APPROVAL_CHIP_LABEL[approvalPolicy]}</span></span>
			<span className="permission-select-separator" aria-hidden />
			<span className="permission-select-part"><IconWorkspace /><span>{SANDBOX_CHIP_LABEL[sandboxMode]}</span></span>
			<IconChevron />
		</button>
		{open ? <div id="approval-mode-menu" className="permission-menu" aria-label="Permissions" data-testid="approval-mode-menu">
			<div className="permission-section" role="listbox" aria-label="Command approvals"><p>Command approvals</p>
				{APPROVAL_OPTIONS.map((option) => <button key={option.id} type="button" role="option" aria-selected={option.id === approvalPolicy} className={`permission-option${option.id === approvalPolicy ? " selected" : ""}`} onClick={() => onSelect(option.id, sandboxMode)}><span><strong>{option.label}</strong><small>{option.description}</small></span>{option.id === approvalPolicy ? <b aria-hidden>✓</b> : null}</button>)}
			</div>
			<div className="permission-section" role="listbox" aria-label="Runtime permissions"><p>Runtime permissions</p>
				{SANDBOX_OPTIONS.map((option) => <button key={option.id} type="button" role="option" aria-selected={option.id === sandboxMode} className={`permission-option${option.id === sandboxMode ? " selected" : ""}`} onClick={() => onSelect(approvalPolicy, option.id)}><span><strong>{option.label}</strong><small>{option.description}</small></span>{option.id === sandboxMode ? <b aria-hidden>✓</b> : null}</button>)}
			</div>
		</div> : null}
	</div>;
}

function ModelKnobMenu({ value, onSelect, knob }: {
	value: ModelKnobTransportValue;
	onSelect: (value: ModelKnobTransportValue) => void;
	knob: ModelKnobSpec;
}) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const selected = knob.options.find((option) => option.transportValue === value) ?? knob.options[0];
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
			aria-label={`${knob.label}: ${selected.displayValue}`}
			aria-expanded={open}
			aria-controls={`${knob.testId}-menu`}
			aria-haspopup="listbox"
			data-testid={`${knob.testId}-select`}
		>
			<span>{selected.displayValue}</span><IconChevron />
		</button>
		{open ? <div id={`${knob.testId}-menu`} className="reasoning-effort-menu" role="listbox" aria-label={knob.label} data-testid={`${knob.testId}-menu`}>
			{knob.options.map((option) => <button
				key={option.transportValue}
				type="button"
				role="option"
				aria-selected={option.transportValue === value}
				className={option.transportValue === value ? "selected" : ""}
				onClick={() => { onSelect(option.transportValue); setOpen(false); }}
			>
				<span>{option.displayValue}</span>{option.transportValue === value ? <b aria-hidden>✓</b> : null}
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

function IconWorkspace() {
	return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden><path d="M1.75 4.25h10.5v6.5a1 1 0 01-1 1h-8.5a1 1 0 01-1-1v-6.5Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round"/><path d="M1.75 4.25V3.5a1 1 0 011-1h2.1l1.1 1.25h5.3a1 1 0 011 1" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
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

function blobToBase64(blob: Blob): Promise<string> {
	return new Promise((resolve, reject) => {
		const reader = new FileReader();
		reader.onloadend = () => {
			const result = reader.result;
			if (typeof result !== "string") {
				reject(new Error("Unexpected FileReader result"));
				return;
			}
			const commaIndex = result.indexOf(",");
			resolve(commaIndex >= 0 ? result.slice(commaIndex + 1) : result);
		};
		reader.onerror = () => reject(reader.error ?? new Error("Failed to read recorded audio"));
		reader.readAsDataURL(blob);
	});
}

async function recordingToWhisperWav(blob: Blob): Promise<Blob> {
	const context = new AudioContext();
	try {
		const decoded = await context.decodeAudioData(await blob.arrayBuffer());
		const targetRate = 16_000;
		const outputLength = Math.max(1, Math.round(decoded.duration * targetRate));
		const pcm = new Float32Array(outputLength);
		for (let outputIndex = 0; outputIndex < outputLength; outputIndex += 1) {
			const sourceIndex = Math.min(
				decoded.length - 1,
				Math.floor((outputIndex * decoded.sampleRate) / targetRate)
			);
			let sample = 0;
			for (let channel = 0; channel < decoded.numberOfChannels; channel += 1) {
				sample += decoded.getChannelData(channel)[sourceIndex] ?? 0;
			}
			pcm[outputIndex] = sample / decoded.numberOfChannels;
		}

		const wav = new ArrayBuffer(44 + pcm.length * 2);
		const view = new DataView(wav);
		const writeAscii = (offset: number, value: string) => {
			for (let index = 0; index < value.length; index += 1) view.setUint8(offset + index, value.charCodeAt(index));
		};
		writeAscii(0, "RIFF");
		view.setUint32(4, 36 + pcm.length * 2, true);
		writeAscii(8, "WAVE");
		writeAscii(12, "fmt ");
		view.setUint32(16, 16, true);
		view.setUint16(20, 1, true);
		view.setUint16(22, 1, true);
		view.setUint32(24, targetRate, true);
		view.setUint32(28, targetRate * 2, true);
		view.setUint16(32, 2, true);
		view.setUint16(34, 16, true);
		writeAscii(36, "data");
		view.setUint32(40, pcm.length * 2, true);
		for (let index = 0; index < pcm.length; index += 1) {
			const sample = Math.max(-1, Math.min(1, pcm[index]));
			view.setInt16(44 + index * 2, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
		}
		return new Blob([wav], { type: "audio/wav" });
	} finally {
		await context.close();
	}
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

function IconStop() {
	return <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden><rect x="3" y="3" width="8" height="8" rx="1" fill="currentColor" /></svg>;
}

function IconImage() {
	return <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden><rect x="2" y="2.5" width="12" height="11" rx="2" stroke="currentColor" strokeWidth="1.3"/><circle cx="5.2" cy="5.7" r="1.15" fill="currentColor"/><path d="m3.5 11 3-3 2.2 2.1 1.7-1.7 2.1 2.6" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round"/></svg>;
}

function IconImageUnsupported() {
	return <svg className="composer-image-unsupported" width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden data-testid="composer-image-unsupported"><circle cx="7" cy="7" r="6" fill="currentColor"/><path d="M7 3.5v4.1M7 10.3v.2" stroke="white" strokeWidth="1.45" strokeLinecap="round"/></svg>;
}

function formatContextWindow(tokens: number): string {
	return `${Math.round(tokens / 1_000)}K context`;
}

function IconChevron() {
	return (
		<svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden>
			<path d="M2 3.5L5 6.5L8 3.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
		</svg>
	);
}

function modelChipLabel(state: LandingState, policy?: LagunaPolicy): string {
	const target = targetOptionForId(state.selectedTargetId);
	if (state.selectedTargetId === "local-laguna") {
		if (state.model.status === "not_installed" || state.model.status === "error") {
			return "Laguna offline";
		}
		if (state.model.status === "starting" || state.model.status === "loading") {
			return "Laguna starting…";
		}
		return policy ? policyLabel(policy) : target?.label ?? `synth/${state.model.name}`;
	}
	return target?.label ?? "Select model";
}

function composerPlaceholder(state: LandingState): string {
	if (state.selectedTargetId === "local-laguna") {
		return state.composerPlaceholder;
	}
	if (state.selectedTargetId.startsWith("synth-cloud-")) {
		if (state.apiKeyConfigured !== true) return "Configure Synth API key in Settings → Account";
		return state.cloudBlockedReason ?? "Ask anything…";
	}
	if (isOpenRouterTargetId(state.selectedTargetId)) {
		return state.openrouterApiKeyConfigured
			? "Ask anything…"
			: "Configure an OpenRouter API key in Settings → Account";
	}
	if (state.selectedTargetId.startsWith("chatgpt-")) {
		return state.codexOauthConfigured
			? "Ask anything…"
			: "Connect ChatGPT subscription in Settings → Models";
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
	if (state.selectedTargetId.startsWith("synth-cloud-")) {
		// Local models keep working when cloud spend is blocked; only the
		// billable cloud target is closed off.
		return state.apiKeyConfigured === true && !state.cloudBlockedReason;
	}
	if (isOpenRouterTargetId(state.selectedTargetId)) {
		return state.openrouterApiKeyConfigured === true;
	}
	if (state.selectedTargetId.startsWith("chatgpt-")) {
		return state.codexOauthConfigured === true;
	}
	if (state.selectedTargetId === "intern-sync" || state.selectedTargetId === "intern-async") {
		return state.internMode !== "unconfigured";
	}
	return state.composerEnabled;
}

const GROUP_ORDER: ExecutionTargetOption["group"][] = ["local", "cloud", "remote", "subscription"];

function formatSkillMention(skill: Skill): string {
	const slug = skill.name.trim().toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9-]/g, "");
	return `$${slug || skill.id}`;
}

function ModelMenu({
	state,
	modelMedianTpsLabel,
	aggregateModelTpsLabels,
	onSelectTarget,
	onConfigureAccount,
	onConfigureModels,
	onResolveBilling,
	lagunaAdapter,
	open,
	onOpenChange
}: {
	state: LandingState;
	modelMedianTpsLabel?: string | null;
	aggregateModelTpsLabels?: Readonly<Record<string, string>>;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	onConfigureModels?: () => void;
	onResolveBilling?: () => void;
	lagunaAdapter?: Props["lagunaAdapter"];
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const ref = useRef<HTMLDivElement>(null);
	const [activeAccess, setActiveAccess] = useState<ModelAccessKind | null>(null);
	const selectedLagunaPolicy = lagunaAdapter?.adapters.find((policy) =>
		policy.isBase ? lagunaAdapter.selectedId === null : policy.modelId === lagunaAdapter.selectedId
	);
	const modelLabel = modelChipLabel(state, selectedLagunaPolicy);
	const modelReady = !(
		state.selectedTargetId === "local-laguna" && state.model.status === "not_installed"
	);
	const selected = targetOptionForId(state.selectedTargetId);
	const selectedCapability = modelCapabilitiesForTarget(state.selectedTargetId);
	const selectedSupportsImages = selectedCapability?.inputModalities.includes("image") ?? false;
	const selectedThroughput = aggregateModelTpsLabels?.[state.selectedTargetId] ?? modelMedianTpsLabel ?? null;

	useEffect(() => {
		if (!open) return;
		const onDocClick = (e: MouseEvent) => {
			if (!ref.current?.contains(e.target as Node)) onOpenChange(false);
		};
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") onOpenChange(false);
		};
		document.addEventListener("mousedown", onDocClick);
		document.addEventListener("keydown", onKey);
		return () => {
			document.removeEventListener("mousedown", onDocClick);
			document.removeEventListener("keydown", onKey);
		};
	}, [open, onOpenChange]);

	const pickTarget = (targetId: string) => {
		onSelectTarget(targetId);
		onOpenChange(false);
	};

	return (
		<div className="composer-model-wrap" ref={ref}>
			<button
				type="button"
				className={`model-chip${modelReady ? "" : " is-empty"}${open ? " open" : ""}`}
				onClick={() => {
					if (!open) setActiveAccess(null);
					onOpenChange(!open);
				}}
				aria-label={`Model: ${modelLabel}`}
				aria-expanded={open}
				aria-controls="composer-model-menu"
				aria-haspopup="listbox"
				data-testid="composer-model"
			>
				<ProviderMark
					kind={providerMarkForTarget(state.selectedTargetId)}
					className={`model-chip-logo model-chip-logo-${providerMarkForTarget(state.selectedTargetId)}`}
				/>
				<span className="model-chip-label">{modelLabel}</span>
				<IconChevron />
			</button>
			{open ? (
				<div id="composer-model-menu" className="composer-model-menu" role="listbox" data-testid="composer-model-menu">
					{activeAccess === null ? MODEL_ACCESS_ORDER.map((access) => (
						<button key={access} type="button" role="option" aria-selected={selected ? modelAccessForTarget(selected) === access : false} className={`composer-model-option composer-model-access${selected && modelAccessForTarget(selected) === access ? " selected" : ""}`} data-testid={`composer-model-access-${access}`} onClick={() => setActiveAccess(access)}>
							<span className="composer-model-option-main">
								<span className="composer-model-option-label">{MODEL_ACCESS_LABEL[access]}</span>
								<span className="composer-model-option-desc">{access === "local" ? "Models on this Mac" : access === "api" ? "Synth and third-party providers" : "Your ChatGPT subscription"}</span>
							</span>
							<span aria-hidden>›</span>
						</button>
					)) : <>
						<button type="button" className="composer-model-option composer-model-access-back" data-testid="composer-model-access-back" onClick={() => setActiveAccess(null)}>
							<span className="composer-model-option-main"><span className="composer-model-option-label">‹ {MODEL_ACCESS_LABEL[activeAccess]}</span><span className="composer-model-option-desc">All access methods</span></span>
						</button>
					{GROUP_ORDER.filter((group) => {
						const sample = LAUNCH_PICKER_TARGETS.find((target) => target.group === group);
						return sample ? modelAccessForTarget(sample) === activeAccess : false;
					}).map((group) => {
						const items = LAUNCH_PICKER_TARGETS.filter((t) => t.group === group);
						if (!items.length) return null;
						return (
							<div key={group} className="composer-model-group">
								<div className="composer-model-group-label">{activeAccess === "api" ? apiProviderForTarget(items[0]) : TARGET_GROUP_LABEL[group]}</div>
								{items.map((target) => {
									if (target.id === "local-laguna" && lagunaAdapter?.adapters.length) {
										return lagunaAdapter.adapters.map((policy) => {
											const policyId = policy.isBase ? null : policy.modelId;
											const selectedHere = state.selectedTargetId === target.id && lagunaAdapter.selectedId === policyId;
											return (
												<button
													key={policy.modelId}
													type="button"
													role="option"
													data-testid={`composer-model-option-local-laguna-${policy.isBase ? "base" : policy.modelId}`}
													aria-selected={selectedHere}
													className={`composer-model-option${selectedHere ? " selected" : ""}`}
													onClick={() => {
														onSelectTarget(target.id);
														lagunaAdapter.onSelect(policyId);
														onOpenChange(false);
													}}
												>
													<span className="composer-model-option-main">
														<span className="composer-model-option-label">{policyLabel(policy)}</span>
														<span className="composer-model-option-desc">{policy.isBase ? "Base model · This Mac" : "Fine-tuned model · This Mac"}</span>
													</span>
													{selectedHere ? <span className="composer-model-check" aria-hidden>✓</span> : null}
												</button>
											);
										});
									}
									const localBlocked =
										target.id === "local-laguna" &&
										(state.model.status === "not_installed" ||
											state.model.status === "error" ||
											state.model.status === "starting" ||
											state.model.status === "loading");
									const needsSynthKey =
										target.id.startsWith("synth-cloud-") && state.apiKeyConfigured !== true;
									const needsOpenRouterKey =
										isOpenRouterTargetId(target.id) && state.openrouterApiKeyConfigured !== true;
									const needsCodexOauth =
										target.id.startsWith("chatgpt-") && state.codexOauthConfigured !== true;
									const allowanceBlocked =
										target.id.startsWith("synth-cloud-") && !needsSynthKey && Boolean(state.cloudBlockedReason);
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
									if (target.selectable === false) {
										return (
											<div key={target.id} className="composer-model-option is-disabled" data-testid={`composer-model-option-${target.id}`}>
												<span className="composer-model-option-main" role="option" aria-selected={false} aria-disabled="true">
													<span className="composer-model-option-label">{target.label}</span>
													<span className="composer-model-option-desc">{target.diagnostic ?? target.availability ?? "Unavailable"}</span>
												</span>
											</div>
										);
									}
									if (allowanceBlocked) {
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
													<span className="composer-model-option-desc" data-testid="composer-model-allowance-blocked">{state.cloudBlockedReason}</span>
												</span>
												<button
													type="button"
													className="composer-model-configure"
													data-testid="composer-model-resolve-synth-billing"
													onClick={() => {
														onResolveBilling?.();
														onOpenChange(false);
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
											<span className="composer-model-option-desc">{needsCodexOauth ? (state.codexOauthStatus?.state === "expired" ? "Authorization expired" : state.codexOauthStatus?.state === "refresh_failed" ? "Re-sync failed" : "Connect in Settings → Models") : `${providerName} API key required`}</span>
												</span>
										<button
											type="button"
											className="composer-model-configure"
											data-testid={needsCodexOauth ? "composer-model-configure-chatgpt-subscription" : `composer-model-configure-${providerName.toLowerCase()}-api-key`}
													onClick={() => {
														(needsCodexOauth ? onConfigureModels : onConfigureAccount)?.();
														onOpenChange(false);
													}}
												>
											{needsCodexOauth ? (state.codexOauthStatus?.action === "reauthenticate" || state.codexOauthStatus?.action === "retry" ? "Re-sync ChatGPT" : "Connect ChatGPT subscription") : `Configure ${providerName} API key`}
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
												{localProgress ? <span className="composer-model-option-desc">{localProgress}</span> : null}
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
					})}</>}
					{selected && activeAccess !== null ? (
						<details className="composer-model-advanced" data-testid="composer-model-advanced">
							<summary>Advanced</summary>
							<div className="composer-model-details">
								<div>
									<span>Selected model</span>
									<strong>{selected.label}</strong>
								</div>
								<div>
									<span>Runtime</span>
									<strong>{selected.description}</strong>
								</div>
								{selectedCapability ? (
									<>
										<div>
											<span>Input</span>
											<strong>{selectedSupportsImages ? "Text and images" : "Text only"}</strong>
										</div>
										<div>
											<span>Context</span>
											<strong>{selectedCapability.maxContextTokens ? formatContextWindow(selectedCapability.maxContextTokens) : "Not verified"}</strong>
										</div>
									</>
								) : null}
								{selectedThroughput ? (
									<div>
										<span>Observed speed</span>
										<strong>{selectedThroughput}</strong>
									</div>
								) : null}
							</div>
						</details>
					) : null}
				</div>
			) : null}
		</div>
	);
}

export function Composer({
	state,
	sentMessages = [],
	onSend,
	onSelectTarget,
	lagunaAdapter,
	permissions,
	model,
	queue,
	turn,
	workspace,
	slash,
	account
}: Props) {
	const { approvalPolicy, sandboxMode, onSelect: onSelectPermissions } = permissions;
	const {
		knobValues: modelKnobValues,
		onSelectKnob: onSelectModelKnob,
		medianTpsLabel: modelMedianTpsLabel,
		aggregateTpsLabels: aggregateModelTpsLabels
	} = model;
	const {
		prompts: queuedPrompts = [],
		onEnqueue,
		onEdit: onEditQueuedPrompt,
		onRemove: onRemoveQueuedPrompt,
		onPromote: onPromoteQueuedPrompt,
		afterStop: queueAfterStop = false,
		onSendNext: onSendNextQueued,
		onKeep: onKeepQueued
	} = queue;
	const {
		agentWorking = false,
		activeEnterAction = "enqueue",
		steerSupported = false,
		steerError = null,
		onSteer,
		onStop,
		sendFailure = null
	} = turn;
	const {
		sessionId: workspaceSessionId,
		onEnsureSession: onEnsureWorkspaceSession,
		fallback: workspaceFallback,
		scope: workspaceScope,
		onScopeChange: onWorkspaceScopeChange,
		onError: onWorkspaceError
	} = workspace;
	const {
		skills = [],
		onNew: onSlashNew,
		onMode: onSlashMode,
		onModel: onSlashModel,
		onMcp: onSlashMcp,
		onRename: onSlashRename,
		onCompact: onSlashCompact
	} = slash;
	const { onConfigureAccount, onConfigureModels, onResolveBilling, onOpenVoiceSettings } = account;
	const [value, setValue] = useState("");
	const [submitting, setSubmitting] = useState(false);
	const [recording, setRecording] = useState(false);
	const [transcribing, setTranscribing] = useState(false);
	const [whisperRuntime, setWhisperRuntime] = useState<WhisperRuntimeStatus | null>(null);
	const [voiceError, setVoiceError] = useState<string | null>(null);
	const [imageAttachments, setImageAttachments] = useState<ComposerImageAttachment[]>([]);
	const [attachmentError, setAttachmentError] = useState<string | null>(null);
	const [imageDragActive, setImageDragActive] = useState(false);
	const [slashDismissed, setSlashDismissed] = useState(false);
	const [skillChip, setSkillChip] = useState<Skill | null>(null);
	const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
	const [modelMenuOpen, setModelMenuOpen] = useState(false);
	// Steering lives in the turn controller, not on the queued-prompt row, so a
	// second Return works from wherever the keyboard is. The ref mirrors the
	// state because a keydown handler must read it before React commits.
	const [steer, setSteer] = useState<SteerState>(IDLE_STEER_STATE);
	const steerRef = useRef<SteerState>(IDLE_STEER_STATE);
	const armedQueuedPromptId = armedPromptId(steer);
	const promotingQueuedPromptId = promotingPromptId(steer);
	const [workspaceMenuSignal, setWorkspaceMenuSignal] = useState(0);
	const dockRef = useRef<HTMLDivElement>(null);
	const textareaRef = useRef<HTMLTextAreaElement>(null);
	const historyIndexRef = useRef<number | null>(null);
	const historyDraftRef = useRef("");
	const slashMenuRef = useRef<SlashCommandMenuHandle>(null);
	const mediaRecorderRef = useRef<MediaRecorder | null>(null);
	const audioChunksRef = useRef<Blob[]>([]);
	const streamRef = useRef<MediaStream | null>(null);
	const enabled = composerEnabled(state);
	const placeholder = composerPlaceholder(state);
	const modelCapabilities = modelCapabilitiesForTarget(state.selectedTargetId);
	const enterAction = agentWorking ? activeEnterAction : "submit";
	const alternateAction = agentWorking
		? (activeEnterAction === "enqueue" ? "steer" : "enqueue")
		: null;

	const slashMatch = /^\/(\S*)$/.exec(value);
	const slashQuery = slashMatch?.[1] ?? "";
	const slashMenuVisible = Boolean(slashMatch) && !slashDismissed;
	const approvalModeLabel = `${APPROVAL_OPTIONS.find((option) => option.id === approvalPolicy)?.label ?? ""} · ${SANDBOX_OPTIONS.find((option) => option.id === sandboxMode)?.label ?? ""}`;

	useEffect(() => {
		setValue("");
		historyIndexRef.current = null;
		historyDraftRef.current = "";
		setSkillChip(null);
		setSlashDismissed(false);
		setImageAttachments([]);
		setAttachmentError(null);
	}, [state.id]);

	useEffect(() => {
		historyIndexRef.current = null;
		historyDraftRef.current = "";
	}, [workspaceSessionId]);

	useEffect(() => {
		const handleImageDrag = (rawEvent: Event) => {
			const event = rawEvent as CustomEvent<{
				phase: "enter" | "over" | "drop" | "leave";
				position: { x: number; y: number } | null;
				images: ComposerImageAttachment[];
			}>;
			if (event.detail.phase === "leave") {
				setImageDragActive(false);
				return;
			}
			if (event.detail.phase === "enter") {
				setImageDragActive(event.detail.images.length > 0);
				return;
			}
			if (event.detail.phase === "over") return;
			if (event.detail.phase !== "drop") return;
			setImageDragActive(false);
			if (!event.detail.images.length) {
				setAttachmentError("Drop PNG, JPEG, WebP, or GIF screenshots here.");
				return;
			}
			setImageAttachments((current) => [...current, ...event.detail.images.filter((image) => !current.some((item) => item.path === image.path))].slice(0, 4));
			setAttachmentError(!modelSupportsImageInput(state.selectedTargetId) ? "This model does not support image input. Choose a multimodal model or remove the screenshots before sending." : null);
			textareaRef.current?.focus();
		};
		window.addEventListener("synth:image-drag", handleImageDrag);
		return () => window.removeEventListener("synth:image-drag", handleImageDrag);
	}, [state.selectedTargetId]);

	useEffect(() => {
		if (!slashMenuVisible) return;
		const close = (event: MouseEvent) => {
			if (!dockRef.current?.contains(event.target as Node)) setSlashDismissed(true);
		};
		document.addEventListener("mousedown", close);
		return () => document.removeEventListener("mousedown", close);
	}, [slashMenuVisible]);

	useLayoutEffect(() => {
		const dock = dockRef.current;
		const mainPane = dock?.closest<HTMLElement>(".main-pane");
		if (!dock || !mainPane) return;

		let frame = 0;
		const updateClearance = () => {
			frame = 0;
			const clearance = Math.ceil(window.innerHeight - dock.getBoundingClientRect().top + 16);
			mainPane.style.setProperty("--composer-clearance", `${clearance}px`);

			/*
			 * Horizontal geometry has the same problem as vertical clearance: the
			 * dock is an overlay, so it cannot inherit the transcript column from
			 * the workbench grid. Measure the scroller's *content* box — clientLeft
			 * and clientWidth exclude a classic scrollbar gutter, which the raw
			 * rect would fold into the centerline — and inset by the same 24px the
			 * scroller uses. This keeps the composer on the transcript's centerline
			 * and clear of the visual, container, and inference panes in every
			 * combination, including ones no static rule enumerated.
			 */
		};
		const scheduleClearanceUpdate = () => {
			if (frame) cancelAnimationFrame(frame);
			frame = requestAnimationFrame(updateClearance);
		};
		const resizeObserver = new ResizeObserver(scheduleClearanceUpdate);
		resizeObserver.observe(dock);
		resizeObserver.observe(mainPane);
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

	useEffect(() => {
		return () => {
			mediaRecorderRef.current?.stop();
			streamRef.current?.getTracks().forEach((track) => track.stop());
		};
	}, []);

	useEffect(() => {
		void bridges.whisper?.getRuntimeStatus?.().then(setWhisperRuntime).catch(() => undefined);
		return bridges.whisper?.onRuntimeStatus?.(setWhisperRuntime);
	}, []);

	const stopMicStream = () => {
		streamRef.current?.getTracks().forEach((track) => track.stop());
		streamRef.current = null;
	};

	const handleRecordingStopped = async (mimeType: string) => {
		stopMicStream();
		const chunks = audioChunksRef.current;
		audioChunksRef.current = [];
		if (!chunks.length) return;
		setTranscribing(true);
		setVoiceError(null);
		try {
			const recordedBlob = new Blob(chunks, { type: mimeType });
			const wavBlob = await recordingToWhisperWav(recordedBlob);
			const base64 = await blobToBase64(wavBlob);
			const text = await bridges.whisper?.transcribeAudio?.(base64, "audio/wav");
			if (text?.trim()) {
				setValue((current) => (current.trim().length ? `${current.trim()} ${text.trim()}` : text.trim()));
			}
		} catch (reason) {
			setVoiceError(publicError(reason));
		} finally {
			setTranscribing(false);
		}
	};

	const startRecording = async () => {
		setVoiceError(null);
		try {
			const getUserMedia = navigator.mediaDevices?.getUserMedia?.bind(navigator.mediaDevices);
			if (!getUserMedia) {
				throw new Error(
					"Microphone capture is unavailable in this app build. Restart Synth Desktop after updating; if it persists, allow microphone access in System Settings → Privacy & Security → Microphone."
				);
			}
			const stream = await getUserMedia({ audio: true });
			streamRef.current = stream;
			const mimeType = ["audio/mp4", "audio/webm"].find((candidate) => MediaRecorder.isTypeSupported(candidate)) ?? "";
			const recorder = mimeType ? new MediaRecorder(stream, { mimeType }) : new MediaRecorder(stream);
			audioChunksRef.current = [];
			recorder.ondataavailable = (event) => {
				if (event.data.size > 0) audioChunksRef.current.push(event.data);
			};
			recorder.onstop = () => {
				void handleRecordingStopped(recorder.mimeType || mimeType || "audio/webm");
			};
			mediaRecorderRef.current = recorder;
			recorder.start();
			setRecording(true);
		} catch (reason) {
			setVoiceError(publicError(reason));
			setRecording(false);
		}
	};

	const stopRecording = () => {
		mediaRecorderRef.current?.stop();
		mediaRecorderRef.current = null;
		setRecording(false);
	};

	const onMicClick = async () => {
		if (transcribing) return;
		if (recording) {
			stopRecording();
			return;
		}
		try {
			const models = (await bridges.whisper?.listModels()) ?? [];
			const selectedModel = models.find((model) => model.selected);
			if (!selectedModel || (!selectedModel.path && !selectedModel.installedBytes)) {
				onOpenVoiceSettings?.();
				return;
			}
		} catch {
			onOpenVoiceSettings?.();
			return;
		}
		// Load the model while the user is speaking, just like Laguna warms its
		// inference model before the first token is needed.
		void bridges.whisper?.warmSelected?.().catch((reason) => {
			setVoiceError(publicError(reason));
		});
		await startRecording();
	};

	const openSlashMenuFromButton = () => {
		if (!enabled) return;
		if (!/^\/(\S*)$/.test(value)) setValue("/");
		setSlashDismissed(false);
		textareaRef.current?.focus();
	};

	const closeSlashMenu = () => setSlashDismissed(true);

	const focusSkillsInSlashMenu = () => {
		setValue("/");
		setSlashDismissed(false);
	};

	const clearSlashInput = () => {
		setValue("");
		setSlashDismissed(false);
		textareaRef.current?.focus();
	};

	const handleSelectSlashCommand = (id: SlashCommandId) => {
		switch (id) {
			case "new":
				onSlashNew?.();
				break;
			case "mode":
				setPermissionMenuOpen(true);
				onSlashMode?.();
				break;
			case "workspace":
				setWorkspaceMenuSignal((value) => value + 1);
				break;
			case "model":
				setModelMenuOpen(true);
				onSlashModel?.();
				break;
			case "mcp":
				onSlashMcp?.();
				break;
			case "rename":
				onSlashRename?.();
				break;
			case "compact":
				void onSlashCompact?.();
				break;
		}
		clearSlashInput();
	};

	const handleSelectSkill = (skill: Skill) => {
		setSkillChip(skill);
		clearSlashInput();
	};

	const perform = async (intent: "submit" | "steer" | "enqueue") => {
		if (!enabled || !value.trim() || submitting) return;
		if (imageAttachments.length && !modelSupportsImageInput(state.selectedTargetId)) {
			setAttachmentError("This model does not support image input. Choose a multimodal model or remove the screenshots before sending.");
			return;
		}
		if (imageAttachments.length && intent !== "submit") {
			setAttachmentError("Screenshots can start a new turn, but cannot be queued or used to steer an active turn yet.");
			return;
		}
		const trimmed = value.trim();
		const text = skillChip ? `${formatSkillMention(skillChip)} ${trimmed}` : trimmed;
		setSubmitting(true);
		try {
			if (intent === "enqueue") {
				const queuedId = onEnqueue?.(text);
				setValue("");
				setSkillChip(null);
				if (queuedId && agentWorking && steerSupported) armQueuedPrompt(queuedId, text);
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
				setSkillChip(null);
				return;
			}
			await onSend(text, imageAttachments);
			setValue("");
			setSkillChip(null);
			setImageAttachments([]);
			setAttachmentError(null);
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

	const showHistoryValue = (next: string) => {
		setValue(next);
		setSlashDismissed(true);
		window.requestAnimationFrame(() => {
			const textarea = textareaRef.current;
			if (textarea) textarea.setSelectionRange(next.length, next.length);
		});
	};

	const navigateSentHistory = (direction: "older" | "newer") => {
		const history = sentMessages.map((message) => message.trim()).filter(Boolean);
		if (!history.length) return false;
		const current = historyIndexRef.current;
		if (direction === "older") {
			if (current === null) {
				historyDraftRef.current = value;
				historyIndexRef.current = history.length - 1;
			} else {
				historyIndexRef.current = Math.max(0, current - 1);
			}
			showHistoryValue(history[historyIndexRef.current]);
			return true;
		}
		if (current === null) return false;
		if (current < history.length - 1) {
			historyIndexRef.current = current + 1;
			showHistoryValue(history[historyIndexRef.current]);
		} else {
			historyIndexRef.current = null;
			showHistoryValue(historyDraftRef.current);
		}
		return true;
	};

	const dispatchSteer = (event: SteerEvent): SteerEffect => {
		const { state: next, effect } = reduceSteer(steerRef.current, event);
		steerRef.current = next;
		setSteer(next);
		return effect;
	};

	/**
	 * Deliver one promotion. The prompt leaves **Next turns** only when the
	 * backend acknowledges it, and a rejection is normalized before it can be
	 * rendered — a raw runtime object never reaches the composer.
	 */
	const runSteerEffect = (effect: SteerEffect): boolean => {
		if (!effect) return false;
		// The queue row is editable, so promote whatever it now says.
		const queued = queuedPrompts.find((item) => item.id === effect.promptId);
		const text = (queued?.text ?? effect.text).trim();
		if (!text) {
			dispatchSteer({ type: "disarm" });
			return false;
		}
		void Promise.resolve(onPromoteQueuedPrompt?.(effect.promptId, text))
			.then(() => dispatchSteer({ type: "acknowledged", promptId: effect.promptId }))
			.catch((reason) => {
				const failure = normalizeSteerFailure(reason);
				// The structured original stays out of the DOM and in the log.
				console.error("[steer] promotion rejected", failure.code, failure.detail);
				dispatchSteer({ type: "rejected", promptId: effect.promptId, failure });
			});
		return true;
	};

	const armQueuedPrompt = (id: string, text: string) => {
		dispatchSteer({ type: "queued", promptId: id, text, at: Date.now() });
	};

	const handleQueuedPromptEnter = (id: string, text: string) => {
		if (!agentWorking || !steerSupported || !text.trim()) return;
		if (armedPromptId(steerRef.current) === id) {
			runSteerEffect(dispatchSteer({ type: "return", composerText: "", at: Date.now() }));
			return;
		}
		armQueuedPrompt(id, text);
	};

	const promoteComposerQueueOnSecondReturn = (
		composerValue: string,
		options: { repeat: boolean; composing: boolean }
	): boolean => {
		if (!agentWorking || activeEnterAction !== "enqueue" || !steerSupported) return false;
		return runSteerEffect(
			dispatchSteer({
				type: "return",
				composerText: composerValue,
				at: Date.now(),
				repeat: options.repeat,
				composing: options.composing
			})
		);
	};

	// A prompt that vanished from the persisted queue — removed here, or
	// replaced by a reconnect — can no longer be promoted.
	const queuedPromptKey = JSON.stringify(queuedPrompts.map((item) => item.id));
	useEffect(() => {
		dispatchSteer({ type: "queueReconciled", promptIds: JSON.parse(queuedPromptKey) as string[] });
		// dispatchSteer is stable in effect; the id list is the real dependency.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [queuedPromptKey]);

	// Once the turn ends there is nothing to steer. The prompt stays queued for
	// the normal next-turn path rather than being lost or delivered twice.
	useEffect(() => {
		if (!agentWorking) dispatchSteer({ type: "turnEnded" });
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [agentWorking]);

	// One steering error surface. The promotion machine owns its own failure;
	// the direct-steer path reports through `steerError`. Both are already
	// normalized strings, so neither can render an object.
	const promotionFailure = steerFailure(steer);
	const steerMessage = promotionFailure?.message ?? (typeof steerError === "string" ? steerError : null);
	const steerErrorCode = promotionFailure?.code ?? null;

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
						<span>{armedQueuedPromptId ? "Return again to steer now" : agentWorking && steerSupported ? "Double Return to steer" : "Sent in order"}</span>
					</div>
					{queuedPrompts.map((item, index) => (
						<div key={item.id} className={`prompt-queue-item${armedQueuedPromptId === item.id ? " is-steer-armed" : ""}${promotingQueuedPromptId === item.id ? " is-promoting" : ""}`} data-testid={`queued-prompt-${item.id}`}>
							<span className="prompt-queue-index" aria-hidden>{index + 1}</span>
							<input
								className="prompt-queue-text"
								aria-label={`Queued prompt ${index + 1}`}
								value={item.text}
								onChange={(event) => onEditQueuedPrompt?.(item.id, event.target.value)}
								disabled={promotingQueuedPromptId === item.id}
								onKeyDown={(event) => {
									if (event.key !== "Enter" || event.shiftKey || event.metaKey || event.ctrlKey) return;
									if (event.nativeEvent.isComposing || event.keyCode === 229) return;
									event.preventDefault();
									if (event.repeat) return;
									handleQueuedPromptEnter(item.id, item.text);
								}}
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
			{steerMessage ? (
				<p
					className="composer-steer-error"
					role="alert"
					data-testid="steer-error"
					data-steer-error-code={steerErrorCode ?? undefined}
				>{steerMessage}</p>
			) : null}
			{sendFailure ? (
				<div className="composer-send-retry" role="status" data-testid="send-retry">
					<span>{sendFailure.message}</span>
					{sendFailure.onRetry ? (
						<button type="button" data-testid="send-retry-button" onClick={sendFailure.onRetry}>
							{sendFailure.actionLabel ?? "Retry"}
						</button>
					) : null}
				</div>
			) : null}
			{voiceError ? (
				<p className="composer-steer-error" role="alert" data-testid="composer-mic-error">{voiceError}</p>
			) : null}
			{attachmentError ? <p className="composer-steer-error" role="alert" data-testid="composer-attachment-error">{attachmentError}</p> : null}
			{armedQueuedPromptId ? (
				<p className="composer-steer-hint" role="status" aria-live="polite" data-testid="composer-steer-hint">
					Queued — Return again to steer
				</p>
			) : null}
			{isOpenRouterTargetId(state.selectedTargetId) && !state.openrouterApiKeyConfigured ? (
				<div className="composer-configuration-required" role="alert" data-testid="openrouter-key-required">
					<span><strong>OpenRouter API key required</strong> Add it under Settings → Account before sending a message.</span>
					<button type="button" onClick={onConfigureAccount} data-testid="configure-openrouter-api-key">Open Settings</button>
				</div>
			) : null}
			{state.selectedTargetId.startsWith("chatgpt-") && !state.codexOauthConfigured ? (
				<div className="composer-configuration-required" role="alert" data-testid="codex-oauth-required">
					<span><strong>{state.codexOauthStatus?.state === "expired" ? "ChatGPT authorization expired" : state.codexOauthStatus?.state === "refresh_failed" ? "ChatGPT re-sync failed" : "ChatGPT subscription required"}</strong> {state.codexOauthStatus?.guidance ?? "Connect it under Settings → Models before sending a message."}</span>
					<button type="button" onClick={onConfigureModels} data-testid="configure-codex-oauth">{state.codexOauthStatus?.action === "reauthenticate" || state.codexOauthStatus?.action === "retry" ? "Re-sync ChatGPT" : "Open Models settings"}</button>
				</div>
			) : null}
			{whisperRuntime?.phase !== "unloaded" ? (
				<p className={`composer-whisper-status is-${whisperRuntime?.phase}`} role="status" data-testid="composer-whisper-status">
					<span aria-hidden />
					{whisperRuntime?.phase === "warming" ? "Warming Whisper…"
						: whisperRuntime?.phase === "transcribing" ? "Transcribing…"
							: whisperRuntime?.phase === "ready" ? "Whisper ready · releases after 15 min idle"
								: "Whisper needs attention"}
				</p>
			) : null}
			<div className={`composer${enabled ? "" : " is-disabled"}${imageDragActive ? " is-image-drag-active" : ""}`} data-testid="composer" data-enter-action={enterAction}>
				{imageDragActive ? <div className="composer-image-drop-target" aria-hidden>Drop screenshots here</div> : null}
				{imageAttachments.length ? <div className="composer-image-tray" data-testid="composer-image-tray">{imageAttachments.map((image) => <figure key={image.path} className="composer-image-chip"><img src={image.previewUrl} alt={image.name}/><button type="button" aria-label={`Remove ${image.name}`} onClick={() => { setImageAttachments((items) => items.filter((item) => item.path !== image.path)); setAttachmentError(null); }}>×</button></figure>)}</div> : null}
				<textarea
					ref={textareaRef}
					className="composer-input"
					rows={2}
					disabled={!enabled || submitting}
					placeholder={placeholder}
					value={value}
					onChange={(e) => {
						setValue(e.target.value);
						historyIndexRef.current = null;
						historyDraftRef.current = e.target.value;
						setSlashDismissed(false);
					}}
					onKeyDown={(e) => {
						if (slashMenuVisible && slashMenuRef.current?.handleKeyDown(e)) return;
						if (e.key === "ArrowUp") {
							const atStart = e.currentTarget.selectionStart === 0 && e.currentTarget.selectionEnd === 0;
							if ((historyIndexRef.current !== null || atStart || !value) && navigateSentHistory("older")) {
								e.preventDefault();
								return;
							}
						}
						if (e.key === "ArrowDown" && historyIndexRef.current !== null && navigateSentHistory("newer")) {
							e.preventDefault();
							return;
						}
						if (e.key !== "Enter" || e.shiftKey) return;
						// An IME commit press belongs to the composition, not to the
						// composer. Let it through untouched.
						if (e.nativeEvent.isComposing || e.keyCode === 229) return;
						e.preventDefault();
						// A held Return is one instruction, not one per repeat.
						if (e.repeat) return;
						if (
							!e.metaKey &&
							!e.ctrlKey &&
							promoteComposerQueueOnSecondReturn(e.currentTarget.value, {
								repeat: e.repeat,
								composing: e.nativeEvent.isComposing
							})
						) return;
						if (e.metaKey || e.ctrlKey) submitAlternate();
						else submit();
					}}
					aria-label="Message composer"
					data-testid="composer-input"
				/>
				{skillChip ? (
					<div className="composer-skill-chip-row">
						<span className="composer-skill-chip" data-testid="composer-skill-chip">
							<IconSparkle />
							<span className="composer-skill-chip-name">{skillChip.name}</span>
							<button
								type="button"
								className="composer-skill-chip-remove"
								aria-label={`Remove ${skillChip.name} skill`}
								onClick={() => setSkillChip(null)}
							>
								<span aria-hidden>×</span>
							</button>
						</span>
					</div>
				) : null}
				<div className="composer-toolbar">
					<div className="composer-left">
						<button type="button" className="composer-icon-btn composer-image-button" aria-label={modelSupportsImageInput(state.selectedTargetId) ? "Add screenshots" : "Add screenshots — selected model does not support image input"} title={modelSupportsImageInput(state.selectedTargetId) ? "Add screenshots" : "Selected model does not support image input"} data-testid="composer-add-images" disabled={!enabled || submitting} onClick={() => void bridges.desktop.chooseImageFiles().then((images) => {
							setImageAttachments((current) => [...current, ...images.filter((image) => !current.some((item) => item.path === image.path))].slice(0, 4));
							setAttachmentError(images.length && !modelSupportsImageInput(state.selectedTargetId) ? "This model does not support image input. Choose a multimodal model or remove the screenshots before sending." : null);
						})}><IconImage />{!modelSupportsImageInput(state.selectedTargetId) ? <IconImageUnsupported /> : null}</button>
						<WorkspaceScopeChip hideTrigger openSignal={workspaceMenuSignal} sessionId={workspaceSessionId ?? null} ensureSession={onEnsureWorkspaceSession} fallbackWorkspace={workspaceFallback ?? null} scope={workspaceScope ?? null} onScopeChange={(next) => onWorkspaceScopeChange?.(next)} onError={(message) => onWorkspaceError?.(message)} />
						<div className="slash-command-wrap">
							<button
								type="button"
								className="composer-icon-btn"
								disabled={!enabled}
								aria-label="Slash commands"
								aria-haspopup="listbox"
								aria-expanded={slashMenuVisible}
								aria-controls="composer-slash-menu"
								data-testid="composer-slash-btn"
								onClick={openSlashMenuFromButton}
							>
								<IconEdit />
							</button>
							{slashMenuVisible ? (
								<SlashCommandMenu
									ref={slashMenuRef}
									query={slashQuery}
									skills={skills}
									approvalModeLabel={approvalModeLabel}
									workspaceLabel={workspaceLabel(workspaceScope?.workspace ?? workspaceFallback ?? "Workspace")}
									onSelectCommand={handleSelectSlashCommand}
									onSelectSkill={handleSelectSkill}
									onFocusSkills={focusSkillsInSlashMenu}
									onClose={closeSlashMenu}
								/>
							) : null}
						</div>
						<PermissionMenu
							approvalPolicy={approvalPolicy}
							sandboxMode={sandboxMode}
							onSelect={onSelectPermissions}
							disabled={!enabled}
							open={permissionMenuOpen}
							onOpenChange={setPermissionMenuOpen}
						/>
					</div>
					<div className="composer-right">
						<ModelMenu
							state={state}
							modelMedianTpsLabel={modelMedianTpsLabel}
							aggregateModelTpsLabels={aggregateModelTpsLabels}
							onSelectTarget={onSelectTarget}
							onConfigureAccount={onConfigureAccount}
							onConfigureModels={onConfigureModels}
							onResolveBilling={onResolveBilling}
							lagunaAdapter={lagunaAdapter}
							open={modelMenuOpen}
							onOpenChange={setModelMenuOpen}
						/>
						{modelCapabilities?.knobs.map((knob) => (
							<ModelKnobMenu
								key={knob.id}
								knob={knob}
								value={modelKnobValue(modelKnobValues, state.selectedTargetId, knob)}
								onSelect={(value) => onSelectModelKnob(state.selectedTargetId, knob.id, value)}
							/>
						))}
						<button
							type="button"
							className={`composer-icon-btn composer-mic-btn${recording ? " recording" : ""}`}
							disabled={!enabled || transcribing}
							aria-label={whisperRuntime?.phase === "warming" ? "Warming Whisper…" : transcribing ? "Transcribing voice input…" : recording ? "Stop recording" : "Voice input"}
							aria-pressed={recording}
							onClick={() => void onMicClick()}
							data-testid="composer-mic"
						>
							<IconMic />
							{recording ? <span className="sr-only" data-testid="composer-mic-recording">Recording</span> : null}
						</button>
						<button
							type="button"
							className="send-btn"
							disabled={!enabled || submitting || (!agentWorking && !value.trim())}
							onClick={agentWorking ? onStop : submit}
							aria-label={agentWorking ? "Stop generating" : sendLabel}
							data-testid={agentWorking ? "composer-stop" : "composer-send"}
							data-intent={enterAction}
						>
							{agentWorking ? <IconStop /> : <IconSend />}
						</button>
					</div>
				</div>
			</div>
		</div>
	);
}
