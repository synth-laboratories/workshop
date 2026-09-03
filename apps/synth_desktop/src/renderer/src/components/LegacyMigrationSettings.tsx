// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { fromGenerated, spectaCommands } from "../bridge";
import { useEffect, useMemo, useState } from "react";
import { publicError } from "../runtime/publicError";

type Detection = {
	sourcePath: string;
	exists: boolean;
	isLegacyRuntime: boolean;
	tables: string[];
	warnings: string[];
};

type Candidate = {
	detection: Detection;
	sourceFingerprint: string | null;
	alreadyMigrated: boolean;
};

type MigrationPlan = {
	confirmationToken: string;
	confirmationPhrase: string;
	sourceDatabase: string;
	sourceFingerprint: string;
	destinationDatabase: string;
	backupDirectory: string;
	expiresAt: string;
	estimatedCounts: Record<string, number>;
	warnings: string[];
};

type MigrationReceipt = {
	migrationId: string;
	counts: Record<string, { found: number; imported: number; existing: number; skipped: number }>;
	warnings: string[];
	integrityCheck: string;
	foreignKeyViolations: number;
	rollback: { backupDatabase: string; receiptPath: string };
};

const isTauri = window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window;

export function LegacyMigrationSettings() {
	const [candidates, setCandidates] = useState<Candidate[]>([]);
	const [sourcePath, setSourcePath] = useState("");
	const [plan, setPlan] = useState<MigrationPlan | null>(null);
	const [confirmation, setConfirmation] = useState("");
	const [receipt, setReceipt] = useState<MigrationReceipt | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const scan = async () => {
		setBusy(true);
		setError(null);
		try {
			const found = await fromGenerated(spectaCommands.migrationScan());
			setCandidates(found);
			const eligible = found.find((item) => item.detection.isLegacyRuntime && !item.alreadyMigrated);
			if (eligible) setSourcePath(eligible.detection.sourcePath);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	useEffect(() => {
		if (isTauri) void scan();
	}, []);

	const total = useMemo(
		() => Object.values(plan?.estimatedCounts ?? {}).reduce((sum, count) => sum + count, 0),
		[plan]
	);

	const inspect = async () => {
		setBusy(true);
		setError(null);
		setReceipt(null);
		try {
			setPlan(await fromGenerated(spectaCommands.migrationPrepare(sourcePath)));
			setConfirmation("");
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const cancel = async () => {
		if (plan) {
			await fromGenerated(spectaCommands.migrationCancel(plan.confirmationToken)).catch(() => undefined);
		}
		setPlan(null);
		setConfirmation("");
	};

	const apply = async () => {
		if (!plan || confirmation !== plan.confirmationPhrase) return;
		setBusy(true);
		setError(null);
		try {
			const next = await fromGenerated(spectaCommands.migrationApply({
					confirmationToken: plan.confirmationToken,
					confirmationPhrase: confirmation
				}));
			setReceipt(next);
			setPlan(null);
			setConfirmation("");
			await scan();
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	return (
		<section className="model-capabilities" data-testid="legacy-migration-settings">
			<header className="model-capabilities-head">
				<h3>Legacy Python runtime data</h3>
				<p>Inspect and copy data into the Rust CoreRuntime. The legacy database is backed up and never changed or deleted.</p>
			</header>
			{!isTauri && <p>This migration tool is available in the installed desktop app.</p>}
			{error ? <div className="model-locations-error" role="alert">{error}</div> : null}
			{receipt ? (
				<div className="finetune-base-card" role="status">
					<span className="finetune-kicker">Migration complete</span>
					<strong>Integrity: {receipt.integrityCheck} · {receipt.foreignKeyViolations} foreign-key violations</strong>
					<span className="finetune-meta">Backup retained at {receipt.rollback.backupDatabase}</span>
					<span className="finetune-file">Restart Synth Desktop to attach imported sessions.</span>
				</div>
			) : null}
			{isTauri && !plan ? (
				<>
					<label className="model-location-field">
						<span>Legacy runtime.sqlite3 path</span>
						<input value={sourcePath} onChange={(event) => setSourcePath(event.target.value)} spellCheck={false} />
					</label>
					<div className="model-capability-controls">
						<button type="button" disabled={busy || !sourcePath.trim()} onClick={() => void inspect()}>Inspect migration</button>
						<button type="button" disabled={busy} onClick={() => void scan()}>Rescan defaults</button>
					</div>
					{candidates.some((item) => item.alreadyMigrated) ? <p>A migration receipt already exists for a detected legacy database.</p> : null}
				</>
			) : isTauri && plan ? (
				<div className="finetune-base-card">
					<span className="finetune-kicker">Confirmation required</span>
					<strong>{total} records found · backup before import</strong>
					<span className="finetune-meta">Source: {plan.sourceDatabase}</span>
					<span className="finetune-meta">Backup directory: {plan.backupDirectory}</span>
					{plan.warnings.map((warning) => <span className="model-capability-warning" key={warning}>{warning}</span>)}
					<label className="model-location-field">
						<span>Type <code>{plan.confirmationPhrase}</code></span>
						<input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} autoComplete="off" />
					</label>
					<div className="model-capability-controls">
						<button type="button" disabled={busy || confirmation !== plan.confirmationPhrase} onClick={() => void apply()}>Back up and import</button>
						<button type="button" disabled={busy} onClick={() => void cancel()}>Cancel</button>
					</div>
				</div>
			) : null}
		</section>
	);
}
