import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { ActiveEnterAction, DesktopPreferences, ThemePreference, ToolActivityMode } from "../preferences";
import {
	applyDefaultLayout,
	listArchivedConversationIds,
	resetLayoutToDefault,
	resetPreferences,
	saveLayoutAsDefault,
	setActiveEnterAction,
	setAutoCompactTokenLimit,
	setAppearanceFonts,
	setTheme,
	setToolActivityMode
} from "../preferences";
import { SettingsCard, SettingsRow } from "./SettingsCard";

type Props = {
	preferences: DesktopPreferences;
	onPreferencesChange: (prefs: DesktopPreferences) => void;
};

const THEME_OPTIONS: Array<{ id: ThemePreference; label: string }> = [
	{ id: "system", label: "System" },
	{ id: "light", label: "Light" },
	{ id: "dark", label: "Dark" }
];

const ACTIVITY_OPTIONS: Array<{ id: ToolActivityMode; label: string; description: string }> = [
	{ id: "detailed", label: "Detailed", description: "Show every tool and progress event." },
	{ id: "grouped", label: "Grouped", description: "Group adjacent activity; expand for chronology." },
	{ id: "compact", label: "Compact", description: "Current activity plus a concise summary." }
];

const ENTER_OPTIONS: Array<{ id: ActiveEnterAction; label: string; description: string }> = [
	{ id: "enqueue", label: "Enqueue", description: "Enter queues the next turn while the agent works. ⌘Enter steers when supported." },
	{ id: "steer", label: "Steer", description: "Enter steers the active turn when supported. ⌘Enter enqueues." }
];


/** Named fonts presented instead of raw CSS stacks; values stay full stacks. */
const CODE_FONT_CHOICES: Array<{ label: string; value: string }> = [
	{ label: "System monospace", value: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace" },
	{ label: "SF Mono", value: '"SF Mono", ui-monospace, SFMono-Regular, Menlo, monospace' },
	{ label: "Menlo", value: "Menlo, ui-monospace, monospace" },
	{ label: "Monaco", value: "Monaco, ui-monospace, monospace" },
	{ label: "JetBrains Mono", value: '"JetBrains Mono", ui-monospace, Menlo, monospace' },
	{ label: "Fira Code", value: '"Fira Code", ui-monospace, Menlo, monospace' }
];

function SegmentedControl<T extends string>({
	ariaLabel,
	options,
	value,
	onChange,
	testIdPrefix
}: {
	ariaLabel: string;
	options: ReadonlyArray<{ id: T; label: string; description?: string }>;
	value: T;
	onChange: (id: T) => void;
	testIdPrefix: string;
}) {
	return (
		<div className="seg-control" role="radiogroup" aria-label={ariaLabel}>
			{options.map((option) => (
				<button
					key={option.id}
					type="button"
					role="radio"
					aria-checked={value === option.id}
					className={value === option.id ? "active" : ""}
					title={option.description}
					data-testid={`${testIdPrefix}-${option.id}`}
					onClick={() => onChange(option.id)}
				>
					{option.label}
				</button>
			))}
		</div>
	);
}

function NumericInput({
	label,
	value,
	min,
	max,
	onChange,
	testId
}: {
	label: string;
	value: number;
	min: number;
	max: number;
	onChange: (value: number) => void;
	testId: string;
}) {
	const [draft, setDraft] = useState(String(value));
	const [error, setError] = useState<string | null>(null);
	const [open, setOpen] = useState(false);
	useEffect(() => { setDraft(String(value)); setError(null); }, [value]);
	const apply = () => {
		const next = Number(draft);
		if (!Number.isFinite(next) || next < min || next > max) {
			setError(`Enter a number between ${min.toLocaleString()} and ${max.toLocaleString()}`);
			return;
		}
		setError(null);
		onChange(Math.round(next));
		setOpen(false);
	};
	return (
		<OptionModal
			label={label}
			value={value.toLocaleString()}
			open={open}
			onOpen={() => { setDraft(String(value)); setError(null); setOpen(true); }}
			onClose={() => setOpen(false)}
			onApply={apply}
		>
			<label className="settings-option-field" htmlFor={testId}>
				<span>Value</span>
				<input id={testId} type="text" inputMode="numeric" value={draft} data-testid={testId} aria-invalid={Boolean(error)} aria-describedby={error ? `${testId}-error` : undefined} onChange={(event) => setDraft(event.target.value.replace(/[^0-9]/g, ""))} onKeyDown={(event) => { if (event.key === "Enter") apply(); }} autoFocus />
				<small>Allowed range: {min.toLocaleString()}–{max.toLocaleString()}</small>
			</label>
			{error ? <span id={`${testId}-error`} className="pref-field-error" role="alert" data-testid={`${testId}-error`}>{error}</span> : null}
		</OptionModal>
	);
}

function OptionModal({ label, value, open, onOpen, onClose, onApply, children }: { label: string; value: string; open: boolean; onOpen: () => void; onClose: () => void; onApply?: () => void; children: ReactNode }) {
	const dialog = useRef<HTMLDialogElement>(null);
	useEffect(() => {
		if (open && !dialog.current?.open) dialog.current?.showModal();
		if (!open && dialog.current?.open) dialog.current.close();
	}, [open]);
	return <div className="settings-option-control">
		<button type="button" className="settings-option-trigger" aria-haspopup="dialog" onClick={onOpen}><span>{value}</span><em>Change</em></button>
		<dialog ref={dialog} className="settings-option-dialog" aria-label={`Change ${label}`} onCancel={(event) => { event.preventDefault(); onClose(); }} onClose={onClose}>
			<header><div><span>Preference</span><h3>{label}</h3></div><button type="button" aria-label="Close" onClick={onClose}>×</button></header>
			<div className="settings-option-dialog-body">{children}</div>
			<footer><button type="button" onClick={onClose}>Cancel</button>{onApply ? <button type="button" className="primary" onClick={onApply}>Apply</button> : null}</footer>
		</dialog>
	</div>;
}

function ChoiceInput({ label, value, choices, onChange, testId }: { label: string; value: string; choices: ReadonlyArray<{ label: string; value: string }>; onChange: (value: string) => void; testId: string }) {
	const [open, setOpen] = useState(false);
	const current = choices.find((choice) => choice.value === value)?.label ?? "Custom";
	return <OptionModal label={label} value={current} open={open} onOpen={() => setOpen(true)} onClose={() => setOpen(false)}>
		<div className="settings-option-list" role="radiogroup" aria-label={label} data-testid={testId}>
			{choices.map((choice) => <button key={choice.value} type="button" role="radio" aria-checked={choice.value === value} className={choice.value === value ? "selected" : ""} onClick={() => { onChange(choice.value); setOpen(false); }}><span>{choice.label}</span><i aria-hidden>{choice.value === value ? "✓" : ""}</i></button>)}
		</div>
	</OptionModal>;
}

export function GeneralPreferencesSettings({ preferences, onPreferencesChange }: Props) {
	const codeFontFamily = preferences.appearance.codeFontFamily;

	return (
		<div className="settings-sections" data-testid="settings-general">
			<SettingsCard title="Appearance" testId="settings-appearance">
				<SettingsRow label="Theme">
					<SegmentedControl
						ariaLabel="Theme"
						options={THEME_OPTIONS}
						value={preferences.appearance.theme}
						testIdPrefix="theme"
						onChange={(theme) => onPreferencesChange(setTheme(theme))}
					/>
				</SettingsRow>
				<div className="settings-item-subhead" aria-hidden>Fonts</div>
				<SettingsRow label="Chat font size" htmlFor="chat-font-size">
					<NumericInput
						label="Chat font size"
						value={preferences.appearance.chatFontSize}
						min={12}
						max={22}
						testId="chat-font-size"
						onChange={(chatFontSize) => onPreferencesChange(setAppearanceFonts({ chatFontSize }))}
					/>
				</SettingsRow>
				<SettingsRow label="Code font size" htmlFor="code-font-size">
					<NumericInput
						label="Code font size"
						value={preferences.appearance.codeFontSize}
						min={10}
						max={20}
						testId="code-font-size"
						onChange={(codeFontSize) => onPreferencesChange(setAppearanceFonts({ codeFontSize }))}
					/>
				</SettingsRow>
				<SettingsRow label="Code font" htmlFor="code-font-family">
					<ChoiceInput label="Code font" value={codeFontFamily} choices={CODE_FONT_CHOICES} testId="code-font-family" onChange={(codeFontFamily) => onPreferencesChange(setAppearanceFonts({ codeFontFamily }))} />
				</SettingsRow>
				<SettingsRow label="Terminal font size" htmlFor="terminal-font-size">
					<NumericInput
						label="Terminal font size"
						value={preferences.appearance.terminalFontSize}
						min={10}
						max={20}
						testId="terminal-font-size"
						onChange={(terminalFontSize) => onPreferencesChange(setAppearanceFonts({ terminalFontSize }))}
					/>
				</SettingsRow>
			</SettingsCard>

			<SettingsCard
				title="Prompt submission"
				description="While an agent is working, Enter performs the preferred action and ⌘Enter the alternate. When idle, Enter submits."
				testId="settings-submission"
			>
				<SettingsRow label="Enter while working">
					<SegmentedControl
						ariaLabel="Active turn Enter behavior"
						options={ENTER_OPTIONS}
						value={preferences.submission.activeEnterAction}
						testIdPrefix="active-enter"
						onChange={(action) => onPreferencesChange(setActiveEnterAction(action))}
					/>
				</SettingsRow>
			</SettingsCard>


			<SettingsCard title="Tool activity" testId="settings-tool-activity">
				<SettingsRow label="Presentation">
					<SegmentedControl
						ariaLabel="Tool activity presentation"
						options={ACTIVITY_OPTIONS}
						value={preferences.toolActivity.mode}
						testIdPrefix="tool-activity"
						onChange={(mode) => onPreferencesChange(setToolActivityMode(mode))}
					/>
				</SettingsRow>
			</SettingsCard>

			<SettingsCard
				title="Agent context"
				description="Older context is summarized at the threshold; changes apply on the next turn."
				testId="settings-agent-context"
			>
				<SettingsRow label="Laguna XS" description="Model max 262,144 tokens" htmlFor="auto-compact-token-limit-laguna-xs">
					<NumericInput
						label="Laguna XS auto-compact token limit"
						value={preferences.agentContext.autoCompactTokenLimits.lagunaXs}
						min={16_001}
						max={235_929}
						testId="auto-compact-token-limit-laguna-xs"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("lagunaXs", limit))}
					/>
				</SettingsRow>
				<SettingsRow label="Laguna S" description="Model max 1,050,000 tokens" htmlFor="auto-compact-token-limit-laguna-s">
					<NumericInput
						label="Laguna S auto-compact token limit"
						value={preferences.agentContext.autoCompactTokenLimits.lagunaS}
						min={16_001}
						max={945_000}
						testId="auto-compact-token-limit-laguna-s"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("lagunaS", limit))}
					/>
				</SettingsRow>
				<SettingsRow label="Luna" description="Model max 1,050,000 tokens" htmlFor="auto-compact-token-limit-luna">
					<NumericInput
						label="Luna auto-compact token limit"
						value={preferences.agentContext.autoCompactTokenLimits.luna}
						min={16_001}
						max={945_000}
						testId="auto-compact-token-limit-luna"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("luna", limit))}
					/>
				</SettingsRow>
			</SettingsCard>

			<SettingsCard title="Layout" testId="settings-layout">
				<SettingsRow label="Window layout">
					<div className="settings-btn-row">
						<button type="button" className="settings-secondary-btn" data-testid="save-layout-default" onClick={() => onPreferencesChange(saveLayoutAsDefault())}>
							Save current as default
						</button>
						<button type="button" className="settings-secondary-btn" data-testid="apply-layout-default" onClick={() => onPreferencesChange(applyDefaultLayout())}>
							Apply default
						</button>
						<button type="button" className="settings-secondary-btn" data-testid="reset-layout" onClick={() => onPreferencesChange(resetLayoutToDefault())}>
							Reset layout
						</button>
					</div>
				</SettingsRow>
			</SettingsCard>

			<div className="settings-reset-row" data-testid="settings-reset">
				<button type="button" className="settings-secondary-btn settings-reset-btn" data-testid="reset-preferences" onClick={() => onPreferencesChange(resetPreferences())}>
					Restore documented defaults
				</button>
			</div>
		</div>
	);
}

const SHORTCUTS: Array<{ keys: string[]; label: string }> = [
	{ keys: ["Enter"], label: "Submit, or the preferred active-turn action" },
	{ keys: ["⌘", "Enter"], label: "Alternate active-turn action" },
	{ keys: ["⌘", "K"], label: "Search conversations" },
	{ keys: ["⌘", "J"], label: "Toggle terminal" },
	{ keys: ["Esc"], label: "Close menus and dialogs" }
];

export function KeyboardShortcutsSettings() {
	return (
		<div className="settings-sections">
			<SettingsCard ariaLabel="Keyboard shortcuts" testId="settings-shortcuts">
				<ul className="pref-shortcut-list">
					{SHORTCUTS.map((shortcut) => (
						<li key={shortcut.label}>
							<span>{shortcut.label}</span>
							<span className="pref-shortcut-keys">
								{shortcut.keys.map((key) => <kbd key={key}>{key}</kbd>)}
							</span>
						</li>
					))}
				</ul>
			</SettingsCard>
		</div>
	);
}

export function ArchivedChatsSettings({
	preferences,
	conversationTitles = {},
	onUnarchive,
	onOpenConversation
}: {
	preferences: DesktopPreferences;
	conversationTitles?: Record<string, string>;
	onUnarchive?: (id: string) => void;
	onOpenConversation?: (id: string) => void;
}) {
	const archivedIds = listArchivedConversationIds(preferences);
	return (
		<div className="settings-sections">
			<SettingsCard ariaLabel="Archived chats" testId="settings-archived-chats">
				{archivedIds.length === 0 ? (
					<p className="empty-hint settings-empty-hint" data-testid="archived-chats-empty">No archived conversations</p>
				) : (
					<ul className="archived-chat-list">
						{archivedIds.map((id) => (
							<li key={id}>
								<button type="button" className="archived-chat-open" onClick={() => onOpenConversation?.(id)} data-testid={`archived-chat-${id}`}>
									{conversationTitles[id] ?? preferences.conversations[id]?.titleOverride ?? id}
								</button>
								<button type="button" className="settings-secondary-btn" onClick={() => onUnarchive?.(id)} data-testid={`unarchive-chat-${id}`}>Unarchive</button>
							</li>
						))}
					</ul>
				)}
			</SettingsCard>
		</div>
	);
}

/** Context menu for conversation rows — keyboard operable. */
export function ConversationContextMenu({
	open,
	x,
	y,
	conversationId,
	pinned,
	archived,
	working,
	onClose,
	onRename,
	onPin,
	onArchive
}: {
	open: boolean;
	x: number;
	y: number;
	conversationId: string;
	pinned: boolean;
	archived: boolean;
	working: boolean;
	onClose: () => void;
	onRename: (id: string) => void;
	onPin: (id: string, pinned: boolean) => void;
	onArchive: (id: string, archived: boolean) => void;
}) {
	const ref = useRef<HTMLDivElement>(null);
	useEffect(() => {
		if (!open) return;
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				onClose();
				return;
			}
			const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
			if (buttons.length === 0) return;
			const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
			const next = event.key === "ArrowDown" ? (current + 1) % buttons.length
				: event.key === "ArrowUp" ? (current - 1 + buttons.length) % buttons.length
					: event.key === "Home" ? 0
						: event.key === "End" ? buttons.length - 1
							: null;
			if (next === null) return;
			event.preventDefault();
			buttons[next]?.focus();
		};
		const onPointer = (event: MouseEvent) => {
			if (!ref.current?.contains(event.target as Node)) onClose();
		};
		document.addEventListener("keydown", onKey);
		document.addEventListener("mousedown", onPointer);
		ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
		return () => {
			document.removeEventListener("keydown", onKey);
			document.removeEventListener("mousedown", onPointer);
		};
	}, [open, onClose]);

	if (!open) return null;
	return (
		<div
			ref={ref}
			className="conversation-context-menu"
			role="menu"
			aria-label="Conversation actions"
			data-testid={`conversation-menu-${conversationId}`}
			style={{ left: x, top: y }}
		>
			<button type="button" role="menuitem" onClick={() => { onRename(conversationId); onClose(); }}>Rename</button>
			<button type="button" role="menuitem" onClick={() => { onPin(conversationId, !pinned); onClose(); }}>
				{pinned ? "Unpin" : "Pin"}
			</button>
			<button
				type="button"
				role="menuitem"
				disabled={working && !archived}
				title={working && !archived ? "Stop the run before archiving" : undefined}
				aria-disabled={working && !archived}
				onClick={() => {
					if (working && !archived) return;
					onArchive(conversationId, !archived);
					onClose();
				}}
			>
				{archived ? "Unarchive" : "Archive"}
			</button>
		</div>
	);
}
