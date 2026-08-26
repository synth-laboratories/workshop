import type { ExperimentGroup } from "../generated/protocol";
import { formatExperimentResult } from "../runtime/experimentPresentation";

const missing = (value: unknown) => (value == null || value === "" ? "—" : String(value));

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
			<div className="experiment-table compact" role="table">
				<div className="experiment-row heading" role="row">
					<span>Experiment</span>
					<span>Status</span>
				</div>
				{rows.map((row) => (
					<button
						className={`experiment-row${selectedId === row.id ? " selected" : ""}`}
						role="row"
						key={row.id}
						aria-selected={selectedId === row.id}
						onClick={() => onSelect(row.id)}
					>
						<strong>{row.title}</strong>
						<span className={`status ${row.status}`}>{row.status}</span>
						<span className="experiment-index-meta">{missing(row.task)} · {formatExperimentResult(row.bestResult)}</span>
					</button>
				))}
			</div>
		</section>
	);
}
