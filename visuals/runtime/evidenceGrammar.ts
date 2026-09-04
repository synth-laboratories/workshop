export type ComparisonContract = {
  digest: string;
  benchmark: string;
  metric: string;
  evaluator?: string;
  dataset?: string;
  split?: string;
};

export type ComparisonContractVerdict =
  | { comparable: true; reason: null }
  | { comparable: false; reason: "missing_contract" | "row_contract_mismatch" };

/**
 * Ranking is an evidence claim, so comparability is derived from shared
 * contract identity rather than trusted as a caller-provided label.
 */
export function comparisonContractVerdict(
  kind: "like_for_like" | "run_catalog" | undefined,
  contract: ComparisonContract | undefined,
  rows: Array<{ comparison_contract_digest?: string }>
): ComparisonContractVerdict {
  if (kind === "run_catalog") return { comparable: false, reason: "missing_contract" };
  if (
    !contract
    || typeof contract.digest !== "string"
    || !contract.digest.trim()
    || typeof contract.benchmark !== "string"
    || !contract.benchmark.trim()
    || typeof contract.metric !== "string"
    || !contract.metric.trim()
  ) return { comparable: false, reason: "missing_contract" };
  if (!rows.every((row) => row.comparison_contract_digest === contract.digest)) {
    return { comparable: false, reason: "row_contract_mismatch" };
  }
  return { comparable: true, reason: null };
}
