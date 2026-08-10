import type { LocalActivityLine } from "../types/landing";
import type { ToolActivityMode } from "./schema";

export type ActivityPresentationItem =
	| { kind: "line"; line: LocalActivityLine }
	| {
		kind: "group";
		id: string;
		label: string;
		summary: string;
		count: number;
		status: "running" | "completed" | "failed" | "cancelled" | "interrupted" | "unhealthy" | "mixed";
		lines: LocalActivityLine[];
		expanded: boolean;
	};

export type ActivityTimelineRow = {
	id: string;
	context: LocalActivityLine[];
	action?: LocalActivityLine;
};

// Grouped mode mirrors Codex's activity disclosures: a run of concrete tool
// calls is one scannable row, while expansion restores every call and the
// reasoning updates that occurred between them in their original order.
const CONNECTIVE_KINDS = new Set(["thought", "working"]);

/** Pair reasoning/working context with the concrete call that follows it. */
export function pairActivityGroupLines(lines: LocalActivityLine[]): ActivityTimelineRow[] {
	const rows: ActivityTimelineRow[] = [];
	let context: LocalActivityLine[] = [];
	for (const line of lines) {
		if (CONNECTIVE_KINDS.has(line.kind ?? "")) {
			context.push(line);
			continue;
		}
		rows.push({ id: `step-${context[0]?.id ?? line.id}`, context, action: line });
		context = [];
	}
	if (context.length > 0) rows.push({ id: `step-${context[0]!.id}`, context });
	return rows;
}

type ActivityStatus = "running" | "completed" | "failed" | "cancelled" | "interrupted" | "unhealthy" | "mixed";

function lineStatus(line: LocalActivityLine): ActivityStatus {
	if (line.toolStatus === "running") return "running";
	if (line.toolStatus === "failed") return "failed";
	if (/cancel/i.test(line.label)) return "cancelled";
	if (/interrupt/i.test(line.label)) return "interrupted";
	if (/unhealthy|detach/i.test(line.label)) return "unhealthy";
	if (line.toolStatus === "completed" || line.kind === "run_summary") return "completed";
	return "mixed";
}

function toolCategory(line: LocalActivityLine): string | null {
	if (line.kind === "file_write") return "edited files";
	if (line.kind === "file_read") return "read files";
	if (line.kind === "command") return "ran commands";
	if (line.kind === "search") return "searched";
	if (line.toolStatus) return "used tools";
	return null;
}

function summarizeGroup(lines: LocalActivityLine[]): { label: string; summary: string; status: ActivityStatus; toolCount: number } {
	const tools = lines.filter((line) => toolCategory(line));
	const statuses = (tools.length > 0 ? tools : lines).map(lineStatus);
	const status = statuses.includes("running")
		? "running"
		: statuses.includes("failed")
			? "failed"
			: statuses.includes("unhealthy")
				? "unhealthy"
				: statuses.includes("cancelled")
					? "cancelled"
					: statuses.includes("interrupted")
						? "interrupted"
						: statuses.every((s) => s === "completed")
							? "completed"
							: "mixed";
	const categories = [...new Set(tools.map(toolCategory).filter((category): category is string => Boolean(category)))];
	const actionLabel = categories.length > 0 ? categories.join(", ") : status === "running" ? "working" : "activity";
	const label = actionLabel.charAt(0).toUpperCase() + actionLabel.slice(1);
	const toolCount = tools.length;
	const summary = toolCount > 0
		? `${toolCount} call${toolCount === 1 ? "" : "s"}`
		: `${lines.length} update${lines.length === 1 ? "" : "s"}`;
	return { label, summary, status, toolCount };
}

/**
 * Present activity lines according to the persisted mode without reordering,
 * duplicating, or deleting underlying events.
 */
export function presentActivityLines(
	lines: LocalActivityLine[],
	mode: ToolActivityMode,
	options?: { running?: boolean; expandedGroupIds?: ReadonlySet<string> }
): ActivityPresentationItem[] {
	const running = options?.running ?? false;
	const expanded = options?.expandedGroupIds ?? new Set<string>();

	if (mode === "detailed") {
		return lines.map((line) => ({ kind: "line" as const, line }));
	}

	if (mode === "compact") {
		if (running) {
			const current = [...lines].reverse().find((line) => line.toolStatus === "running" || line.kind === "working")
				?? lines[lines.length - 1];
			const priorCount = Math.max(0, lines.length - (current ? 1 : 0));
			const items: ActivityPresentationItem[] = [];
			if (priorCount > 0) {
				const prior = current ? lines.slice(0, lines.indexOf(current)) : lines;
				const groupId = `compact-prior-${prior[0]?.id ?? "none"}`;
				const { label, summary, status } = summarizeGroup(prior);
				items.push({
					kind: "group",
					id: groupId,
					label: `${label} · earlier`,
					summary: `${priorCount} prior · ${summary}`,
					count: priorCount,
					status,
					lines: prior,
					expanded: expanded.has(groupId)
				});
			}
			if (current) items.push({ kind: "line", line: current });
			return items;
		}
		if (lines.length === 0) return [];
		const groupId = `compact-done-${lines[0]?.id ?? "none"}`;
		const { label, summary, status } = summarizeGroup(lines);
		return [{
			kind: "group",
			id: groupId,
			label,
			summary,
			count: lines.length,
			status,
			lines,
			expanded: expanded.has(groupId)
		}];
	}

	// grouped: fold consecutive tool calls together, including the short
	// reasoning/working updates between them. Approval, visual, summary, and
	// other authored rows are hard boundaries.
	const items: ActivityPresentationItem[] = [];
	let buffer: LocalActivityLine[] = [];
	const flush = () => {
		if (buffer.length === 0) return;
		const { label, summary, status, toolCount } = summarizeGroup(buffer);
		const shouldGroup = toolCount >= 2 || (toolCount === 0 && buffer.length >= 2);
		if (!shouldGroup) {
			items.push(...buffer.map((line) => ({ kind: "line" as const, line })));
			buffer = [];
			return;
		}
		const groupId = `group-${buffer[0]!.id}`;
		items.push({
			kind: "group",
			id: groupId,
			label,
			summary,
			count: toolCount || buffer.length,
			status,
			lines: buffer,
			expanded: expanded.has(groupId)
		});
		buffer = [];
	};

	for (const line of lines) {
		const isTool = Boolean(toolCategory(line));
		const isConnective = CONNECTIVE_KINDS.has(line.kind ?? "");
		if (isTool || isConnective) buffer.push(line);
		else {
			flush();
			items.push({ kind: "line", line });
		}
	}
	flush();
	return items;
}

export function activityStatusAnnouncement(
	previous: LocalActivityLine[] | undefined,
	next: LocalActivityLine[],
	running: boolean
): string | null {
	if (running) {
		const current = [...next].reverse().find((line) => line.toolStatus === "running") ?? next[next.length - 1];
		if (!current) return "Working";
		const prevId = previous?.at(-1)?.id;
		if (prevId === current.id && previous?.at(-1)?.toolStatus === current.toolStatus) return null;
		return current.label;
	}
	const last = next[next.length - 1];
	if (!last) return null;
	if (last.kind === "run_summary") return last.label;
	if (last.toolStatus === "failed") return `Failed: ${last.label}`;
	return null;
}
