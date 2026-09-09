import { createContext, useContext, type ComponentType, type CSSProperties, type ReactNode } from "react";
import type { SemanticRef, SemanticScene, VisualAction } from "@synth/visuals-protocol";

export type VisualActionDispatcher = (action: VisualAction) => void;

const SceneContext = createContext<SemanticScene | null>(null);
const ActionContext = createContext<VisualActionDispatcher | null>(null);

export function SemanticSceneProvider({ scene, dispatch, children }: { scene: SemanticScene; dispatch: VisualActionDispatcher; children: ReactNode }) {
  return <SceneContext.Provider value={scene}><ActionContext.Provider value={dispatch}>{children}</ActionContext.Provider></SceneContext.Provider>;
}

export function useSemanticScene(): SemanticScene {
  const scene = useContext(SceneContext);
  if (!scene) throw new Error("useSemanticScene must be used within SemanticSceneProvider");
  return scene;
}

export function useVisualAction(): VisualActionDispatcher {
  const dispatch = useContext(ActionContext);
  if (!dispatch) throw new Error("useVisualAction must be used within SemanticSceneProvider");
  return dispatch;
}

export function semanticAttributes(ref: SemanticRef): Record<string, string> {
  return {
    "data-semantic-kind": ref.kind,
    "data-semantic-id": ref.id,
    "data-annotation-kind": ref.kind,
    "data-annotation-id": ref.id,
  };
}

export function SemanticTarget({ ref, children, className, style }: { ref: SemanticRef; children: ReactNode; className?: string; style?: CSSProperties }) {
  return <div {...semanticAttributes(ref)} className={className} style={style}>{children}</div>;
}

export function TruthValue({ state, value }: { state: string; value?: ReactNode }) {
  if (state === "observed") return <>{value}</>;
  return <span data-truth-state={state} aria-label={`Value ${state}`}>— <small>{state.replaceAll("_", " ")}</small></span>;
}

export type ReactVisualRenderer<TArtifact> = {
  id: string;
  matches: (artifact: TArtifact) => boolean;
  component: ComponentType<{ artifact: TArtifact }>;
  observe?: boolean;
};

export class ReactVisualRendererRegistry<TArtifact> {
  readonly #renderers: ReactVisualRenderer<TArtifact>[] = [];

  register(renderer: ReactVisualRenderer<TArtifact>): this {
    if (this.#renderers.some((candidate) => candidate.id === renderer.id)) throw new Error(`React visual renderer ${renderer.id} is already registered`);
    this.#renderers.push(renderer);
    return this;
  }

  resolve(artifact: TArtifact): ReactVisualRenderer<TArtifact> {
    const renderer = this.#renderers.find((candidate) => candidate.matches(artifact));
    if (!renderer) throw new Error("No registered visual renderer accepts this artifact");
    return renderer;
  }

  descriptors(): Array<{ id: string; observe: boolean }> {
    return this.#renderers.map((renderer) => ({ id: renderer.id, observe: renderer.observe === true }));
  }
}

export * from "./state.tsx";
export * from "./viewport.tsx";
export * from "./timeline.tsx";
export * from "./surfaces.tsx";
export * from "./session.ts";
export * from "./sessionToolbar.tsx";
export * from "./rendition.tsx";
export * from "./dynamicDiagram.tsx";
export * from "./frameSession.ts";
export * from "./playback.ts";
export * from "./captureBarrier.ts";
export * from "./staticDocument.tsx";
export {useVisualEvidence} from "./evidence.ts";
