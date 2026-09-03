/**
 * Scalable child-rollout browser: grouped summaries by default, filters,
 * search, pagination, and a per-rollout inspector. Handles hundreds or
 * thousands of rollouts without rendering them all. Algorithm-agnostic —
 * GEPA feeds candidate/stage groups; SFT can feed checkpoint campaigns.
 */

import { useMemo, useState, type ReactNode } from "react";
import { formatMissingNumber, formatMissingUsd } from "../../../../../../runtime/liveStream.ts";

export type RolloutRow = {
  id: string;
  groupKey: string;
  sequence: number;
  exampleId?: string;
  stage?: string;
  reward?: number | null;
  completed?: boolean;
  costUsd?: number;
  usage?: Record<string, unknown>;
  streamId?: string;
  rewardUrl?: string;
  occurredAt?: string;
};

export type RolloutGroup = {
  key: string;
  title: string;
  subtitle?: string;
  extras?: ReactNode;
};

type Filter = "all" | "failures" | "passes" | "active";

const PAGE_SIZE = 20;

function matchesFilter(row: RolloutRow, filter: Filter): boolean {
  if (filter === "all") return true;
  if (filter === "active") return row.completed !== true && row.reward == null;
  if (filter === "failures") return row.reward != null && row.reward <= 0;
  return row.reward != null && row.reward > 0;
}

function RolloutInspector({ row, onClose }: { row: RolloutRow; onClose: () => void }) {
  return (
    <div
      role="region"
      aria-label={`Rollout ${row.id}`}
      data-testid={`inspect-child-${row.id}`}
      style={{ marginTop: 8, padding: "10px 12px", border: "1px solid var(--sv-border-strong)", borderRadius: 9, background: "var(--sv-surface-muted)", fontSize: 12 }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8 }}>
        <strong className="sv-mono" style={{ overflowWrap: "anywhere" }}>{row.id}</strong>
        <button type="button" className="sv-btn" onClick={onClose}>Close</button>
      </div>
      <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 12px", margin: "8px 0 0" }}>
        <dt style={{ color: "var(--sv-text-faint)" }}>Example</dt>
        <dd className="sv-mono" style={{ margin: 0 }}>{row.exampleId ?? "—"}</dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Stage</dt>
        <dd style={{ margin: 0 }}>{row.stage ? row.stage.replaceAll("_", " ") : "—"}</dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Reward</dt>
        <dd style={{ margin: 0 }} data-testid={`gepa-eval-reward-${row.id}`}>
          {row.reward == null ? (row.completed ? "not loaded" : "pending") : formatMissingNumber(row.reward)}
        </dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Cost</dt>
        <dd style={{ margin: 0 }}>{row.costUsd != null && row.costUsd > 0 ? formatMissingUsd(row.costUsd) : "unavailable"}</dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Tokens</dt>
        <dd style={{ margin: 0 }}>
          {typeof row.usage?.total_tokens === "number" ? row.usage.total_tokens.toLocaleString() : "—"}
        </dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Stream</dt>
        <dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{row.streamId ?? "—"}</dd>
        <dt style={{ color: "var(--sv-text-faint)" }}>Reward URL</dt>
        <dd className="sv-mono" style={{ margin: 0, overflowWrap: "anywhere" }}>{row.rewardUrl ?? "—"}</dd>
      </dl>
    </div>
  );
}

export function RolloutBrowser({
  groups,
  rows,
  totalRows,
  loading = false,
  stale = false,
  emptyText,
  itemLabel = "rollouts",
  testId,
  selectedId,
  onInspect
}: {
  groups: RolloutGroup[];
  rows: RolloutRow[];
  /** Total matching durable rows across server pages. */
  totalRows?: number;
  loading?: boolean;
  stale?: boolean;
  emptyText: string;
  itemLabel?: string;
  testId?: string;
  selectedId?: string | null;
  onInspect?: (row: RolloutRow | null) => void;
}) {
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [openGroup, setOpenGroup] = useState<string | null>(null);
  const [pageByGroup, setPageByGroup] = useState<Record<string, number>>({});
  const [localInspectedId, setLocalInspectedId] = useState<string | null>(null);
  const inspectedId = selectedId === undefined ? localInspectedId : selectedId;
  const inspect = (row: RolloutRow | null) => {
    if (selectedId === undefined) setLocalInspectedId(row?.id ?? null);
    onInspect?.(row);
  };

  const byGroup = useMemo(() => {
    const map = new Map<string, RolloutRow[]>();
    for (const row of rows) {
      const list = map.get(row.groupKey) ?? [];
      list.push(row);
      map.set(row.groupKey, list);
    }
    return map;
  }, [rows]);

  const query = search.trim().toLowerCase();
  const filteredByGroup = useMemo(() => {
    const map = new Map<string, RolloutRow[]>();
    for (const [key, list] of byGroup) {
      map.set(key, list.filter((row) =>
        matchesFilter(row, filter) &&
        (!query || row.id.toLowerCase().includes(query) || (row.exampleId ?? "").toLowerCase().includes(query))
      ));
    }
    return map;
  }, [byGroup, filter, query]);

  const inspected = inspectedId ? rows.find((row) => row.id === inspectedId) ?? null : null;
  const totalShown = [...filteredByGroup.values()].reduce((sum, list) => sum + list.length, 0);

  return (
    <section className="sv-section" aria-label="Evaluation rollouts" data-testid={testId}>
      <div className="sv-section-head">
        <h3>Evaluations</h3>
        <span className="sv-mono">
          {loading && rows.length === 0
            ? "loading"
            : rows.length === 0
              ? "none yet"
              : `${totalShown} shown of ${totalRows ?? rows.length} ${itemLabel}`}
          {stale ? " · refreshing" : ""}
        </span>
      </div>
      {/*
        Controls for a set that does not exist. With no rollouts, four filter
        chips and a search box offer to narrow nothing, and `0 shown of 0
        rollouts` restates the empty line below it in the language of a filter
        result -- as if a filter were hiding something. The empty state says
        what will appear and when; that is the whole message until a row exists.
      */}
      {rows.length > 0 ? (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 8 }}>
          {(["all", "failures", "passes", "active"] as const).map((option) => (
            <button
              key={option}
              type="button"
              className="sv-btn"
              aria-pressed={filter === option}
              data-testid={`eval-filter-${option}`}
              onClick={() => setFilter(option)}
            >
              {option === "all" ? "All" : option === "failures" ? "Failures" : option === "passes" ? "Passes" : "In flight"}
            </button>
          ))}
          <input
            type="search"
            value={search}
            placeholder="Find example or rollout ID"
            aria-label="Search rollouts"
            data-testid="eval-search"
            onChange={(event) => setSearch(event.target.value)}
            style={{ flex: "1 1 180px", minWidth: 140, padding: "5px 9px", border: "1px solid var(--sv-border-strong)", borderRadius: 8, font: "inherit", fontSize: 12 }}
          />
        </div>
      ) : null}
      {rows.length === 0 ? (
        <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>{emptyText}</p>
      ) : (
        <div style={{ display: "grid", gap: 8 }}>
          {groups.map((group) => {
            const all = byGroup.get(group.key) ?? [];
            const matching = filteredByGroup.get(group.key) ?? [];
            const completed = all.filter((row) => row.completed === true || row.reward != null);
            const scored = completed.filter((row) => row.reward != null);
            const solved = scored.filter((row) => (row.reward ?? 0) > 0).length;
            const failures = scored.length - solved;
            const mean = scored.length > 0
              ? scored.reduce((sum, row) => sum + (row.reward ?? 0), 0) / scored.length
              : undefined;
            const isOpen = openGroup === group.key;
            const page = pageByGroup[group.key] ?? 0;
            const pageCount = Math.max(1, Math.ceil(matching.length / PAGE_SIZE));
            const visible = matching.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
            return (
              <article key={group.key} style={{ border: "1px solid var(--sv-border)", borderRadius: 9, overflow: "hidden" }}>
                <button
                  type="button"
                  data-testid={`eval-group-${group.key}`}
                  aria-expanded={isOpen}
                  onClick={() => setOpenGroup(isOpen ? null : group.key)}
                  style={{ appearance: "none", display: "flex", flexWrap: "wrap", alignItems: "baseline", gap: 10, width: "100%", padding: "9px 12px", border: 0, background: "var(--sv-surface-muted)", font: "inherit", textAlign: "left", cursor: "pointer" }}
                >
                  <strong style={{ fontSize: 12.5 }}>{group.title}</strong>
                  {group.subtitle ? <span style={{ color: "var(--sv-text-muted)", fontSize: 11.5 }}>{group.subtitle}</span> : null}
                  <span className="sv-mono" style={{ marginLeft: "auto", color: "var(--sv-text-muted)" }}>
                    {completed.length}/{all.length} done
                    {mean != null ? ` · mean ${mean.toFixed(2)}` : ""}
                    {scored.length > 0 ? ` · ${solved} solved · ${failures} failed` : ""}
                  </span>
                </button>
                {isOpen ? (
                  <div style={{ padding: "8px 12px 10px" }}>
                    {group.extras}
                    {matching.length === 0 ? (
                      <p style={{ color: "var(--sv-text-faint)", fontSize: 12, margin: 0 }}>No rollouts match the current filter.</p>
                    ) : (
                      <>
                        <table className="sv-table">
                          <thead>
                            <tr>
                              <th scope="col">Example</th>
                              <th scope="col">Reward</th>
                              <th scope="col">Rollout</th>
                              <th scope="col" aria-label="Inspect" />
                            </tr>
                          </thead>
                          <tbody>
                            {visible.map((row) => (
                              <tr key={row.id} data-testid={`gepa-eval-${row.id}`}>
                                <td className="sv-mono">{row.exampleId ?? "—"}</td>
                                <td className="sv-mono" style={{ color: row.reward == null ? "var(--sv-text-faint)" : row.reward > 0 ? "#1e7a43" : "#b23830" }}>
                                  {row.reward == null ? "pending" : formatMissingNumber(row.reward)}
                                </td>
                                <td className="sv-mono" style={{ overflowWrap: "anywhere" }}>{row.id}</td>
                                <td>
                                  <button type="button" className="sv-btn" onClick={() => inspect(row)} aria-label={`Inspect rollout ${row.id}`} data-annotation-kind="evaluation" data-annotation-id={row.id}>
                                    Inspect
                                  </button>
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                        {pageCount > 1 ? (
                          <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
                            <button type="button" className="sv-btn" disabled={page === 0} onClick={() => setPageByGroup({ ...pageByGroup, [group.key]: page - 1 })}>
                              ← Prev
                            </button>
                            <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>page {page + 1} / {pageCount}</span>
                            <button type="button" className="sv-btn" disabled={page >= pageCount - 1} onClick={() => setPageByGroup({ ...pageByGroup, [group.key]: page + 1 })}>
                              Next →
                            </button>
                          </div>
                        ) : null}
                      </>
                    )}
                  </div>
                ) : null}
              </article>
            );
          })}
        </div>
      )}
      {inspected ? <RolloutInspector row={inspected} onClose={() => inspect(null)} /> : null}
    </section>
  );
}
