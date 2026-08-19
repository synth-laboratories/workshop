import { useEffect, useMemo, useState } from "react";
import type { OptimizerResourceRef, OptimizerRunRecord } from "@synth/runtime-protocol";
import type { ReportRecord } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import type { LocalChat } from "../types/landing";

export function outputContainerIds(chat: LocalChat): string[] {
	return [
		...new Set(
			Object.values(chat.activityByMessageId ?? {})
				.flat()
				.map((line) => line.containerId)
				.filter((id): id is string => Boolean(id))
		)
	];
}

const TERMINAL_RUN_STATUSES = new Set([
	"completed",
	"succeeded",
	"failed",
	"cancelled",
	"degraded"
]);

export function isCheckpointRef(ref: OptimizerResourceRef): boolean {
	const kind = ref.kind.toLowerCase();
	const role = (ref.role ?? "").toLowerCase();
	return kind.includes("checkpoint") || role.includes("checkpoint") || kind.includes("adapter");
}

export function primaryVisualId(run: OptimizerRunRecord): string | null {
	const primary = run.visualRefs.find((ref) => ref.kind === "visual" && ref.role === "primary");
	const anyVisual = run.visualRefs.find((ref) => ref.kind === "visual");
	return primary?.id ?? anyVisual?.id ?? null;
}

export type ChatCheckpoint = {
	runId: string;
	ref: OptimizerResourceRef;
};

export type ChatOutputs = {
	containerIds: string[];
	reports: ReportRecord[];
	runs: OptimizerRunRecord[];
	checkpoints: ChatCheckpoint[];
	count: number;
	hasResources: boolean;
};

/**
 * Durable Outputs authorities for one chat. Visuals already restore through the
 * session visual registry; this hook is the independent authority for reports,
 * optimizer/eval/training runs, and checkpoints so a journal outage or a
 * missing visual publication cannot empty the shelf.
 */
export function useChatOutputs(chat: LocalChat): ChatOutputs {
	const [reports, setReports] = useState<ReportRecord[]>([]);
	const [runs, setRuns] = useState<OptimizerRunRecord[]>([]);
	const sessionId = chat.id;

	useEffect(() => {
		const reportsBridge = bridges.reports;
		if (!reportsBridge) return;
		let disposed = false;
		const reload = () => {
			void reportsBridge.list({ includeArchived: false, limit: 500 }).then(
				(rows) => {
					if (!disposed) setReports(rows);
				},
				() => {
					if (!disposed) setReports([]);
				}
			);
		};
		reload();
		const unlisten = reportsBridge.onEvent?.((event) => {
			if (event.kind.startsWith("report.")) reload();
		});
		return () => {
			disposed = true;
			unlisten?.();
		};
	}, []);

	useEffect(() => {
		const optimizers = bridges.optimizers;
		if (!optimizers || !sessionId) {
			setRuns([]);
			return;
		}
		let disposed = false;
		const reload = () => {
			void optimizers.list({ sessionRef: sessionId, limit: 500 }).then(
				async (rows) => {
					const owned = rows.filter((run) => !run.sessionRef || run.sessionRef === sessionId);
					const reconciled = await Promise.all(
						owned.map(async (run) => {
							if (TERMINAL_RUN_STATUSES.has(run.status) || run.source !== "local") return run;
							try {
								return await optimizers.refresh(run.id);
							} catch {
								return run;
							}
						})
					);
					if (!disposed) setRuns(reconciled);
				},
				() => {
					if (!disposed) setRuns([]);
				}
			);
		};
		reload();
		const unlisten = optimizers.onEvent?.((event) => {
			if (!event.kind.startsWith("optimizer.")) return;
			if (event.sessionId && event.sessionId !== sessionId) return;
			reload();
		});
		return () => {
			disposed = true;
			unlisten?.();
		};
	}, [sessionId]);

	const containerIds = outputContainerIds(chat);
	const artifacts = chat.artifacts ?? [];
	const checkpoints = useMemo<ChatCheckpoint[]>(
		() =>
			runs.flatMap((run) =>
				run.outputRefs.filter(isCheckpointRef).map((ref) => ({ runId: run.id, ref }))
			),
		[runs]
	);
	const visualIds = useMemo(() => new Set(artifacts.map((artifact) => artifact.id)), [artifacts]);
	const extraRuns = runs.filter((run) => {
		const visualId = primaryVisualId(run);
		return !visualId || !visualIds.has(visualId);
	});
	const count =
		containerIds.length + artifacts.length + reports.length + extraRuns.length + checkpoints.length;
	return {
		containerIds,
		reports,
		runs,
		checkpoints,
		count,
		hasResources: count > 0
	};
}
