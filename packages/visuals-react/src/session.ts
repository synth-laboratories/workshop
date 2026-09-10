import { createContext, createElement, useCallback, useContext, useEffect, useRef, useState, useSyncExternalStore, type Dispatch, type ReactNode, type SetStateAction } from "react";
import type { JsonValue, SemanticScene, VisualControl,VisualValueSchema } from "@synth/visuals-protocol";
import { type VisualSessionClient } from "@synth/visuals-sdk";

const SessionContext = createContext<VisualSessionClient | null>(null);
const emptySubscribe = () => () => {};
const emptySnapshot = () => null;

/** Preserve the required structure of non-null defaults. Empty containers and
 * null placeholders still require explicit items/properties from the author. */
function defaultSchema(value:unknown):VisualValueSchema{
  const type=value==null?"string":Array.isArray(value)?"array":typeof value;
  if(value&&typeof value==="object"&&!Array.isArray(value)){
    const entries=Object.entries(value).filter(([,item])=>item!==undefined);
    return {type:"object",required:entries.map(([key])=>key),properties:Object.fromEntries(entries.filter(([,item])=>item!==null).map(([key,item])=>[key,defaultSchema(item)]))};
  }
  return {type:type as VisualValueSchema["type"],...(value==null?{nullable:true}:{})};
}

export function VisualSessionProvider({ client, children }: { client: VisualSessionClient; children: ReactNode }) {
  useEffect(() => { void client.start(); return () => client.dispose(); }, [client]);
  return createElement(SessionContext.Provider, { value:client }, children);
}
export function useVisualSessionClient(): VisualSessionClient | null { return useContext(SessionContext); }
export function useVisualSessionSnapshot() {
  const client = useVisualSessionClient();
  return useSyncExternalStore(client?.subscribe ?? emptySubscribe, client?.getSnapshot ?? emptySnapshot, client?.getSnapshot ?? emptySnapshot);
}

/** Serializable user intent. Evidence hydration, animation ticks and hover stay
 * outside this hook. Unhosted fixtures retain ordinary local React behavior. */
export function useVisualState<T>(id: string, initial: T | (() => T), options: Partial<Omit<VisualControl, "id">> = {}): [T, Dispatch<SetStateAction<T>>] {
  const client = useVisualSessionClient();
  const [fallback, setFallback] = useState(initial);
  // A control subscribes to its value, not scene publications or unrelated
  // controls. Otherwise read-only host updates re-render every input and React
  // temporarily clears input names even while the presentation is unchanged.
  const readValue = useCallback(() => {
    const snapshot=client?.getSnapshot();
    return snapshot && Object.hasOwn(snapshot.state.values,id) ? snapshot.state.values[id] : fallback;
  },[client,id,fallback]);
  const value=useSyncExternalStore(client?.subscribe ?? emptySubscribe,readValue,readValue);
  const defaultValue = useRef(fallback);
  const controlRef = useRef<VisualControl | null>(null);
  if (!controlRef.current || controlRef.current.id !== id) {
    const value = defaultValue.current;
    controlRef.current = { id, label: options.label ?? id, ...defaultSchema(value), ...options };
  }
  useEffect(() => { client?.register(controlRef.current!, (defaultValue.current ?? null) as JsonValue); }, [client, id]);
  const set = useCallback<Dispatch<SetStateAction<T>>>((update) => {
    if (!client) { setFallback(update); return; }
    void client.set(id, (previous) => {
      const decoded = previous === null && defaultValue.current === undefined ? undefined : previous;
      const next = typeof update === "function" ? (update as (value: T) => T)(decoded as T) : update;
      // Optional object fields are absent on the wire, just as with typed
      // domain envelopes. Unsupported/cyclic values still fail serialization.
      return JSON.parse(JSON.stringify(next ?? null)) as JsonValue;
    }).catch(() => {}); // The shared session surface displays persistence errors.
  }, [client, id]);
  return [(value === null && defaultValue.current === undefined ? undefined : value) as T, set];
}

export function usePublishVisualScene(scene: SemanticScene | undefined): void {
  const client = useVisualSessionClient();
  useEffect(() => { if (scene) client?.publishScene(scene); }, [client, scene]);
}
