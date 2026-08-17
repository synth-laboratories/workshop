#!/usr/bin/env node
/**
 * Seal `run_progress_history.v1` onto runs that finished before Workshop learned
 * to record it.
 *
 * A run's card estimates its remaining time from the shape earlier runs of the
 * same recipe traced through the same work. Going forward that shape is sealed
 * at terminal by `src-tauri/src/optimizers/progress_history.rs`, but runs that
 * completed before this shipped carry nothing — so a fresh install would have no
 * history to estimate against until three more runs finished. This walks the
 * existing instance databases and fills them in from the events already stored.
 *
 * It is the same computation as the Rust sealer, deliberately duplicated here
 * rather than shared: this is a one-off migration that should not become a
 * second live code path. `run_progress_history.test.mjs` asserts the two agree.
 *
 * Usage:
 *   node apps/synth_desktop/scripts/backfill-progress-history.mjs [--apply] [--db PATH]…
 *
 * Without `--apply` it only reports what it would change.
 */

import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { DatabaseSync } from "node:sqlite";

const CURVE_POINTS = 19;
const MIN_UNITS = 8;
const SCHEMA = "run_progress_history.v1";

const COMPLETION_TYPES = {
	gepa: ["optimizer.evaluation_result.received"],
	eval: ["eval.trial.terminal"],
	sft: ["sft.checkpoint_rollout.completed"]
};
const UNIT = { gepa: "rollouts", eval: "trials", sft: "rollouts" };
const UNIT_ID_KEYS = ["rollout_id", "rolloutId", "trial_id", "trialId"];

function parseArgs(argv) {
	const dbs = [];
	let apply = false;
	for (let index = 0; index < argv.length; index += 1) {
		if (argv[index] === "--apply") apply = true;
		else if (argv[index] === "--db") dbs.push(argv[index + 1], index += 1)[0];
	}
	return { apply, dbs: dbs.filter((entry) => typeof entry === "string") };
}

/** Every instance database under ~/.synth-desktop, newest first. */
function discoverDatabases() {
	const root = join(homedir(), ".synth-desktop", "instances");
	const found = [];
	const walk = (dir, depth) => {
		if (depth > 3) return;
		let entries;
		try {
			entries = readdirSync(dir, { withFileTypes: true });
		} catch {
			return;
		}
		for (const entry of entries) {
			const path = join(dir, entry.name);
			if (entry.isDirectory()) walk(path, depth + 1);
			else if (entry.name === "synth.sqlite3") found.push(path);
		}
	};
	walk(root, 0);
	const direct = join(homedir(), ".synth-desktop", "synth.sqlite3");
	try {
		if (statSync(direct).isFile()) found.push(direct);
	} catch { /* not every layout has one */ }
	return found;
}

function unitId(delta, item) {
	for (const key of UNIT_ID_KEYS) {
		if (typeof delta?.[key] === "string" && delta[key].length > 0) return delta[key];
	}
	return typeof item?.id === "string" ? item.id : null;
}

/** The curve a completed run traced, or null when it cannot teach anything. */
export function buildCurve(algorithmId, status, events) {
	if (status !== "completed") return null;
	const wanted = COMPLETION_TYPES[algorithmId];
	if (!wanted) return null;
	const seen = new Set();
	const completions = [];
	for (const event of events) {
		if (!wanted.includes(event.type)) continue;
		const at = Date.parse(event.occurredAt);
		if (!Number.isFinite(at)) continue;
		const id = unitId(event.delta, event.item);
		if (id != null) {
			if (seen.has(id)) continue;
			seen.add(id);
		}
		completions.push(at);
	}
	completions.sort((left, right) => left - right);
	if (completions.length < MIN_UNITS) return null;
	const first = completions[0];
	const span = completions.at(-1) - first;
	if (span <= 0) return null;
	const total = completions.length;
	const curve = [];
	for (let step = 1; step <= CURVE_POINTS; step += 1) {
		const fraction = step / (CURVE_POINTS + 1);
		const index = Math.min(total, Math.max(1, Math.ceil(fraction * total))) - 1;
		curve.push((completions[index] - first) / span);
	}
	return {
		schemaVersion: SCHEMA,
		unit: UNIT[algorithmId],
		totalUnits: total,
		wallTimeMs: span,
		curve
	};
}

function backfill(path, apply) {
	const db = new DatabaseSync(path, { readOnly: !apply });
	let sealed = 0;
	let skipped = 0;
	let already = 0;
	try {
		const runs = db
			.prepare("select id, algorithm_id, status, summary_json from optimizer_runs where status = 'completed'")
			.all();
		for (const run of runs) {
			let summary;
			try {
				summary = JSON.parse(run.summary_json ?? "{}");
			} catch {
				summary = {};
			}
			if (summary?.progressHistory?.schemaVersion === SCHEMA) {
				already += 1;
				continue;
			}
			const events = db
				.prepare("select payload_json from optimizer_events where optimizer_run_id = ? order by sequence_number")
				.all(run.id)
				.map((row) => {
					try {
						return JSON.parse(row.payload_json);
					} catch {
						return null;
					}
				})
				.filter(Boolean);
			const history = buildCurve(run.algorithm_id, run.status, events);
			if (!history) {
				skipped += 1;
				continue;
			}
			if (apply) {
				db.prepare("update optimizer_runs set summary_json = ? where id = ?").run(
					JSON.stringify({ ...summary, progressHistory: history }),
					run.id
				);
			}
			sealed += 1;
		}
	} finally {
		db.close();
	}
	return { sealed, skipped, already };
}

/*
 * Only run as a command. `buildCurve` is imported by
 * `tests/run_progress_history.test.mjs`, which checks it agrees with the Rust
 * sealer, and importing a module must never write to a database.
 */
const invokedDirectly = process.argv[1] != null &&
	fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (!invokedDirectly) {
	// Imported for `buildCurve`; nothing else to do.
} else {
main();
}

function main() {
const { apply, dbs } = parseArgs(process.argv.slice(2));
const targets = dbs.length > 0 ? dbs : discoverDatabases();
if (targets.length === 0) {
	console.error("no instance databases found; pass --db PATH");
	process.exit(1);
}
console.log(`${apply ? "sealing" : "dry run over"} ${targets.length} database(s)\n`);
let totals = { sealed: 0, skipped: 0, already: 0 };
for (const path of targets) {
	let result;
	try {
		result = backfill(path, apply);
	} catch (reason) {
		console.log(`  !  ${path}\n     ${reason.message}`);
		continue;
	}
	if (result.sealed || result.already) {
		console.log(
			`  ${String(result.sealed).padStart(4)} sealed  ${String(result.already).padStart(4)} already  ${String(result.skipped).padStart(4)} not teachable   ${path.replace(homedir(), "~")}`
		);
	}
	for (const key of Object.keys(totals)) totals[key] += result[key];
}
console.log(
	`\n${totals.sealed} run(s) ${apply ? "sealed" : "would be sealed"}, ${totals.already} already carried a curve, ${totals.skipped} had too little work to teach anything.`
);
if (!apply) console.log("re-run with --apply to write.");
}
