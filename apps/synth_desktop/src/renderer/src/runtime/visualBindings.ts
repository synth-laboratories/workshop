/**
 * Read optimizer identity from the canonical typed visual binding.
 *
 * `VisualRecord.runId` belongs to the separate generic `runs` domain. The
 * backend rejects conflicting optimizer bindings; this reader still fails
 * closed if malformed or historical input contains more than one identity.
 */
export function optimizerRunIdFromBindings(value: unknown): string | undefined {
	return uniqueBindingSource(value, new Set(["optimizer_run"]));
}

/** Read one unambiguous retained trace identity from typed bindings. */
export function traceIdFromBindings(value: unknown): string | undefined {
	return uniqueBindingSource(value, new Set(["trace_v5", "trace"]));
}

function uniqueBindingSource(value: unknown, kinds: Set<string>): string | undefined {
	if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
	const bindings = value as Record<string, unknown>;
	const rows = Array.isArray(bindings.inputs)
		? bindings.inputs
		: Array.isArray(bindings.slots)
			? bindings.slots
			: [];
	const ids = new Set(rows.flatMap((row) => {
		if (!row || typeof row !== "object" || Array.isArray(row)) return [];
		const binding = row as Record<string, unknown>;
		return typeof binding.kind === "string" && kinds.has(binding.kind) && typeof binding.source === "string" && binding.source.trim()
			? [binding.source.trim()]
			: [];
	}));
	return ids.size === 1 ? [...ids][0] : undefined;
}
