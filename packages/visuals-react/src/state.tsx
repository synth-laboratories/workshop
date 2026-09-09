import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { JsonValue, PresentationState, SelectionSet, SemanticRef } from "@synth/visuals-protocol";
import { presentationState, type PresentationStateStore } from "@synth/visuals-sdk";

type SelectionContextValue = {
  selection: SelectionSet;
  select: (ref: SemanticRef, additive?: boolean) => void;
  clear: () => void;
};

const SelectionContext = createContext<SelectionContextValue | null>(null);

export function SelectionProvider({ initial = { members: [] }, children }: { initial?: SelectionSet; children: ReactNode }) {
  const [selection, setSelection] = useState<SelectionSet>(initial);
  const select = useCallback((ref: SemanticRef, additive = false) => setSelection((current) => {
    const members = additive
      ? [...current.members.filter((member) => member.kind !== ref.kind || member.id !== ref.id), ref]
      : [ref];
    return { primary: ref, members };
  }), []);
  const clear = useCallback(() => setSelection({ members: [] }), []);
  const value = useMemo(() => ({ selection, select, clear }), [selection, select, clear]);
  return <SelectionContext.Provider value={value}>{children}</SelectionContext.Provider>;
}

export function useSelection(): SelectionContextValue {
  const value = useContext(SelectionContext);
  if (!value) throw new Error("useSelection must be used within SelectionProvider");
  return value;
}

type PresentationContextValue = {
  state: PresentationState;
  loading: boolean;
  error?: string;
  update: (patch: Record<string, JsonValue>) => Promise<void>;
};

const PresentationContext = createContext<PresentationContextValue | null>(null);

export function PresentationStateProvider({ visualId, schemaVersion, revision, initial = {}, store, children }: {
  visualId: string;
  schemaVersion: string;
  revision: number;
  initial?: Record<string, JsonValue>;
  store: PresentationStateStore;
  children: ReactNode;
}) {
  const [state, setState] = useState(() => presentationState(schemaVersion, revision, initial));
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void store.load(visualId, schemaVersion).then((restored) => {
      if (!cancelled && restored) setState(restored);
    }).catch((reason) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [schemaVersion, store, visualId]);

  const update = useCallback(async (patch: Record<string, JsonValue>) => {
    const candidate = presentationState(schemaVersion, revision, { ...state.value, ...patch });
    try {
      const saved = await store.save(visualId, candidate, state.stateVersion);
      setState(saved);
      setError(undefined);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      throw reason;
    }
  }, [revision, schemaVersion, state, store, visualId]);

  return <PresentationContext.Provider value={{ state, loading, error, update }}>{children}</PresentationContext.Provider>;
}

export function usePresentationState(): PresentationContextValue {
  const value = useContext(PresentationContext);
  if (!value) throw new Error("usePresentationState must be used within PresentationStateProvider");
  return value;
}

