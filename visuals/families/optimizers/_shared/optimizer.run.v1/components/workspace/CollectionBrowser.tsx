/**
 * `CollectionBrowser` — the shared, algorithm-neutral browser over a run's
 * durable collections (candidates, rollouts, evaluations, metric points,
 * proposer calls, artifacts, evidence refs).
 *
 * It owns no transport, cache, reducer, or retry policy: the host hands it a
 * client bound to one run, and every read it makes is one explicit page or
 * one item. Detail is fetched when a row is selected, never up front, and a
 * page that arrives after the reader moved on is dropped.
 */

import { useEffect, useMemo, useState, type ReactNode } from "react";

export type RunCollectionName =
  | "candidates"
  | "rollouts"
  | "evaluations"
  | "metric_points"
  | "proposer_calls"
  | "artifacts"
  | "evidence_refs";

export type RunCollectionFilterLike = {
  parentId?: string | null;
  label?: string | null;
  status?: string | null;
  kind?: string | null;
  changedAfterRevision?: number | null;
};

export type RunCollectionQueryLike = {
  cursor?: string | null;
  limit?: number | null;
  descending?: boolean;
  filter?: RunCollectionFilterLike | null;
};

export type RunCollectionRowLike = {
  itemId: string;
  ordinal: number;
  sequence: number;
  revision: number;
  kind: string;
  label?: string | null;
  parentId?: string | null;
  score?: number | null;
  costUsd?: number | null;
  status?: string | null;
  detailsVersion: string;
  detailsDeferred?: boolean;
  detailsBytes?: number;
  details: unknown;
};

export type RunCollectionPageLike = {
  rows: RunCollectionRowLike[];
  nextCursor?: string | null;
  total: number;
  projectionRevision: number;
  asOfSequence: number;
  truncatedByBytes: boolean;
  limit: number;
};

export type RunCollectionPageStateLike = {
  status: "loading" | "ready" | "stale" | "error" | "unavailable";
  page: RunCollectionPageLike | null;
  stale: boolean;
  error?: string;
};

export type RunCollectionItemStateLike = {
  status: "loading" | "ready" | "stale" | "error" | "unavailable";
  row: RunCollectionRowLike | null;
  stale: boolean;
  error?: string;
};

export type RunCollectionsClient = {
  page(collection: RunCollectionName, query: RunCollectionQueryLike): Promise<RunCollectionPageLike>;
  item(collection: RunCollectionName, itemId: string): Promise<RunCollectionRowLike | null>;
  /** Shared host-store subscriptions. Older fixture hosts may omit these. */
  subscribePage?(
    collection: RunCollectionName,
    query: RunCollectionQueryLike,
    listener: (state: RunCollectionPageStateLike) => void
  ): () => void;
  subscribeItem?(
    collection: RunCollectionName,
    itemId: string,
    listener: (state: RunCollectionItemStateLike) => void
  ): () => void;
};

export const COLLECTION_BROWSER_PAGE_SIZE = 25;

type PageState = {
  status: "idle" | "loading" | "ready" | "stale" | "error";
  page: RunCollectionPageLike | null;
  error?: string;
};

/** One bounded, live page for a product view. Transport, coalescing, and the
 * byte-bounded cache remain host-owned; this hook only maps their state into
 * the visual runtime. */
export function useCollectionPage(
  client: RunCollectionsClient | undefined,
  collection: RunCollectionName,
  query: RunCollectionQueryLike,
  enabled = true
): PageState {
  const key = JSON.stringify(query);
  const stableQuery = useMemo<RunCollectionQueryLike>(() => query, [key]);
  const [state, setState] = useState<PageState>({ status: "idle", page: null });

  useEffect(() => {
    if (!client || !enabled) {
      setState({ status: "idle", page: null });
      return;
    }
    if (client.subscribePage) {
      let cancelled = false;
      let directReadStarted = false;
      const directRead = () => {
        if (directReadStarted) return;
        directReadStarted = true;
        void client.page(collection, stableQuery).then(
          (page) => {
            if (!cancelled) setState({ status: "ready", page });
          },
          (reason) => {
            if (!cancelled) {
              setState((current) => ({
                status: "error",
                page: current.page,
                error: reason instanceof Error ? reason.message : String(reason)
              }));
            }
          }
        );
      };
      const unsubscribe = client.subscribePage(collection, stableQuery, (next) => {
        setState({
          status: next.status === "unavailable" ? "error" : next.status,
          page: next.page,
          error: next.error
        });
        // The shared store is preferred because it coalesces identical reads.
        // A host can nevertheless expose the direct collection bridge before
        // its shared store transport has initialized. Do not silently fall
        // back to the one-point summary in that transient state.
        if (!next.page && (next.status === "unavailable" || next.status === "error")) {
          directRead();
        }
      });
      return () => {
        cancelled = true;
        unsubscribe();
      };
    }
    let cancelled = false;
    setState((current) => ({ ...current, status: "loading" }));
    void client.page(collection, stableQuery).then(
      (page) => {
        if (!cancelled) setState({ status: "ready", page });
      },
      (reason) => {
        if (!cancelled) {
          setState((current) => ({
            status: "error",
            page: current.page,
            error: reason instanceof Error ? reason.message : String(reason)
          }));
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, [client, collection, enabled, stableQuery]);

  return state;
}

function filterKey(filter: RunCollectionFilterLike | null | undefined): string {
  if (!filter) return "";
  return [filter.parentId ?? "", filter.label ?? "", filter.status ?? "", filter.kind ?? "", filter.changedAfterRevision ?? ""].join("|");
}

function formatScore(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(3) : "—";
}

export function CollectionBrowser({
  client,
  collection,
  title,
  pageSize = COLLECTION_BROWSER_PAGE_SIZE,
  descending = false,
  filter,
  testId,
  renderDetail
}: {
  client?: RunCollectionsClient;
  collection: RunCollectionName;
  title?: string;
  pageSize?: number;
  descending?: boolean;
  filter?: RunCollectionFilterLike | null;
  testId?: string;
  /** Detail renderer for a selected row; defaults to the row's JSON. */
  renderDetail?: (row: RunCollectionRowLike) => ReactNode;
}) {
  const [cursorTrail, setCursorTrail] = useState<Array<string | null>>([null]);
  const [active, setActive] = useState(false);
  const [state, setState] = useState<PageState>({ status: "idle", page: null });
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<{ itemId: string; row: RunCollectionRowLike | null; error?: string } | null>(null);
  const key = filterKey(filter);
  const cursor = cursorTrail[cursorTrail.length - 1] ?? null;
  const query = useMemo<RunCollectionQueryLike>(
    () => ({ cursor, limit: pageSize, descending, filter: filter ?? null }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cursor, pageSize, descending, key]
  );

  useEffect(() => {
    setCursorTrail([null]);
    setSelected(null);
    setDetail(null);
    setActive(false);
  }, [collection, key, descending]);

  useEffect(() => {
    if (!client || !active) {
      setState({ status: "idle", page: null });
      return;
    }
    if (client.subscribePage) {
      return client.subscribePage(collection, query, (next) => {
        setState({
          status: next.status === "unavailable" ? "error" : next.status,
          page: next.page,
          error: next.error
        });
      });
    }
    let cancelled = false;
    setState((current) => ({ ...current, status: "loading" }));
    void client.page(collection, query)
      .then((page) => {
        if (!cancelled) setState({ status: "ready", page });
      })
      .catch((reason) => {
        if (!cancelled) setState((current) => ({ status: "error", page: current.page, error: reason instanceof Error ? reason.message : String(reason) }));
      });
    return () => {
      cancelled = true;
    };
  }, [active, client, collection, query]);

  useEffect(() => {
    if (!client || !selected) {
      setDetail(null);
      return;
    }
    if (client.subscribeItem) {
      return client.subscribeItem(collection, selected, (next) => {
        setDetail({ itemId: selected, row: next.row, error: next.error });
      });
    }
    let cancelled = false;
    void client
      .item(collection, selected)
      .then((row) => {
        if (!cancelled) setDetail({ itemId: selected, row });
      })
      .catch((reason) => {
        if (!cancelled) setDetail({ itemId: selected, row: null, error: reason instanceof Error ? reason.message : String(reason) });
      });
    return () => {
      cancelled = true;
    };
  }, [client, collection, selected]);

  const page = state.page;
  const heading = title ?? collection.replaceAll("_", " ");

  if (!client) {
    return (
      <section className="sv-section" data-testid={testId ?? `collection-browser-${collection}`} data-collection={collection} data-state="unavailable">
        <div className="sv-section-head"><h3>{heading}</h3></div>
        <p className="sv-lede">Durable collections are unavailable in this host.</p>
      </section>
    );
  }

  if (!active) {
    return (
      <section
        className="sv-section"
        data-testid={testId ?? `collection-browser-${collection}`}
        data-collection={collection}
        data-state="idle"
      >
        <div className="sv-section-head">
          <h3>{heading}</h3>
          <button type="button" className="sv-btn" onClick={() => setActive(true)}>
            Load {heading.toLowerCase()}
          </button>
        </div>
        <p className="sv-lede">Paged durable data loads only when requested.</p>
      </section>
    );
  }

  return (
    <section
      className="sv-section"
      data-testid={testId ?? `collection-browser-${collection}`}
      data-collection={collection}
      data-state={state.status}
      data-projection-revision={page?.projectionRevision}
      data-total={page?.total}
    >
      <div className="sv-section-head">
        <h3>{heading}</h3>
        <span className="sv-mono">
          {page ? `${page.rows.length} of ${page.total}` : state.status === "loading" ? "loading" : "—"}
          {page?.truncatedByBytes ? " · page cut on bytes" : ""}
        </span>
      </div>
      {state.error ? <p className="sv-lede" role="alert">{state.error}</p> : null}
      {page && page.rows.length === 0 && state.status === "ready" ? (
        <p className="sv-lede">No rows in this collection yet.</p>
      ) : null}
      {page && page.rows.length > 0 ? (
        <table className="sv-table" style={{ width: "100%", fontSize: 12 }}>
          <thead>
            <tr>
              <th style={{ textAlign: "left" }}>#</th>
              <th style={{ textAlign: "left" }}>Item</th>
              <th style={{ textAlign: "left" }}>Kind</th>
              <th style={{ textAlign: "left" }}>Label</th>
              <th style={{ textAlign: "right" }}>Score</th>
              <th style={{ textAlign: "left" }}>Status</th>
              <th style={{ textAlign: "right" }}>Rev</th>
            </tr>
          </thead>
          <tbody>
            {page.rows.map((row) => (
              <tr
                key={row.itemId}
                data-testid={`collection-row-${row.itemId}`}
                aria-selected={selected === row.itemId}
                onClick={() => setSelected((current) => (current === row.itemId ? null : row.itemId))}
                style={{ cursor: "pointer", background: selected === row.itemId ? "var(--sv-surface-muted)" : undefined }}
              >
                <td className="sv-mono">{row.ordinal}</td>
                <td className="sv-mono" style={{ overflowWrap: "anywhere" }}>{row.itemId}</td>
                <td>{row.kind}</td>
                <td>{row.label ?? "—"}</td>
                <td className="sv-mono" style={{ textAlign: "right" }}>{formatScore(row.score)}</td>
                <td>{row.status ?? "—"}</td>
                <td className="sv-mono" style={{ textAlign: "right" }}>{row.revision}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      <div style={{ display: "flex", gap: 8, marginTop: 8, alignItems: "center" }}>
        <button
          type="button"
          className="sv-btn"
          disabled={cursorTrail.length <= 1 || state.status === "loading"}
          onClick={() => setCursorTrail((trail) => (trail.length > 1 ? trail.slice(0, -1) : trail))}
        >
          Previous page
        </button>
        <button
          type="button"
          className="sv-btn"
          disabled={!page?.nextCursor || state.status === "loading"}
          onClick={() => {
            if (page?.nextCursor) setCursorTrail((trail) => [...trail, page.nextCursor ?? null]);
          }}
        >
          Next page
        </button>
        <span className="sv-mono" style={{ color: "var(--sv-text-faint)" }}>
          page {cursorTrail.length}{page ? ` · as of #${page.asOfSequence}` : ""}
        </span>
      </div>
      {selected && detail?.itemId === selected ? (
        <div role="region" aria-label={`${collection} ${selected}`} data-testid={`collection-detail-${selected}`} style={{ marginTop: 8, padding: "10px 12px", border: "1px solid var(--sv-border-strong)", borderRadius: 9, background: "var(--sv-surface-muted)", fontSize: 12 }}>
          {detail.error ? (
            <p className="sv-lede" role="alert">{detail.error}</p>
          ) : detail.row ? (
            renderDetail ? renderDetail(detail.row) : (
              <pre className="sv-mono" style={{ margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: 320, overflow: "auto" }}>
                {JSON.stringify(detail.row.details, null, 2)}
              </pre>
            )
          ) : (
            <p className="sv-lede">This item is no longer in the collection.</p>
          )}
        </div>
      ) : selected ? (
        <p className="sv-lede" role="status">Loading item…</p>
      ) : null}
    </section>
  );
}
