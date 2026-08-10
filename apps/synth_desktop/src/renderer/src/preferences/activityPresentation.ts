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

// Reasoning is an authored disclosure, not tool noise. It must remain adjacent
// to the assistant answer in every activity density, rather than disappearing
// into a generic "Activity" group.
const GROUPABLE = new Set(["command", "search", "file_read", "file_write", "working"]);

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

function summarizeGroup(lines: LocalActivityLine[]): { label: string; summary: string; status: ActivityStatus } {
	const statuses = lines.map(lineStatus);
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
	const commands = lines.filter((line) => line.kind === "command" || line.toolStatus).length;
	const label = status === "running"
		? "Working"
		: status === "failed"
			? "Failed"
			: status === "cancelled"
				? "Cancelled"
				: status === "interrupted"
					? "Interrupted"
					: status === "unhealthy"
						? "Unhealthy"
						: "Activity";
	const summary = `${lines.length} step${lines.length === 1 ? "" : "s"}${commands ? ` · ${commands} tool${commands === 1 ? "" : "s"}` : ""}`;
	return { label, summary, status };
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

	// grouped
	const items: ActivityPresentationItem[] = [];
	let buffer: LocalActivityLine[] = [];
	const flush = () => {
		if (buffer.length === 0) return;
		if (buffer.length === 1 && !running) {
			items.push({ kind: "line", line: buffer[0]! });
			buffer = [];
			return;
		}
		const groupId = `group-${buffer[0]!.id}`;
		const { label, summary, status } = summarizeGroup(buffer);
		const shouldExpand = running || expanded.has(groupId);
		if (running && !expanded.has(groupId)) {
			// While running, show current activity plus a count of prior adjacent tools.
			const current = buffer[buffer.length - 1]!;
			const prior = buffer.slice(0, -1);
			if (prior.length > 0) {
				const priorId = `group-prior-${prior[0]!.id}`;
				const priorSummary = summarizeGroup(prior);
				items.push({
					kind: "group",
					id: priorId,
					label: priorSummary.label,
					summary: priorSummary.summary,
					count: prior.length,
					status: priorSummary.status,
					lines: prior,
					expanded: expanded.has(priorId)
				});
			}
			items.push({ kind: "line", line: current });
		} else {
			items.push({
				kind: "group",
				id: groupId,
				label,
				summary,
				count: buffer.length,
				status,
				lines: buffer,
				expanded: shouldExpand && expanded.has(groupId)
			});
		}
		buffer = [];
	};

	for (const line of lines) {
		const isAgentMessageLike = line.kind === "approval" || line.kind === "visual" || line.kind === "subagent" || line.kind === "run_summary";
		if (isAgentMessageLike || !GROUPABLE.has(line.kind ?? "") && !line.toolStatus) {
			flush();
			items.push({ kind: "line", line });
			continue;
		}
		buffer.push(line);
	}
	flush();

	if (!running) {
		// After finish, collapse groupable runs into concise summaries unless expanded.
		return items.map((item) => {
			if (item.kind !== "group") return item;
			return { ...item, expanded: expanded.has(item.id) };
		});
	}
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
