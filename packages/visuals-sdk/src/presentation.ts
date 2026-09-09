import type { JsonValue, PresentationState } from "@synth/visuals-protocol";
import { stableValueDigest } from "./query.ts";

export interface PresentationStateStore {
  load(visualId: string, schemaVersion: string): Promise<PresentationState | undefined>;
  save(visualId: string, state: PresentationState, expectedStateVersion?: number): Promise<PresentationState>;
}

export class InMemoryPresentationStateStore implements PresentationStateStore {
  readonly #states = new Map<string, PresentationState>();

  async load(visualId: string, schemaVersion: string): Promise<PresentationState | undefined> {
    const state = this.#states.get(`${visualId}:${schemaVersion}`);
    return state ? structuredClone(state) : undefined;
  }

  async save(visualId: string, input: PresentationState, expectedStateVersion?: number): Promise<PresentationState> {
    const key = `${visualId}:${input.schemaVersion}`;
    const current = this.#states.get(key);
    if (expectedStateVersion !== undefined && (current?.stateVersion ?? 0) !== expectedStateVersion) {
      throw new Error(`Stale presentation state: expected ${expectedStateVersion}, current ${current?.stateVersion ?? 0}`);
    }
    const state: PresentationState = {
      ...structuredClone(input),
      stateVersion: (current?.stateVersion ?? 0) + 1,
      digest: stableValueDigest(input.value),
      updatedAt: new Date().toISOString(),
    };
    this.#states.set(key, state);
    return structuredClone(state);
  }
}

export function presentationState(schemaVersion: string, revision: number, value: Record<string, JsonValue>): PresentationState {
  return { schemaVersion, revision, stateVersion: 0, value, digest: stableValueDigest(value), updatedAt: new Date().toISOString() };
}

