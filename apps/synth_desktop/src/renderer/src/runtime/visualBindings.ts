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

/**
 * Count an explicitly bound or producer-declared trace set without choosing a
 * fake primary trace. Optimizer overview/workbench visuals commonly represent
 * several rollout traces, so a singular trace id is only returned by
 * `traceIdFromBindings` when the binding itself is unambiguous.
 */
export function traceSetCountFromBindings(value: unknown): number | undefined {
	const rows = bindingRows(value);
	const traceSources = new Set(bindingSources(rows, new Set(["trace_v5", "trace"])));
	if (traceSources.size > 1) return traceSources.size;
	const counts = new Set<number>();
	for (const row of rows) {
		const data = row.data;
		if (!data || typeof data !== "object" || Array.isArray(data)) continue;
		const aggregate = (data as Record<string, unknown>).aggregate;
		if (!aggregate || typeof aggregate !== "object" || Array.isArray(aggregate)) continue;
		const count = (aggregate as Record<string, unknown>).traceCount;
		if (typeof count === "number" && Number.isSafeInteger(count) && count >= 0) counts.add(count);
	}
	return counts.size === 1 ? [...counts][0] : undefined;
}

type BindingRow = Record<string, unknown>;

function bindingRows(value: unknown): BindingRow[] {
	if (!value || typeof value !== "object" || Array.isArray(value)) return [];
	const bindings = value as Record<string, unknown>;
	const rows = Array.isArray(bindings.inputs)
		? bindings.inputs
		: Array.isArray(bindings.slots)
			? bindings.slots
			: [];
	return rows.filter((row): row is BindingRow => Boolean(row && typeof row === "object" && !Array.isArray(row)));
}

function bindingSources(rows: BindingRow[], kinds: Set<string>): string[] {
	return rows.flatMap((binding) =>
		typeof binding.kind === "string"
		&& kinds.has(binding.kind)
		&& typeof binding.source === "string"
		&& binding.source.trim()
			? [binding.source.trim()]
			: []
	);
}

function uniqueBindingSource(value: unknown, kinds: Set<string>): string | undefined {
	const ids = new Set(bindingSources(bindingRows(value), kinds));
	return ids.size === 1 ? [...ids][0] : undefined;
}
