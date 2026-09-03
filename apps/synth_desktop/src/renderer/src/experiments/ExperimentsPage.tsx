import { useEffect, useMemo, useState } from "react";
import { commands, type ExperimentGroup, type ExperimentNode, type ReportRecord, type ResearchJournalEntry } from "../generated/protocol";
import { fromGenerated, n } from "../bridge";
import { orderLineageNodes } from "../lineage/orderLineageNodes";
import { ExperimentIndex } from "./ExperimentIndex";
import { ExperimentWorkspace } from "./ExperimentWorkspace";
import { PluginEmptyState, PluginPage, PluginPageHeader, PluginTabs } from "../components/PluginPage";

export { orderLineageNodes } from "../lineage/orderLineageNodes";

type ResearchSection = "experiments" | "log" | "reports";

export function ExperimentsPage({ initialId, onBack, onOpenReport, onSectionChange }: { initialId?: string; onBack: () => void; onOpenReport: (id: string) => void; onSectionChange?: (section: ResearchSection) => void }) {
	const [section, setSection] = useState<ResearchSection>("experiments");
	const [query, setQuery] = useState("");
	const [rows, setRows] = useState<ExperimentGroup[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(initialId ?? null);
	const [nodeId, setNodeId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [loaded, setLoaded] = useState(false);
	const [log, setLog] = useState<ResearchJournalEntry[]>([]);
	const [reports, setReports] = useState<ReportRecord[]>([]);
	const [logTitle, setLogTitle] = useState("");
	const [logBody, setLogBody] = useState("");
	const [logKind, setLogKind] = useState("observation");
	const [logTags, setLogTags] = useState("");

	const refresh = (keepId?: string | null) =>
		fromGenerated(commands.experimentsList(n(query)))
			.then((next) => {
				setRows(next);
				setError(null);
				setLoaded(true);
				const preferred = keepId ?? selectedId;
				if (preferred && next.some((row) => row.id === preferred)) {
					setSelectedId(preferred);
				}
			})
			.catch((e) => {
				setError(String(e));
				setLoaded(true);
			});

	useEffect(() => {
		void refresh();
	}, [query]);

	useEffect(() => {
		if (initialId) setSelectedId(initialId);
	}, [initialId]);

	const refreshLog = () => fromGenerated(commands.researchLogList(n(query), null)).then(setLog).catch((e) => setError(String(e)));
	const refreshReports = () => fromGenerated(commands.reportsList({ status: null, search: n(query), limit: 100, includeArchived: false })).then(setReports).catch((e) => setError(String(e)));

	useEffect(() => {
		if (section === "log") void refreshLog();
		if (section === "reports") void refreshReports();
	}, [section, query]);

	const appendLog = async () => {
		if (!logBody.trim()) return;
		setBusy(true);
		try {
			await fromGenerated(commands.researchLogAppend({
				occurredAt: null,
				author: "User",
				actorKind: "human",
				entryKind: logKind,
				title: logTitle.trim() || logKind.replace("_", " "),
				body: logBody.trim(),
				tags: logTags.split(",").map((tag) => tag.trim()).filter(Boolean),
				links: [],
				experimentId: selectedId,
				supersedesEntryId: null,
				sourceDigest: null,
			}));
			setLogTitle(""); setLogBody(""); setLogTags("");
			await refreshLog();
		} catch (e) { setError(String(e)); } finally { setBusy(false); }
	};

	const selected = useMemo(() => rows.find((row) => row.id === selectedId) ?? null, [rows, selectedId]);
	const researchLogDays = useMemo(() => {
		const experimentTitles = new Map(rows.map((row) => [row.id, row.title]));
		const days = new Map<string, { label: string; projects: Map<string, { title: string; entries: ResearchJournalEntry[] }> }>();
		for (const entry of log) {
			const occurred = new Date(entry.occurredAt);
			const dayKey = Number.isNaN(occurred.valueOf()) ? entry.occurredAt : `${occurred.getFullYear()}-${String(occurred.getMonth() + 1).padStart(2, "0")}-${String(occurred.getDate()).padStart(2, "0")}`;
			const dayLabel = Number.isNaN(occurred.valueOf()) ? entry.occurredAt : occurred.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric", year: "numeric" });
			if (!days.has(dayKey)) days.set(dayKey, { label: dayLabel, projects: new Map() });
			const projectKey = entry.experimentId ?? "general";
			const projectTitle = entry.experimentId ? experimentTitles.get(entry.experimentId) ?? "Unknown experiment" : "General research";
			const projects = days.get(dayKey)!.projects;
			if (!projects.has(projectKey)) projects.set(projectKey, { title: projectTitle, entries: [] });
			projects.get(projectKey)!.entries.push(entry);
		}
		return [...days.entries()].map(([key, day]) => ({ key, label: day.label, projects: [...day.projects.entries()].map(([projectKey, project]) => ({ key: projectKey, ...project })) }));
	}, [log, rows]);
	const memberNodes = useMemo(
		() => (selected ? orderLineageNodes(selected.nodes, selected.edges) : []),
		[selected],
	);
	const memberEdges = useMemo(
		() =>
			(selected?.edges ?? []).map((edge) => ({
				id: edge.id,
				sourceId: edge.sourceNodeId,
				targetId: edge.targetNodeId,
				relation: edge.relation,
			})),
		[selected],
	);
	const forestNodes: ExperimentNode[] = useMemo(
		() =>
			rows.map((row) => ({
				id: row.id,
				kind: "experiment",
				title: row.title,
				status: row.status,
				config: {},
				metrics: row.bestResult,
				costUsd: null,
				artifactRefs: [],
				traceRefs: [],
				evidenceRefs: [],
				provenance: {},
				createdAt: row.createdAt,
				updatedAt: row.updatedAt,
				candidates: [],
			})),
		[rows],
	);
	const forestEdges = useMemo(
		() =>
			rows.flatMap((row) =>
				(row.lineage ?? []).map((edge) => ({
					id: edge.id,
					sourceId: edge.sourceExperimentId,
					targetId: edge.targetExperimentId,
					relation: edge.relation,
				})),
			),
		[rows],
	);
	const canvasNodes = selected ? memberNodes : forestNodes;
	const canvasEdges = selected ? memberEdges : forestEdges;
	const inspectorNode = canvasNodes.find((item) => item.id === nodeId) ?? canvasNodes[0] ?? null;

	const selectExperiment = (id: string) => {
		const row = rows.find((item) => item.id === id);
		setSelectedId(id);
		setNodeId(null);
		if (!row) return;
		void fromGenerated(commands.experimentsActivate(row.sessionId, row.id)).catch((e) => setError(String(e)));
	};

	const selectCanvasNode = (id: string) => {
		const experiment = rows.find((row) => row.id === id);
		if (experiment) {
			selectExperiment(id);
			return;
		}
		setNodeId(id);
	};

	const createChild = async (
		relation: "follow_up" | "forked_from" | "rerun_of" = "follow_up",
	) => {
		if (!selected) return;
		setBusy(true);
		setError(null);
		const prefix =
			relation === "forked_from" ? "Fork" : relation === "rerun_of" ? "Rerun" : "Follow-up";
		try {
			const child = await fromGenerated(
				commands.experimentsCreateChild({
					parentExperimentId: selected.id,
					sessionId: selected.sessionId,
					requestId: `child:${selected.id}:${crypto.randomUUID()}`,
					title: `${prefix}: ${selected.title}`,
					task: selected.task,
					model: selected.model,
					createdAt: new Date().toISOString(),
					relation,
				}),
			);
			await refresh(child.id);
			setNodeId(null);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};

	const relate = async () => {
		await refresh(selectedId);
	};

	return (
		<PluginPage className="experiments-page workbench" testId="experiments-workbench">
			<PluginPageHeader title="Experiments" description="Saved comparisons, working notes, and publication-ready reports. Nothing is uploaded." onBack={onBack} />
			<PluginTabs tabs={[{ id: "experiments", label: "Experiments" }, { id: "log", label: "Research logs" }, { id: "reports", label: "Reports" }]} selected={section} onSelect={(item) => { setSection(item); onSectionChange?.(item); }} label="Research sections" testIdPrefix="research-tab" />
			{section === "experiments" && loaded && !error && rows.length === 0 && !query.trim() ? (
				<PluginEmptyState testId="experiments-empty" title="No experiments yet" description="Comparisons created from evaluation runs will appear here with their results and lineage." guidance="Start from a conversation and ask Workshop to compare models, policies, or prompts." />
			) : section === "experiments" ? <div className="experiments-workbench">
				<ExperimentIndex
					query={query}
					rows={rows}
					selectedId={selectedId}
					error={error}
					onQuery={setQuery}
					onSelect={selectExperiment}
				/>
				<ExperimentWorkspace
					group={selected}
					lineageNodes={canvasNodes}
					edges={canvasEdges}
					node={inspectorNode}
					onSelectNode={selectCanvasNode}
					onCreateChild={() => void createChild("follow_up")}
					onFork={() => void createChild("forked_from")}
					onRerun={() => void createChild("rerun_of")}
					onRelated={() => void relate()}
					onShowForest={() => {
						setSelectedId(null);
						setNodeId(null);
					}}
					busy={busy}
				/>
			</div> : null}
			{section === "log" ? <div className="research-log-workspace">
				<div className="research-log-compose">
					<h2>Add research log</h2>
					<p>Create multiple focused entries as the work evolves. Link each entry to its project when possible.</p>
					<select aria-label="Entry kind" value={logKind} onChange={(event) => setLogKind(event.target.value)}>{["observation","hypothesis","decision","result","failure","limitation","follow_up"].map((kind) => <option key={kind} value={kind}>{kind.replace("_", " ")}</option>)}</select>
					<input aria-label="Log title" placeholder="Short title" value={logTitle} onChange={(event) => setLogTitle(event.target.value)} />
					<textarea aria-label="Log body" placeholder="What did you observe, decide, or learn?" value={logBody} onChange={(event) => setLogBody(event.target.value)} />
					<input aria-label="Log tags" placeholder="tags, comma separated" value={logTags} onChange={(event) => setLogTags(event.target.value)} />
					<label>Linked experiment<select value={selectedId ?? ""} onChange={(event) => setSelectedId(event.target.value || null)}><option value="">None</option>{rows.map((row) => <option key={row.id} value={row.id}>{row.title}</option>)}</select></label>
					<button type="button" disabled={busy || !logBody.trim()} onClick={() => void appendLog()}>Add log entry</button>
				</div>
				<div className="research-log-list">{researchLogDays.length === 0 ? <div className="ws-empty"><p>No research logs yet.</p></div> : researchLogDays.map((day) => <section key={day.key} className="research-log-day"><h2>{day.label}</h2>{day.projects.map((project) => <section key={project.key} className="research-log-project"><header><h3>{project.title}</h3><span>{project.entries.length} {project.entries.length === 1 ? "entry" : "entries"}</span></header>{project.entries.map((entry) => <article key={entry.entryId} className="research-log-entry"><header><span>{entry.entryKind.replace("_", " ")}</span><time>{new Date(entry.occurredAt).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}</time></header><h3>{entry.title}</h3><p>{entry.body}</p>{entry.tags.length ? <div className="research-log-tags">{entry.tags.map((tag) => <span key={tag}>{tag}</span>)}</div> : null}<footer>{entry.author}{entry.supersedesEntryId ? " · correction" : ""}</footer></article>)}</section>)}</section>)}</div>
			</div> : null}
			{section === "reports" ? <div className="research-reports-list">{reports.length === 0 ? <div className="ws-empty"><p>No reports yet.</p></div> : reports.map((report) => <article key={report.id}><div><span className="eyebrow">{report.status}</span><h2>{report.title}</h2><p>{report.summary || "No summary yet."}</p><small>Revision {report.currentRevision} · updated {new Date(report.updatedAt).toLocaleString()}</small></div><button type="button" onClick={() => onOpenReport(report.id)}>Open</button></article>)}</div> : null}
		</PluginPage>
	);
}
