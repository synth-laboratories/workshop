/**
 * Generate the live-fold golden: what the fold answers over every checked-in
 * fixture, in a form both implementations can be held to.
 *
 * The fold has one authoritative home (`src-tauri/src/stream_fold.rs`) and one
 * mirror that hosts without Rust — browser preview, fixture replay, the two
 * shipped shells — still need in order to draw anything at all. A mirror is
 * only honest while something checks it, so this writes the answer down once
 * and both sides assert against the same file:
 *
 *   node visuals/tests/live_fold_golden_gen.mjs      # rewrite the golden
 *   node --test visuals/tests/live_fold_golden.test.mjs
 *   cargo test -p synth_desktop stream_fold::tests::golden
 *
 * What is recorded is what a fold decides, not what a renderer draws: the
 * identity of every envelope it accepted, in order, plus the scalar projection
 * fields. Envelope bodies are deliberately absent — they carry model output,
 * and a golden nobody can read is a golden nobody reviews.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  emptyLiveIngest,
  envelopeIdentity,
  envelopeScope,
  ingestLiveEnvelopeBatch,
  isControlEnvelope,
} from "../runtime/liveStream.ts";
import { projectLiveEval } from "../runtime/liveEvalReducer.ts";

const here = dirname(fileURLToPath(import.meta.url));
export const visualsRoot = join(here, "..");
export const repoRoot = join(visualsRoot, "..");
export const GOLDEN_PATH = join(visualsRoot, "fixtures", "live_fold_golden.json");
export const GOLDEN_SCHEMA = "synth.live-fold-golden.v1";

/** Fixture files, repo-relative so the Rust side reads the same bytes. */
const FIXTURE_FILES = [
  "visuals/fixtures/live_eval_events.json",
  "visuals/fixtures/live_container_rollout_events.json",
  "visuals/families/first_class_example_containers/live.craftax.v1/examples/events.json",
  "visuals/families/first_class_example_containers/live.craftax.v1/examples/cua-luna-low-10.json",
  "visuals/families/first_class_example_containers/live.harbor_eval.v1/examples/events.json",
  "visuals/families/optimizers/eval/optimizer.eval.live.v1/examples/events.json",
  "visuals/families/optimizers/_shared/optimizer.run.v1/examples/gepa_events.json",
  "visuals/families/optimizers/_shared/optimizer.run.v1/examples/sft_events.json",
  // A verbatim ten-lane producer capture: ten rollouts that each restart at
  // sequence 1 and each carry an `event_id: "1"`. This is the case a bare
  // `event_id` identity collapses into one lane while leaving the aggregate
  // count looking valid, so a golden without it proves much less than it looks.
  "docs/receipts/2026-08-16/v0.4-evidence-contract/ten-lane-producer-stream.json",
];

/**
 * The scenarios the fixtures do not contain. Every one of these is a rule the
 * two implementations agreed on separately and could drift on again: control
 * sequences, `control: true`, lane collapse, conflicting duplicates, opaque
 * string sequences, and an absent sequence that must not read as zero.
 */
const SCENARIOS = [
  {
    name: "multiplexed lanes keep rollout-local event ids apart",
    events: [
      { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-0", lane: "seed-0", payload: { step: 0 } },
      { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-1", lane: "seed-1", payload: { step: 0 } },
      { kind: "reward_signal", event_id: "2", sequence: 2, rollout_id: "seed-0", lane: "seed-0", payload: { value: 1 } },
      { kind: "reward_signal", event_id: "2", sequence: 2, rollout_id: "seed-1", lane: "seed-1", payload: { value: 0 } },
      { kind: "observation", event_id: "1", sequence: 1, rollout_id: "seed-0", lane: "seed-0", payload: { step: 0 } },
    ],
  },
  {
    name: "payload-carried rollout identity is promoted before dedupe",
    events: [
      { kind: "observation", event_id: "1", sequence: 1, payload: { rollout_id: "seed-2001", step: 0 } },
      { kind: "observation", event_id: "1", sequence: 1, payload: { rollout_id: "seed-2002", step: 0 } },
      { kind: "reward_signal", event_id: "5", sequence: 5, payload: { rollout_id: "seed-2001", reward: 2 } },
      { kind: "reward_signal", event_id: "5", sequence: 5, payload: { rollout_id: "seed-2002", reward: 1 } },
    ],
  },
  {
    name: "a sequenced heartbeat is not evidence and not a gap",
    events: [
      { kind: "observation", sequence: 1, stream_id: "s", payload: {} },
      { kind: "heartbeat", sequence: 2, stream_id: "s" },
      { kind: "observation", sequence: 3, stream_id: "s", payload: {} },
    ],
  },
  {
    name: "control true is honoured alongside the control kinds",
    events: [
      { kind: "observation", sequence: 1, stream_id: "s", payload: {} },
      { kind: "observation", sequence: 2, stream_id: "s", control: true, payload: {} },
      { kind: "stream.subscribed", sequence: 3, stream_id: "s" },
      { kind: "observation", sequence: 4, stream_id: "s", payload: {} },
    ],
  },
  {
    name: "one hole is one gap bracketed by its neighbours",
    events: [
      { kind: "observation", sequence: 1, stream_id: "s", payload: {} },
      { kind: "observation", sequence: 4, stream_id: "s", payload: {} },
      { kind: "observation", sequence: 5, stream_id: "s", payload: {} },
      { kind: "observation", sequence: 9, stream_id: "s", payload: {} },
    ],
  },
  {
    name: "an exact duplicate collapses and a conflicting one is reported",
    events: [
      { kind: "observation", stream_id: "s", sequence: 1, payload: { step: 0 } },
      { kind: "observation", stream_id: "s", sequence: 1, payload: { step: 0 } },
      { kind: "observation", stream_id: "s", sequence: 1, payload: { step: 7 } },
    ],
  },
  {
    name: "a producer-declared digest decides equality",
    events: [
      { kind: "observation", stream_id: "s", sequence: 1, digest: "d1", payload: { step: 0 } },
      { kind: "observation", stream_id: "s", sequence: 1, digest: "d1", payload: { step: 7 } },
    ],
  },
  {
    name: "an absent sequence is absent and never zero",
    events: [
      { kind: "observation", stream_id: "s", payload: {} },
      { kind: "observation", stream_id: "s", sequence: null, payload: {} },
      { kind: "observation", stream_id: "s", sequence: "", payload: {} },
      { kind: "observation", stream_id: "s", sequence: 2, payload: {} },
    ],
  },
  {
    name: "opaque string sequences are not a sequence space",
    events: [
      { kind: "snapshot", stream_id: "s", sequence: "lane#a:frame:0", payload: {} },
      { kind: "snapshot", stream_id: "s", sequence: "lane#a:frame:9", payload: {} },
    ],
  },
  {
    name: "sequence_number wins over sequence",
    events: [
      { kind: "observation", stream_id: "s", sequence_number: 1, sequence: 99, payload: {} },
      { kind: "observation", stream_id: "s", sequence_number: null, sequence: 2, payload: {} },
    ],
  },
  {
    name: "reward and usage come from the last envelope that carries them",
    events: [
      { kind: "verifier", stream_id: "s", sequence: 1, payload: { "reward.txt": 0.25 } },
      { kind: "verifier", stream_id: "s", sequence: 2, payload: { "reward.txt": 0.75 } },
      { kind: "frame", stream_id: "s", sequence: 3, payload: { usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15, cost_usd: 0.002 } } },
      { kind: "eval.run.terminal", stream_id: "s", sequence: 4, payload: { value: 1 } },
    ],
  },
  {
    name: "an envelope with no identity of its own falls back to kind and stamp",
    events: [
      { kind: "tick", occurred_at: "2026-08-28T00:00:00Z" },
      { kind: "tick", ts: 1700 },
      { kind: "tick" },
      { kind: "tick" },
    ],
  },
];

/** The three shapes a fixture file uses for its envelope array. */
export function envelopesFromFixture(parsed) {
  if (Array.isArray(parsed)) return parsed;
  if (Array.isArray(parsed?.events)) return parsed.events;
  if (Array.isArray(parsed?.page?.events)) return parsed.page.events;
  return [];
}

/**
 * The sequence-gap scan, kept here and nowhere else on this side.
 *
 * The shipped mirror no longer scans for gaps — `stream_fold.rs` owns that
 * claim, and the host emits it from the poll seam. This is the frozen
 * reference the golden was first captured from, retained so the golden stays
 * regenerable rather than becoming a file nobody can reproduce. It is test
 * scaffolding: nothing renders from it and nothing ships it.
 */
function referenceGaps(sequencesByScope) {
  const gaps = [];
  for (const scope of [...sequencesByScope.keys()].sort()) {
    const ordered = [...sequencesByScope.get(scope)].sort((a, b) => a - b);
    for (let index = 1; index < ordered.length; index++) {
      if (ordered[index] > ordered[index - 1] + 1) {
        gaps.push({ scope, after: ordered[index - 1], before: ordered[index] });
      }
    }
  }
  return gaps.sort((a, b) => a.scope.localeCompare(b.scope) || a.after - b.after || a.before - b.before);
}

/**
 * The fold's answer for one envelope array.
 *
 * Ordinals are the delivered-envelope ordinal — position in the input,
 * one-based — for every envelope, duplicates included. That is the one
 * numbering both implementations can compute without agreeing first about
 * what got accepted.
 */
export function foldGolden(events) {
  const state = ingestLiveEnvelopeBatch(emptyLiveIngest(), events);
  const projection = projectLiveEval(state.events);
  const accepted = [];
  const seen = new Set();
  const sequencesByScope = new Map();
  for (const [index, event] of events.entries()) {
    const identity = envelopeIdentity(event, index + 1);
    if (seen.has(identity)) continue;
    seen.add(identity);
    const scope = envelopeScope(event);
    accepted.push({ identity, scope, control: isControlEnvelope(event) });
    // Rule 2: a control envelope keeps its sequence, so it holds the
    // producer's numbering contiguous instead of forging a phantom hole.
    const raw = event.sequence_number ?? event.sequence;
    const sequence =
      typeof raw === "number"
        ? raw
        : raw != null && String(raw).length > 0
          ? Number(raw)
          : Number.NaN;
    if (!Number.isInteger(sequence)) continue;
    if (!sequencesByScope.has(scope)) sequencesByScope.set(scope, new Set());
    sequencesByScope.get(scope).add(sequence);
  }
  const gaps = referenceGaps(sequencesByScope);
  return {
    deliveredCount: events.length,
    acceptedCount: accepted.length,
    evidenceCount: state.events.length,
    accepted,
    ready: state.ready,
    gaps,
    conflicts: [...state.conflicts].sort(),
    lastSequenceByScope: Object.fromEntries(
      [...state.lastSequenceByScope.entries()].sort(([a], [b]) => a.localeCompare(b)),
    ),
    projection: {
      kinds: projection.kinds,
      hasLiveFrames: projection.has_live_frames,
      hasRewardTxt: projection.has_reward_txt,
      reward: projection.reward,
      usage: projection.usage,
      eventCount: projection.events.length,
    },
  };
}

export function buildGolden() {
  const cases = [];
  for (const file of FIXTURE_FILES) {
    const events = envelopesFromFixture(JSON.parse(readFileSync(join(repoRoot, file), "utf8")));
    cases.push({ name: file, source: { file }, ...foldGolden(events) });
  }
  for (const scenario of SCENARIOS) {
    cases.push({
      name: scenario.name,
      source: { inline: scenario.events },
      ...foldGolden(scenario.events),
    });
  }
  return { schema: GOLDEN_SCHEMA, cases };
}

if (process.argv[1] && relative(process.argv[1], fileURLToPath(import.meta.url)) === "") {
  const golden = buildGolden();
  const target = process.argv[2] ? resolve(process.cwd(), process.argv[2]) : GOLDEN_PATH;
  writeFileSync(target, `${JSON.stringify(golden, null, 2)}\n`);
  process.stdout.write(`[live-fold] wrote ${golden.cases.length} cases to ${target}\n`);
}
