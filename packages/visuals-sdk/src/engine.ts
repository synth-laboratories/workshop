import {
  VISUAL_SESSION_SCHEMA,
  type JsonValue, type SessionCheckpoint, type SessionEvent, type SessionReceipt,
  type SessionRecording, type VisualAction, type VisualControl, type VisualSessionIdentity,
  type VisualSessionState, type VisualValueSchema,
} from "@synth/visuals-protocol";
import { stableSerialize } from "./query.ts";

export function assertJson(value: unknown, depth = 0): asserts value is JsonValue {
  if (depth > 40) throw new Error("Visual state exceeds maximum nesting");
  if (value === null || typeof value === "boolean" || typeof value === "string") return;
  if (typeof value === "number" && Number.isFinite(value)) return;
  if (Array.isArray(value)) { value.forEach((item) => assertJson(item, depth + 1)); return; }
  if (typeof value === "object" && value && Object.getPrototypeOf(value) === Object.prototype) {
    for (const [key, item] of Object.entries(value)) {
      if (["__proto__", "prototype", "constructor"].includes(key)) throw new Error("Unsafe visual state key");
      assertJson(item, depth + 1);
    }
    return;
  }
  throw new Error("Visual state must contain finite, serializable JSON values");
}

export function validateControl(control: VisualControl, value: unknown): asserts value is JsonValue {
  assertJson(value);
  validateValue(control,value,control.id);
}

export function validateValue(control: VisualValueSchema, value: JsonValue, path = "value", depth = 0): void {
  if(depth>40)throw new Error("Visual schema exceeds maximum nesting");
  if (value === null && control.nullable) return;
  const type = value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
  if (type !== control.type) throw new Error(`${path} requires ${control.type}`);
  if(control.oneOf){
    if(!Array.isArray(control.oneOf)||control.oneOf.length<1||control.oneOf.length>16)throw new Error("Visual union requires 1..16 variants");
    const matches=control.oneOf.filter(schema=>{try{validateValue(schema,value,path,depth+1);return true;}catch{return false;}});
    if(matches.length!==1)throw new Error(`${path} must match exactly one declared variant`);
  }
  if (control.options && !control.options.some((option) => stableSerialize(option) === stableSerialize(value))) throw new Error(`${path} is not an allowed option`);
  if (typeof value === "number" && ((control.minimum !== undefined && value < control.minimum) || (control.maximum !== undefined && value > control.maximum))) throw new Error(`${path} is outside its declared range`);
  if(Array.isArray(value)){
    if(control.maxItems!==undefined&&value.length>control.maxItems)throw new Error(`${path} exceeds item limit`);
    if(control.items)value.forEach((item,index)=>validateValue(control.items!,item,`${path}[${index}]`,depth+1));
  }else if(value!==null&&typeof value==="object"){
    for(const key of control.required??[])if(!Object.hasOwn(value,key))throw new Error(`${path}.${key} is required`);
    for(const [key,item] of Object.entries(value)){
      const schema=control.properties?.[key];
      if(schema)validateValue(schema,item,`${path}.${key}`,depth+1);
      else if(control.additionalProperties===false)throw new Error(`${path}.${key} is not allowed`);
      else if(typeof control.additionalProperties==="object")validateValue(control.additionalProperties,item,`${path}.${key}`,depth+1);
    }
  }
}

export function initialSession(identity: VisualSessionIdentity, definition = { id: "visual", version: "1" }): VisualSessionState {
  if (!identity.visualId || !Number.isSafeInteger(identity.revision) || identity.revision < 1 || !identity.viewKey) throw new Error("Invalid visual session identity");
  return { ...identity, definition, schemaVersion: VISUAL_SESSION_SCHEMA, stateVersion: 0, values: {}, controls: [] };
}

export function validateSession(value: VisualSessionState): void {
  assertJson(value);
  if (value.schemaVersion !== VISUAL_SESSION_SCHEMA) throw new Error("Unsupported visual session schema");
  if(!value.definition?.id?.trim()||!value.definition?.version?.trim())throw new Error("Visual definition pin required");
  initialSession(value, value.definition);
  if (!Number.isSafeInteger(value.stateVersion) || value.stateVersion < 0 || !Array.isArray(value.controls) || !value.values || Array.isArray(value.values)) throw new Error("Invalid visual session state");
  if(value.controls.length>512||new TextEncoder().encode(JSON.stringify(value)).length>1_048_576)throw new Error("Visual state exceeds bounds");
  if (value.replay !== undefined) {
    const replay = value.replay as Record<string, unknown>;
    const checkpoint = replay && Object.keys(replay).length === 1 && typeof replay.checkpointId === "string" && replay.checkpointId.trim();
    const recording = replay && Object.keys(replay).every(key=>["recordingId","sequence","eventCount","playing","intervalMs"].includes(key)) && typeof replay.recordingId === "string" && replay.recordingId.trim() && Number.isSafeInteger(replay.sequence) && Number(replay.sequence) >= 0
      && (replay.intervalMs===undefined || (Number.isInteger(replay.intervalMs) && Number(replay.intervalMs)>=16 && Number(replay.intervalMs)<=60000))
      && (replay.playing===undefined || typeof replay.playing==="boolean")
      && (replay.eventCount===undefined || (Number.isSafeInteger(replay.eventCount) && Number(replay.eventCount)>=Number(replay.sequence)));
    if (!checkpoint && !recording) throw new Error("Invalid replay cursor");
  }
  const ids = new Set<string>();
  for (const control of value.controls) {
    if (!control.id || ids.has(control.id)) throw new Error("Duplicate or missing visual control identity");
    ids.add(control.id);
    if (!Object.hasOwn(value.values, control.id)) throw new Error("Control value missing");
    validateControl(control, value.values[control.id]);
  }
  if(Object.keys(value.values).some(id=>!ids.has(id)))throw new Error("Unregistered visual state value");
}

/** Pure and shared by live interaction and recording validation. */
export function reduceVisualSession(state: VisualSessionState, action: VisualAction): VisualSessionState {
  if (!action.id || !action.kind) throw new Error("Visual action identity and kind are required");
  if (action.expectedStateVersion !== state.stateVersion) throw new Error(`Stale visual state: expected ${action.expectedStateVersion}, current ${state.stateVersion}`);
  if (!["presentation.set", "presentation.patch"].includes(action.kind)) throw new Error(`Unsupported visual action ${action.kind}`);
  const patch = action.kind === "presentation.set" ? { [action.target?.id ?? ""]: action.payload?.value } : action.payload?.values;
  if (!patch || typeof patch !== "object" || Array.isArray(patch) || Object.keys(patch).length === 0) throw new Error("Visual action requires control values");
  const values = { ...state.values };
  for (const [id,value] of Object.entries(patch)) {
    const control = state.controls.find((item) => item.id === id);
    if (!control) throw new Error(`Unknown visual control ${id}`);
    validateControl(control, value); values[id] = structuredClone(value);
  }
  const next={ ...structuredClone(state), stateVersion: state.stateVersion + 1, values };
  delete next.scene;
  delete next.replay;
  return next;
}

export async function canonicalDigest(value: unknown): Promise<string> {
  assertJson(value);
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(canonicalValue(value))));
  return `sha256:${[...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function canonicalValue(value: JsonValue): unknown {
  if (value === null) return ["null"];
  if (typeof value === "number") {
    const buffer = new ArrayBuffer(8); const view = new DataView(buffer);
    view.setFloat64(0, value === 0 ? 0 : value);
    return ["number", view.getBigUint64(0).toString(16).padStart(16, "0")];
  }
  if (typeof value === "string" || typeof value === "boolean") return [typeof value, value];
  if (Array.isArray(value)) return ["array", value.map(canonicalValue)];
  return ["object", Object.keys(value).sort().map((key) => [key, canonicalValue(value[key]!)])];
}

export async function checkpoint(state: VisualSessionState, renditionRefs: string[] = []): Promise<SessionCheckpoint> {
  validateSession(state);
  const captured = structuredClone(state);
  return { schemaVersion: "synth.visual-checkpoint.v1", id: crypto.randomUUID(), capturedAt: new Date().toISOString(), state: captured, digest: await canonicalDigest(captured), renditionRefs: [...renditionRefs] };
}

export async function verifyCheckpoint(saved: SessionCheckpoint): Promise<void> {
  if (saved.schemaVersion !== "synth.visual-checkpoint.v1") throw new Error("Unsupported visual checkpoint schema");
  if(typeof saved.id!=="string"||!saved.id||!Number.isFinite(Date.parse(saved.capturedAt))||!Array.isArray(saved.renditionRefs)||saved.renditionRefs.some(ref=>typeof ref!=="string"))throw new Error("Invalid visual checkpoint metadata");
  validateSession(saved.state);
  if (await canonicalDigest(saved.state) !== saved.digest) throw new Error("Visual checkpoint integrity mismatch");
}

export async function replayRecording(recording: SessionRecording, through = recording.events.length): Promise<VisualSessionState> {
  if (recording.schemaVersion !== "synth.visual-session-recording.v1" || !Number.isSafeInteger(through) || through < 0 || through > recording.events.length) throw new Error("Invalid visual recording or cursor");
  await verifyCheckpoint(recording.initial);
  let state = structuredClone(recording.initial.state);
  for (let index = 0; index < through; index++) {
    const event = recording.events[index]!;
    if (event.sequence !== index + 1) throw new Error("Visual recording contains a sequence gap");
    if(event.before){
      validateSession(event.before);
      if(event.before.stateVersion!==state.stateVersion||event.before.visualId!==state.visualId||event.before.revision!==state.revision||event.before.viewKey!==state.viewKey||stableSerialize(event.before.definition)!==stableSerialize(state.definition))throw new Error("Recording context identity mismatch");
      for(const control of state.controls){
        if(stableSerialize(event.before.controls.find(item=>item.id===control.id))!==stableSerialize(control)||stableSerialize(event.before.values[control.id])!==stableSerialize(state.values[control.id]))throw new Error("Recording context changed committed presentation");
      }
      state=structuredClone(event.before);
    }
    const next = reduceVisualSession(state, event.action);
    // A renderer may publish a new scene/control inventory between commands.
    // Verify reducer-owned state, then validate the complete recorded projection.
    if (stableSerialize(next.values) !== stableSerialize(event.state.values) || next.stateVersion !== event.state.stateVersion
      || next.visualId !== event.state.visualId || next.revision !== event.state.revision || next.viewKey !== event.state.viewKey) throw new Error("Visual recording transition mismatch");
    validateSession(event.state);
    state = structuredClone(event.state);
  }
  return state;
}

/** Standalone host/reference implementation with identical semantic controls to
 * the native adapter. It never executes domain effects during presentation replay. */
export class VisualSession {
  #state: VisualSessionState;
  #listeners = new Set<() => void>();
  #receipts = new Map<string, { input: string; receipt: SessionReceipt }>();
  #recording?: SessionRecording;
  #disposed = false;
  constructor(state: VisualSessionState) { validateSession(state); this.#state = structuredClone(state); }
  get state(): VisualSessionState { return structuredClone(this.#state); }
  subscribe = (listener: () => void): (() => void) => { this.#listeners.add(listener); return () => this.#listeners.delete(listener); };
  register(control: VisualControl, initial: JsonValue): void {
    this.#assertOpen();
    validateControl(control, initial);
    const prior = this.#state.controls.find((item) => item.id === control.id);
    if (prior && stableSerialize(prior) !== stableSerialize(control)) throw new Error(`Conflicting control ${control.id}`);
    if (prior) return;
    this.#state.controls.push(structuredClone(control));
    if (!Object.hasOwn(this.#state.values, control.id)) this.#state.values[control.id] = structuredClone(initial);
    this.#publish();
  }
  execute(action: VisualAction): SessionReceipt {
    this.#assertOpen();
    const key = action.idempotencyKey ?? action.id;
    const input = stableSerialize(action);
    const previous = this.#receipts.get(key);
    if (previous) {
      if (previous.input !== input) throw new Error("Visual idempotency key reused with different input");
      return { ...structuredClone(previous.receipt), duplicate: true };
    }
    const before=this.state;
    this.#state = reduceVisualSession(this.#state, action);
    const receipt = { commandId: action.id, state: this.state };
    this.#receipts.set(key, { input, receipt });
    if (this.#recording && !this.#recording.endedAt) {
      const event: SessionEvent = { sequence: this.#recording.events.length + 1, action: structuredClone(action), before, state: this.state, occurredAt: new Date().toISOString() };
      this.#recording.events.push(event);
    }
    this.#publish();
    return structuredClone(receipt);
  }
  async restore(saved: SessionCheckpoint): Promise<void> {
    return this.#restore(saved,{checkpointId:saved.id},this.#state.stateVersion);
  }
  async seekRecording(recording:SessionRecording,sequence:number,expectedStateVersion=this.#state.stateVersion,playing=false):Promise<void>{
    const state=await replayRecording(recording,sequence);
    const current=this.#state.replay;
    const intervalMs=current && "recordingId" in current && current.recordingId===recording.id ? current.intervalMs : undefined;
    await this.#restore(await checkpoint(state),{recordingId:recording.id,sequence,eventCount:recording.events.length,playing:playing && sequence<recording.events.length,...(intervalMs===undefined?{}:{intervalMs})},expectedStateVersion);
  }
  playRecording(playing:boolean,expectedStateVersion=this.#state.stateVersion,intervalMs?:number):void{
    this.#assertOpen();
    if(expectedStateVersion!==this.#state.stateVersion)throw new Error("Stale replay state");
    const replay=this.#state.replay;
    if(!replay || !("recordingId" in replay))throw new Error("Select a recording before playback");
    if(this.#recording&&!this.#recording.endedAt)throw new Error("Stop recording before playback");
    if(intervalMs!==undefined && (!Number.isInteger(intervalMs)||intervalMs<16||intervalMs>60000))throw new Error("Playback interval must be 16..60000 ms");
    this.#state={...this.#state,stateVersion:this.#state.stateVersion+1,replay:{...replay,...(intervalMs===undefined?{}:{intervalMs}),playing:playing && replay.sequence<(replay.eventCount ?? 0)}};
    delete this.#state.scene;this.#publish();
  }
  async #restore(saved:SessionCheckpoint,replay:NonNullable<VisualSessionState["replay"]>,expectedStateVersion:number):Promise<void>{
    this.#assertOpen();
    if(this.#recording&&!this.#recording.endedAt)throw new Error("Stop recording before restoring a checkpoint");
    await verifyCheckpoint(saved);
    if(expectedStateVersion!==this.#state.stateVersion)throw new Error("Stale replay state");
    if (saved.state.visualId !== this.#state.visualId || saved.state.revision !== this.#state.revision
      || saved.state.viewKey !== this.#state.viewKey
      || saved.state.definition.id !== this.#state.definition.id || saved.state.definition.version !== this.#state.definition.version) throw new Error("Snapshot requires its original visual revision and definition");
    this.#state = { ...structuredClone(saved.state), stateVersion: this.#state.stateVersion + 1, viewKey: this.#state.viewKey,replay };
    delete this.#state.scene;
    this.#publish();
  }
  async startRecording(): Promise<SessionRecording> {
    this.#assertOpen();
    if (!this.#recording || this.#recording.endedAt) this.#recording = { schemaVersion: "synth.visual-session-recording.v1", id: crypto.randomUUID(), initial: await checkpoint(this.#state), events: [] };
    return structuredClone(this.#recording);
  }
  stopRecording(): SessionRecording | undefined {
    if (this.#recording && !this.#recording.endedAt) this.#recording.endedAt = new Date().toISOString();
    return this.#recording ? structuredClone(this.#recording) : undefined;
  }
  dispose(): void { this.#disposed = true; this.#listeners.clear(); }
  #assertOpen(): void { if (this.#disposed) throw new Error("Visual session is disposed"); }
  #publish(): void { for (const listener of this.#listeners) listener(); }
}
