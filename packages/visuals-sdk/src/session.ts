import type {
  CohortRef,
  CorpusRef,
  ExplorationPath,
  SemanticRef,
  SelectionSet,
  VisualAction,
  EvidenceGraph,
  ExtensionPin,
  SemanticScene,
  VisualRecording,
  VisualRecordingEvent,
  VisualSnapshot,
} from "@synth/visuals-protocol";
import { stableValueDigest } from "./query.ts";

export class VisualExplorationSession {
  readonly visualId: string;
  readonly revision: number;
  readonly corpus: CorpusRef;
  #cohort: CohortRef;
  #selection: SelectionSet = { members: [] };
  #path: ExplorationPath;
  #cohortHistory: CohortRef[] = [];
  #selectionHistory: SelectionSet[] = [];
  #stateVersion = 0;
  #recording?: VisualRecording;

  constructor(input: { visualId: string; revision?: number; corpus: CorpusRef; rootCohort: CohortRef }) {
    this.visualId = input.visualId;
    this.revision = input.revision ?? 1;
    this.corpus = input.corpus;
    this.#cohort = input.rootCohort;
    this.#path = { root: input.corpus, steps: [], current: { kind: "cohort", id: input.rootCohort.id } };
  }

  get stateVersion(): number { return this.#stateVersion; }
  get cohort(): CohortRef { return this.#cohort; }
  get selection(): SelectionSet { return this.#selection; }
  get exploration(): ExplorationPath { return structuredClone(this.#path); }
  get recording(): VisualRecording | undefined { return this.#recording ? structuredClone(this.#recording) : undefined; }

  dispatch(action: VisualAction, next?: { cohort?: CohortRef; target?: SemanticRef }): number {
    if (action.expectedStateVersion !== undefined && action.expectedStateVersion !== this.#stateVersion) {
      throw new Error(`Stale visual state: expected ${action.expectedStateVersion}, current ${this.#stateVersion}`);
    }
    const from = this.#path.current;
    if (action.kind === "back") {
      if (action.target) {
        const reachable = [this.#path.current, ...this.#path.steps.map((step) => step.from)];
        if (!reachable.some((candidate) => candidate.kind === action.target!.kind && candidate.id === action.target!.id)) {
          throw new Error(`Back target ${action.target.kind}:${action.target.id} is not in this exploration path`);
        }
      }
      do {
        const previous = this.#path.steps.pop();
        if (!previous) break;
        this.#path.current = previous.from;
        this.#selection = this.#selectionHistory.pop() ?? { members: [] };
        if (previous.cohort) this.#cohort = this.#cohortHistory.pop() ?? this.#cohort;
      } while (action.target && (this.#path.current.kind !== action.target.kind || this.#path.current.id !== action.target.id));
    } else {
      const target = next?.target ?? action.target ?? from;
      this.#selectionHistory.push(structuredClone(this.#selection));
      if (next?.cohort) {
        this.#cohortHistory.push(this.#cohort);
        this.#cohort = next.cohort;
      }
      this.#path.steps.push({ id: action.id, action, from, to: target, cohort: next?.cohort, occurredAt: new Date().toISOString() });
      this.#path.current = target;
    }
    if (action.kind === "select" || action.kind === "drill_down") {
      const target = next?.target ?? action.target;
      if (target) this.#selection = { primary: target, members: [target] };
    }
    this.#stateVersion += 1;
    this.#record("action", { action });
    return this.#stateVersion;
  }

  startRecording(): VisualRecording {
    if (this.#recording && !this.#recording.endedAt) return structuredClone(this.#recording);
    const initialSnapshot = this.snapshot();
    this.#recording = { id: `recording:${crypto.randomUUID()}`, visualId: this.visualId, startedAt: new Date().toISOString(), initialSnapshot, events: [], checkpoints: [] };
    return structuredClone(this.#recording);
  }

  stopRecording(): VisualRecording | undefined {
    if (!this.#recording) return undefined;
    this.#recording.endedAt = new Date().toISOString();
    return structuredClone(this.#recording);
  }

  snapshot(renditionRefs: string[] = [], context: {
    scene?: SemanticScene;
    evidenceGraph?: EvidenceGraph;
    presentationDigest?: string;
    projectionDigest?: string;
    renderer?: ExtensionPin;
    projector?: ExtensionPin;
    viewport?: { width: number; height: number; scaleFactor?: number };
  } = {}): VisualSnapshot {
    const capturedAt = new Date().toISOString();
    const snapshot: VisualSnapshot = {
      id: `snapshot:${crypto.randomUUID()}`,
      visualId: this.visualId,
      revision: this.revision,
      stateVersion: this.#stateVersion,
      capturedAt,
      corpus: this.corpus,
      cohort: this.#cohort,
      exploration: structuredClone(this.#path),
      selection: structuredClone(this.#selection),
      semanticSceneDigest: stableValueDigest(context.scene ?? { current: this.#path.current, selection: this.#selection }),
      presentationDigest: context.presentationDigest ?? stableValueDigest({ path: this.#path, cohort: this.#cohort }),
      renditionRefs,
      evidenceGraph: context.evidenceGraph ?? context.scene?.evidenceGraph,
      projectionDigest: context.projectionDigest,
      renderer: context.renderer,
      projector: context.projector,
      viewport: context.viewport,
    };
    if (this.#recording && !this.#recording.endedAt) {
      this.#recording.checkpoints.push(snapshot);
      this.#record("snapshot_captured", { snapshotId: snapshot.id });
    }
    return snapshot;
  }

  #record(kind: VisualRecordingEvent["kind"], fields: Partial<VisualRecordingEvent>): void {
    if (!this.#recording || this.#recording.endedAt) return;
    this.#recording.events.push({
      sequence: this.#recording.events.length + 1,
      occurredAt: new Date().toISOString(),
      kind,
      stateVersion: this.#stateVersion,
      ...fields,
    });
  }
}
