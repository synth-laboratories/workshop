/**
 * Restoration contract for `live.harbor_eval.v1` and the structured command
 * failure summary.
 *
 * The fixture is the real persisted binding from the 2026-09-03 DeepSWE
 * five-task sample (`opt_eval_deepswe_348bdbe13925`, visual revision 14),
 * trimmed to two rollouts. Reopening that visual showed `connecting`, `—`
 * trials and `0/0` events while holding this exact document.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { harborEvalSnapshot, snapshotWorkbenchVisualId } from "../runtime/harborEvalSnapshot.ts";
import { commandFailureHeadline, projectCommandFailures } from "../runtime/commandFailure.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(join(root, relative), "utf8");
const OVERVIEW = JSON.parse(read("tests/fixtures/deepswe_terminal_overview.json"));
const SHELL = "families/first_class_example_containers/live.harbor_eval.v1/shell.tsx";

test("a settled run restores from its persisted snapshot with no live stream", () => {
  const snapshot = harborEvalSnapshot(OVERVIEW);
  assert.ok(snapshot, "the persisted experiment projection is accepted");
  assert.equal(snapshot.lifecycle, "terminal");
  assert.equal(snapshot.status, "failed");
  assert.equal(snapshot.work.planned, 5);
  assert.equal(snapshot.work.failed, 5);
  assert.equal(snapshot.work.succeeded, 0);
  assert.equal(snapshot.elapsed, "1m 21s");
});

test("rollouts are named by task, not by seed", () => {
  const snapshot = harborEvalSnapshot(OVERVIEW);
  assert.deepEqual(
    snapshot.trials.map((trial) => trial.label),
    ["anko-default-function-arguments", "happy-dom-abort-pending-body-reads"]
  );
  assert.equal(snapshot.trials[0].taskInstanceId, "deepswe/anko-default-function-arguments");
  assert.equal(snapshot.trials[0].seed, 0);
});

test("the selected task keeps its terminal reason and its retained trace", () => {
  const snapshot = harborEvalSnapshot(OVERVIEW);
  const trial = snapshot.trials[0];
  assert.match(trial.stopReason ?? "", /producer_terminal_failure_missing_reason/);
  assert.match(trial.traceId ?? "", /^tracev5_/);
  assert.equal(trial.reward, null, "an unscored trial has no reward, not a zero");
});

test("the snapshot links out to the trace workstation for the same run", () => {
  const snapshot = harborEvalSnapshot(OVERVIEW);
  assert.match(snapshotWorkbenchVisualId(snapshot) ?? "", /^vis_/);
});

test("incomplete usage renders as unavailable and never as zero", () => {
  const snapshot = harborEvalSnapshot(OVERVIEW);
  assert.equal(snapshot.usage.tokens, "unavailable");
  assert.equal(snapshot.usage.cost, "unavailable / $16.00");
  assert.equal(snapshot.usage.complete, false);
  assert.equal(snapshot.meanReward, null);
  assert.ok(
    snapshot.limitations.some((limitation) => /cost is unavailable, not zero/.test(limitation)),
    "the producer's own limitation about cost survives restoration"
  );
});

test("a document that is not an experiment overview is refused rather than guessed", () => {
  assert.equal(harborEvalSnapshot(null), null);
  assert.equal(harborEvalSnapshot({ schemaVersion: "something.else.v1" }), null);
  assert.equal(harborEvalSnapshot({ aggregate: { lifecycle: "terminal" } }), null);
});

test("the shell never reports connecting for a settled run", () => {
  const source = read(SHELL);
  // `restored` gates every pane that can only say "waiting" on a closed
  // stream, and the status metric reads the snapshot instead of the transport.
  assert.match(source, /const restored = settled && events\.length === 0;/);
  assert.match(source, /const statusLabel = restored/);
  assert.match(source, /\{restored \? null : \(/);
  assert.match(source, /harborEvalSnapshot/);
});

test("a bwrap namespace denial becomes one named, counted failure with a remedy", () => {
  const events = [0, 1].flatMap((seed) => [
    {
      delta: {
        container_event: {
          kind: "span.policy.data",
          rollout_id: `roll_deepswe_train_${seed}`,
          payload: {
            kind: "codex.event",
            event: {
              type: "item.completed",
              item: {
                id: "item_0",
                type: "command_execution",
                status: "failed",
                exit_code: 1,
                command: "/bin/bash -lc \"sed -n '1,240p' observation.txt\"",
                aggregated_output:
                  "bwrap: No permissions to create a new namespace, likely because the kernel does not allow non-privileged user namespaces.\n"
              }
            }
          }
        }
      }
    },
    {
      delta: {
        container_event: {
          kind: "span.policy.data",
          rollout_id: `roll_deepswe_train_${seed}`,
          payload: {
            kind: "codex.event",
            event: {
              type: "item.completed",
              item: { id: "item_1", type: "command_execution", status: "completed", exit_code: 0, command: "pwd" }
            }
          }
        }
      }
    }
  ]);
  const summary = projectCommandFailures(events);
  assert.equal(summary.total, 2, "only failed commands count");
  assert.equal(summary.groups.length, 1);
  assert.equal(summary.dominant.reason, "sandbox_namespace_unavailable");
  assert.equal(summary.dominant.infrastructure, true);
  assert.deepEqual(summary.dominant.rolloutIds, ["roll_deepswe_train_0", "roll_deepswe_train_1"]);
  assert.match(summary.dominant.remedy, /danger-full-access/);
  assert.equal(
    commandFailureHeadline(summary),
    "The inner agent sandbox could not be created · 2 commands across 2 rollouts"
  );
});

test("an environment fault outranks a larger pile of ordinary non-zero exits", () => {
  const command = (status, output) => ({
    kind: "span.policy.data",
    rollout_id: "roll_1",
    payload: {
      event: {
        item: { type: "command_execution", status, exit_code: 1, command: "x", aggregated_output: output }
      }
    }
  });
  const summary = projectCommandFailures([
    command("failed", "assertion failed"),
    command("failed", "assertion failed"),
    command("failed", "assertion failed"),
    command("failed", "bwrap: No permissions to create a new namespace")
  ]);
  assert.equal(summary.dominant.reason, "sandbox_namespace_unavailable");
  assert.deepEqual(
    summary.groups.map((group) => group.reason),
    ["sandbox_namespace_unavailable", "command_failed"]
  );
});

test("no failure at all yields no card rather than an empty one", () => {
  const summary = projectCommandFailures([]);
  assert.equal(summary.total, 0);
  assert.equal(summary.dominant, null);
  assert.equal(commandFailureHeadline(summary), null);
});

test("the workstation labels rollouts by task and leads with the dominant cause", () => {
  const source = read("families/first_class_example_containers/_shared/traceWorkbench.tsx");
  assert.match(source, /\{trialLabel\(row\) \? <span>\{trialLabel\(row\)\}<\/span> : null\}/);
  assert.doesNotMatch(source, /<span style=\{mono\}>seed \{row\.seed \?\? MISSING\}<\/span>/);
  assert.match(source, /<CommandFailureCard summary=\{commandFailures\} testId=\{branding\.testId\} \/>/);
  assert.match(source, /projectCommandFailures\(optimizerEvents\)/);
});

test("a trial carries the task it ran, from the plan or from its terminal record", async () => {
  const { craftaxTrialsFromRun } = await import("../runtime/craftaxTraceView.ts");
  const run = { id: "opt_1", summary: { task: "deepswe" } };
  const trials = craftaxTrialsFromRun(run, [
    {
      type: "eval.trial.queued",
      delta: {
        workItemId: "eval:trial:0",
        trial_id: "trial:deepswe:0",
        seed: 0,
        scenario: "deepswe",
        task_instance_id: "deepswe/anko-default-function-arguments"
      }
    },
    {
      type: "eval.trial.started",
      delta: {
        workItemId: "eval:trial:0",
        trial_id: "trial:deepswe:0",
        rollout_id: "roll_0",
        seed: 0,
        pool: "train",
        scenario: "deepswe/anko-default-function-arguments"
      }
    }
  ]);
  assert.equal(trials.length, 1);
  assert.equal(trials[0].taskInstanceId, "deepswe/anko-default-function-arguments");
});

test("an older run with only a per-trial scenario still names its task", async () => {
  const { craftaxTrialsFromRun } = await import("../runtime/craftaxTraceView.ts");
  const trials = craftaxTrialsFromRun({ id: "opt_1", summary: { task: "deepswe" } }, [
    {
      type: "eval.trial.started",
      delta: {
        workItemId: "eval:trial:1",
        trial_id: "trial:deepswe:1",
        rollout_id: "roll_1",
        seed: 1,
        pool: "train",
        scenario: "deepswe/happy-dom-abort-pending-body-reads"
      }
    }
  ]);
  assert.equal(trials[0].taskInstanceId, "deepswe/happy-dom-abort-pending-body-reads");
});

test("a run-wide family is not mistaken for a per-task identity", async () => {
  const { craftaxTrialsFromRun } = await import("../runtime/craftaxTraceView.ts");
  const trials = craftaxTrialsFromRun({ id: "opt_1", summary: { task: "craftax" } }, [
    {
      type: "eval.trial.started",
      delta: { workItemId: "eval:trial:0", trial_id: "t0", rollout_id: "r0", seed: 7, scenario: "craftax" }
    }
  ]);
  assert.equal(trials[0].taskInstanceId, null, "the family names every trial and so names none");
});

// ---------------------------------------------------------------------------
// Visual QA ship blockers, 2026-09-03.
// ---------------------------------------------------------------------------

test("the template's own observation wins over the host's transport wrapper", async () => {
  const { selectObservationSurface } = await import("../runtime/observationSurface.ts");
  const element = (attributes) => ({
    attributes,
    getAttribute: (name) => attributes[name] ?? null,
    hasAttribute: (name) => name in attributes
  });
  // Document order: the host wrapper comes first and says `idle`.
  const wrapper = element({ "data-visual-transport-state": "idle" });
  const template = element({
    "data-visual-observation": "template",
    "data-visual-transport-state": "terminal",
    "data-visual-rendered-frame-count": "12"
  });
  assert.equal(selectObservationSurface([wrapper, template]), template);

  // A template that writes the attributes by hand is still recognised.
  const handWritten = element({
    "data-visual-transport-state": "live",
    "data-visual-rollout-count": "4"
  });
  assert.equal(selectObservationSurface([wrapper, handWritten]), handWritten);

  // With nothing else, the wrapper is still the answer rather than nothing.
  assert.equal(selectObservationSurface([wrapper]), wrapper);
  assert.equal(selectObservationSurface([]), undefined);
});

test("the shared chrome marks the observation it publishes", () => {
  assert.match(read("chrome/VisualChrome.tsx"), /"data-visual-observation": "template",/);
  assert.match(
    read("families/first_class_example_containers/live.craftax.v1/shell.tsx"),
    /data-visual-observation="template"/
  );
});

test("frame evidence is counted across the run, not only the selected trial", () => {
  const source = read("families/first_class_example_containers/_shared/traceWorkbench.tsx");
  assert.match(source, /const renderedFrameCount = useMemo\(/);
  assert.match(source, /trials\.reduce\(\(total, row\) => total \+ row\.view\.frames\.filter/);
  assert.doesNotMatch(source, /renderedFrameCount: view\?\.frames\.filter/);
});

test("the anonymous data prop does not shadow a single declared input", async () => {
  const { anonymousDataProp } = await import("../runtime/bind.ts");
  const acceptance = { events: [{ kind: "acceptance", payload: { decision: "pass" } }] };
  // `live.intern_acceptance.v1` reads `props.data ?? props.acceptance`. Handing
  // it the whole map made it read `{ acceptance: {...} }`, find no `events`,
  // and rest at "awaiting source" while fully resolved.
  assert.deepEqual(anonymousDataProp({ acceptance }), acceptance);
  // Several inputs have no unambiguous anonymous payload; the map is kept.
  const many = { trace: { steps: [] }, annotations: { markers: [] } };
  assert.deepEqual(anonymousDataProp(many), many);
  // `data` may be an explicitly declared input alongside supporting inputs.
  // In that case it remains the anonymous payload instead of being shadowed
  // by the resolved-props map.
  const explicit = { data: { rows: [1, 2] }, provenance: { run: "opt_1" } };
  assert.deepEqual(anonymousDataProp(explicit), explicit.data);
  // An optimizer payload still wins, as does a bound optimizer_run.
  assert.equal(anonymousDataProp({ acceptance }, "payload"), "payload");
  assert.equal(anonymousDataProp({ optimizer_run: "run", other: 1 }), "run");
});

test("a fixture binding resolves from packaged assets instead of failing closed", async () => {
  const { bindTemplateSlots } = await import("../runtime/bind.ts");
  const template = {
    id: "annotation.overlay.v1",
    inputs: [{ name: "annotations", accepts: ["fixture"], required: true }]
  };
  const loaded = [];
  const result = await bindTemplateSlots(
    template,
    [{ input: "annotations", kind: "fixture", source: "fixtures/annotation_markers.json" }],
    {
      loadFixture: async (source) => {
        loaded.push(source);
        return { markers: [{ kind: "note", step_index: 0 }] };
      }
    }
  );
  assert.deepEqual(result.errors, []);
  assert.deepEqual(loaded, ["fixtures/annotation_markers.json"]);
  assert.deepEqual(result.slots.annotations.data, { markers: [{ kind: "note", step_index: 0 }] });
});

test("a fixture binding that already carries its payload needs no loader", async () => {
  const { bindTemplateSlots } = await import("../runtime/bind.ts");
  const result = await bindTemplateSlots(
    { id: "t", inputs: [{ name: "annotations", accepts: ["fixture"], required: true }] },
    [{ input: "annotations", kind: "fixture", data: { markers: [] } }],
    {}
  );
  assert.deepEqual(result.errors, []);
  assert.deepEqual(result.slots.annotations.data, { markers: [] });
});

test("an unbound container-rollouts surface says so instead of waiting forever", () => {
  const source = read("families/first_class_example_containers/live.container_rollouts.v1/shell.tsx");
  assert.match(source, /!hasSource && bindingFor\(props\.bindings, "stream"\) === null/);
  assert.match(source, /no rollout stream is bound to this visual/);
});

test("a task instance that only restates the seed is not treated as a task name", async () => {
  const { craftaxTrialsFromRun } = await import("../runtime/craftaxTraceView.ts");
  // Craftax declares `taskInstanceId: "seed:0"`, which rendered as
  // `seed:0 · seed 0` on every rollout chip.
  const trials = craftaxTrialsFromRun({ id: "opt_1", summary: { task: "craftax" } }, [
    {
      type: "eval.trial.started",
      delta: { workItemId: "eval:trial:0", trial_id: "trial:craftax:0", rollout_id: "r0", seed: 0, scenario: "craftax" }
    },
    {
      type: "eval.trial.terminal",
      delta: { trial_id: "trial:craftax:0" },
      item: { raw: { rolloutId: "r0", taskInstanceId: "seed:0", reward: 1 } }
    }
  ]);
  assert.equal(trials[0].taskInstanceId, "seed:0", "the producer's identity is retained as data");
  const source = read("families/first_class_example_containers/_shared/traceWorkbench.tsx");
  assert.match(source, /\(\^\|:\)seed:\$\{row\.seed\}\$/, "and the label rejects it as a name");
});
