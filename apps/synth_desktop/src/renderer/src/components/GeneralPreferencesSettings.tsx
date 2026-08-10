import { useEffect, useId, useRef, useState } from "react";
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

type Props = {
	preferences: DesktopPreferences;
	onPreferencesChange: (prefs: DesktopPreferences) => void;
	conversationTitles?: Record<string, string>;
	onUnarchive?: (id: string) => void;
	onOpenConversation?: (id: string) => void;
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

function NumericSetting({
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
	useEffect(() => { setDraft(String(value)); setError(null); }, [value]);
	return (
		<label className="pref-field">
			<span>{label}</span>
			<input
				type="number"
				min={min}
				max={max}
				value={draft}
				data-testid={testId}
				aria-invalid={Boolean(error)}
				aria-describedby={error ? `${testId}-error` : undefined}
				onChange={(event) => setDraft(event.target.value)}
				onBlur={() => {
					const next = Number(draft);
					if (!Number.isFinite(next) || next < min || next > max) {
						setError(`Enter a number between ${min} and ${max}`);
						setDraft(String(value));
						return;
					}
					setError(null);
					onChange(Math.round(next));
				}}
			/>
			{error ? <span id={`${testId}-error`} className="pref-field-error" role="alert" data-testid={`${testId}-error`}>{error}</span> : null}
		</label>
	);
}

export function GeneralPreferencesSettings({
	preferences,
	onPreferencesChange,
	conversationTitles = {},
	onUnarchive,
	onOpenConversation
}: Props) {
	const archivedIds = listArchivedConversationIds(preferences);
	const shortcutsId = useId();

	return (
		<div className="settings-finetunes" data-testid="settings-general">
			<header className="settings-section-head">
				<div>
					<h2>General</h2>
					<p>Appearance, fonts, submission, tool activity, and layout defaults.</p>
				</div>
			</header>

			<section className="pref-section" aria-labelledby="pref-appearance" data-testid="settings-appearance">
				<h3 id="pref-appearance">Appearance</h3>
				<div className="pref-chip-row" role="radiogroup" aria-label="Theme">
					{THEME_OPTIONS.map((option) => (
						<button
							key={option.id}
							type="button"
							role="radio"
							aria-checked={preferences.appearance.theme === option.id}
							className={preferences.appearance.theme === option.id ? "active" : ""}
							data-testid={`theme-${option.id}`}
							onClick={() => onPreferencesChange(setTheme(option.id))}
						>
							{option.label}
						</button>
					))}
				</div>
				<div className="pref-grid">
					<NumericSetting
						label="Chat font size"
						value={preferences.appearance.chatFontSize}
						min={12}
						max={22}
						testId="chat-font-size"
						onChange={(chatFontSize) => onPreferencesChange(setAppearanceFonts({ chatFontSize }))}
					/>
					<NumericSetting
						label="Code font size"
						value={preferences.appearance.codeFontSize}
						min={10}
						max={20}
						testId="code-font-size"
						onChange={(codeFontSize) => onPreferencesChange(setAppearanceFonts({ codeFontSize }))}
					/>
					<label className="pref-field">
						<span>Code font family</span>
						<input
							type="text"
							value={preferences.appearance.codeFontFamily}
							data-testid="code-font-family"
							onChange={(event) => onPreferencesChange(setAppearanceFonts({ codeFontFamily: event.target.value }))}
						/>
					</label>
					<NumericSetting
						label="Terminal font size"
						value={preferences.appearance.terminalFontSize}
						min={10}
						max={20}
						testId="terminal-font-size"
						onChange={(terminalFontSize) => onPreferencesChange(setAppearanceFonts({ terminalFontSize }))}
					/>
				</div>
			</section>

			<section className="pref-section" aria-labelledby="pref-submission" data-testid="settings-submission">
				<h3 id="pref-submission">Prompt submission</h3>
				<p className="settings-runtime-copy">While an agent is working, Enter performs the preferred action. ⌘Enter performs the alternate. When idle, Enter always submits normally.</p>
				<div className="pref-option-list" role="radiogroup" aria-label="Active turn Enter behavior">
					{ENTER_OPTIONS.map((option) => (
						<button
							key={option.id}
							type="button"
							role="radio"
							aria-checked={preferences.submission.activeEnterAction === option.id}
							className={`pref-option${preferences.submission.activeEnterAction === option.id ? " active" : ""}`}
							data-testid={`active-enter-${option.id}`}
							onClick={() => onPreferencesChange(setActiveEnterAction(option.id))}
						>
							<strong>{option.label}</strong>
							<small>{option.description}</small>
						</button>
					))}
				</div>
			</section>

			<section className="pref-section" aria-labelledby="pref-activity" data-testid="settings-tool-activity">
				<h3 id="pref-activity">Tool activity</h3>
				<div className="pref-option-list" role="radiogroup" aria-label="Tool activity presentation">
					{ACTIVITY_OPTIONS.map((option) => (
						<button
							key={option.id}
							type="button"
							role="radio"
							aria-checked={preferences.toolActivity.mode === option.id}
							className={`pref-option${preferences.toolActivity.mode === option.id ? " active" : ""}`}
							data-testid={`tool-activity-${option.id}`}
							onClick={() => onPreferencesChange(setToolActivityMode(option.id))}
						>
							<strong>{option.label}</strong>
							<small>{option.description}</small>
						</button>
					))}
				</div>
			</section>

			<section className="pref-section" aria-labelledby="pref-agent-context" data-testid="settings-agent-context">
				<h3 id="pref-agent-context">Agent context</h3>
				<p className="settings-runtime-copy">Laguna S and Luna default to 250,000 tokens; Laguna XS defaults to 80% of its smaller context window. Codex summarizes older context at the selected threshold; changes apply on the next turn.</p>
				<div className="pref-grid">
					<NumericSetting
						label="Laguna XS (262,144 max)"
						value={preferences.agentContext.autoCompactTokenLimits.lagunaXs}
						min={16_000}
						max={235_929}
						testId="auto-compact-token-limit-laguna-xs"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("lagunaXs", limit))}
					/>
					<NumericSetting
						label="Laguna S (1,050,000 max)"
						value={preferences.agentContext.autoCompactTokenLimits.lagunaS}
						min={16_000}
						max={945_000}
						testId="auto-compact-token-limit-laguna-s"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("lagunaS", limit))}
					/>
					<NumericSetting
						label="Luna (1,050,000 max)"
						value={preferences.agentContext.autoCompactTokenLimits.luna}
						min={16_000}
						max={945_000}
						testId="auto-compact-token-limit-luna"
						onChange={(limit) => onPreferencesChange(setAutoCompactTokenLimit("luna", limit))}
					/>
				</div>
			</section>

			<section className="pref-section" aria-labelledby="pref-layout" data-testid="settings-layout">
				<h3 id="pref-layout">Layout</h3>
				<div className="pref-chip-row">
					<button type="button" data-testid="save-layout-default" onClick={() => onPreferencesChange(saveLayoutAsDefault())}>
						Save current as default
					</button>
					<button type="button" data-testid="apply-layout-default" onClick={() => onPreferencesChange(applyDefaultLayout())}>
						Apply default
					</button>
					<button type="button" data-testid="reset-layout" onClick={() => onPreferencesChange(resetLayoutToDefault())}>
						Reset layout
					</button>
				</div>
			</section>

			<section className="pref-section" aria-labelledby={shortcutsId} data-testid="settings-shortcuts">
				<h3 id={shortcutsId}>Keyboard shortcuts</h3>
				<ul className="pref-shortcut-list">
					<li><kbd>Enter</kbd> Submit or preferred active-turn action</li>
					<li><kbd>⌘</kbd>+<kbd>Enter</kbd> Alternate active-turn action</li>
					<li><kbd>⌘</kbd>+<kbd>K</kbd> Search conversations</li>
					<li><kbd>⌘</kbd>+<kbd>J</kbd> Toggle terminal</li>
					<li><kbd>Esc</kbd> Close menus and dialogs</li>
				</ul>
			</section>

			<section className="pref-section" aria-labelledby="pref-archived" data-testid="settings-archived-chats">
				<h3 id="pref-archived">Archived chats</h3>
				{archivedIds.length === 0 ? (
					<p className="empty-hint" data-testid="archived-chats-empty">No archived conversations</p>
				) : (
					<ul className="archived-chat-list">
						{archivedIds.map((id) => (
							<li key={id}>
								<button type="button" className="archived-chat-open" onClick={() => onOpenConversation?.(id)} data-testid={`archived-chat-${id}`}>
									{conversationTitles[id] ?? preferences.conversations[id]?.titleOverride ?? id}
								</button>
								<button type="button" onClick={() => onUnarchive?.(id)} data-testid={`unarchive-chat-${id}`}>Unarchive</button>
							</li>
						))}
					</ul>
				)}
			</section>

			<section className="pref-section" aria-labelledby="pref-reset" data-testid="settings-reset">
				<h3 id="pref-reset">Reset</h3>
				<button type="button" className="settings-secondary-btn" data-testid="reset-preferences" onClick={() => onPreferencesChange(resetPreferences())}>
					Restore documented defaults
				</button>
			</section>
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
