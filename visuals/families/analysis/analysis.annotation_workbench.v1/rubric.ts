export type RubricCriterionInput = {
  id?: string;
  label?: string;
  criterion_id?: string;
  judgment?: string;
  rationale?: string;
  verdict?: string;
  status?: string;
  passed?: boolean | null;
  score?: number | null;
  citations?: string[];
};

export type RubricCriterionView = {
  id: string;
  label: string;
  judgment: string;
  rationale?: string;
  score?: number | null;
};

function words(value: string): string {
  const spaced = value.trim().replace(/[._-]+/g, " ").replace(/\s+/g, " ");
  return spaced ? spaced[0].toUpperCase() + spaced.slice(1) : "Criterion";
}

export function rubricCriterionView(criterion: RubricCriterionInput, index: number): RubricCriterionView {
  const identity = criterion.criterion_id || criterion.id || `criterion_${index + 1}`;
  const judgment =
    criterion.judgment ||
    criterion.verdict ||
    criterion.status ||
    (criterion.passed === true ? "pass" : criterion.passed === false ? "fail" : "not applicable");
  return {
    id: identity,
    label: criterion.label || words(identity),
    judgment,
    rationale: criterion.rationale,
    score: criterion.score
  };
}

export function rubricScore(value?: number | null): string {
  if (value == null || !Number.isFinite(value)) return "—";
  return value >= 0 && value <= 1 ? `${(value * 100).toFixed(1)}%` : String(value);
}
