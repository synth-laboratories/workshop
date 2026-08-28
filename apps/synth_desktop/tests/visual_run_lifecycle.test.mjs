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

test("a completed run accepts sealed record evidence and preserves cost unavailability from a proxy receipt", () => {
	const records = [65, 66, 85, 56, 31].map((steps, index) => ({
		seed: 780005 + index,
		rolloutId: `rollout-${index}`,
		status: "completed",
		evidenceState: "sealed_complete",
		steps,
		sealedTrace: { imported: true, traces: [{ digest: `sha256:${index}` }] }
	}));
	const lifecycle = projectVisualRunLifecycle({
		id: "opt_eval_craftax_313e406208e5",
		algorithmId: "eval",
		status: "completed",
		summary: {
			bounds: { hardTotalCostUsd: 2.45 },
			records,
			progress: {
				authoritative: {
					runState: "terminal",
					evidence: { completeness: "complete" }
				}
			}
		},
		usage: {
			costUsd: null,
			extra: {
				providerUsageReceipt: {
					authority: "workshop.secrets_proxy",
					calls: 50,
					costUsd: null,
					capabilities: [{ provider: "openrouter" }]
				}
			}
		}
	}, { status: "completed", terminal: true });

	assert.equal(lifecycle.evidence.state, "accepted");
	assert.equal(lifecycle.evidence.valid, 5);
	assert.equal(lifecycle.evidence.sealedTraces, 5);
	assert.equal(lifecycle.evidence.missing, 0);
	assert.equal(lifecycle.usage.calls, 50);
	assert.equal(lifecycle.usage.costUsd, undefined);
	assert.equal(lifecycle.usage.costSource, "workshop_proxy");
	assert.equal(lifecycle.usage.provider, "openrouter");
});

test("sealed replay remains trustworthy when evaluator reward and declared terminal steps are missing", () => {
	const records = [780005, 780006, 780007, 780008, 780009].map((seed) => ({
		seed,
		rolloutId: `rollout-${seed}`,
		trialId: `trial-${seed}`,
		status: "failed",
		evidenceState: "missing",
		evaluatorOutcome: {
			status: "failed",
			reason: "evaluator_numeric_reward_missing",
			detail: "container completed but NanoHorizon returned no numeric reward (absent)"
		},
		evidenceOutcome: {
			status: "failed",
			reason: "required_trace_evidence_failed",
			detail: "full_trace_step_count_missing: terminal record has no terminal environment-step count"
		},
		sealedTrace: { imported: true, traces: [{ digest: `sha256:${seed}` }] }
	}));
	const lifecycle = projectVisualRunLifecycle({
		id: "opt_eval_craftax_18ee39092b6c",
		algorithmId: "eval",
		status: "failed",
		error: { message: "5 of 5 required rollouts failed; evaluator_measurement_missing" },
		summary: {
			bounds: { hardTotalCostUsd: 2.45 },
			records,
			terminalManifest: {
				terminal: { kind: "failed" },
				evidenceLedger: records.map((row) => ({ state: "missing", trialId: row.trialId })),
				evidence: { refs: records.map((row) => ({ digest: row.sealedTrace.traces[0].digest })) },
				usage: {
					providerReceipt: {
						authority: "workshop.secrets_proxy",
						calls: 31,
						costUsd: 0.013007,
						capabilities: [{ provider: "openrouter" }]
					}
				},
				work: { planned: 5, failed: 5, succeeded: 0 }
			}
		},
		usage: {}
	}, { status: "failed", terminal: true });
	assert.equal(lifecycle.evidence.state, "missing");
	assert.equal(lifecycle.evidence.rejected, 0);
	assert.equal(lifecycle.evidence.sealedTraces, 5);
	assert.equal(lifecycle.evidence.gaps.filter((gap) => gap.code === "evaluator_numeric_reward_missing").length, 5);
	assert.equal(lifecycle.evidence.gaps.filter((gap) => gap.code === "full_trace_step_count_missing").length, 5);
	assert.deepEqual(lifecycle.usage, {
		calls: 31,
		costUsd: 0.013007,
		costCapUsd: 2.45,
		costSource: "workshop_proxy",
		provider: "openrouter"
	});
});

test("the host disables sealing from the rendered rejected-evidence state and exposes the reason", () => {
	const host = readFileSync(join(appRoot, "src/renderer/src/components/VisualHost.tsx"), "utf8");
	assert.match(host, /data-run-evidence-state="rejected"/);
	assert.match(host, /runtimeSealBlockReason/);
	assert.match(host, /Seal unavailable — run failed with/);
	assert.match(host, /qualityGate\.revision === revision && !runtimeSealBlockReason/);
});
