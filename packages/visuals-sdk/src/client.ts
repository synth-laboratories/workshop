import type { JsonValue, SemanticScene, SessionCheckpoint, SessionRecording, VisualAction, VisualControl, VisualSessionState } from "@synth/visuals-protocol";
import { assertJson, initialSession, validateControl, validateSession, verifyCheckpoint, replayRecording } from "./engine.ts";
import { stableSerialize } from "./query.ts";

export interface VisualEngineTransport {
  pixelCapture?:boolean;
  evidenceCuts?:boolean;
  request(request: Record<string, unknown>): Promise<Record<string, unknown>>;
  subscribe?(changed: () => void): () => void;
}
export type VisualClientSnapshot = { state: VisualSessionState; ready: boolean; error?: string; recordingId?: string };
function freezeSnapshot<T>(value:T):T {
  if(value&&typeof value==="object"&&!Object.isFrozen(value)){Object.values(value).forEach(freezeSnapshot);Object.freeze(value);}
  return value;
}
function transportError(error:unknown):string{
  if(error instanceof Error)return error.message;
  if(error&&typeof error==="object"&&"message" in error)return String(error.message);
  return typeof error==="string"?error:JSON.stringify(error);
}

/** One controller per mounted visual revision/view. Native owns committed state;
 * this cache has no second reducer and cannot acknowledge an uncommitted action. */
export class VisualSessionClient {
  readonly transport: VisualEngineTransport;
  #snapshot: VisualClientSnapshot;
  #controls = new Map<string, { control: VisualControl; initial: JsonValue }>();
  #listeners = new Set<() => void>();
  #queue: Promise<unknown> = Promise.resolve();
  #unsubscribe?: () => void;
  #timer?: ReturnType<typeof setInterval>;
  #disposed = false;
  #started = false;
  #generation = 0;
  #syncing = false;
  #publishScheduled = false;
  #sceneKey = "";
  #baseScene?:SemanticScene;
  #sceneOwners=new Set<string>();
  #sceneContributions=new Map<string,{stateVersion:number;truth:SemanticScene['truth'];diagnostics:string[]}>();
  #pending: Array<{id:string; update:JsonValue | ((previous:JsonValue)=>JsonValue); resolve:()=>void; reject:(error:unknown)=>void}> = [];
  constructor(state: VisualSessionState, transport: VisualEngineTransport) {
    validateSession(state);this.#snapshot = freezeSnapshot({ state:structuredClone(state), ready: false }); this.transport = transport;
  }
  getSnapshot = (): VisualClientSnapshot => this.#snapshot;
  get supportsPixelCapture():boolean{return this.transport.pixelCapture===true;}
  get supportsEvidenceCuts():boolean{return this.transport.evidenceCuts===true;}
  capturePixels():Promise<Record<string,unknown>>{
    return this.#enqueue(()=>this.#request("capture.pixels"));
  }
  analyticalRequest = (request:Record<string,unknown>):Promise<Record<string,unknown>> => this.transport.request({...request,revision:this.#snapshot.state.revision,viewKey:this.#snapshot.state.viewKey});
  commitEvidence(id:string,value:unknown):Promise<string|undefined>{
    return this.#enqueue(async()=>{
      if(this.#snapshot.state.replay)return undefined;
      if(!this.#snapshot.ready)await this.#attach();
      const answer=await this.#request("evidence.put",{value});
      const digest=String(answer.digest);
      const state=this.#snapshot.state;
      if(state.replay)return undefined;
      if(state.values[id]!==digest)await this.#request("act",{action:{id:crypto.randomUUID(),kind:"presentation.patch",expectedStateVersion:state.stateVersion,payload:{values:{[id]:digest}}}});
      return digest;
    });
  }
  subscribe = (listener: () => void): (() => void) => { this.#listeners.add(listener); return () => this.#listeners.delete(listener); };
  async start(): Promise<void> {
    if (this.#started) return;
    this.#started = true; this.#disposed = false;
    const generation = ++this.#generation;
    try {
      await this.#attach();
      if (this.#disposed || generation !== this.#generation) return;
      this.#unsubscribe = this.transport.subscribe?.(() => { void this.sync(); });
      // Bounded recovery probe covers missed native events/reconnection.
      this.#timer = setInterval(() => { void this.sync(); }, 5000);
    } catch (error) { if (generation === this.#generation) this.#fail(error); }
  }
  register(control: VisualControl, initial: JsonValue): void {
    this.registerMany([{control,initial}]);
  }
  /** Validate the entire inventory before installing any member. An invalid
   * sandbox batch must not leave a partially registered presentation API. */
  registerMany(entries:ReadonlyArray<{control:VisualControl;initial:JsonValue}>):void{
    const next=new Map(this.#controls);
    for(const {control,initial} of entries){
      assertJson(control);validateControl(control,initial);
      if(!control.id?.trim())throw new Error("Visual control identity required");
      const prior=next.get(control.id);
      if(prior&&stableSerialize(prior.control)!==stableSerialize(control))throw new Error(`Conflicting visual control ${control.id}`);
      if(!prior)next.set(control.id,{control:structuredClone(control),initial:structuredClone(initial)});
    }
    if(next.size>512)throw new Error("Too many visual controls");
    if(next.size===this.#controls.size)return;
    this.#controls=next;
    if(this.#started)this.#update({ready:false});
    if (this.#started && !this.#publishScheduled) {
      this.#publishScheduled = true;
      queueMicrotask(() => { this.#publishScheduled = false; void this.#enqueue(() => this.#attach()).catch(() => {}); });
    }
  }
  set(id: string, update: JsonValue | ((previous: JsonValue) => JsonValue)): Promise<void> {
    return new Promise((resolve,reject) => {
      this.#pending.push({id,update,resolve,reject});
      if(this.#pending.length===1) queueMicrotask(() => this.#flush());
    });
  }
  dispatch(action:VisualAction):Promise<Record<string,unknown>>{
    return this.#enqueue(()=>this.#request("act",{action}));
  }
  playbackTick(clock:string, playingControl:string, intervalMs:number, project:(values:Readonly<Record<string,JsonValue>>)=>Record<string,JsonValue>):Promise<void>{
    return this.#enqueue(async()=>{
      const state=this.#snapshot.state;
      if(!this.#snapshot.ready || state.replay || state.values[playingControl]!==true)return;
      const values=project(state.values);
      if(!Object.keys(values).length)return;
      for(const [id,value] of Object.entries(values)){
        const control=this.#controls.get(id)?.control;
        if(!control)throw new Error(`Unregistered playback control ${id}`);
        validateControl(control,value);
      }
      await this.#request("playback.tick",{clock,playingControl,intervalMs,
        action:{id:crypto.randomUUID(),kind:"presentation.patch",expectedStateVersion:state.stateVersion,payload:{values}}});
    });
  }
  #flush(): void {
    const batch = this.#pending.splice(0);
    void this.#enqueue(async () => {
      if(!this.#snapshot.ready) await this.#attach();
      const state=this.#snapshot.state, values:Record<string,JsonValue>={};
      for(const {id,update} of batch){
        const known=this.#controls.get(id); if(!known)throw new Error(`Unregistered visual control ${id}`);
        const previous=Object.hasOwn(values,id)?values[id]!:Object.hasOwn(state.values,id)?state.values[id]!:known.initial;
        const value=typeof update==="function"?update(previous):update;
        validateControl(known.control,value); values[id]=value;
      }
      if(Object.entries(values).every(([id,value])=>stableSerialize(state.values[id])===stableSerialize(value)))return;
      await this.#request("act",{action:{id:crypto.randomUUID(),kind:"presentation.patch",payload:{values},expectedStateVersion:state.stateVersion}});
    }).then(()=>batch.forEach(item=>item.resolve()),error=>batch.forEach(item=>item.reject(error)));
  }
  async sync(): Promise<void> {
    if (this.#syncing || this.#disposed || !this.#snapshot.ready) return;
    this.#syncing = true;
    try { await this.#request("inspect"); } catch (error) { this.#fail(error); }
    finally { this.#syncing = false; }
  }
  publishSceneContribution(owner:string,stateVersion:number,contribution:{truth:SemanticScene['truth'];diagnostics?:string[]}):void{
    if(!/^[a-zA-Z0-9_-]{1,64}$/.test(owner))throw new Error("Invalid scene contributor identity");
    if(!this.#sceneOwners.has(owner)&&this.#sceneOwners.size>=16)throw new Error("Too many scene contributors");
    if(stateVersion!==this.#snapshot.state.stateVersion)return;
    assertJson(contribution);
    if(new TextEncoder().encode(JSON.stringify(contribution)).length>65_536)throw new Error("Scene contribution exceeds 64 KiB");
    if(!contribution.truth||Array.isArray(contribution.truth)||typeof contribution.truth!=="object"
      ||contribution.diagnostics?.some(message=>typeof message!=="string"))throw new Error("Invalid scene contribution");
    this.#sceneOwners.add(owner);
    this.#sceneContributions.set(owner,{stateVersion,truth:structuredClone(contribution.truth),diagnostics:[...(contribution.diagnostics??[])]});
    const state=this.#snapshot.state;
    this.publishScene(this.#baseScene?.stateVersion===stateVersion?this.#baseScene:state.scene??{
      visualId:state.visualId,revision:state.revision,stateVersion,clocks:{},landmarks:[],selection:{members:[]},truth:{},diagnostics:[]
    });
  }
  publishScene(scene: SemanticScene): void {
    if (!this.#snapshot.ready || scene.stateVersion !== this.#snapshot.state.stateVersion) return;
    // One canonical scene owns both domain landmarks and generic controls.
    // Without this normalization, a family publisher and generic chrome can
    // continually overwrite each other at the SAME state version, starving
    // queued human actions behind an unbounded stream of scene publications.
    const state=this.#snapshot.state;
    this.#baseScene=scene;
    const ownedTruth=(key:string)=>[...this.#sceneOwners].some(owner=>key.startsWith(owner+'.'));
    const truth=Object.fromEntries(Object.entries(scene.truth).filter(([key])=>!ownedTruth(key)));
    const diagnostics=(scene.diagnostics??[]).filter(message=>![...this.#sceneOwners].some(owner=>message.startsWith(`[${owner}] `)));
    for(const [owner,contribution] of this.#sceneContributions){
      if(contribution.stateVersion!==state.stateVersion)continue;
      for(const [key,value] of Object.entries(contribution.truth))truth[`${owner}.${key}`]=value;
      diagnostics.push(...contribution.diagnostics.map(message=>`[${owner}] ${message}`));
    }
    const controlClocks=new Set(state.controls.flatMap(control=>control.clock?[control.clock]:[]));
    scene={...scene,truth,diagnostics,
      clocks:{...Object.fromEntries(Object.entries(scene.clocks).filter(([key])=>!controlClocks.has(key))),...Object.fromEntries(state.controls.filter(control=>control.clock&&(typeof state.values[control.id]==="number"||typeof state.values[control.id]==="string")).map(control=>[control.clock!,{domain:control.clock!,value:state.values[control.id] as number|string}]))},
      landmarks:[...scene.landmarks.filter(item=>item.ref.kind!=="control"),...state.controls.map(control=>({ref:{kind:"control",id:control.id},role:control.type,label:control.label,actions:["presentation.set"]}))],
      availableActions:[...(scene.availableActions??[]).filter(action=>!["presentation.set","presentation.patch"].includes(action.kind)),{kind:"presentation.set",tier:"presentation",idempotent:true},{kind:"presentation.patch",tier:"presentation",idempotent:true}],
    };
    const key = stableSerialize(scene);
    if (key === this.#sceneKey) return;
    this.#sceneKey = key;
    void this.#enqueue(async () => {
      if (scene.stateVersion !== this.#snapshot.state.stateVersion) return;
      await this.#request("publish", { expectedStateVersion: scene.stateVersion, scene });
    }).catch(() => { this.#sceneKey = ""; });
  }
  async capture(): Promise<SessionCheckpoint> {
    return this.#enqueue(async () => (await this.#request("capture", { expectedStateVersion: this.#snapshot.state.stateVersion })).checkpoint as SessionCheckpoint);
  }
  restore(id: string): Promise<void> {
    return this.#enqueue(async () => { await this.#request("restore", { checkpointId: id, expectedStateVersion: this.#snapshot.state.stateVersion }); });
  }
  seekRecording(id: string, sequence: number): Promise<void> {
    return this.#enqueue(async () => { await this.#request("record.seek", { recordingId: id, sequence, expectedStateVersion: this.#snapshot.state.stateVersion }); });
  }
  playRecording(playing:boolean,intervalMs?:number):Promise<void>{
    return this.#enqueue(async()=>{await this.#request("record.play",{playing,...(intervalMs===undefined?{}:{intervalMs}),expectedStateVersion:this.#snapshot.state.stateVersion});});
  }
  tickRecording():Promise<void>{
    return this.#enqueue(async()=>{
      const replay=this.#snapshot.state.replay;
      if(!replay || !("recordingId" in replay) || !replay.playing)return;
      await this.#request("record.tick",{clock:"recording",intervalMs:replay.intervalMs??300,expectedStateVersion:this.#snapshot.state.stateVersion});
    });
  }
  async startRecording(): Promise<void> {
    await this.#enqueue(async () => { const answer = await this.#request("record.start"); this.#update({ recordingId: String(answer.recordingId) }); });
  }
  async stopRecording(): Promise<void> {
    await this.#enqueue(async () => { await this.#request("record.stop"); this.#update({ recordingId: undefined }); });
  }
  async checkpoints(): Promise<Array<Pick<SessionCheckpoint,"id"|"capturedAt"|"digest">>> { return (await this.#request("checkpoints")).items as Array<Pick<SessionCheckpoint,"id"|"capturedAt"|"digest">>; }
  async readCheckpoint(id:string):Promise<SessionCheckpoint>{
    const saved=(await this.#request("checkpoint.read",{checkpointId:id})).checkpoint as SessionCheckpoint;
    await verifyCheckpoint(saved);return saved;
  }
  async importCheckpoint(saved:SessionCheckpoint):Promise<SessionCheckpoint>{
    await verifyCheckpoint(saved);
    return (await this.#request("checkpoint.import",{checkpoint:saved})).checkpoint as SessionCheckpoint;
  }
  async importRecording(recording:SessionRecording):Promise<string>{
    await replayRecording(recording);
    return String((await this.#request("record.import",{recording})).recordingId);
  }
  async recordings(): Promise<Array<{ id: string; createdAt: string; endedAt?: string; eventCount:number }>> { return (await this.#request("recordings")).items as Array<{ id: string; createdAt: string; endedAt?: string; eventCount:number }>; }
  async recording(id: string): Promise<SessionRecording> {
    let after = 0; let result: SessionRecording | undefined;let bytes=0;
    for (;;) {
      const response=await this.#request("record.read", { recordingId: id, after, limit: 100 });
      const page = response.recording as SessionRecording;
      bytes+=JSON.stringify(page).length;
      if(bytes>16_777_216)throw new Error("Recording exceeds 16 MiB in-memory limit; use paginated record.read");
      result ??= { ...page, events: [] };
      if (result.events.length + page.events.length > 10000) throw new Error("Recording exceeds interactive replay limit; use paginated record.read");
      result.events.push(...page.events);
      if (response.hasMore===false || response.hasMore===undefined&&page.events.length<100) return result;
      if(page.events.length===0)throw new Error("Recording pagination made no progress");
      after = page.events.at(-1)!.sequence;
    }
  }
  dispose(): void { this.#disposed = true; this.#started = false; this.#generation++; this.#unsubscribe?.(); this.#unsubscribe = undefined; clearInterval(this.#timer); this.#timer = undefined; this.#listeners.clear(); }
  async #attach(): Promise<void> {
    if (this.#disposed) return;
    await this.#request("attach", { definition: this.#snapshot.state.definition,
      controls: [...this.#controls.values()].map((item) => item.control),
      defaults: Object.fromEntries([...this.#controls].map(([id, item]) => [id, item.initial])),
    });
  }
  async #request(operation: string, fields: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    const { revision, viewKey } = this.#snapshot.state;
    const generation = this.#generation;
    let answer:Record<string,unknown>;
    for(let retry=0;;retry++){
      if(this.#disposed||generation!==this.#generation)throw new Error("Visual session request cancelled");
      try{answer=await this.transport.request({ operation, revision, viewKey, ...fields });break;}
      catch(error){
        if(this.#disposed||retry>=150||!transportError(error).includes("capture in progress"))throw error;
        // Capture is a short read barrier, not a persistence failure. Preserve
        // the queued intent; its expected version still applies after release.
        await new Promise(resolve=>setTimeout(resolve,200));
      }
    }
    if (this.#disposed || generation !== this.#generation) return answer;
    if (answer.state) {
      const state = answer.state as VisualSessionState; validateSession(state);
      const current = this.#snapshot.state;
      if (state.visualId !== current.visualId || state.revision !== current.revision || state.viewKey !== current.viewKey) throw new Error("Visual transport returned the wrong session");
      if(stableSerialize(state.definition)!==stableSerialize(current.definition))throw new Error("Visual transport returned the wrong definition version");
      if (state.stateVersion >= current.stateVersion) {
        const missing=[...this.#controls].filter(([id])=>!state.controls.some(control=>control.id===id));
        const conflict=[...this.#controls].find(([id,expected])=>{
          const registered=state.controls.find(control=>control.id===id);
          return registered&&stableSerialize(registered)!==stableSerialize(expected.control);
        });
        if(conflict){
          const message=`Visual control schema changed for ${conflict[0]}; open a compatible visual revision`;
          this.#update({ready:false,error:message});throw new Error(message);
        }
        const patch: Partial<VisualClientSnapshot> = { state, ready: missing.length===0, error: undefined };
        if (Object.hasOwn(answer, "activeRecording")) patch.recordingId = typeof answer.activeRecording === "string" ? answer.activeRecording : undefined;
        this.#update(patch);
        // Older checkpoints can predate a currently mounted detail control.
        // Reconcile the inventory without overwriting restored values.
        if(operation!=="attach"&&!this.#publishScheduled&&missing.length){
          this.#publishScheduled=true;
          queueMicrotask(()=>{this.#publishScheduled=false;void this.#enqueue(()=>this.#attach()).catch(()=>{});});
        }
      }
    }
    return answer;
  }
  #enqueue<T>(work: () => Promise<T>): Promise<T> {
    const next = this.#queue.then(() => { if (this.#disposed) throw new Error("Visual session disposed"); return work(); });
    this.#queue = next.catch((error) => { this.#fail(error); });
    return next;
  }
  #fail(error: unknown): void { if (!this.#disposed) this.#update({ error: transportError(error) }); }
  #update(patch: Partial<VisualClientSnapshot>): void {
    const next = { ...this.#snapshot, ...patch };
    if (stableSerialize(next) === stableSerialize(this.#snapshot)) return;
    this.#snapshot = freezeSnapshot(next); for (const listener of this.#listeners) listener();
  }
}

export function createVisualClient(input: Parameters<typeof initialSession>[0], definition: Parameters<typeof initialSession>[1], transport: VisualEngineTransport): VisualSessionClient {
  assertJson(input); return new VisualSessionClient(initialSession(input, definition), transport);
}
