import { useEffect, useState } from "react";
import { fromGenerated, spectaCommands } from "../bridge";
import { parseHostedInferenceLifecycle, type HostedInferenceLifecycle } from "../runtime/hostedInferenceLifecycle";

export function useHostedInferenceLifecycle(
	sessionId: string | null,
	model: string | null,
	enabled: boolean,
	active = false
): HostedInferenceLifecycle | null {
	const [status, setStatus] = useState<HostedInferenceLifecycle | null>(null);
	useEffect(() => {
		if (!enabled || !sessionId || !model) {
			setStatus(null);
			return;
		}
		let disposed = false;
		let timer: number | null = null;
		const read = async () => {
			try {
				const value = await fromGenerated(spectaCommands.synthCloudInferenceStatus(sessionId, model));
				if (!disposed) setStatus(parseHostedInferenceLifecycle(value));
			} catch {
				// Keep the prior authoritative observation across a transient poll failure.
			}
			if (!disposed) timer = window.setTimeout(read, active ? 500 : 2_000);
		};
		void read();
		return () => {
			disposed = true;
			if (timer != null) window.clearTimeout(timer);
		};
	}, [active, enabled, model, sessionId]);
	return status;
}
