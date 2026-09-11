import type { AppEvent, ScopeView, ScopedSessions, ScopedEvents, SessionRecord } from "../generated/protocol";

/** Separate cache for the gated scoped API; legacy rows are never adopted. */
export type ScopedHistoryState = {
  scope: ScopeView;
  sessions: SessionRecord[];
  events: Record<string, AppEvent[]>;
};
export const emptyScopedHistory = (): ScopedHistoryState => ({
  scope: { generation: 0, availability: "qualification_required" }, sessions: [], events: {}
});
const local = (session: SessionRecord) => session.kind === "codex";
export function observeScope(state: ScopedHistoryState, scope: ScopeView): ScopedHistoryState {
  if (!Number.isSafeInteger(scope.generation) || scope.generation < state.scope.generation) return state;
  if (scope.generation === state.scope.generation) {
    // An availability change must carry a new host generation. Same-generation
    // refreshes cannot restore an account after a reset.
    return state;
  }
  const sessions = state.sessions.filter(local);
  const ids = new Set(sessions.map(session => session.id));
  return { scope, sessions, events: Object.fromEntries(Object.entries(state.events).filter(([id]) => ids.has(id))) };
}
export function acceptHistory(state: ScopedHistoryState, page: ScopedSessions): ScopedHistoryState {
  if (page.generation !== state.scope.generation) return state;
  const sessions = page.sessions.filter(session => local(session) || (session.kind === "intern" && state.scope.availability === "ready"));
  const ids = new Set(sessions.map(session => session.id));
  return { ...state, sessions, events: Object.fromEntries(Object.entries(state.events).filter(([id]) => ids.has(id))) };
}
export function acceptEvents(state: ScopedHistoryState, sessionId: string, page: ScopedEvents): ScopedHistoryState {
  if (page.generation !== state.scope.generation || !state.sessions.some(session => session.id === sessionId)) return state;
  if (page.events.some(event => event.sessionId !== sessionId)) return state;
  const events = new Map((state.events[sessionId] ?? []).map(event => [event.sequence, event]));
  for (const event of page.events) events.set(event.sequence, event);
  return { ...state, events: { ...state.events, [sessionId]: [...events.values()].sort((a, b) => Number(a.sequence) - Number(b.sequence)) } };
}

export type ScopedHistoryTransport = {
  observe(listener: (scope: ScopeView) => void): Promise<() => void>;
  view(): Promise<ScopeView>;
  history(): Promise<ScopedSessions>;
};
/** Attach before snapshotting, so a late bootstrap response cannot undo reset. */
export function connectScopedHistory(transport: ScopedHistoryTransport, publish: (state: ScopedHistoryState) => void): () => void {
  let disposed = false;
  let detach: (() => void) | undefined;
  let state = emptyScopedHistory();
  const update = async (scope: ScopeView) => {
    if (disposed) return;
    const next = observeScope(state, scope);
    if (next !== state) { state = next; publish(state); }
    if (scope.generation !== state.scope.generation) return;
    try {
      const page = await transport.history();
      if (disposed) return;
      const next = acceptHistory(state, page);
      if (next !== state) { state = next; publish(state); }
    } catch { /* Local UI boot does not depend on scoped cloud availability. */ }
  };
  void transport.observe(scope => { void update(scope); }).then(async unlisten => {
    if (disposed) { unlisten(); return; }
    detach = unlisten;
    await update(await transport.view());
  }).catch(() => { /* Optional gated observer cannot fail Local boot. */ });
  return () => { disposed = true; detach?.(); };
}
