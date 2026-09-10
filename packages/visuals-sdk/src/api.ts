import type { CommandReceipt, SemanticScene, VisualCommand, VisualQuery, VisualQueryResult } from "@synth/visuals-protocol";
import type { VisualExplorationSession } from "./session.ts";

export interface VisualsApi {
  execute(command: VisualCommand): Promise<CommandReceipt>;
  query(query: VisualQuery): Promise<VisualQueryResult>;
}

type SessionBinding = {
  session: VisualExplorationSession;
  scene: () => SemanticScene;
  interact?: (action: Extract<VisualCommand, { kind: "interact" }>["action"], session: VisualExplorationSession) => { cohort?: import("@synth/visuals-protocol").CohortRef; target?: import("@synth/visuals-protocol").SemanticRef } | void;
};

export class InMemoryVisualsApi implements VisualsApi {
  readonly #sessions = new Map<string, SessionBinding>();

  register(visualId: string, session: VisualExplorationSession, scene: () => SemanticScene, interact?: SessionBinding["interact"]): void {
    if (this.#sessions.has(visualId)) throw new Error(`Visual session ${visualId} is already registered`);
    this.#sessions.set(visualId, { session, scene, interact });
  }

  async execute(command: VisualCommand): Promise<CommandReceipt> {
    const binding = this.#require(command.visualId);
    if (command.kind === "interact") {
      if (command.action.expectedStateVersion !== undefined && command.action.expectedStateVersion !== binding.session.stateVersion) throw new Error("Stale visual state");
      binding.session.dispatch(command.action, binding.interact?.(command.action, binding.session) || undefined);
    }
    if (command.kind === "start_recording") binding.session.startRecording();
    if (command.kind === "stop_recording") binding.session.stopRecording();
    const snapshot = command.kind === "capture_snapshot" ? binding.session.snapshot(command.renditionRefs, { scene: binding.scene() }) : undefined;
    return {
      commandId: command.kind === "interact" ? command.action.id : crypto.randomUUID(),
      visualId: command.visualId,
      accepted: true,
      stateVersion: binding.session.stateVersion,
      snapshot,
      recording: command.kind === "stop_recording" ? binding.session.recording : undefined,
    };
  }

  async query(query: VisualQuery): Promise<VisualQueryResult> {
    const binding = this.#require(query.visualId);
    return {
      visualId: query.visualId,
      stateVersion: binding.session.stateVersion,
      scene: query.kind === "inspect" ? binding.scene() : undefined,
      recording: query.kind === "get_recording" ? binding.session.recording : undefined,
    };
  }

  #require(visualId: string): SessionBinding {
    const binding = this.#sessions.get(visualId);
    if (!binding) throw new Error(`Unknown visual session ${visualId}`);
    return binding;
  }
}
