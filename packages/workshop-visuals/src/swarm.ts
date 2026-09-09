import { VISUALS_PROTOCOL_VERSION, type QueryExpression, type QuerySpec, type SamplingStrategy, type VisualDefinition } from "@synth/visuals-protocol";
import { InMemoryCorpus, allQuery, type CorpusRow } from "@synth/visuals-sdk";

export const WORKSHOP_TRAJECTORY_SCHEMA = "synth.workshop.agent-trajectory.v1" as const;

export type TrajectoryOutcome = "success" | "failure" | "timeout";
export type TrajectoryBehavior = "direct" | "recovery" | "tool_loop" | "exploratory" | "abandonment";
export type RewardBand = "negative" | "low" | "medium" | "high";

export type AgentTrajectory = CorpusRow & {
  id: string;
  model: string;
  seed: number;
  outcome: TrajectoryOutcome;
  reward: number;
  rewardBand: RewardBand;
  durationMs: number;
  toolCalls: number;
  steps: number;
  failed: boolean;
  behaviors: TrajectoryBehavior[];
  events: Array<{ id: string; step: number; kind: string; summary: string }>;
};

export type SwarmFilters = {
  outcome?: TrajectoryOutcome;
  model?: string;
  behavior?: TrajectoryBehavior;
  rewardBand?: RewardBand;
};

export const swarmTrajectoryDefinition: VisualDefinition = {
  id: "analysis.swarm_trajectories.v1",
  version: "1.0.0",
  renderer: "template",
  inputSchemas: [WORKSHOP_TRAJECTORY_SCHEMA],
  capabilities: ["query", "aggregate", "drill_down", "logical_time", "interaction", "snapshot", "recording", "semantic_scene", "mcp"],
  inputs: [{ name: "trajectories", schemas: [WORKSHOP_TRAJECTORY_SCHEMA], bindingKinds: ["inline", "fixture", "local_cas", "query_snapshot"], required: false }],
  time: {
    primaryDomain: "trajectory_step",
    domains: [{ id: "trajectory_step", kind: "sequence", scope: "entity", unit: "step", ordering: "total" }],
  },
  interaction: {
    actions: [
      { kind: "query", tier: "ephemeral", idempotent: true },
      { kind: "drill_down", tier: "presentation", idempotent: false },
      { kind: "back", tier: "presentation", idempotent: false },
      { kind: "select", tier: "presentation", idempotent: true },
      { kind: "seek", tier: "presentation", idempotent: true },
      { kind: "snapshot", tier: "overlay", idempotent: false },
    ],
  },
  presentationSchema: "synth.workshop.swarm-presentation.v1",
};

function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state += 0x6d2b79f5;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

/** Deterministic, versioned release fixture; no provider calls or credentials. */
export function generateTrajectoryFixture(count = 1_000, fixtureSeed = 10): AgentTrajectory[] {
  if (!Number.isInteger(count) || count < 1 || count > 10_000) throw new Error("Fixture count must be within 1..10000");
  const random = mulberry32(fixtureSeed);
  const models = ["astra", "sol", "terra", "luna"];
  return Array.from({ length: count }, (_, index) => {
    const seed = index + 1;
    const roll = random();
    const outcome: TrajectoryOutcome = roll < 0.68 ? "success" : roll < 0.92 ? "failure" : "timeout";
    const toolCalls = Math.floor(random() * 22);
    const steps = 8 + Math.floor(random() * 92);
    const reward = Number(Math.max(-1, Math.min(1, (outcome === "success" ? 0.45 : -0.3) + random() * 0.7 - 0.2)).toFixed(3));
    const behaviors: TrajectoryBehavior[] = [];
    if (toolCalls <= 4 && outcome === "success") behaviors.push("direct");
    if (outcome === "success" && steps > 55) behaviors.push("recovery");
    if (toolCalls > 15) behaviors.push("tool_loop");
    if (steps > 70) behaviors.push("exploratory");
    if (outcome !== "success" && steps < 25) behaviors.push("abandonment");
    if (!behaviors.length) behaviors.push("exploratory");
    return {
      id: `trajectory-${String(seed).padStart(4, "0")}`,
      model: models[index % models.length] ?? "unknown",
      seed,
      outcome,
      reward,
      rewardBand: reward < 0 ? "negative" : reward < 0.35 ? "low" : reward < 0.7 ? "medium" : "high",
      durationMs: 1_000 + steps * (80 + Math.floor(random() * 80)),
      toolCalls,
      steps,
      failed: outcome !== "success",
      behaviors,
      events: Array.from({ length: Math.min(steps, 12) }, (__, eventIndex) => ({
        id: `trajectory-${String(seed).padStart(4, "0")}:event-${eventIndex + 1}`,
        step: Math.round((eventIndex / Math.max(1, Math.min(steps, 12) - 1)) * (steps - 1)) + 1,
        kind: eventIndex % 3 === 1 ? "tool_call" : eventIndex === 11 ? "outcome" : "model",
        summary: eventIndex === 11 ? `Finished with ${outcome}` : eventIndex % 3 === 1 ? `Tool call ${eventIndex + 1}` : `Decision ${eventIndex + 1}`,
      })),
    };
  });
}

export function createTrajectoryCorpus(rows: AgentTrajectory[], id = "workshop:trajectory-corpus:v1"): InMemoryCorpus<AgentTrajectory> {
  return new InMemoryCorpus({ id, schema: WORKSHOP_TRAJECTORY_SCHEMA, rows, completeness: "complete" });
}

export function swarmQuery(filters: SwarmFilters = {}): QuerySpec {
  const expressions: QueryExpression[] = [];
  if (filters.outcome) expressions.push({ op: "eq", field: "outcome", value: filters.outcome });
  if (filters.model) expressions.push({ op: "eq", field: "model", value: filters.model });
  if (filters.behavior) expressions.push({ op: "contains", field: "behaviors", value: filters.behavior });
  if (filters.rewardBand) expressions.push({ op: "eq", field: "rewardBand", value: filters.rewardBand });
  return expressions.length
    ? { schemaVersion: VISUALS_PROTOCOL_VERSION, where: { op: "and", expressions }, orderBy: [{ field: "id", direction: "asc" }] }
    : allQuery();
}

export function sampleLabel(strategy: SamplingStrategy): string {
  return ({ random: "Random", representative: "Representative", diverse: "Diverse", boundary: "Boundary", failure: "Failure", outlier: "Outlier" })[strategy];
}
