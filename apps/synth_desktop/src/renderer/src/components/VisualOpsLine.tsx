import { useEffect, useMemo, useState } from "react";
import {
	classifyVisualOpsRoute,
	formatVisualOpsPart,
	type VisualOpsKind
} from "../types/landing";
import { bridges } from "../runtime/desktopBridge";
import {
	VISUAL_OPS_FOLLOW_EVENT,
	VISUAL_OPS_UNREACHABLE_EVENT,
	type VisualOpsFollowDetail
} from "../runtime/visualReferences";

type Props = {
	sessionId?: string | null;
	runId?: string | null;
	traceId?: string | null;
	testId: string;
	compact?: boolean;
	/** Pane header probes local disk so "not a Workshop route" is visible without a click. */
	probe?: boolean;
};

type Openable = {
	session: boolean | null;
	run: boolean | null;
	trace: boolean | null;
};

const unknownOpenable: Openable = { session: null, run: null, trace: null };

function followOps(kind: VisualOpsKind, id: string) {
	window.dispatchEvent(new CustomEvent(VISUAL_OPS_FOLLOW_EVENT, { detail: { kind, id } satisfies VisualOpsFollowDetail }));
}

async function sessionIsLocalWorkshop(sessionId: string): Promise<boolean> {
	const rows = await bridges.codex?.list?.();
	return Boolean(rows?.some((row) => row.sessionId === sessionId));
}

async function runIsLocalOptimizer(runId: string): Promise<boolean> {
	if (!bridges.optimizers) return false;
	await bridges.optimizers.get(runId);
	return true;
}

async function traceIsLocal(traceId: string): Promise<boolean> {
	if (!bridges.inventory) return false;
	await bridges.inventory.getTrace(traceId);
	return true;
}

function OpsPart({
	kind,
	id,
	openable
}: {
	kind: VisualOpsKind;
	id?: string | null;
	openable: boolean | null;
}) {
	const route = classifyVisualOpsRoute(kind, id, openable);
	const label = formatVisualOpsPart(kind, id, openable);
	const followable = Boolean(id?.trim()) && route !== "not-a-workshop-route";
	if (!followable) {
		return <span>{label}</span>;
	}
	return (
		<button
			type="button"
			className="visual-ops-follow"
			onClick={() => followOps(kind, id!.trim())}
		>
			{label}
		</button>
	);
}

export function VisualOpsLine({ sessionId, runId, traceId, testId, compact, probe }: Props) {
	const [openable, setOpenable] = useState<Openable>(unknownOpenable);

	useEffect(() => {
		setOpenable(unknownOpenable);
		if (!probe) return;
		let cancelled = false;
		void (async () => {
			const next: Openable = { session: null, run: null, trace: null };
			if (sessionId?.trim()) {
				try {
					next.session = await sessionIsLocalWorkshop(sessionId.trim());
				} catch {
					next.session = false;
				}
			}
			if (runId?.trim()) {
				try {
					next.run = await runIsLocalOptimizer(runId.trim());
				} catch {
					next.run = false;
				}
			}
			if (traceId?.trim()) {
				try {
					next.trace = await traceIsLocal(traceId.trim());
				} catch {
					next.trace = false;
				}
			}
			if (!cancelled) setOpenable(next);
		})();
		return () => {
			cancelled = true;
		};
	}, [probe, sessionId, runId, traceId]);

	useEffect(() => {
		const unreachable = (event: Event) => {
			const detail = (event as CustomEvent<VisualOpsFollowDetail>).detail;
			if (!detail?.id || !detail.kind) return;
			setOpenable((current) => {
				if (detail.kind === "session" && detail.id === sessionId) return { ...current, session: false };
				if (detail.kind === "run" && detail.id === runId) return { ...current, run: false };
				if (detail.kind === "trace" && detail.id === traceId) return { ...current, trace: false };
				return current;
			});
		};
		window.addEventListener(VISUAL_OPS_UNREACHABLE_EVENT, unreachable);
		return () => window.removeEventListener(VISUAL_OPS_UNREACHABLE_EVENT, unreachable);
	}, [sessionId, runId, traceId]);

	const className = useMemo(
		() => `visual-ops-line${compact ? " visual-ops-compact" : ""}`,
		[compact]
	);

	return (
		<span className={className} data-testid={testId}>
			<OpsPart kind="session" id={sessionId} openable={openable.session} />
			{" · "}
			<OpsPart kind="run" id={runId} openable={openable.run} />
			{" · "}
			<OpsPart kind="trace" id={traceId} openable={openable.trace} />
		</span>
	);
}
