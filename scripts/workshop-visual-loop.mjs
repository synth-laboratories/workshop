#!/usr/bin/env node
// Run → record → parse → note, over Workshop's own loopback control planes.
//
// The eval driver runs an agent; the visuals IPC photographs the app while it
// does. Neither is new here — what this adds is that a run and the pictures of
// it land in one evidence directory with one manifest, so a review reads a
// single record instead of correlating a transcript against loose PNGs.
//
// Dependency-free by design: global fetch, plus a raw socket for the visuals
// IPC, which speaks HTTP/1.1 with `Connection: close`.
//
// Usage:
//   workshop-visual-loop.mjs --instance qa-abc --task banking77 --job eval \
//     [--frames 40] [--interval-ms 3000] [--out DIR] [--timeout-ms 1800000]
//     [--prompt TEXT | --prompt-file PATH] [--no-run]

import { createConnection } from "node:net";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import process from "node:process";

/** Task services, by the port each canonical container listens on. */
const TASKS = {
	banking77: { port: 8099, family: "banking77", label: "Banking77" },
	craftax: { port: 8097, family: "craftax", label: "Craftax" },
	healthbench: { port: 8114, family: "healthbench", label: "HealthBench" },
	runebench: { port: 8104, family: "runebench", label: "RuneBench" }
};

/** Plugin surfaces worth a still on every run, in reading order. */
const SURFACES = ["visuals", "optimizers", "experiments"];

function parseArgs(argv) {
	const args = { frames: 40, intervalMs: 3000, timeoutMs: 1_800_000, run: true };
	for (let i = 0; i < argv.length; i += 1) {
		const flag = argv[i];
		const next = () => {
			i += 1;
			if (i >= argv.length) throw new Error(`${flag} requires a value`);
			return argv[i];
		};
		if (flag === "--instance") args.instance = next();
		else if (flag === "--task") args.task = next();
		else if (flag === "--job") args.job = next();
		else if (flag === "--frames") args.frames = Number(next());
		else if (flag === "--interval-ms") args.intervalMs = Number(next());
		else if (flag === "--timeout-ms") args.timeoutMs = Number(next());
		else if (flag === "--out") args.out = next();
		else if (flag === "--prompt") args.prompt = next();
		else if (flag === "--prompt-file") args.prompt = readFileSync(next(), "utf8");
		else if (flag === "--no-run") args.run = false;
		else throw new Error(`unknown flag ${flag}`);
	}
	if (!args.instance) throw new Error("--instance is required");
	if (args.run && !TASKS[args.task]) {
		throw new Error(`--task must be one of ${Object.keys(TASKS).join(", ")}`);
	}
	return args;
}

const dataRoot = (instance) =>
	join(homedir(), ".synth-desktop/instances/v09", instance, "data");

function descriptor(instance, file) {
	const path = join(dataRoot(instance), file);
	try {
		return JSON.parse(readFileSync(path, "utf8"));
	} catch (error) {
		throw new Error(`cannot read ${path}: ${error.message}`);
	}
}

/** The eval driver speaks ordinary HTTP; fetch is enough. */
async function driver(desc, method, path, body) {
	const response = await fetch(`${desc.url}${path}`, {
		method,
		headers: {
			authorization: `Bearer ${desc.token}`,
			"x-synth-eval-driver": desc.schemaVersion,
			"content-type": "application/json"
		},
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	const payload = await response.json().catch(() => ({}));
	if (!response.ok) {
		throw new Error(`${method} ${path} -> ${response.status}: ${payload.error ?? JSON.stringify(payload)}`);
	}
	return payload;
}

/**
 * The visuals IPC closes the connection per request and does not always send a
 * framed body, so a raw socket read to EOF is more reliable here than fetch.
 */
function ipc(conn, method, path, body, timeoutMs = 120_000) {
	return new Promise((resolve, reject) => {
		const [host, port] = conn.url.replace(/^https?:\/\//, "").split("/")[0].split(":");
		const payload = Buffer.from(JSON.stringify(body ?? {}));
		const socket = createConnection({ host, port: Number(port) }, () => {
			socket.write(
				`${method} ${path} HTTP/1.1\r\nHost: ${host}:${port}\r\n` +
					`Authorization: Bearer ${conn.token}\r\nContent-Type: application/json\r\n` +
					`Content-Length: ${payload.length}\r\nConnection: close\r\n\r\n`
			);
			socket.write(payload);
		});
		socket.setTimeout(timeoutMs, () => socket.destroy(new Error(`ipc timeout ${method} ${path}`)));
		const chunks = [];
		socket.on("data", (chunk) => chunks.push(chunk));
		socket.on("error", reject);
		socket.on("end", () => {
			const text = Buffer.concat(chunks).toString("utf8");
			const split = text.indexOf("\r\n\r\n");
			if (split < 0) return reject(new Error(`malformed IPC response for ${path}`));
			const status = Number(text.slice(9, 12));
			const raw = text.slice(split + 4);
			let parsed;
			try {
				parsed = JSON.parse(raw);
			} catch {
				return reject(new Error(`non-JSON IPC response ${status} for ${path}: ${raw.slice(0, 200)}`));
			}
			if (status < 200 || status >= 300) {
				return reject(new Error(`${method} ${path} -> ${status}: ${JSON.stringify(parsed).slice(0, 300)}`));
			}
			resolve(parsed);
		});
	});
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * One capture, recorded into the run's evidence directory.
 *
 * Failures are recorded, not thrown: a run that produced a real result must not
 * be reported as a failure because one screenshot did not land, and a review
 * needs to see which frames are missing.
 */
async function capture(conn, dir, name, request) {
	const path = join(dir, `${name}.png`);
	try {
		const receipt = await ipc(conn, "POST", "/v1/capture", { ...request, outputPath: path });
		return { name, ok: true, ...receipt };
	} catch (error) {
		return { name, ok: false, error: error.message, request };
	}
}

/**
 * Findings, grouped so a reviewer reads causes rather than instances.
 *
 * The same defect appears in every frame of a recording, so counting raw
 * findings makes a one-line bug look like forty. Group by rule and target, and
 * report how many frames each was seen in - a defect present in the first frame
 * and the last is structural; one that comes and goes is a live-update problem.
 */
export function summarizeFindings(records) {
	const groups = new Map();
	for (const record of records) {
		const findings = record?.audit?.findings ?? [];
		for (const finding of findings) {
			const key = `${finding.rule} ${finding.target}`;
			const existing = groups.get(key);
			if (existing) {
				existing.frames.push(record.name);
				continue;
			}
			groups.set(key, { ...finding, frames: [record.name] });
		}
	}
	const surfaces = records.filter((record) => record.ok !== false).length;
	return [...groups.values()]
		.map((group) => ({
			...group,
			frameCount: group.frames.length,
			// Present everywhere it could be: structural, not a transient.
			structural: group.frames.length === surfaces && surfaces > 1
		}))
		.sort(
			(a, b) =>
				(a.severity === b.severity ? 0 : a.severity === "egregious" ? -1 : 1) ||
				b.frameCount - a.frameCount
		);
}

/**
 * What a human still has to decide.
 *
 * "egregious" findings are mechanically decidable and get proposed fixes;
 * everything else is queued with the evidence needed to judge it. Nothing is
 * auto-applied here - this writes notes, and a person or a later pass acts.
 */
export function triage(summary) {
	const fix = summary.filter((finding) => finding.severity === "egregious");
	const review = summary.filter((finding) => finding.severity !== "egregious");
	return { fix, review };
}

function markdown(manifest) {
	const { fix, review } = triage(manifest.summary);
	const failed = manifest.captures.filter((capture) => capture.ok === false).length;
	const lines = [
		`# Visual QA - ${manifest.task ?? "ad-hoc"} ${manifest.job ?? ""}`.trim(),
		"",
		`**Run:** \`${manifest.sessionId ?? "none"}\` | **Instance:** \`${manifest.instance}\``,
		`**Source revision:** \`${manifest.sourceRevision ?? "unknown"}\``,
		`**Captured:** ${manifest.startedAt} to ${manifest.finishedAt}`,
		`**Surfaces:** ${manifest.captures.length} (${failed} failed)`,
		"",
		"## Findings",
		""
	];
	const measured = manifest.captures.length - failed;
	if (measured === 0) {
		// "No findings" and "nothing was measured" must never read the same. A
		// review that mistakes the second for the first concludes a surface is
		// clean when in fact no surface was ever photographed.
		lines.push(
			`**Nothing was measured.** All ${failed} captures failed, so this run makes no claim`,
			"about these surfaces at all. Fix the capture path before reading anything below.",
			""
		);
	} else if (fix.length === 0 && review.length === 0) {
		lines.push(
			`No machine-checkable finding across ${measured} captured surfaces. That is not the`,
			"same as good: semantics, hierarchy, and task grammar are not decidable here and",
			"still need eyes.",
			""
		);
	} else if (failed > 0) {
		lines.push(
			`Note: ${failed} of ${manifest.captures.length} captures failed, so the findings below`,
			"cover only the surfaces that were actually photographed.",
			""
		);
	}
	if (fix.length) {
		lines.push("### Egregious - mechanically decided, safe to act on", "");
		lines.push("| Rule | Target | Detail | Frames | Structural |", "|---|---|---|---:|---|");
		for (const f of fix) {
			lines.push(
				`| \`${f.rule}\` | \`${f.target}\` | ${f.detail} | ${f.frameCount} | ${f.structural ? "yes" : "no"} |`
			);
		}
		lines.push("");
	}
	if (review.length) {
		lines.push("### For human review - measured, but the judgement is not a machine's", "");
		lines.push("| Rule | Target | Detail | Frames |", "|---|---|---|---:|");
		for (const f of review) {
			lines.push(`| \`${f.rule}\` | \`${f.target}\` | ${f.detail} | ${f.frameCount} |`);
		}
		lines.push("");
	}
	lines.push("## Captures", "");
	for (const capture of manifest.captures) {
		const status =
			capture.ok === false
				? `FAILED - ${capture.error}`
				: `${capture.width}x${capture.height} \`${capture.digest ?? "no digest"}\``;
		lines.push(`- \`${capture.name}\` - ${status}`);
	}
	return `${lines.join("\n")}\n`;
}

/** The agent prompt for one task/job pair. Explicit about not substituting. */
function prompt(task, job) {
	const spec = TASKS[task];
	const shared = [
		`The QA-owned ${spec.label} service is at http://127.0.0.1:${spec.port}.`,
		`Register that URL as task family ${spec.family} if it is not already registered, then probe it.`,
		"Create and subscribe the product-owned visual before starting any paid work.",
		"Poll to terminal and report the run id, rollout counts, and the numeric result.",
		"Do not substitute a hand-built loop, a fixture, or a different recipe.",
		"Do not ask for confirmation; this session runs under the unattended QA profile.",
		"If admission is blocked, copy the structured blocker code, owner, and retryable value,",
		"and state explicitly that no run id or rollout records were created."
	].join(" ");
	const lead = {
		eval: `Run an ordinary evaluation campaign on ${spec.label} using your available product tools.`,
		gepa: `Run a GEPA prompt search on ${spec.label} using your available product tools. Report seed score, winning train score, heldout score, and numeric lift. Do not claim uplift unless the heldout evidence proves it.`,
		sft: `Run an SFT training job on ${spec.label} using your available product tools. Report the baseline, the selected checkpoint, and the paired heldout comparison. Selection is not uplift.`,
		cispo: `Run a CISPO training job on ${spec.label} using your available product tools. Report clip bounds, group size, reward variance, advantage distribution, and whether a learning signal existed at all.`
	}[job];
	if (!lead) throw new Error(`unknown job ${job}; expected eval, gepa, sft, or cispo`);
	return `${lead} ${shared}`;
}

/**
 * Record the app while the run happens, then photograph the surfaces it left.
 *
 * The filmstrip and the stills answer different questions. Frames catch what
 * only exists while a job is live - a pre-start state, a partial result, a
 * chart with one point - and the stills are the durable surfaces a reviewer
 * will actually open afterwards.
 */
async function recordWhile(conn, dir, running, { frames, intervalMs }) {
	const records = [];
	for (let index = 0; index < frames && running.active; index += 1) {
		records.push(await capture(conn, dir, `frame-${String(index).padStart(4, "0")}`, { scope: "app" }));
		if (index + 1 < frames && running.active) await sleep(intervalMs);
	}
	return records;
}

async function main() {
	const args = parseArgs(process.argv.slice(2));
	const evalDriver = descriptor(args.instance, "eval-driver.json");
	const visuals = descriptor(args.instance, "visuals-ipc.json");
	const startedAt = new Date().toISOString();
	const slug = [args.task, args.job].filter(Boolean).join("-") || "adhoc";
	const dir = args.out ?? join(dataRoot(args.instance), "visual-qa", `${slug}-${Date.now().toString(36)}`);
	mkdirSync(dir, { recursive: true });

	const health = await driver(evalDriver, "GET", "/v1/health");
	if (health.ok !== true) throw new Error("eval driver health check failed");
	const sourceRevision = health.instance?.buildRevision ?? evalDriver.sourceRevision;

	// Register the task container before the agent needs it. Without this the
	// agent correctly refuses to start -- it has no URL-registration tool of its
	// own -- and the run reports an honest failure that says nothing about the
	// visuals. Registration is idempotent on base URL.
	if (args.run) {
		const spec = TASKS[args.task];
		const probe = await fetch(`http://127.0.0.1:${spec.port}/health`).catch(() => null);
		if (!probe?.ok) {
			throw new Error(`${spec.label} is not serving on 127.0.0.1:${spec.port}; start it before running the loop`);
		}
		await ipc(visuals, "POST", "/v1/containers", {
			name: `${spec.family}-qa`,
			baseUrl: `http://127.0.0.1:${spec.port}`,
			location: "local",
			taskFamily: spec.family
		});
	}

	// Before anything runs: what the app looked like at rest. Without this the
	// review cannot tell a defect the run caused from one that was already there.
	const captures = [await capture(visuals, dir, "00-app-at-rest", { scope: "app" })];

	let sessionId;
	let terminal;
	if (args.run) {
		const body = args.prompt ?? prompt(args.task, args.job);
		sessionId = `vq_${Date.now().toString(36)}`;
		await driver(evalDriver, "POST", "/v1/sessions", { sessionId });
		await driver(evalDriver, "POST", `/v1/sessions/${sessionId}/messages`, { body });
		writeFileSync(join(dir, "prompt.txt"), `${body}\n`);

		// The recording runs beside the wait: the driver holds one HTTP exchange
		// at a time, and a run that only reports its terminal state has no
		// evidence of how it got there.
		const running = { active: true };
		const recording = recordWhile(visuals, dir, running, args);
		const deadline = Date.now() + args.timeoutMs;
		try {
			do {
				const remaining = deadline - Date.now();
				if (remaining <= 0) throw new Error(`session ${sessionId} did not reach terminal in ${args.timeoutMs}ms`);
				terminal = await driver(evalDriver, "POST", `/v1/sessions/${sessionId}/wait_terminal`, {
					timeoutMs: Math.min(20_000, remaining)
				});
			} while (terminal?.terminal !== true);
		} finally {
			running.active = false;
			captures.push(...(await recording));
		}
		writeFileSync(join(dir, "terminal.json"), `${JSON.stringify(terminal, null, 2)}\n`);

		const exported = await driver(evalDriver, "GET", `/v1/sessions/${sessionId}/export`);
		writeFileSync(join(dir, "session-export.json"), `${JSON.stringify(exported, null, 2)}\n`);

		// Every visual the run produced, photographed in isolation the way
		// authoring review sees it, then again inside the app.
		for (const entry of exported.visuals ?? []) {
			const id = entry?.visual?.id;
			if (!id) continue;
			captures.push(await capture(visuals, dir, `visual-${id}`, { scope: "visual", target: id }));
		}
	}

	for (const surface of SURFACES) {
		captures.push(await capture(visuals, dir, `surface-${surface}`, { scope: "plugin", target: surface }));
	}
	// The compact right-panel breakpoint the review protocol asks for, plus the
	// wide one, so responsive geometry is measured and not assumed.
	for (const [name, width, height] of [["compact", 900, 1000], ["wide", 1680, 1050]]) {
		captures.push(
			await capture(visuals, dir, `visuals-${name}`, { scope: "plugin", target: "visuals", width, height })
		);
	}

	const summary = summarizeFindings(captures);
	const manifest = {
		schemaVersion: "synth.visual-qa-run.v1",
		instance: args.instance,
		task: args.task,
		job: args.job,
		sessionId,
		sourceRevision,
		startedAt,
		finishedAt: new Date().toISOString(),
		directory: dir,
		terminal: terminal ?? null,
		captures,
		summary,
		triage: triage(summary)
	};
	writeFileSync(join(dir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
	writeFileSync(join(dir, "REVIEW.md"), markdown(manifest));
	process.stdout.write(
		`${JSON.stringify({
			directory: dir,
			sessionId,
			captures: captures.length,
			failedCaptures: captures.filter((capture) => capture.ok === false).length,
			egregious: manifest.triage.fix.length,
			review: manifest.triage.review.length
		})}\n`
	);
}

// Importable for tests; only the CLI path runs main.
const invokedDirectly = process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/^.*\//, ""));
if (invokedDirectly) {
	main().catch((error) => {
		process.stderr.write(`workshop-visual-loop: ${error.message}\n`);
		process.exit(1);
	});
}
