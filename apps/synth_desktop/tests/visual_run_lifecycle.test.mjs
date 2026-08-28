import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const outfile = join(compiledDir, "visualRunLifecycle.mjs");
buildSync({
	entryPoints: [join(appRoot, "src/renderer/src/runtime/visualRunLifecycle.ts")],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "node",
	outfile
});
const { projectVisualRunLifecycle } = await import(pathToFileURL(outfile).href);

test("terminal optimizer failure overrides an open visual transport and identifies rejected evidence", () => {
	const failures = [780005, 780006, 780007, 780008, 780009].map((seed) => ({
		seed,
		status: "failed",
		evidenceState: "missing",
		error: `journal event digest mismatch at sequence 10: computed bad${seed}, producer reported old${seed}`
	}));
	const run = {
		id: "opt_eval_craftax_test",
		algorithmId: "eval",
		status: "failed",
		summary: {
			bounds: { hardTotalCostUsd: 2.45 },
			credentialChain: { provider: "openrouter" },
			records: failures,
			terminalManifest: {
				terminal: { kind: "failed", reason: "producer_failed" },
				error: { message: "5 of 5 required rollouts failed" },
				evidence: { completeness: "partial", reason: "5 missing" },
				evidenceLedger: failures.map((row) => ({ state: "missing", trialId: `trial:${row.seed}` })),
				usage: {
					calls: 37,
					costUsd: 0.018659,
					providerReceipt: {
						authority: "workshop.secrets_proxy",
						calls: 37,
						costUsd: 0.018659,
						capabilities: [{ provider: "openrouter" }]
					}
				},
				work: { planned: 5, failed: 5, succeeded: 0 }
			}
		},
		usage: {}
	};
	const lifecycle = projectVisualRunLifecycle(run, { status: "failed", terminal: true });
	assert.equal(lifecycle.terminal, true);
	assert.equal(lifecycle.failed, true);
	assert.equal(lifecycle.evidence.state, "rejected");
	assert.equal(lifecycle.evidence.rejected, 5);
	assert.equal(lifecycle.evidence.missing, 0, "integrity-rejected journals are not relabeled missing");
	assert.ok(lifecycle.evidence.failures.every((failure) => failure.code === "journal_digest_mismatch" && failure.sequence === 10));
	assert.deepEqual(lifecycle.usage, {
		calls: 37,
		costUsd: 0.018659,
		costCapUsd: 2.45,
		costSource: "workshop_proxy",
		provider: "openrouter"
	});
});

test("an active run remains pending without fabricating failures or usage", () => {
	const lifecycle = projectVisualRunLifecycle({
		id: "active",
		algorithmId: "eval",
		status: "running",
		summary: {},
		usage: {}
	});
	assert.equal(lifecycle.terminal, false);
	assert.equal(lifecycle.failed, false);
	assert.equal(lifecycle.evidence.state, "pending");
	assert.equal(lifecycle.usage.costSource, "unavailable");
});

test("the host disables sealing from the rendered rejected-evidence state and exposes the reason", () => {
	const host = readFileSync(join(appRoot, "src/renderer/src/components/VisualHost.tsx"), "utf8");
	assert.match(host, /data-run-evidence-state="rejected"/);
	assert.match(host, /runtimeSealBlockReason/);
	assert.match(host, /Seal unavailable — run failed with/);
	assert.match(host, /qualityGate\.revision === revision && !runtimeSealBlockReason/);
});
