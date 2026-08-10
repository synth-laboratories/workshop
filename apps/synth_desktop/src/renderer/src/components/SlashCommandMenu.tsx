import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import type { Skill } from "../runtime/skills";

/** Commands we can actually honor today. Anything else (dump/load/share/…) is intentionally omitted. */
export type SlashCommandId = "new" | "workspace" | "mode" | "model" | "mcp" | "rename";

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
		case "skills":
			return <IconSparkle />;
		default:
			return <IconSlash />;
	}
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
