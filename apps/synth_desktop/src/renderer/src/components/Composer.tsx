import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
	EXECUTION_TARGETS,
	LAUNCH_PICKER_TARGETS,
	TARGET_GROUP_LABEL,
	type ExecutionTargetOption,
	type LandingState
} from "../types/landing";
import { ProviderMark, providerMarkForTarget } from "./ProviderMark";
import type { ApprovalPolicy, SandboxMode } from "../runtime/nativeCodex";
import {
	modelCapabilitiesForTarget,
	modelKnobValue,
	type ModelKnobSpec,
	type ModelKnobValue,
	type ModelKnobValues
} from "../runtime/modelCapabilities";
import { IconSparkle, SlashCommandMenu, type SlashCommandId, type SlashCommandMenuHandle } from "./SlashCommandMenu";
import type { Skill } from "../runtime/skills";
import type { ConversationWorkspaceScope } from "../env";
import { WorkspaceScopeChip, workspaceLabel } from "./WorkspaceScopeChip";

type Props = {
	state: LandingState;
	onSend: (text: string) => void;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	approvalPolicy: ApprovalPolicy;
	sandboxMode: SandboxMode;
	onSelectPermissions: (approvalPolicy: ApprovalPolicy, sandboxMode: SandboxMode) => void;
	modelKnobValues: ModelKnobValues;
	onSelectModelKnob: (targetId: string, knobId: string, value: ModelKnobValue) => void;
	modelMedianTpsLabel?: string | null;
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
	/** Opens Settings → Voice so the user can pick/download a Whisper model. */
	onOpenVoiceSettings?: () => void;
	/** Skills selectable from the "/" menu; each attaches as a removable chip. */
	skills?: Array<Skill>;
	onSlashNew?: () => void;
	onSlashMode?: () => void;
	onSlashModel?: () => void;
	onSlashMcp?: () => void;
	onSlashRename?: () => void;
	onSlashCompact?: () => void | Promise<void>;
	workspaceSessionId?: string | null;
	onEnsureWorkspaceSession?: () => Promise<string | null>;
	workspaceFallback?: string | null;
	workspaceScope?: ConversationWorkspaceScope | null;
	onWorkspaceScopeChange?: (scope: ConversationWorkspaceScope) => void;
	onWorkspaceError?: (message: string) => void;
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
		<button type="button" className="permission-select" disabled={disabled} onClick={() => onOpenChange(!open)} aria-expanded={open} aria-controls="approval-mode-menu" aria-haspopup="listbox" data-testid="approval-mode-select">
			<IconAsk /><span>{selectedApproval.label} · {selectedSandbox.label}</span><IconChevron />
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
			? "Ask anything…"
			: "Configure Synth API key in Settings → Account";
	}
	if (state.selectedTargetId.startsWith("openrouter-")) {
		return "Ask anything…";
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

function formatSkillMention(skill: Skill): string {
	const slug = skill.name.trim().toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9-]/g, "");
	return `$${slug || skill.id}`;
}

function ModelMenu({
	state,
	onSelectTarget,
	onConfigureAccount,
	open,
	onOpenChange
}: {
	state: LandingState;
	onSelectTarget: (id: string) => void;
	onConfigureAccount?: () => void;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const ref = useRef<HTMLDivElement>(null);
	const modelLabel = modelChipLabel(state);
	const modelReady = !(
		state.selectedTargetId === "local-laguna" && state.model.status === "not_installed"
	);
	const selected = EXECUTION_TARGETS.find((t) => t.id === state.selectedTargetId);

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
				onClick={() => onOpenChange(!open)}
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
					{GROUP_ORDER.map((group) => {
						const items = LAUNCH_PICKER_TARGETS.filter((t) => t.group === group);
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
														onOpenChange(false);
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
	approvalPolicy,
	sandboxMode,
	onSelectPermissions,
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
	steerError = null,
	onOpenVoiceSettings,
	skills = [],
	onSlashNew,
	onSlashMode,
	onSlashModel,
	onSlashMcp,
	onSlashRename,
	onSlashCompact,
	workspaceSessionId,
	onEnsureWorkspaceSession,
	workspaceFallback,
	workspaceScope,
	onWorkspaceScopeChange,
	onWorkspaceError
}: Props) {
	const [value, setValue] = useState("");
	const [submitting, setSubmitting] = useState(false);
	const [recording, setRecording] = useState(false);
	const [transcribing, setTranscribing] = useState(false);
	const [voiceError, setVoiceError] = useState<string | null>(null);
	const [slashDismissed, setSlashDismissed] = useState(false);
	const [skillChip, setSkillChip] = useState<Skill | null>(null);
	const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
	const [modelMenuOpen, setModelMenuOpen] = useState(false);
	const [workspaceMenuSignal, setWorkspaceMenuSignal] = useState(0);
	const dockRef = useRef<HTMLDivElement>(null);
	const textareaRef = useRef<HTMLTextAreaElement>(null);
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
		setSkillChip(null);
		setSlashDismissed(false);
	}, [state.id]);

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
			const scroller = mainPane.querySelector<HTMLElement>(".chat-transcript-scroll");
			if (!scroller) {
				mainPane.style.removeProperty("--composer-dock-left");
				mainPane.style.removeProperty("--composer-dock-right");
				return;
			}
			const paneRect = mainPane.getBoundingClientRect();
			const scrollerRect = scroller.getBoundingClientRect();
			const contentLeft = scrollerRect.left + scroller.clientLeft;
			const contentRight = contentLeft + scroller.clientWidth;
			mainPane.style.setProperty("--composer-dock-left", `${Math.round(contentLeft - paneRect.left + 24)}px`);
			mainPane.style.setProperty("--composer-dock-right", `${Math.round(paneRect.right - contentRight + 24)}px`);
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
			mainPane.style.removeProperty("--composer-dock-left");
			mainPane.style.removeProperty("--composer-dock-right");
		};
	}, []);

	useEffect(() => {
		return () => {
			mediaRecorderRef.current?.stop();
			streamRef.current?.getTracks().forEach((track) => track.stop());
		};
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
			const blob = new Blob(chunks, { type: mimeType });
			const base64 = await blobToBase64(blob);
			const text = await window.synthWhisper?.transcribeAudio?.(base64, mimeType);
			if (text?.trim()) {
				setValue((current) => (current.trim().length ? `${current.trim()} ${text.trim()}` : text.trim()));
			}
		} catch (reason) {
			setVoiceError(reason instanceof Error ? reason.message : String(reason));
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
			const mimeType = MediaRecorder.isTypeSupported("audio/webm") ? "audio/webm" : "";
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
			setVoiceError(reason instanceof Error ? reason.message : String(reason));
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
			const models = (await window.synthWhisper?.listModels()) ?? [];
			const selectedModel = models.find((model) => model.selected);
			if (!selectedModel || (!selectedModel.path && !selectedModel.installedBytes)) {
				onOpenVoiceSettings?.();
				return;
			}
		} catch {
			onOpenVoiceSettings?.();
			return;
		}
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
		const trimmed = value.trim();
		const text = skillChip ? `${formatSkillMention(skillChip)} ${trimmed}` : trimmed;
		setSubmitting(true);
		try {
			if (intent === "enqueue") {
				onEnqueue?.(text);
				setValue("");
				setSkillChip(null);
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
			onSend(text);
			setValue("");
			setSkillChip(null);
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
	const intentSummary = activeEnterAction === "enqueue" ? "Queue next" : "Steer current";
	const intentDescription = activeEnterAction === "enqueue"
		? steerSupported
			? "Enter queues the next message. Command-Enter steers the active turn."
			: "Enter queues the next message. Steering is unavailable until the runtime supports it."
		: "Enter steers the active turn. Command-Enter queues the next message.";

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
			{voiceError ? (
				<p className="composer-steer-error" role="alert" data-testid="composer-mic-error">{voiceError}</p>
			) : null}
			<div className={`composer${enabled ? "" : " is-disabled"}`} data-testid="composer" data-enter-action={enterAction}>
				<textarea
					ref={textareaRef}
					className="composer-input"
					rows={2}
					disabled={!enabled || submitting}
					placeholder={placeholder}
					value={value}
					onChange={(e) => {
						setValue(e.target.value);
						setSlashDismissed(false);
					}}
					onKeyDown={(e) => {
						if (slashMenuVisible && slashMenuRef.current?.handleKeyDown(e)) return;
						if (e.key !== "Enter" || e.shiftKey) return;
						e.preventDefault();
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
						{agentWorking ? (
							<span className="composer-intent-hint" data-testid="composer-intent-hint" aria-label={intentDescription} title={intentDescription}>
								{intentSummary}
							</span>
						) : null}
					</div>
					<div className="composer-right">
						<ModelMenu
							state={state}
							onSelectTarget={onSelectTarget}
							onConfigureAccount={onConfigureAccount}
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
							aria-label={transcribing ? "Transcribing voice input…" : recording ? "Stop recording" : "Voice input"}
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
