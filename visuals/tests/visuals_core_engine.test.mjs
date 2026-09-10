import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { VISUALS_PROTOCOL_VERSION } from "@synth/visuals-protocol";
import {
  BindingEngine,
  BindingResolverRegistry,
  ClockController,
  InMemoryPresentationStateStore,
  InMemoryVisualsApi,
  OrderedFeed,
  VisualExplorationSession,
  VisualExtensionRegistry,
  createVisualsMcpAdapter,
  inlineBindingResolver,
  presentationState,
} from "@synth/visuals-sdk";
import {
  createTrajectoryCorpus,
  generateTrajectoryFixture,
  swarmQuery,
  swarmTrajectoryDefinition,
} from "@synth/workshop-visuals";

test("versioned swarm fixture is deterministic and queryable at 1,000 trajectories", () => {
  const left = generateTrajectoryFixture();
  const right = generateTrajectoryFixture();
  assert.equal(left.length, 1_000);
  assert.deepEqual(left, right);
  const corpus = createTrajectoryCorpus(left);
  const failures = corpus.query(swarmQuery({ outcome: "failure" }), { offset: 0, limit: 20 });
  assert.ok(failures.total > 0);
  assert.equal(failures.rows.length, 20);
  assert.ok(failures.rows.every((row) => row.outcome === "failure"));
  assert.equal(failures.excluded, 1_000 - failures.total);
  assert.equal(failures.exactness, "exact");
  assert.equal(failures.completeness, "complete");
});

test("aggregate drill-down retains truthful denominators and membership", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const root = corpus.cohort("All trajectories", swarmQuery());
  const aggregate = corpus.aggregate(root, "behaviors");
  const recovery = aggregate.buckets.find((bucket) => bucket.key === "recovery");
  assert.ok(recovery);
  assert.equal(recovery.denominator, 1_000);
  const cohort = corpus.cohort("Recovery", swarmQuery({ behavior: "recovery" }), root);
  assert.equal(cohort.count, recovery.count);
  assert.equal(cohort.denominator, root.count);
  assert.ok(corpus.query(cohort.query, { offset: 0, limit: 1_000 }).rows.every((row) => row.behaviors.includes("recovery")));
});

test("child cohort membership is always intersected with its parent", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const failures = corpus.cohort("Failures", swarmQuery({ outcome: "failure" }));
  const terraWithinFailures = corpus.cohort("Terra failures", swarmQuery({ model: "terra" }), failures);
  const rows = corpus.query(terraWithinFailures.query, { offset: 0, limit: 1_000 }).rows;
  assert.ok(rows.length > 0);
  assert.ok(rows.every((row) => row.outcome === "failure" && row.model === "terra"));
  assert.equal(terraWithinFailures.denominator, failures.count);
});

test("aggregate and sampling operate on the full corpus beyond the query page limit", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture(2_500));
  const root = corpus.cohort("All trajectories", swarmQuery());
  assert.equal(root.count, 2_500);
  assert.equal(corpus.aggregate(root, "outcome").buckets.reduce((sum, bucket) => sum + bucket.count, 0), 2_500);
  assert.equal(corpus.sample(root, "random", 8, { seed: 7 }).receipt.sourceCohortId, root.id);
});

test("sampling is reproducible and discloses source cohort", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const root = corpus.cohort("All trajectories", swarmQuery());
  const first = corpus.sample(root, "diverse", 8, { seed: 42, scoreField: "reward" });
  const second = corpus.sample(root, "diverse", 8, { seed: 42, scoreField: "reward" });
  assert.deepEqual(first.rows.map((row) => row.id), second.rows.map((row) => row.id));
  assert.deepEqual(first.receipt, second.receipt);
  assert.equal(first.receipt.sourceCohortId, root.id);
  assert.equal(first.receipt.returned, 8);
});

test("semantic recording snapshots a coherent cohort and supports exact backtracking", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const root = corpus.cohort("All trajectories", swarmQuery());
  const rare = corpus.cohort("Abandonment", swarmQuery({ behavior: "abandonment" }), root);
  const session = new VisualExplorationSession({ visualId: swarmTrajectoryDefinition.id, corpus: corpus.ref, rootCohort: root });
  session.startRecording();
  session.dispatch({ id: "drill-1", kind: "drill_down", target: { kind: "cohort", id: rare.id }, expectedStateVersion: 0 }, { cohort: rare, target: { kind: "cohort", id: rare.id } });
  assert.equal(session.cohort.id, rare.id);
  const sample = corpus.sample(rare, "representative", 1, { scoreField: "reward" });
  const selected = sample.rows[0];
  assert.ok(selected);
  session.dispatch({ id: "select-1", kind: "select", target: { kind: "trajectory", id: selected.id }, expectedStateVersion: 1 });
  const snapshot = session.snapshot(["rendition:test"]);
  assert.equal(snapshot.cohort.id, rare.id);
  assert.equal(snapshot.selection.primary?.id, selected.id);
  assert.equal(snapshot.stateVersion, 2);
  session.dispatch({ id: "back-1", kind: "back", expectedStateVersion: 2 });
  assert.equal(session.cohort.id, rare.id);
  assert.equal(session.selection.primary?.id, rare.id);
  session.dispatch({ id: "back-2", kind: "back", target: { kind: "cohort", id: root.id }, expectedStateVersion: 3 });
  assert.equal(session.cohort.id, root.id);
  const recording = session.stopRecording();
  assert.ok(recording);
  assert.equal(recording.checkpoints.length, 1);
  assert.deepEqual(recording.events.map((event) => event.sequence), [1, 2, 3, 4, 5]);
  assert.throws(() => session.dispatch({ id: "stale", kind: "select", expectedStateVersion: 1 }), /Stale visual state/);
});

test("targeted backtracking restores the exact nested cohort", () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const root = corpus.cohort("All trajectories", swarmQuery());
  const failed = corpus.cohort("Failed", swarmQuery({ outcome: "failure" }), root);
  const failedRecovery = corpus.cohort("Failed recovery", swarmQuery({ outcome: "failure", behavior: "recovery" }), failed);
  const session = new VisualExplorationSession({ visualId: "nested", corpus: corpus.ref, rootCohort: root });
  session.dispatch({ id: "nested-1", kind: "drill_down", target: { kind: "cohort", id: failed.id }, expectedStateVersion: 0 }, { cohort: failed });
  session.dispatch({ id: "nested-2", kind: "drill_down", target: { kind: "cohort", id: failedRecovery.id }, expectedStateVersion: 1 }, { cohort: failedRecovery });
  session.dispatch({ id: "nested-back", kind: "back", target: { kind: "cohort", id: failed.id }, expectedStateVersion: 2 });
  assert.equal(session.cohort.id, failed.id);
  assert.equal(session.exploration.current.id, failed.id);
});

test("extension registry advertises capabilities without Workshop branching", () => {
  const registry = new VisualExtensionRegistry();
  registry.registerRenderer({ id: "template", version: "1.0.0", isolation: "in_process", formats: ["interactive"], capabilities: ["interaction", "semantic_scene"] });
  registry.registerDefinition(swarmTrajectoryDefinition);
  assert.equal(registry.definition("analysis.swarm_trajectories.v1")?.renderer, "template");
  assert.ok(registry.definition("analysis.swarm_trajectories.v1")?.capabilities.includes("mcp"));
  assert.equal(VISUALS_PROTOCOL_VERSION, "synth.visuals-core.v1");
  assert.throws(() => registry.registerDefinition(swarmTrajectoryDefinition), /already registered/);
  assert.throws(() => registry.registerDefinition({ ...swarmTrajectoryDefinition, id: "bad", renderer: "missing" }), /unknown renderer/);
});

test("registered binding resolution returns evidence rather than anonymous props", async () => {
  const registry = new BindingResolverRegistry();
  registry.register(inlineBindingResolver());
  const result = await new BindingEngine(registry).resolve([{ input: "rows", kind: "inline", schema: "test.rows.v1", data: { rows: [1, 2] }, path: "rows" }]);
  assert.deepEqual(result.sources[0].value, [1, 2]);
  assert.equal(result.sources[0].evidence.schema, "test.rows.v1");
  assert.equal(result.sources[0].evidence.completeness, "complete");
  assert.deepEqual(result.diagnostics, []);
});

test("ordered feeds preserve scoped identity, gaps, conflicts, and closure", () => {
  const feed = new OrderedFeed();
  feed.ingest([
    { scope: "a", sequence: 1, value: { value: "first" } },
    { scope: "a", sequence: 3, value: { value: "third" } },
    { scope: "b", sequence: 1, value: { value: "other lane" } },
    { scope: "a", sequence: 1, value: { value: "conflict" } },
  ]);
  feed.close("b");
  const snapshot = feed.snapshot();
  assert.deepEqual(snapshot.gaps, [{ scope: "a", after: 1, before: 3 }]);
  assert.equal(snapshot.conflicts.length, 1);
  assert.equal(snapshot.events.length, 3);
  assert.equal(snapshot.closedScopes.has("b"), true);
});

test("clock mappings keep logical domains explicit", () => {
  const clocks = new ClockController({
    domains: [
      { id: "event", kind: "sequence", scope: "lane", ordering: "total" },
      { id: "frame", kind: "discrete", scope: "lane", ordering: "total" },
    ],
    correspondences: [{ from: { domain: "event", scopeId: "r1", value: 4 }, to: { domain: "frame", scopeId: "r1", value: 2 } }],
  });
  clocks.seek({ domain: "event", scopeId: "r1", value: 4, mode: "fixed" });
  assert.deepEqual(clocks.corresponding(clocks.cursor("event"), "frame"), { domain: "frame", scopeId: "r1", value: 2, mode: "fixed" });
});

test("presentation state is versioned independently from evidence", async () => {
  const store = new InMemoryPresentationStateStore();
  const first = await store.save("visual-1", presentationState("swarm.presentation.v1", 1, { tab: "outcomes" }), 0);
  assert.equal(first.stateVersion, 1);
  await assert.rejects(() => store.save("visual-1", presentationState("swarm.presentation.v1", 1, { tab: "models" }), 0), /Stale presentation state/);
  assert.deepEqual((await store.load("visual-1", "swarm.presentation.v1")).value, { tab: "outcomes" });
});

test("portable JSON Schema and TypeScript protocol share one version identity", () => {
  const schema = JSON.parse(readFileSync(new URL("../../packages/visuals-protocol/schema/synth.visuals-core.v1.schema.json", import.meta.url), "utf8"));
  assert.equal(schema.$defs.querySpec.properties.schemaVersion.const, VISUALS_PROTOCOL_VERSION);
  assert.equal(schema.$defs.visualAction.properties.expectedStateVersion.minimum, 0);
  assert.equal(schema.$defs.cohortRef.required.includes("denominator"), true);
  assert.ok(schema.$defs.semanticScene.required.includes("stateVersion"));
  assert.ok(schema.$defs.visualSnapshot.required.includes("semanticSceneDigest"));
  assert.ok(schema.$defs.visualRecording.required.includes("checkpoints"));
  assert.ok(schema.$defs.visualDefinition.required.includes("renderer"));
});

test("MCP inspection, interaction, capture, and recording use the same visual session", async () => {
  const corpus = createTrajectoryCorpus(generateTrajectoryFixture());
  const root = corpus.cohort("All trajectories", swarmQuery());
  const session = new VisualExplorationSession({ visualId: "swarm-test", corpus: corpus.ref, rootCohort: root });
  const api = new InMemoryVisualsApi();
  api.register("swarm-test", session, () => ({
    visualId: "swarm-test",
    revision: 1,
    stateVersion: session.stateVersion,
    clocks: {},
    selection: session.selection,
    landmarks: [{ ref: { kind: "cohort", id: session.cohort.id }, role: "region", label: session.cohort.name, actions: ["select", "snapshot"] }],
    truth: { cohort_count: { state: "observed", value: session.cohort.count } },
    diagnostics: [],
  }));
  const mcp = createVisualsMcpAdapter(api);
  assert.deepEqual(mcp.tools().map((tool) => tool.name), ["visual_inspect", "visual_interact", "visual_capture", "visual_record"]);
  await mcp.call("visual_record", { visual_id: "swarm-test", operation: "start" });
  const receipt = await mcp.call("visual_interact", { visual_id: "swarm-test", action: { id: "select-via-mcp", kind: "select", target: { kind: "trajectory", id: "trajectory-0001" }, expectedStateVersion: 0 } });
  assert.equal(receipt.stateVersion, 1);
  const inspection = await mcp.call("visual_inspect", { visual_id: "swarm-test" });
  assert.equal(inspection.scene.selection.primary.id, "trajectory-0001");
  assert.equal(inspection.scene.stateVersion, receipt.stateVersion);
  const capture = await mcp.call("visual_capture", { visual_id: "swarm-test", rendition_refs: ["png:sha256:test"] });
  assert.equal(capture.snapshot.selection.primary.id, "trajectory-0001");
  assert.deepEqual(capture.snapshot.renditionRefs, ["png:sha256:test"]);
  const stopped = await mcp.call("visual_record", { visual_id: "swarm-test", operation: "stop" });
  assert.deepEqual(stopped.recording.events.map((event) => event.kind), ["action", "snapshot_captured"]);
});
