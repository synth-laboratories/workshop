import { useMemo, useState } from "react";
import type { ContainerDeployment } from "@synth/runtime-protocol";

function IconContainer({ size = 16 }: { size?: number }) {
	return (
		<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden>
			<path d="M2.5 4.4 8 1.8l5.5 2.6v7.2L8 14.2l-5.5-2.6V4.4Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
			<path d="m2.8 4.5 5.2 2.5 5.2-2.5M8 7v6.8" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
		</svg>
	);
}

export function ContainerIcon({ size }: { size?: number }) {
	return <IconContainer size={size} />;
}

type JsonObject = Record<string, unknown>;

function object(value: unknown): JsonObject {
	return value && typeof value === "object" && !Array.isArray(value) ? value as JsonObject : {};
}

function strings(value: unknown): string[] {
	return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
}

function interfaces(value: unknown): string[] {
	if (Array.isArray(value)) return strings(value);
	const record = object(value);
	return Object.entries(record).flatMap(([key, entry]) => {
		if (Array.isArray(entry)) return entry.map((item) => `${key}:${String(item)}`);
		if (entry && typeof entry === "object") return [key];
		return entry === false || entry == null ? [] : [key];
	});
}

type MetadataRow = { path: string; value: string };
type InferredField = { path: string; type: "number" | "boolean" | "string"; values: string[] };

function flattenMetadata(value: unknown, path = "", rows: MetadataRow[] = []): MetadataRow[] {
	if (Array.isArray(value)) {
		value.forEach((entry, index) => flattenMetadata(entry, `${path}[${index}]`, rows));
	} else if (value && typeof value === "object") {
		Object.entries(value as JsonObject).forEach(([key, entry]) => flattenMetadata(entry, path ? `${path}.${key}` : key, rows));
	} else if (value != null) {
		rows.push({ path, value: String(value) });
	}
	return rows;
}

function inferInstanceFields(instances: JsonObject[]): InferredField[] {
	const fields = new Map<string, { types: Set<string>; values: Map<string, number> }>();
	for (const instance of instances) for (const row of flattenMetadata(instance)) {
		const entry = fields.get(row.path) ?? { types: new Set<string>(), values: new Map<string, number>() };
		const raw = row.value;
		entry.types.add(raw === "true" || raw === "false" ? "boolean" : raw !== "" && Number.isFinite(Number(raw)) ? "number" : "string");
		entry.values.set(raw, (entry.values.get(raw) ?? 0) + 1);
		fields.set(row.path, entry);
	}
	return [...fields].map(([path, entry]) => ({
		path,
		type: entry.types.size === 1 ? [...entry.types][0] as InferredField["type"] : "string",
		values: [...entry.values].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([value]) => value)
	})).sort((a, b) => a.path.localeCompare(b.path));
}

function stripSqlValue(value: string): string {
	const trimmed = value.trim();
	return ((trimmed.startsWith("'") && trimmed.endsWith("'")) || (trimmed.startsWith('"') && trimmed.endsWith('"')))
		? trimmed.slice(1, -1) : trimmed;
}

function instanceFieldValue(instance: JsonObject, requested: string): unknown {
	const rows = flattenMetadata(instance);
	const normalized = requested.trim().toLowerCase();
	const exact = rows.find((row) => row.path.toLowerCase() === normalized);
	if (exact) return exact.value;
	const metadata = rows.find((row) => row.path.toLowerCase() === `metadata.${normalized}`);
	if (metadata) return metadata.value;
	const leaf = rows.filter((row) => row.path.toLowerCase().split(".").at(-1) === normalized);
	return leaf.length === 1 ? leaf[0].value : undefined;
}

function queryInstance(instance: JsonObject, query: string): { match: boolean; error?: string } {
	const trimmed = query.trim();
	if (!trimmed) return { match: true };
	const legacy = !/\s+(?:and|or|like|in)\s+|[=!<>]/i.test(trimmed) && trimmed.includes(":");
	const clauses = legacy ? trimmed.split(/\s+/).map((part) => {
		const [field, ...value] = part.split(":"); return `${field} LIKE '%${value.join(":")}%'`;
	}) : trimmed.split(/\s+AND\s+/i);
	for (const clause of clauses) {
		const inMatch = clause.match(/^([\w.[\]-]+)\s+IN\s*\((.*)\)$/i);
		if (inMatch) {
			const actual = instanceFieldValue(instance, inMatch[1]);
			const expected = inMatch[2].split(",").map(stripSqlValue);
			if (!expected.some((value) => String(actual) === value)) return { match: false };
			continue;
		}
		const match = clause.match(/^([\w.[\]-]+)\s*(=|!=|>=|<=|>|<|LIKE)\s*(.+)$/i);
		if (!match) {
			const haystack = JSON.stringify(instance).toLowerCase();
			if (clauses.length === 1) return { match: haystack.includes(stripSqlValue(clause).toLowerCase()) };
			return { match: false, error: `Could not parse: ${clause}` };
		}
		const [, field, operatorRaw, rawExpected] = match;
		const actual = instanceFieldValue(instance, field);
		if (actual === undefined) return { match: false, error: `Unknown field: ${field}` };
		const expected = stripSqlValue(rawExpected);
		const operator = operatorRaw.toUpperCase();
		let passes = false;
		if (operator === "LIKE") {
			const pattern = expected.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/%/g, ".*").replace(/_/g, ".");
			passes = new RegExp(`^${pattern}$`, "i").test(String(actual));
		} else if ([">", ">=", "<", "<="].includes(operator)) {
			const left = Number(actual); const right = Number(expected);
			if (!Number.isFinite(left) || !Number.isFinite(right)) return { match: false, error: `${field} is not numeric` };
			passes = operator === ">" ? left > right : operator === ">=" ? left >= right : operator === "<" ? left < right : left <= right;
		} else passes = operator === "=" ? String(actual).toLowerCase() === expected.toLowerCase() : String(actual).toLowerCase() !== expected.toLowerCase();
		if (!passes) return { match: false };
	}
	return { match: true };
}

function taskEntries(catalog: unknown, fallback: unknown): JsonObject[] {
	const catalogObject = object(catalog);
	const candidates = Array.isArray(catalog)
		? catalog
		: [catalogObject.tasks, catalogObject.items, catalogObject.task_instances].find(Array.isArray) ?? [];
	const tasks = (candidates as unknown[]).map(object).filter((task) => Object.keys(task).length > 0);
	if (tasks.length) return tasks;
	const fallbackTask = object(object(fallback).task);
	return Object.keys(fallbackTask).length ? [fallbackTask] : [];
}

function taskInstances(catalog: unknown): JsonObject[] {
	const catalogObject = object(catalog);
	const candidates = [catalogObject.instances, catalogObject.task_instances, catalogObject.items].find(Array.isArray) ?? [];
	return (candidates as unknown[]).map(object).filter((instance) => Object.keys(instance).length > 0);
}

function text(value: unknown): string | null {
	return typeof value === "string" && value.trim() ? value : null;
}

function taskId(task: JsonObject, index: number): string {
	return text(task.task_id) ?? text(task.id) ?? text(task.slug) ?? `task-${index + 1}`;
}

function DataBlock({ label, value }: { label: string; value: unknown }) {
	if (value == null) return null;
	if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
		return <div className="container-data-block"><strong>{label}</strong><p>{String(value)}</p></div>;
	}
	return <details className="container-data-block"><summary>{label}</summary><pre>{JSON.stringify(value, null, 2)}</pre></details>;
}

export function ContainerPane({
	container,
	expanded,
	onExpandedChange,
	onClose,
	onProbe
}: {
	container: ContainerDeployment;
	expanded: boolean;
	onExpandedChange: (expanded: boolean) => void;
	onClose: () => void;
	onProbe: () => void;
}) {
	const metadata = object(container.metadata);
	const info = object(metadata.info);
	const capabilities = interfaces(info.capabilities);
	const actions = strings(info.action_names);
	const taskInfo = object(metadata.taskInfo);
	const tasks = useMemo(() => taskEntries(metadata.taskCatalog, taskInfo), [metadata.taskCatalog, metadata.taskInfo]);
	const instances = useMemo(() => taskInstances(metadata.taskCatalog), [metadata.taskCatalog]);
	const [selectedTask, setSelectedTask] = useState<string | null>(null);
	const [metadataQuery, setMetadataQuery] = useState("");
	const [instanceQuery, setInstanceQuery] = useState("");
	const inferredFields = useMemo(() => inferInstanceFields(instances), [instances]);
	const [builderField, setBuilderField] = useState("");
	const [builderOperator, setBuilderOperator] = useState("=");
	const [builderValue, setBuilderValue] = useState("");
	const selectedIndex = Math.max(0, tasks.findIndex((task, index) => taskId(task, index) === selectedTask));
	const selected = tasks[selectedIndex];
	const detailedTask = object(taskInfo.task);
	const selectedHasDetail = selected && taskId(selected, selectedIndex) === (text(detailedTask.task_id) ?? text(detailedTask.id));
	const health = object(container.health);
	const healthPayload = Object.keys(object(health.payload)).length ? object(health.payload) : health;
	const sessions = typeof healthPayload.sessions === "number" ? healthPayload.sessions : null;
	const queryRows = useMemo(() => flattenMetadata({ task: metadata.taskInfo, program: metadata.program, dataset: metadata.dataset }), [metadata.taskInfo, metadata.program, metadata.dataset]);
	const queryParts = metadataQuery.trim().toLowerCase().split(/\s+/).filter(Boolean);
	const queryMatches = queryParts.length ? queryRows.filter((row) => queryParts.every((part) => {
		const [pathPart, ...valueParts] = part.split(":");
		if (!valueParts.length) return `${row.path} ${row.value}`.toLowerCase().includes(pathPart);
		return row.path.toLowerCase().includes(pathPart) && row.value.toLowerCase().includes(valueParts.join(":"));
	})).slice(0, 100) : [];
	const queryResults = instances.map((instance) => ({ instance, result: queryInstance(instance, instanceQuery) }));
	const queryError = queryResults.find((entry) => entry.result.error)?.result.error;
	const filteredInstances = queryError ? instances : queryResults.filter((entry) => entry.result.match).map((entry) => entry.instance);
	const selectedBuilderField = inferredFields.find((field) => field.path === builderField);
	const addBuilderClause = () => {
		if (!builderField || !builderValue.trim()) return;
		const quoted = selectedBuilderField?.type === "number" || selectedBuilderField?.type === "boolean"
			? builderValue.trim() : `'${builderValue.trim().replace(/'/g, "''")}'`;
		const clause = builderOperator === "IN" ? `${builderField} IN (${builderValue.split(",").map((value) => `'${value.trim().replace(/'/g, "''")}'`).join(", ")})` : `${builderField} ${builderOperator} ${quoted}`;
		setInstanceQuery((current) => current.trim() ? `${current.trim()} AND ${clause}` : clause);
		setBuilderValue("");
	};

	return (
		<aside className={`container-pane${expanded ? " expanded" : ""}`} data-testid="container-pane" aria-label="Container inspector">
			<header className="container-pane-head">
				<span className="container-pane-icon"><IconContainer size={17} /></span>
				<div className="container-pane-heading"><span>Container</span><strong>{container.name}</strong></div>
				<button type="button" className="container-pane-control" onClick={() => onExpandedChange(!expanded)} aria-label={expanded ? "Collapse container inspector" : "Expand container inspector"} title={expanded ? "Collapse" : "Expand"} data-testid="container-pane-expand">{expanded ? "↙" : "↗"}</button>
				<button type="button" className="container-pane-control" onClick={onClose} aria-label="Close container inspector">×</button>
			</header>
			<div className="container-pane-body">
				<section className="container-overview">
					<div className="container-status-line"><span className={`container-status-dot status-${container.status}`} aria-hidden /><strong>{container.status}</strong>{sessions !== null ? <span>{sessions} active sessions</span> : null}</div>
					<code className="container-endpoint">{container.baseUrl ?? container.location}</code>
					<button type="button" className="container-probe" onClick={onProbe}>Refresh metadata</button>
				</section>

				<section className="container-pane-section">
					<p className="container-pane-kicker">Overview</p>
					<h3>{container.taskFamily ?? text(info.name) ?? "Synth container"}</h3>
					<dl className="container-facts">
						<div><dt>Definitions</dt><dd>{tasks.length || "Not reported"}</dd></div>
						<div><dt>Instances</dt><dd>{instances.length || "Not reported"}</dd></div>
						<div><dt>Interfaces</dt><dd>{capabilities.length || "Not reported"}</dd></div>
						{container.lastRolloutId ? <div><dt>Last rollout</dt><dd title={container.lastRolloutId}>{container.lastRolloutId.slice(0, 8)}</dd></div> : null}
						{text(info.version) ? <div><dt>Version</dt><dd>{text(info.version)}</dd></div> : null}
					</dl>
				</section>

				<section className="container-pane-section">
					<p className="container-pane-kicker">Task definitions</p>
					{tasks.length ? <div className="container-task-list">{tasks.map((task, index) => {
						const id = taskId(task, index);
						return <button type="button" className={index === selectedIndex ? "selected" : ""} key={id} onClick={() => setSelectedTask(id)}><strong>{text(task.name) ?? text(task.task_name) ?? text(task.title) ?? id}</strong><code>{id}</code>{text(task.description) ? <span>{text(task.description)}</span> : null}</button>;
					})}</div> : <p>No task catalog reported. This container can still expose non-task interfaces.</p>}
					{selected ? <div className="container-task-detail">
						<DataBlock label="Objective" value={selectedHasDetail ? taskInfo.objective : selected.objective} />
						<DataBlock label="Output contract" value={selectedHasDetail ? taskInfo.output_space : selected.output_space} />
						<DataBlock label="Dataset" value={selectedHasDetail ? taskInfo.dataset : selected.dataset} />
						<DataBlock label="Metrics" value={selectedHasDetail ? taskInfo.metrics : selected.metrics} />
						<DataBlock label="Constraints" value={selectedHasDetail ? taskInfo.constraints : selected.constraints} />
						<DataBlock label="Task metadata" value={selectedHasDetail ? taskInfo.metadata : selected.metadata} />
					</div> : null}
				</section>

				{instances.length ? <section className="container-pane-section container-instance-browser">
					<p className="container-pane-kicker">Task instances</p>
					<p>{filteredInstances.length} of {instances.length} instances</p>
					<div className="container-query-builder">
						<strong>Inferred query</strong><span>{inferredFields.length} fields inferred from cached instances</span>
						<div className="container-query-controls">
							<select value={builderField} onChange={(event) => { setBuilderField(event.target.value); setBuilderValue(""); }} aria-label="Query field"><option value="">Choose field…</option>{inferredFields.map((field) => <option key={field.path} value={field.path}>{field.path} · {field.type}</option>)}</select>
							<select value={builderOperator} onChange={(event) => setBuilderOperator(event.target.value)} aria-label="Query operator">{["=", "!=", "LIKE", "IN", ">", ">=", "<", "<="].map((operator) => <option key={operator}>{operator}</option>)}</select>
							<input list="container-query-values" value={builderValue} onChange={(event) => setBuilderValue(event.target.value)} placeholder={builderOperator === "IN" ? "value, value" : "Value"} aria-label="Query value" />
							<datalist id="container-query-values">{selectedBuilderField?.values.map((value) => <option key={value} value={value} />)}</datalist>
							<button type="button" onClick={addBuilderClause} disabled={!builderField || !builderValue.trim()}>Add</button>
						</div>
						{selectedBuilderField?.values.length ? <div className="container-query-suggestions">{selectedBuilderField.values.slice(0, 8).map((value) => <button type="button" key={value} onClick={() => setBuilderValue(value)}>{value}</button>)}</div> : null}
					</div>
					<label className="container-sql-query"><span>SQL-like filter</span><input value={instanceQuery} onChange={(event) => setInstanceQuery(event.target.value)} placeholder="split = 'test' AND metadata.output_label LIKE 'card%'" aria-label="Filter task instances" /></label>
					{queryError ? <p className="container-query-error" role="alert">{queryError}</p> : null}
					{instanceQuery.trim() ? <button type="button" className="container-query-clear" onClick={() => setInstanceQuery("")}>Clear query</button> : null}
					<div className="container-instance-list">{filteredInstances.slice(0, 100).map((instance, index) => {
						const id = text(instance.task_instance_id) ?? text(instance.id) ?? `instance-${index + 1}`;
						const instanceMetadata = object(instance.metadata);
						return <details key={id}><summary><strong>{id}</strong><span>{text(instance.split) ?? "unspecified split"}</span>{text(instanceMetadata.output_label) ? <code>{text(instanceMetadata.output_label)}</code> : null}</summary><pre>{JSON.stringify(instance, null, 2)}</pre></details>;
					})}</div>
					{filteredInstances.length > 100 ? <p>Showing the first 100 matching instances.</p> : null}
				</section> : null}

				<section className="container-pane-section"><p className="container-pane-kicker">Interfaces</p><div className="container-chip-grid">{capabilities.length ? capabilities.map((capability) => <code key={capability}>{capability}</code>) : <span>None reported</span>}</div></section>

				{Object.keys(object(metadata.program)).length ? <details className="container-pane-section"><summary>Program contract</summary><pre>{JSON.stringify(metadata.program, null, 2)}</pre></details> : null}
				{Object.keys(object(metadata.dataset)).length ? <details className="container-pane-section"><summary>Dataset contract</summary><pre>{JSON.stringify(metadata.dataset, null, 2)}</pre></details> : null}

				{queryRows.length ? <section className="container-pane-section container-metadata-query">
					<p className="container-pane-kicker">Metadata query</p>
					<p>Search cached task, program, and dataset metadata. Use text or <code>path:value</code>.</p>
					<input value={metadataQuery} onChange={(event) => setMetadataQuery(event.target.value)} placeholder="e.g. labels:card or split:test" aria-label="Query container metadata" />
					{metadataQuery.trim() ? <div className="container-query-results"><span>{queryMatches.length}{queryMatches.length === 100 ? "+" : ""} matches</span>{queryMatches.map((row, index) => <div key={`${row.path}-${index}`}><code>{row.path}</code><span>{row.value}</span></div>)}</div> : null}
				</section> : null}

				{actions.length ? <details className="container-pane-section"><summary>Actions ({actions.length})</summary><div className="container-action-grid">{actions.map((action) => <code key={action}>{action}</code>)}</div></details> : null}
				{typeof info.glyph_legend === "string" ? <details className="container-pane-section"><summary>Environment legend</summary><pre>{info.glyph_legend}</pre></details> : null}
				<details className="container-pane-section"><summary>Provider metadata</summary><pre>{JSON.stringify({ health: container.health, metadata: container.metadata }, null, 2)}</pre></details>
			</div>
		</aside>
	);
}
