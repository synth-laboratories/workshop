import type { SemanticScene, VisualRecording, VisualSnapshot } from "@synth/visuals-protocol";
import type { VisualExplorationSession } from "./session.ts";

export type StoredVisualSession = {
  visualId: string;
  session: VisualExplorationSession;
  scene: () => SemanticScene;
};

export interface VisualSessionStore {
  register(binding: StoredVisualSession): void;
  get(visualId: string): StoredVisualSession | undefined;
  putSnapshot(snapshot: VisualSnapshot): Promise<void>;
  snapshots(visualId: string): Promise<VisualSnapshot[]>;
  putRecording(recording: VisualRecording): Promise<void>;
  recordings(visualId: string): Promise<VisualRecording[]>;
}

export class InMemoryVisualSessionStore implements VisualSessionStore {
  readonly #sessions = new Map<string, StoredVisualSession>();
  readonly #snapshots = new Map<string, VisualSnapshot[]>();
  readonly #recordings = new Map<string, VisualRecording[]>();

  register(binding: StoredVisualSession): void {
    if (this.#sessions.has(binding.visualId)) throw new Error(`Visual session ${binding.visualId} is already registered`);
    this.#sessions.set(binding.visualId, binding);
  }
  get(visualId: string): StoredVisualSession | undefined { return this.#sessions.get(visualId); }
  async putSnapshot(snapshot: VisualSnapshot): Promise<void> {
    this.#snapshots.set(snapshot.visualId, [...(this.#snapshots.get(snapshot.visualId) ?? []), structuredClone(snapshot)]);
  }
  async snapshots(visualId: string): Promise<VisualSnapshot[]> { return structuredClone(this.#snapshots.get(visualId) ?? []); }
  async putRecording(recording: VisualRecording): Promise<void> {
    const rows = (this.#recordings.get(recording.visualId) ?? []).filter((row) => row.id !== recording.id);
    this.#recordings.set(recording.visualId, [...rows, structuredClone(recording)]);
  }
  async recordings(visualId: string): Promise<VisualRecording[]> { return structuredClone(this.#recordings.get(visualId) ?? []); }
}

