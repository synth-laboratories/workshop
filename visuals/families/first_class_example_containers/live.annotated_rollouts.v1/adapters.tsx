import type { ReactNode } from "react";
import { formatMissingNumber } from "../../../runtime/liveStream.ts";
import type { Lane } from "./project.ts";

export type TaskFamily = "craftax" | "runescape" | "banking77" | "healthbench" | "deepswe" | "generic";

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}
function number(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() && Number.isFinite(Number(value))) return Number(value);
  return undefined;
}

export function taskFamily(lane: Lane): TaskFamily {
  const haystack = [lane.family, lane.protocol?.protocolId, lane.name, lane.task.task, lane.task.family]
    .filter(Boolean).join(" ").toLowerCase();
  if (haystack.includes("runebench") || haystack.includes("runescape") || haystack.includes("woodcutting")) return "runescape";
  if (haystack.includes("banking77")) return "banking77";
  if (haystack.includes("healthbench")) return "healthbench";
  if (haystack.includes("deepswe")) return "deepswe";
  if (haystack.includes("craftax") || lane.frameUrl || lane.health != null) return "craftax";
  return "generic";
}

export function progressLabel(lane: Lane): string {
  const family = taskFamily(lane);
  if (family === "craftax") return lane.total ? `${lane.done}/${lane.total} steps` : `${lane.done} steps`;
  if (family === "runescape") return lane.total ? `${lane.done}/${lane.total} samples` : `${lane.done} samples`;
  if (family === "banking77") return lane.status === "finished" ? "classified" : lane.status;
  if (family === "healthbench") return lane.status === "finished" ? "graded" : lane.status;
  if (family === "deepswe") return lane.total ? `${lane.done}/${lane.total} actions` : `${lane.done} actions`;
  return lane.total ? `${lane.done}/${lane.total}` : lane.status;
}

/**
 * A score that can be negative carries its sign either way.
 *
 * Two HealthBench rollouts listed as `score −0.14` and `score 0.06`: the
 * negative one is marked and the positive one is bare, so the pair reads as
 * one signed number beside one unsigned magnitude rather than as two scores
 * either side of zero. The trace workstation already writes `+1.00` for the
 * same quantity; matching it keeps direction legible in a screenshot, which
 * is how these rows are usually read.
 *
 * Magnitudes -- a peak rate, a count -- are not signed and do not come here.
 */
function signedScore(value: number | null | undefined): string {
  const formatted = formatMissingNumber(value);
  if (formatted === "—") return formatted;
  return typeof value === "number" && value >= 0 ? `+${formatted}` : formatted;
}

export function outcomeLabel(lane: Lane): string {
  const reward = lane.metrics.cumulative_reward ?? lane.reward ?? number(lane.task.reward_value);
  const family = taskFamily(lane);
  if (family === "banking77") return reward == null ? "unscored" : reward >= 1 ? "correct" : "incorrect";
  if (family === "healthbench") return reward == null ? "score —" : `score ${signedScore(reward)}`;
  if (family === "runescape") return reward == null ? "score —" : `peak ${formatMissingNumber(reward)} XP/min`;
  return `reward ${signedScore(reward)}`;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return <div style={{ minWidth: 120 }}><div className="sv-mono" style={{ color: "var(--sv-text-faint)", fontSize: 9, textTransform: "uppercase" }}>{label}</div><div style={{ marginTop: 2, fontSize: 11, overflowWrap: "anywhere" }}>{children}</div></div>;
}

function Vital({ label, value }: { label: string; value?: number }) {
  const pct = value == null ? 0 : value <= 9 ? value / 9 * 100 : Math.min(100, value);
  return <div title={`${label}: ${value ?? "unknown"}`} style={{ display: "grid", gap: 3 }}><span className="sv-mono" style={{ fontSize: 9, color: "var(--sv-text-faint)" }}>{label}</span><span style={{ width: 42, height: 4, borderRadius: 9, background: "var(--sv-border)", overflow: "hidden" }}><span style={{ display: "block", width: `${pct}%`, height: "100%", background: pct < 34 ? "#d84b3f" : pct < 67 ? "#e5a226" : "#39a46b" }} /></span></div>;
}

function CraftaxDetails({ lane, streamBase }: { lane: Lane; streamBase: URL | null }) {
  return <>
    <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}><Vital label="HLTH" value={lane.health} /><Vital label="FOOD" value={lane.food} /><Vital label="DRNK" value={lane.drink} /><Vital label="NRGY" value={lane.energy} /><Fact label="Achievements">{lane.achievements.length}</Fact><Fact label="Policy calls">{lane.calls}</Fact></div>
    {lane.frameUrl && streamBase ? <img src={new URL(lane.frameUrl, streamBase).toString()} alt={`World for ${lane.name} at step ${lane.done}`} style={{ display: "block", width: "100%", maxHeight: 420, borderRadius: 8, objectFit: "contain", imageRendering: "pixelated", background: "#111" }} /> : null}
  </>;
}

function RuneScapeDetails({ lane, streamBase }: { lane: Lane; streamBase: URL | null }) {
  return <>
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(120px, 1fr))", gap: 12 }}>
      <Fact label="Skill">{text(lane.task.skill) ?? "Woodcutting"}</Fact>
      <Fact label="Level">{number(lane.task.level) ?? "waiting"}</Fact>
      <Fact label="XP">{formatMissingNumber(number(lane.task.xp))}</Fact>
      <Fact label="Latest gain">{formatMissingNumber(number(lane.task.xp_delta))}</Fact>
      <Fact label="Current rate">{formatMissingNumber(number(lane.task.xp_per_min))} XP/min</Fact>
      <Fact label="Peak rate">{formatMissingNumber(number(lane.task.peak_xp_per_min) ?? lane.reward)} XP/min</Fact>
    </div>
    {lane.frameUrl && streamBase ? <img src={new URL(lane.frameUrl, streamBase).toString()} alt={`RuneScape client for ${lane.name}`} style={{ display: "block", width: "100%", maxHeight: 420, borderRadius: 8, objectFit: "contain", background: "#111" }} /> : null}
  </>;
}

function BankingDetails({ lane }: { lane: Lane }) {
  const query = text(lane.task.query) ?? text(lane.task.customer_query) ?? text(lane.task.prompt);
  const prediction = text(lane.task.predicted_label) ?? text(lane.task.canonical_label) ?? text(lane.task.response);
  return <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 12 }}>
    <Fact label="Customer query">{query ?? "waiting for observation"}</Fact>
    <Fact label="Predicted intent">{prediction ?? "waiting for action"}</Fact>
    <Fact label="Result">{outcomeLabel(lane)}</Fact>
    <Fact label="Split / seed">{[text(lane.task.split), number(lane.task.seed)].filter((v) => v != null).join(" · ") || "—"}</Fact>
  </div>;
}

function HealthBenchDetails({ lane }: { lane: Lane }) {
  const grades = Array.isArray(lane.task.rubric_grades) ? lane.task.rubric_grades as Record<string, unknown>[] : [];
  const met = grades.filter((row) => row.criteria_met === true).length;
  const prompt = text(lane.task.prompt) ?? text(lane.task.query);
  const response = text(lane.task.response) ?? text(lane.task.answer);
  return <div style={{ display: "grid", gap: 10 }}>
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 12 }}><Fact label="Rubric items">{grades.length || number(lane.task.items) || "waiting"}</Fact><Fact label="Criteria met">{grades.length ? `${met}/${grades.length}` : number(lane.task.met) ?? "—"}</Fact><Fact label="Outcome">{outcomeLabel(lane)}</Fact><Fact label="Finish reason">{text(lane.task.finish_reason) ?? "—"}</Fact></div>
    {prompt || response ? <details style={{ padding: "8px 10px", border: "1px solid var(--sv-border)", borderRadius: 7, background: "var(--sv-canvas)" }}><summary style={{ cursor: "pointer", fontSize: 10, fontWeight: 700 }}>Prompt and model response</summary><div style={{ display: "grid", gap: 10, marginTop: 10 }}>{prompt ? <Fact label="Clinical prompt">{prompt}</Fact> : null}{response ? <Fact label="Model response">{response}</Fact> : null}</div></details> : null}
  </div>;
}

function GenericDetails({ lane }: { lane: Lane }) {
  const facts = Object.entries(lane.task).filter(([, value]) => ["string", "number", "boolean"].includes(typeof value)).slice(0, 12);
  return <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 12 }}>{facts.map(([key, value]) => <Fact key={key} label={key.replaceAll("_", " ")}>{String(value)}</Fact>)}{!facts.length ? <span style={{ color: "var(--sv-text-faint)" }}>Task facts will appear as rollout evidence streams in.</span> : null}</div>;
}

export function TaskDetails({ lane, streamBase }: { lane: Lane; streamBase: URL | null }) {
  switch (taskFamily(lane)) {
    case "craftax": return <CraftaxDetails lane={lane} streamBase={streamBase} />;
    case "runescape": return <RuneScapeDetails lane={lane} streamBase={streamBase} />;
    case "banking77": return <BankingDetails lane={lane} />;
    case "healthbench": return <HealthBenchDetails lane={lane} />;
    default: return <GenericDetails lane={lane} />;
  }
}

export function familyLabel(lane: Lane): string {
  const family = taskFamily(lane);
  return family === "generic" ? "rollout" : family === "healthbench" ? "HealthBench" : family === "banking77" ? "Banking77" : family === "deepswe" ? "DeepSWE" : family === "runescape" ? "RuneScape" : "Craftax";
}
