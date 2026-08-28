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

type InterfaceMetadata = { chips: string[]; invalid: boolean };

function scalar(value: unknown): value is string | number | boolean {
	return typeof value === "string" || typeof value === "number" || typeof value === "boolean";
}

export function interfaces(value: unknown): InterfaceMetadata {
	if (value == null) return { chips: [], invalid: false };
	if (scalar(value)) return { chips: typeof value === "string" && value.trim() ? [value] : [], invalid: typeof value !== "string" };
	if (Array.isArray(value)) {
		if (!value.every(scalar)) return { chips: [], invalid: true };
		return { chips: value.map((entry) => String(entry)), invalid: false };
	}
	if (typeof value !== "object") return { chips: [], invalid: true };
	const chips: string[] = [];
	let invalid = false;
	for (const [key, entry] of Object.entries(value as JsonObject)) {
		if (Array.isArray(entry)) {
			if (!entry.length) chips.push(`${key}:0`);
			else if (entry.every(scalar)) chips.push(...entry.map((item) => `${key}:${String(item)}`));
			else if (entry.every((item) => Object.keys(object(item)).length > 0)) chips.push(`${key}:${entry.length}`);
			else invalid = true;
		} else if (entry && typeof entry === "object") chips.push(key);
		else if (entry === true || (typeof entry === "string" && entry.trim()) || typeof entry === "number") chips.push(key);
		else if (entry !== false && entry != null) invalid = true;
	}
	return { chips: [...new Set(chips)], invalid };
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

type TaskMetadata = { tasks: JsonObject[]; instances: JsonObject[]; error: string | null };

function objectRows(value: unknown): JsonObject[] | null {
	if (!Array.isArray(value)) return null;
	const rows = value.map(object);
	return rows.every((row) => Object.keys(row).length > 0) ? rows : null;
}

export function taskMetadata(catalog: unknown, fallback: unknown): TaskMetadata {
	if (catalog != null) {
		const catalogObject = object(catalog);
		if (!Object.keys(catalogObject).length) return { tasks: [], instances: [], error: "invalid task catalog: expected an object" };
		if (catalogObject.schema_version != null && catalogObject.schema_version !== "synth.container.task-catalog.v1") {
			return { tasks: [], instances: [], error: "invalid task catalog: unsupported schema_version" };
		}
		const strict = catalogObject.schema_version === "synth.container.task-catalog.v1";
		const taskValue = catalogObject.tasks ?? catalogObject.items;
		const instanceValue = catalogObject.instances ?? catalogObject.task_instances ?? (strict ? undefined : catalogObject.items);
		const tasks = objectRows(taskValue);
		const instances = objectRows(instanceValue);
		if (tasks === null || instances === null) {
			return { tasks: [], instances: [], error: "invalid task catalog: tasks and instances must be arrays of objects" };
		}
		if (strict && tasks.some((task) => !text(task.id) && !text(task.task_id))) {
			return { tasks: [], instances: [], error: "invalid task catalog: task id is required" };
		}
		if (strict && instances.some((instance) => !text(instance.id) || !text(instance.task_id) || !text(instance.task_instance_id))) {
			return { tasks: [], instances: [], error: "invalid task catalog: instance identity is required" };
		}
		return { tasks, instances, error: null };
	}
	const fallbackObject = object(fallback);
	if (fallback != null && !Object.keys(fallbackObject).length) {
		return { tasks: [], instances: [], error: "invalid task info: expected an object" };
	}
	const nested = object(fallbackObject.task);
	const fallbackTask = Object.keys(nested).length ? nested : fallbackObject;
	if (Object.keys(fallbackTask).length && !text(fallbackTask.id) && !text(fallbackTask.task_id)) {
		return { tasks: [], instances: [], error: "invalid task info: task id is required" };
	}
	return Object.keys(fallbackTask).length
		? { tasks: [fallbackTask], instances: [], error: null }
		: { tasks: [], instances: [], error: null };
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

function freshness(value: unknown): { kind: "live" | "cached" | "unavailable"; observedAt: string | null; reason: string | null } {
	const entry = object(value);
	const kind = text(entry.kind);
	if (kind === "live" || kind === "cached" || kind === "unavailable") {
		return { kind, observedAt: text(entry.observedAt) ?? text(entry.observed_at), reason: text(entry.reason) };
	}
	return { kind: "unavailable", observedAt: null, reason: "not_reported" };
}

export function countLabel(count: number, reported: boolean, kind: "live" | "cached" | "unavailable"): string {
	if (!reported || kind === "unavailable") return "Not reported";
	if (kind === "cached") return `${count} cached`;
	return `${count} live`;
}

function shortRevision(value: string): string {
	return value.length > 12 ? value.slice(0, 8) : value;
}

function launchProblem(error: JsonObject): { headline: string; path: string | null; next: string | null } | null {
	const code = text(error.code);
	const message = text(error.error) ?? text(error.message);
	const declared = text(error.declared_path) ?? text(error.declaredPath);
	const resolved = text(error.resolved_path) ?? text(error.resolvedPath);
	const next = text(error.remediation);
	if (!code && !message && !declared) return null;
	const headlines: Record<string, string> = {
		launch_source_path_not_found: "Couldn't find a declared launch file.",
		launch_source_path_escapes_root: "A launch file points outside this repository.",
		launch_absolute_path_rejected: "Absolute launch paths aren't allowed.",
		launch_source_digest_mismatch: "Launch files changed since they were declared.",
		launch_checkout_revision_mismatch: "This checkout doesn't match the declared revision.",
		launch_source_root_not_approved: "This repository isn't attached to the conversation.",
		launch_manifest_unreadable: "Couldn't read the container launch file."
	};
	return {
		headline: (code && headlines[code]) || message || "Launch files need attention.",
		path: declared && resolved ? `${declared} → ${resolved}` : declared ?? resolved,
		next
	};
}

export function ContainerPane({
	container,
	expanded,
	onExpandedChange,
	onClose,
	onProbe,
	onRestart,
	onRepair
}: {
	container: ContainerDeployment;
	expanded: boolean;
	onExpandedChange: (expanded: boolean) => void;
	onClose: () => void;
	onProbe: () => void;
	onRestart?: () => void;
	onRepair?: () => void;
}) {
	const metadata = object(container.metadata);
	const info = object(metadata.info);
	const capabilityMetadata = interfaces(info.capabilities);
	const capabilities = capabilityMetadata.chips;
	const actions = strings(info.action_names);
	const taskInfo = object(metadata.taskInfo);
	const taskCatalogReported = metadata.taskCatalog != null;
	const taskDefinitionsReported = taskCatalogReported || metadata.taskInfo != null;
	const catalog = useMemo(() => taskMetadata(metadata.taskCatalog, taskInfo), [metadata.taskCatalog, metadata.taskInfo]);
	const tasks = catalog.tasks;
	const instances = catalog.instances;
	const policyState = object(metadata.policyState);
	const policyReported = metadata.policyState != null;
	const policySchemaValid = !policyReported || policyState.schema_version === "synth.container-policy.v1";
	const policyStatus = text(policyState.status);
	const policyRef = object(policyState.policy_ref);
	const [selectedTask, setSelectedTask] = useState<string | null>(null);
	const [metadataQuery, setMetadataQuery] = useState("");
	const [instanceQuery, setInstanceQuery] = useState("");
	const inferredFields = useMemo(() => inferInstanceFields(instances), [instances]);
	const [builderField, setBuilderField] = useState("");
	const [builderOperator, setBuilderOperator] = useState("=");
	const [builderValue, setBuilderValue] = useState("");
	const selectedIndex = Math.max(0, tasks.findIndex((task, index) => taskId(task, index) === selectedTask));
	const selected = tasks[selectedIndex];
	const nestedDetailedTask = object(taskInfo.task);
	const detailedTask = Object.keys(nestedDetailedTask).length ? nestedDetailedTask : taskInfo;
	const detailPayload = Object.keys(nestedDetailedTask).length ? taskInfo : detailedTask;
	const selectedHasDetail = selected && taskId(selected, selectedIndex) === (text(detailedTask.task_id) ?? text(detailedTask.id));
	const health = object(container.health);
	const healthPayload = Object.keys(object(health.payload)).length ? object(health.payload) : health;
	const sessions = typeof healthPayload.sessions === "number" ? healthPayload.sessions : null;
	const runtimeLive = container.status === "ready" && health.ok !== false;
	const instanceFreshness = freshness(metadata.taskCatalogFreshness);
	const interfaceFreshness = freshness(metadata.interfaceFreshness);
	const policyFreshness = freshness(metadata.policyFreshness);
	const launch = object(metadata.launchDeclaration);
	const launchValid = launch.valid === true;
	const launchError = object(launch.error);
	const origin = object(metadata.declarationOrigin);
	const problem = launchProblem(launchError);
	const command = Array.isArray(launch.command) ? (launch.command as unknown[]).map(String).join(" ") : null;
	const sourceRoot = text(origin.sourceRoot);
	const sourceRevision = text(origin.sourceRevision);
	const showRestart = !runtimeLive && launchValid && onRestart;
	const showRepair = (!launchValid || launch.valid === false) && onRepair;
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
				{catalog.error ? <p className="container-query-error" role="alert">Task metadata error: {catalog.error}</p> : null}
				<section className="container-overview">
					<div className="container-status-line">
						<span className={`container-status-dot status-${container.status}`} aria-hidden />
						<strong>{runtimeLive ? "Ready" : "Couldn't reach this container"}</strong>
						{sessions !== null ? <span>{sessions} {sessions === 1 ? "session" : "sessions"}</span> : null}
					</div>
					<code className="container-endpoint" title={container.baseUrl ?? container.location}>{container.baseUrl ?? container.location}</code>
					<div className="container-overview-actions">
						<button type="button" className="container-probe" onClick={onProbe}>Refresh</button>
						{showRestart ? <button type="button" className="container-probe container-probe-primary" onClick={() => onRestart?.()} data-testid="container-restart" title="Workshop will ask before replacing this container" aria-label="Restart this container">Restart…</button> : null}
						{showRepair ? <button type="button" className="container-probe container-probe-primary" onClick={() => onRepair?.()} data-testid="container-repair">Fix launch files</button> : null}
					</div>
					{command || sourceRoot ? <p className="container-launch-identity">
						{command ? <code title={command}>{command}</code> : null}
						{sourceRoot ? <code title={sourceRoot}>{sourceRoot}</code> : null}
						{sourceRevision ? <span title={sourceRevision}>rev {shortRevision(sourceRevision)}</span> : null}
					</p> : null}
					{problem ? <div className="ws-note ws-note-danger" role="alert">
						<strong>{problem.headline}</strong>
						{problem.path ? <code>{problem.path}</code> : null}
						{problem.next ? <p>{problem.next}</p> : null}
					</div> : null}
				</section>

				<section className="container-pane-section">
					<p className="container-pane-kicker">Overview</p>
					<h3>{container.taskFamily ?? text(info.name) ?? "Synth container"}</h3>
					<dl className="container-facts">
						<div><dt>Definitions</dt><dd>{taskDefinitionsReported ? tasks.length : "Not reported"}</dd></div>
						<div><dt>Instances</dt><dd>{countLabel(instances.length, taskCatalogReported, instanceFreshness.kind)}</dd></div>
						<div><dt>Interfaces</dt><dd>{info.capabilities == null ? "Not reported" : countLabel(capabilities.length, true, interfaceFreshness.kind)}</dd></div>
						{container.lastRolloutId ? <div><dt>Last rollout</dt><dd title={container.lastRolloutId}>{container.lastRolloutId.slice(0, 8)}</dd></div> : null}
						{text(info.version) ? <div><dt>Version</dt><dd>{text(info.version)}</dd></div> : null}
					</dl>
				</section>

				<section className="container-pane-section">
					<p className="container-pane-kicker">Installed policy</p>
					{!runtimeLive ? <p>Policy isn't available while this container is offline. Restart it to inspect the installed policy.</p> : !policyReported || policyFreshness.kind === "unavailable" ? <p>Not reported</p> : !policySchemaValid ? <p role="alert">Invalid response: unsupported policy schema</p> : policyStatus === "not_installed" ? <p>None</p> : policyStatus === "installed" ? <dl className="container-facts">
						<div><dt>Reference</dt><dd>{text(policyRef.namespace) && text(policyRef.name) ? `${text(policyRef.namespace)}/${text(policyRef.name)}` : "Invalid response"}</dd></div>
						<div><dt>Revision</dt><dd>{text(policyState.policy_revision_id) ?? "Invalid response"}</dd></div>
						<div><dt>Source</dt><dd>{text(policyState.source_revision) ?? "Unavailable"}</dd></div>
						<div><dt>Configuration</dt><dd>{text(policyState.configuration_digest) ?? "Unavailable"}</dd></div>
					</dl> : <p role="alert">{policyStatus === "installing" || policyStatus === "failed" ? policyStatus : "Invalid response"}</p>}
				</section>

				<section className="container-pane-section">
					<p className="container-pane-kicker">Task definitions</p>
					{tasks.length ? <div className="container-task-list">{tasks.map((task, index) => {
						const id = taskId(task, index);
						return <button type="button" className={index === selectedIndex ? "selected" : ""} key={id} onClick={() => setSelectedTask(id)}><strong>{text(task.name) ?? text(task.task_name) ?? text(task.title) ?? id}</strong><code>{id}</code>{text(task.description) ? <span>{text(task.description)}</span> : null}</button>;
					})}</div> : <p>No task catalog reported. This container can still expose non-task interfaces.</p>}
					{selected ? <div className="container-task-detail">
						<DataBlock label="Objective" value={selectedHasDetail ? detailPayload.objective : selected.objective} />
						<DataBlock label="Output contract" value={selectedHasDetail ? detailPayload.output_space : selected.output_space} />
						<DataBlock label="Dataset" value={selectedHasDetail ? detailPayload.dataset : selected.dataset} />
						<DataBlock label="Metrics" value={selectedHasDetail ? detailPayload.metrics : selected.metrics} />
						<DataBlock label="Constraints" value={selectedHasDetail ? detailPayload.constraints : selected.constraints} />
						<DataBlock label="Task metadata" value={selectedHasDetail ? detailPayload.metadata : selected.metadata} />
					</div> : null}
				</section>

				{instances.length ? <section className="container-pane-section container-instance-browser">
					<p className="container-pane-kicker">Task instances</p>
					<p>{filteredInstances.length} of {instances.length} {instanceFreshness.kind === "cached" ? "instances from the last successful probe" : "instances"}</p>
					<div className="container-query-builder">
						<strong>Filter instances</strong><span>{instanceFreshness.kind === "cached" ? `${inferredFields.length} fields from the last cached catalog` : `${inferredFields.length} fields inferred from the catalog`}</span>
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

				<section className="container-pane-section"><p className="container-pane-kicker">Interfaces</p><div className="container-chip-grid">{capabilities.map((capability) => <code key={capability}>{capability}</code>)}{capabilityMetadata.invalid ? <span role="alert">invalid interface metadata</span> : !capabilities.length ? <span>None reported</span> : null}</div><DataBlock label="Policy references" value={object(info.capabilities).policy_refs} /></section>

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
