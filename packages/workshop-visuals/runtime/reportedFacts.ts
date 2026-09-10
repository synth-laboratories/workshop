export const REPORTED_FACT_NAMES = [
  "calls",
  "steps",
  "tokens",
  "costUsd",
  "achievements",
  "frames"
] as const;

export type ReportedFactName = (typeof REPORTED_FACT_NAMES)[number];
export type NumericReportedFactName = Exclude<ReportedFactName, "achievements">;

export type ReportedFact<T> = {
  value: T | null;
  source: string;
  unavailableReason: string | null;
};

export type ReportedFacts = {
  calls: ReportedFact<number>;
  steps: ReportedFact<number>;
  tokens: ReportedFact<number>;
  costUsd: ReportedFact<number>;
  achievements: ReportedFact<string[]>;
  frames: ReportedFact<number>;
};

export type ReportedFactsRead =
  | { status: "absent" }
  | { status: "invalid"; reason: string }
  | { status: "present"; facts: ReportedFacts };

export type ReportedFactSummary<T> = {
  authoritative: boolean;
  value: T | null;
  present: number;
  total: number;
  sources: string[];
  unavailableReasons: string[];
  contractErrors: string[];
};

export type AchievementFactSummary = ReportedFactSummary<string[]> & {
  byRecord: Array<string[] | null>;
};

type Json = Record<string, unknown>;

const SNAKE_CASE = /^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/;

function object(value: unknown): Json | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Json : null;
}

function hasOwn(value: Json, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function reportedFactsCandidate(record: unknown): { found: boolean; value?: unknown } {
  const row = object(record);
  if (!row) return { found: false };
  if (hasOwn(row, "reportedFacts")) return { found: true, value: row.reportedFacts };
  if (hasOwn(row, "reported_facts")) return { found: true, value: row.reported_facts };
  const raw = object(row.raw);
  if (raw && hasOwn(raw, "reportedFacts")) return { found: true, value: raw.reportedFacts };
  if (raw && hasOwn(raw, "reported_facts")) return { found: true, value: raw.reported_facts };
  return { found: false };
}

function readFact(name: ReportedFactName, value: unknown): ReportedFact<unknown> | string {
  const fact = object(value);
  if (!fact) return `${name} is not an object`;
  const keys = Object.keys(fact).sort();
  if (keys.join(",") !== "source,unavailableReason,value") {
    return `${name} must contain exactly value, source, and unavailableReason`;
  }
  if (typeof fact.source !== "string" || !SNAKE_CASE.test(fact.source)) {
    return `${name}.source is not a snake_case enum value`;
  }
  if (fact.unavailableReason !== null && (
    typeof fact.unavailableReason !== "string" || !SNAKE_CASE.test(fact.unavailableReason)
  )) {
    return `${name}.unavailableReason is not null or a snake_case enum value`;
  }
  if (name === "achievements") {
    if (fact.value !== null && (
      !Array.isArray(fact.value) || !fact.value.every((entry) => typeof entry === "string")
    )) {
      return "achievements.value is not null or a string array";
    }
  } else if (fact.value !== null && (
    typeof fact.value !== "number" || !Number.isFinite(fact.value) || fact.value < 0
  )) {
    return `${name}.value is not null or a non-negative finite number`;
  }
  return fact as unknown as ReportedFact<unknown>;
}

/** Read the exact six-fact contract without silently accepting partial rows. */
export function readReportedFacts(record: unknown): ReportedFactsRead {
  const candidate = reportedFactsCandidate(record);
  if (!candidate.found) return { status: "absent" };
  const facts = object(candidate.value);
  if (!facts) return { status: "invalid", reason: "reportedFacts is not an object" };
  const keys = Object.keys(facts).sort();
  if (keys.join(",") !== [...REPORTED_FACT_NAMES].sort().join(",")) {
    return { status: "invalid", reason: "reportedFacts must contain exactly the six declared facts" };
  }
  const parsed: Partial<Record<ReportedFactName, ReportedFact<unknown>>> = {};
  for (const name of REPORTED_FACT_NAMES) {
    const fact = readFact(name, facts[name]);
    if (typeof fact === "string") return { status: "invalid", reason: fact };
    parsed[name] = fact;
  }
  return { status: "present", facts: parsed as ReportedFacts };
}

function unique(values: Array<string | null | undefined>): string[] {
  return [...new Set(values.filter((value): value is string => Boolean(value)))];
}

function readStates(records: readonly unknown[]): ReportedFactsRead[] {
  return records.map(readReportedFacts);
}

/**
 * Sum only a complete authoritative fact set. If one row is unavailable,
 * absent, or invalid, the total is unavailable rather than a partial sum.
 */
export function summarizeNumericReportedFact(
  records: readonly unknown[],
  name: NumericReportedFactName,
  legacyValues: readonly (number | null)[]
): ReportedFactSummary<number> {
  const reads = readStates(records);
  const authoritative = reads.some((read) => read.status !== "absent");
  if (!authoritative) {
    const present = legacyValues.filter((value): value is number => value !== null);
    return {
      authoritative: false,
      value: present.length ? present.reduce((sum, value) => sum + value, 0) : null,
      present: present.length,
      total: records.length,
      sources: [],
      unavailableReasons: [],
      contractErrors: []
    };
  }
  const facts = reads.map((read) => read.status === "present" ? read.facts[name] : null);
  const values = facts.map((fact) => fact?.value ?? null);
  const contractErrors = reads.flatMap((read, index) => read.status === "invalid"
    ? [`row_${index + 1}:${read.reason}`]
    : read.status === "absent" ? [`row_${index + 1}:reported_facts_absent`] : []);
  return {
    authoritative: true,
    value: contractErrors.length === 0 && values.every((value) => value !== null)
      ? (values as number[]).reduce((sum, value) => sum + value, 0)
      : null,
    present: values.filter((value) => value !== null).length,
    total: records.length,
    sources: unique(facts.map((fact) => fact?.source)),
    unavailableReasons: unique(facts.map((fact) => fact?.unavailableReason)),
    contractErrors
  };
}

/** Authoritative `[]` is available and distinct from any unavailable row. */
export function summarizeAchievementReportedFacts(
  records: readonly unknown[],
  legacyValues: readonly string[][]
): AchievementFactSummary {
  const reads = readStates(records);
  const authoritative = reads.some((read) => read.status !== "absent");
  if (!authoritative) {
    return {
      authoritative: false,
      value: [...new Set(legacyValues.flat())],
      byRecord: legacyValues.map((value) => [...value]),
      present: legacyValues.length,
      total: records.length,
      sources: [],
      unavailableReasons: [],
      contractErrors: []
    };
  }
  const facts = reads.map((read) => read.status === "present" ? read.facts.achievements : null);
  const byRecord = facts.map((fact) => fact?.value ?? null);
  const contractErrors = reads.flatMap((read, index) => read.status === "invalid"
    ? [`row_${index + 1}:${read.reason}`]
    : read.status === "absent" ? [`row_${index + 1}:reported_facts_absent`] : []);
  const available = contractErrors.length === 0 && byRecord.every((value) => value !== null);
  return {
    authoritative: true,
    value: available ? [...new Set((byRecord as string[][]).flat())] : null,
    byRecord,
    present: byRecord.filter((value) => value !== null).length,
    total: records.length,
    sources: unique(facts.map((fact) => fact?.source)),
    unavailableReasons: unique(facts.map((fact) => fact?.unavailableReason)),
    contractErrors
  };
}
