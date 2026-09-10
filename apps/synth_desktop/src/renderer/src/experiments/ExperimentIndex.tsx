import type { ExperimentGroup } from "../generated/protocol";
import { formatExperimentResult } from "../runtime/experimentPresentation";

const missing = (value: unknown) => (value == null || value === "" ? "—" : String(value));

function formatUpdated(value: string): string {
	const parsed = Date.parse(value);
	return Number.isFinite(parsed) ? new Date(parsed).toLocaleString() : "—";
}

export function ExperimentIndex({
	query,
	rows,
	selectedId,
	error,
	onQuery,
	onSelect,
}: {
	query: string;
	rows: ExperimentGroup[];
	selectedId: string | null;
	error: string | null;
	onQuery: (value: string) => void;
	onSelect: (id: string) => void;
}) {
	return (
		<section className="experiment-index" data-testid="experiments-index">
			<label className="experiment-search">
				Search experiments
				<input autoFocus value={query} onChange={(e) => onQuery(e.target.value)} placeholder="Title, task, model, or status" />
			</label>
			{error ? <p role="alert">{error}</p> : null}
			{query.trim() && rows.length === 0 ? (
				<div className="ws-empty" data-testid="experiments-no-results">
					<p>No experiments match “{query}”.</p>
					<button type="button" data-testid="experiments-clear-search" onClick={() => onQuery("")}>
						Clear search
					</button>
				</div>
			) : (
				<div className="experiment-table compact" role="table">
					<div className="experiment-row heading" role="row">
						<span>Experiment</span>
						<span className="experiment-col-task">Task</span>
						<span>Status</span>
						<span>Result</span>
						<span>Runs</span>
						<span>Updated</span>
					</div>
					{rows.map((row) => {
						const runCount = row.members.length;
						const result = formatExperimentResult(row.bestResult);
						const updated = formatUpdated(row.updatedAt);
						return (
							<div
								className={`experiment-row${selectedId === row.id ? " selected" : ""}`}
								role="row"
								key={row.id}
								aria-selected={selectedId === row.id}
								onClick={() => onSelect(row.id)}
							>
								<button type="button" className="experiment-row-hit" onClick={() => onSelect(row.id)}>
									<strong>{row.title}</strong>
									<span className="experiment-col-task">{missing(row.task)}</span>
									<span className={`status ${row.status}`}>{row.status}</span>
									<span className="experiment-result-summary">{result}</span>
									<span className="experiment-col-runs" aria-label={`${runCount} runs`}>{runCount}</span>
									<time className="experiment-col-updated" dateTime={row.updatedAt}>{updated}</time>
								</button>
								<details className="experiment-task-disclosure" onClick={(event) => event.stopPropagation()}>
									<summary>Task</summary>
									<p>{missing(row.task)}</p>
								</details>
							</div>
						);
					})}
				</div>
			)}
		</section>
	);
}
