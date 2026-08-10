import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import type { Skill } from "../runtime/skills";

/** Commands we can actually honor today. Anything else (dump/load/share/…) is intentionally omitted. */
export type SlashCommandId = "new" | "workspace" | "mode" | "model" | "mcp" | "rename" | "compact";

type SlashCommandItem =
	| { kind: "command"; id: SlashCommandId; label: string; description: string }
	| { kind: "skill"; id: string; label: string; description: string; skill: Skill }
	| { kind: "focus-skills"; id: "skills"; label: string; description: string };

type SlashSection = { id: string; label: string; items: SlashCommandItem[] };

export type SlashCommandMenuHandle = {
	/** Returns true when the key was consumed (caller should not fall through to default handling). */
	handleKeyDown: (event: { key: string; preventDefault: () => void }) => boolean;
};

type Props = {
	/** Text typed after the leading "/", e.g. "" for "/" or "mo" for "/mo". */
	query: string;
	skills: Skill[];
	approvalModeLabel: string;
	workspaceLabel: string;
	onSelectCommand: (id: SlashCommandId) => void;
	onSelectSkill: (skill: Skill) => void;
	/** Fired when the user picks the "Skills" quick-jump row; the menu narrows to skills and the caller should clear any typed filter. */
	onFocusSkills: () => void;
	onClose: () => void;
};

function matches(query: string, ...haystacks: string[]): boolean {
	const needle = query.trim().toLowerCase();
	if (!needle) return true;
	return haystacks.some((text) => text.toLowerCase().includes(needle));
}

export const SlashCommandMenu = forwardRef<SlashCommandMenuHandle, Props>(function SlashCommandMenu(
	{ query, skills, approvalModeLabel, workspaceLabel, onSelectCommand, onSelectSkill, onFocusSkills, onClose },
	ref
) {
	// Selecting the "skills" quick-jump command narrows the list to just skills
	// until the menu is closed/reopened (fresh mount resets this).
	const [skillsOnly, setSkillsOnly] = useState(false);

	const skillItems = useMemo<SlashCommandItem[]>(
		() =>
			skills
				.filter((skill) => matches(query, skill.name, skill.description))
				.map((skill) => ({
					kind: "skill" as const,
					id: skill.id,
					label: skill.name,
					description: skill.description,
					skill
				})),
		[skills, query]
	);

	const sections = useMemo<SlashSection[]>(() => {
		if (skillsOnly) {
			return [{ id: "skills", label: "Skills", items: skillItems }];
		}

		const result: SlashSection[] = [];

		const topItems: SlashCommandItem[] = [
			{ kind: "command" as const, id: "new" as const, label: "New conversation", description: "Start a new conversation" }
		].filter((item) => matches(query, item.label, item.description, item.id));
		if (topItems.length) result.push({ id: "top", label: "", items: topItems });

	const configItems: SlashCommandItem[] = [
			{ kind: "command" as const, id: "workspace" as const, label: "Workspace", description: `Current: ${workspaceLabel}` },
			{ kind: "command" as const, id: "mode" as const, label: "Mode", description: `Current: ${approvalModeLabel}` },
			{ kind: "command" as const, id: "model" as const, label: "Model", description: "Open model picker" }
		].filter((item) => matches(query, item.label, item.description, item.id));
		if (configItems.length) result.push({ id: "config", label: "Config", items: configItems });

		if (skillItems.length) result.push({ id: "skills", label: "Skills", items: skillItems });

		const commandItems: SlashCommandItem[] = [
			{ kind: "command" as const, id: "compact" as const, label: "Compact context", description: "Summarize older context now" },
			{ kind: "command" as const, id: "mcp" as const, label: "Connectors", description: "Show connectors" },
			{ kind: "command" as const, id: "rename" as const, label: "Rename", description: "Rename session" },
			{ kind: "focus-skills" as const, id: "skills" as const, label: "Skills", description: "Browse all skills" }
		].filter((item) => matches(query, item.label, item.description, item.id));
		if (commandItems.length) result.push({ id: "commands", label: "Commands", items: commandItems });

		return result;
	}, [query, approvalModeLabel, workspaceLabel, skillItems, skillsOnly]);

	const flatItems = useMemo(() => sections.flatMap((section) => section.items), [sections]);

	const [activeIndex, setActiveIndex] = useState(0);

	useEffect(() => {
		setActiveIndex(0);
	}, [query, skillsOnly]);

	useEffect(() => {
		if (flatItems.length === 0) return;
		if (activeIndex > flatItems.length - 1) setActiveIndex(flatItems.length - 1);
	}, [flatItems.length, activeIndex]);

	const selectItem = (item: SlashCommandItem) => {
		if (item.kind === "skill") {
			onSelectSkill(item.skill);
			return;
		}
		if (item.kind === "focus-skills") {
			setSkillsOnly(true);
			onFocusSkills();
			return;
		}
		onSelectCommand(item.id);
	};

	useImperativeHandle(
		ref,
		() => ({
			handleKeyDown: (event) => {
				if (event.key === "ArrowDown") {
					event.preventDefault();
					setActiveIndex((index) => (flatItems.length ? (index + 1) % flatItems.length : 0));
					return true;
				}
				if (event.key === "ArrowUp") {
					event.preventDefault();
					setActiveIndex((index) => (flatItems.length ? (index - 1 + flatItems.length) % flatItems.length : 0));
					return true;
				}
				if (event.key === "Enter") {
					event.preventDefault();
					const item = flatItems[activeIndex];
					if (item) selectItem(item);
					return true;
				}
				if (event.key === "Escape") {
					event.preventDefault();
					onClose();
					return true;
				}
				return false;
			}
		}),
		[flatItems, activeIndex, onClose]
	);

	let runningIndex = -1;

	return (
		<div className="slash-command-menu" role="listbox" aria-label="Slash commands" data-testid="slash-command-menu">
			{flatItems.length === 0 ? <div className="slash-command-empty">No matches</div> : null}
			{sections.map((section) => (
				<div key={section.id} className="slash-command-section">
					{section.label ? <div className="slash-command-section-label">{section.label}</div> : null}
					{section.items.map((item) => {
						runningIndex += 1;
						const index = runningIndex;
						const isActive = index === activeIndex;
						return (
							<button
								key={`${section.id}-${item.id}`}
								type="button"
								role="option"
								aria-selected={isActive}
								className={`slash-command-item${isActive ? " active" : ""}`}
								data-testid={`slash-command-item-${item.id}`}
								onMouseEnter={() => setActiveIndex(index)}
								onClick={() => selectItem(item)}
							>
								<span className="slash-command-item-icon" aria-hidden>
									{item.kind === "skill" ? <IconSparkle /> : <IconForCommand id={item.id} />}
								</span>
								<span className="slash-command-item-body">
									<span className="slash-command-item-label">{item.label}</span>
									<span className="slash-command-item-desc">{item.description}</span>
								</span>
							</button>
						);
					})}
				</div>
			))}
		</div>
	);
});

function IconForCommand({ id }: { id: string }) {
	switch (id) {
		case "new":
			return <IconPlus />;
		case "mode":
			return <IconShield />;
		case "workspace":
			return <IconFolder />;
		case "model":
			return <IconCpu />;
		case "mcp":
			return <IconPlug />;
		case "rename":
			return <IconPencil />;
		case "compact":
			return <IconCompact />;
		case "skills":
			return <IconSparkle />;
		default:
			return <IconSlash />;
	}
}

function IconCompact() {
	return (
		<svg width="15" height="15" viewBox="0 0 20 20" fill="none" aria-hidden>
			<path d="M12.666 3.50098C13.3549 3.50098 13.9121 3.50133 14.3623 3.53809C14.8202 3.5755 15.2268 3.65483 15.6035 3.84668C16.1988 4.15007 16.6829 4.63424 16.9863 5.22949C17.1782 5.60603 17.2575 6.01205 17.2949 6.46973C17.3317 6.91983 17.3311 7.47721 17.3311 8.16602V15.1377C17.9209 15.3944 18.333 15.9827 18.333 16.667C18.3328 17.5872 17.5872 18.3328 16.667 18.333C15.7466 18.333 15.0002 17.5873 15 16.667C15 15.9832 15.4119 15.3957 16.001 15.1387V8.16602C16.001 7.45532 16.0011 6.96153 15.9697 6.57812C15.939 6.20279 15.8822 5.99093 15.8018 5.83301C15.6258 5.4879 15.3442 5.20711 14.999 5.03125C14.8411 4.95091 14.6291 4.89394 14.2539 4.86328C13.8705 4.83199 13.3767 4.83105 12.666 4.83105H7.5C7.13284 4.83092 6.83496 4.5332 6.83496 4.16602C6.8353 3.79912 7.13305 3.50111 7.5 3.50098H12.666Z" fill="currentColor" />
			<path d="M3.33301 1.66699C4.25337 1.66699 4.99981 2.41269 5 3.33301C5 4.01711 4.58759 4.60453 3.99805 4.86133V11.833C3.99805 12.5438 3.99896 13.0374 4.03027 13.4209C4.06095 13.7963 4.11783 14.008 4.19824 14.166C4.37411 14.5112 4.6549 14.7918 5 14.9678C5.15797 15.0483 5.36958 15.105 5.74512 15.1357C6.12859 15.1671 6.6221 15.168 7.33301 15.168H12.5L12.6338 15.1816C12.9367 15.2437 13.1649 15.5118 13.165 15.833C13.165 16.1543 12.9368 16.4223 12.6338 16.4844L12.5 16.498H7.33301C6.64403 16.498 6.08691 16.4987 5.63672 16.4619C5.17904 16.4245 4.77303 16.3451 4.39648 16.1533C3.8011 15.8499 3.31608 15.365 3.0127 14.7695C2.82102 14.393 2.7415 13.987 2.7041 13.5293C2.66734 13.0791 2.66797 12.5219 2.66797 11.833V4.86035C2.07898 4.60332 1.66699 4.0167 1.66699 3.33301C1.66718 2.41283 2.41284 1.66721 3.33301 1.66699Z" fill="currentColor" />
			<path d="M10.1338 11.0146C10.4366 11.0766 10.6647 11.345 10.665 11.666C10.665 11.9873 10.4367 12.2553 10.1338 12.3174L10 12.3311H7.5C7.13284 12.3309 6.83496 12.0332 6.83496 11.666C6.8353 11.2991 7.13305 11.0011 7.5 11.001H10L10.1338 11.0146Z" fill="currentColor" />
			<path d="M12.6338 7.68164C12.9367 7.74367 13.1649 8.01182 13.165 8.33301C13.165 8.65433 12.9368 8.92232 12.6338 8.98438L12.5 8.99805H7.5C7.13284 8.99791 6.83496 8.7002 6.83496 8.33301C6.83513 7.96596 7.13294 7.6681 7.5 7.66797H12.5L12.6338 7.68164Z" fill="currentColor" />
		</svg>
	);
}

function IconFolder() { return <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden><path d="M2.25 4.25h4l1.2 1.4h6.3v6.1a1 1 0 01-1 1h-9.5a1 1 0 01-1-1v-7.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>; }

export function IconSparkle() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M8 2.4l1.02 2.98a2 2 0 001.6 1.6L13.6 8l-2.98 1.02a2 2 0 00-1.6 1.6L8 13.6l-1.02-2.98a2 2 0 00-1.6-1.6L2.4 8l2.98-1.02a2 2 0 001.6-1.6L8 2.4z"
				stroke="currentColor"
				strokeWidth="1.15"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconPlus() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M8 3.25v9.5M3.25 8h9.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}

function IconShield() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M8 2.3l4.5 1.6v3.5c0 3.2-1.95 5.4-4.5 6.3-2.55-.9-4.5-3.1-4.5-6.3V3.9L8 2.3z"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconCpu() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="4.5" y="4.5" width="7" height="7" rx="1.2" stroke="currentColor" strokeWidth="1.2" />
			<path
				d="M8 2v1.4M8 12.6V14M2 8h1.4M12.6 8H14M2 5.5h1.4M2 10.5h1.4M12.6 5.5H14M12.6 10.5H14"
				stroke="currentColor"
				strokeWidth="1.1"
				strokeLinecap="round"
			/>
		</svg>
	);
}

function IconPlug() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M6 2.5v2.7M10 2.5v2.7M4.5 5.2h7v2.1a3.5 3.5 0 01-3.5 3.5 3.5 3.5 0 01-3.5-3.5V5.2zM8 10.8v2.7"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconPencil() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path
				d="M11.1 2.9a1.4 1.4 0 012 2L5.4 12.6l-2.7.7.7-2.7L11.1 2.9z"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

function IconSlash() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M10.5 2.5l-5 11" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
		</svg>
	);
}
