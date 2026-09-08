import { useMemo, useState } from "react";
import type { RuntimeEvent } from "@synth/runtime-protocol";
import { eventsToLocalActivity } from "../runtime/sessionView";
import { bridges } from "../runtime/desktopBridge";
import { PaidComputeApprovalModal } from "./ChatTranscript";

/** Operator launches own a durable host approval scope, without a model session. */
export function OperatorTrainingApproval({ eventsBySession, onError }: {
    eventsBySession: Record<string, RuntimeEvent[]>;
    onError: (message: string) => void;
}) {
    const [settling, setSettling] = useState(false);
    const pending = useMemo(() => Object.entries(eventsBySession).flatMap(([sessionId, events]) => {
        if (!sessionId.startsWith("operator-training-")) return [];
        return Object.values(eventsToLocalActivity(events, [])).flat()
            .filter(line => line.kind === "approval" && line.approvalKind === "paid_compute" && line.approvalId)
            .map(line => ({ sessionId, line }));
    })[0], [eventsBySession]);
    const resolve = async (approvalId: string, decision: "once" | "reject") => {
        if (!pending || settling) return;
        setSettling(true);
        try {
            if (!bridges.codex) throw new Error("Native approval service is unavailable");
            await bridges.codex.resolveApproval(pending.sessionId, approvalId, decision);
        } catch (error) {
            onError(error instanceof Error ? error.message : "Approval could not be resolved");
        } finally { setSettling(false); }
    };
    return pending ? <PaidComputeApprovalModal line={pending.line}
        onApprove={id => void resolve(id, "once")} onReject={id => void resolve(id, "reject")} /> : null;
}
