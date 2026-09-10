import { useMemo, useRef, useState } from "react";
import type { AggregateBucket, CohortRef, SamplingStrategy, SemanticLandmark, SemanticScene, VisualRecording, VisualSnapshot } from "@synth/visuals-protocol";
import { VisualExplorationSession } from "@synth/visuals-sdk";
import { useSwarmProjection } from "@synth/workshop-visuals/swarmProjection";
import { SemanticSceneProvider, SemanticTarget, useVisualState, useVisualSessionClient, useVisualSessionSnapshot, usePublishVisualScene } from "@synth/visuals-react";
import {
  createTrajectoryCorpus,
  generateTrajectoryFixture,
  sampleLabel,
  swarmQuery,
  type AgentTrajectory,
  type RewardBand,
  type SwarmFilters,
  type TrajectoryBehavior,
  type TrajectoryOutcome,
} from "@synth/workshop-visuals";
import "./style.css";
import { swarmViewSchema, swarmHistorySchema } from "../../../runtime/presentationSchemas.ts";
import {RecordedCollectionExplorer} from "../../../components/RecordedCollectionExplorer.tsx";

type Props = {
  run?:{id?:string};
  title?: string;
  trajectories?: AgentTrajectory[];
  data?: AgentTrajectory[];
  visualId?: string;
  revision?: number | null;
  fixture?: boolean;
  visualState?: {
    putSnapshot(snapshot: VisualSnapshot): Promise<VisualSnapshot>;
    putRecording(recording: VisualRecording): Promise<VisualRecording>;
  };
};

type ViewState = { filters: SwarmFilters; label: string };
type HistoryEntry = { view: ViewState; strategy: SamplingStrategy; selectedId?: string; eventIndex: number };

const STRATEGIES: SamplingStrategy[] = ["representative", "diverse", "failure", "boundary", "outlier", "random"];

function validRows(value: unknown): value is AgentTrajectory[] {
  return Array.isArray(value) && value.every((row) => row && typeof row === "object" && typeof row.id === "string"
    && typeof row.model === "string" && Number.isFinite(row.reward) && Array.isArray(row.behaviors) && Array.isArray(row.events)
    && row.events.every((event: AgentTrajectory["events"][number]) => typeof event.id === "string" && Number.isFinite(event.step) && typeof event.kind === "string" && typeof event.summary === "string"));
}

function pct(count: number, denominator: number): string {
  return denominator ? `${((count / denominator) * 100).toFixed(1)}%` : "0.0%";
}

function Bar({ bucket, max, onSelect }: { bucket: AggregateBucket; max: number; onSelect: () => void }) {
  return <button className="swarm-bar" onClick={onSelect} aria-label={`${bucket.key}: ${bucket.count} of ${bucket.denominator}`}>
    <span className="swarm-bar__label">{bucket.key}</span>
    <span className="swarm-bar__track"><span style={{ width: `${Math.max(2, bucket.count / Math.max(1, max) * 100)}%` }} /></span>
    <span className="swarm-bar__value">{bucket.count} · {pct(bucket.count, bucket.denominator)}</span>
  </button>;
}

export function Shell(props: Props) {
  return props.run?.id?<RecordedCollectionExplorer runId={props.run.id} title={props.title}/>:<TrajectoryShell {...props}/>;
}

function TrajectoryShell(props: Props) {
  const sharedClient=useVisualSessionClient();
  const sharedState=useVisualSessionSnapshot();
  const rows = useMemo(() => {
    const input:unknown=props.trajectories??props.data;
    if(input!==undefined){if(!validRows(input))throw new Error(`Invalid trajectory binding; expected complete agent-trajectory records (received ${Array.isArray(input)?`array of ${input.length} rows`:input===null?'null':typeof input})`);return input;}
    return props.fixture===true?generateTrajectoryFixture():[];
  }, [props.trajectories, props.data,props.fixture]);
  const corpus = useMemo(() => createTrajectoryCorpus(rows, "workshop:swarm-explorer:v1"), [rows]);
  const [source,setSource]=useVisualState("swarm.source",corpus.ref,{required:["id","revision","schema","count"],properties:{id:{type:"string"},revision:{type:"string"},schema:{type:"string"},count:{type:"number",minimum:0}}});
  const [view, setView] = useVisualState<ViewState>("swarm.view", { filters: {}, label: "All trajectories" },swarmViewSchema);
  const [history, setHistory] = useVisualState<HistoryEntry[]>("swarm.history", [],swarmHistorySchema);
  const [strategy, setStrategy] = useVisualState<SamplingStrategy>("swarm.sampling", "representative",{options:STRATEGIES});
  const [selectedId, setSelectedId] = useVisualState<string|undefined>("swarm.trajectory",undefined);
  const [eventIndex, setEventIndex] = useVisualState("swarm.eventIndex",0,{minimum:0,clock:"trajectory.eventIndex"});
  const [recording, setRecording] = useState(false);
  const [snapshotId, setSnapshotId] = useState<string>();
  const [persistenceError, setPersistenceError] = useState<string>();
  const query = useMemo(() => swarmQuery(view.filters), [view]);
  const parentCohort = useMemo(()=>history.reduce<CohortRef|undefined>((parent,item)=>corpus.cohort(item.view.label,swarmQuery(item.view.filters),parent),undefined),[history,corpus]);
  const localCohort = useMemo(() => corpus.cohort(view.label, query, parentCohort), [corpus, query, view.label, parentCohort]);
  const remote=useSwarmProjection({client:sharedClient,source,current:corpus.ref,rows,filters:view.filters,label:view.label,history:history.map(item=>item.view),strategy,selectedId});
  const cohort=remote?.projection?.cohort??localCohort;
  const sessionRef = useRef<VisualExplorationSession | undefined>(undefined);
  if (!sessionRef.current
    || sessionRef.current.corpus.revision !== corpus.ref.revision
    || sessionRef.current.visualId !== (props.visualId ?? "analysis.swarm_trajectories.v1")
    || sessionRef.current.revision !== (props.revision ?? 1)) {
    const root = corpus.cohort("All trajectories", swarmQuery());
    sessionRef.current = new VisualExplorationSession({ visualId: props.visualId ?? "analysis.swarm_trajectories.v1", revision: props.revision ?? 1, corpus: corpus.ref, rootCohort: root });
  }
  const session = sessionRef.current;
  const localOutcomes = useMemo(() => sharedClient?undefined:corpus.aggregate(localCohort, "outcome"), [corpus, localCohort,sharedClient]);
  const localBehaviors = useMemo(() => sharedClient?undefined:corpus.aggregate(localCohort, "behaviors"), [corpus, localCohort,sharedClient]);
  const localModels = useMemo(() => sharedClient?undefined:corpus.aggregate(localCohort, "model"), [corpus, localCohort,sharedClient]);
  const localRewards = useMemo(() => sharedClient?undefined:corpus.aggregate(localCohort, "rewardBand"), [corpus, localCohort,sharedClient]);
  const localSample = useMemo(() => sharedClient?undefined:corpus.sample(localCohort, strategy, 8, { seed: 10, scoreField: "reward", failureField: "failed" }), [corpus, localCohort, strategy,sharedClient]);
  const outcomes=remote?.projection?.outcomes??localOutcomes;
  const behaviors=remote?.projection?.behaviors??localBehaviors;
  const models=remote?.projection?.models??localModels;
  const rewards=remote?.projection?.rewards??localRewards;
  const sample=remote?.projection?.sample??localSample;
  const selected = sharedClient ? remote?.projection?.selected ?? sample?.rows[0] : rows.find((row) => row.id === selectedId) ?? sample?.rows[0];
  const selectedEvent = selected?.events[Math.min(eventIndex, Math.max(0, selected.events.length - 1))];
  const rootCount = sharedClient?source.count:corpus.ref.count;

  const scene: SemanticScene = {
    visualId: props.visualId ?? "analysis.swarm_trajectories.v1",
    revision: props.revision ?? 1,
    stateVersion: sharedState?.state.stateVersion ?? session.stateVersion,
    clocks: selectedEvent ? { trajectory_step: { domain: "trajectory_step", value: selectedEvent.step } } : {},
    selection: selected ? { primary: { kind: "trajectory", id: selected.id }, members: [{ kind: "trajectory", id: selected.id }] } : { members: [] },
    landmarks: [
      { ref: { kind: "corpus", id: source.id }, role: "region", label: `${rootCount} trajectory corpus`, actions: ["corpus.query", "capture"] },
      { ref: { kind: "cohort", id: cohort.id }, role: "region", label: `${cohort.name}: ${cohort.count} trajectories`, actions: ["query", "drill_down", "back"] },
      ...(sample?.rows??[]).map((row): SemanticLandmark => ({ ref: { kind: "trajectory", id: row.id }, role: "button", label: `${row.id}, ${row.outcome}, reward ${row.reward}`, actions: ["select", "drill_down"] })),
    ],
    truth: {
      corpus_count: { state: "observed", value: rootCount },
      cohort_count: { state: "observed", value: cohort.count },
      prevalence: { state: "observed", value: rootCount ? cohort.count / rootCount : 0 },
    },
    diagnostics: rows === props.trajectories || rows === props.data ? [] : [props.fixture===true?"Using explicitly selected deterministic fixture.":"No trajectory binding. Bind recorded trajectories or explicitly select fixture mode."],
    evidenceGraph: {
      nodes: [{
        id: source.id,
        kind: rows === props.trajectories || rows === props.data ? "bound_trajectory_corpus" : "bundled_fixture",
        schema: source.schema,
        schemaVersion: "1",
        authority: rows === props.trajectories || rows === props.data ? "untrusted" : "derived",
        freshness: source.revision===corpus.ref.revision?"current":"stale",
        completeness: "complete",
        exactness: "exact",
      }],
      edges: [],
    },
  };
  usePublishVisualScene(sharedState && remote?.projection ? scene : undefined);

  function drill(filters: SwarmFilters, label: string) {
    if(history.length>=16){setPersistenceError("History limit reached. Return to a prior cohort before drilling further.");return;}
    const nextFilters = { ...view.filters, ...filters };
    const nextCohort = corpus.cohort(label, swarmQuery(nextFilters), cohort);
    setHistory((current) => [...current, { view, strategy, ...(selectedId===undefined?{}:{selectedId}), eventIndex }]);
    setView({ filters: nextFilters, label });
    setSelectedId(undefined);
    setEventIndex(0);
    if(!sharedClient)session.dispatch({ id: crypto.randomUUID(), kind: "drill_down", target: { kind: "cohort", id: nextCohort.id }, expectedStateVersion: session.stateVersion }, { cohort: nextCohort, target: { kind: "cohort", id: nextCohort.id } });
  }

  function back() {
    const previous = history.at(-1);
    if (!previous) return;
    setView(previous.view);
    setStrategy(previous.strategy);
    setSelectedId(previous.selectedId);
    setEventIndex(previous.eventIndex);
    setHistory((current) => current.slice(0, -1));
    if(!sharedClient && parentCohort)session.dispatch({ id: crypto.randomUUID(), kind: "back", target: { kind: "cohort", id: parentCohort.id }, expectedStateVersion: session.stateVersion });
  }

  function selectTrajectory(row: AgentTrajectory) {
    setSelectedId(row.id);
    setEventIndex(0);
    if(!sharedClient)session.dispatch({ id: crypto.randomUUID(), kind: "select", target: { kind: "trajectory", id: row.id }, expectedStateVersion: session.stateVersion });
  }

  function seekTrajectory(index: number) {
    setEventIndex(index);
    const event = selected?.events[index];
    if (!sharedClient && selected && event) session.dispatch({
      id: crypto.randomUUID(),
      kind: "seek",
      target: { kind: "trajectory_event", id: event.id, parent: { kind: "trajectory", id: selected.id } },
      payload: { clockDomain: "trajectory_step", value: event.step },
      expectedStateVersion: session.stateVersion,
    });
  }

  function toggleRecording() {
    if (recording) {
      const stopped = session.stopRecording();
      if (stopped && props.visualState) void props.visualState.putRecording(stopped).then(() => setPersistenceError(undefined)).catch((reason) => setPersistenceError(reason instanceof Error ? reason.message : String(reason)));
    } else session.startRecording();
    setRecording(!recording);
  }

  function capture() {
    const snapshot = session.snapshot([], { scene });
    setSnapshotId(snapshot.id);
    if (props.visualState) void props.visualState.putSnapshot(snapshot).then(() => setPersistenceError(undefined)).catch((reason) => setPersistenceError(reason instanceof Error ? reason.message : String(reason)));
  }

  function resetToCorpus() {
    const rootCohort = corpus.cohort("All trajectories", swarmQuery());
    if(!sharedClient)session.dispatch({ id: crypto.randomUUID(), kind: "back", target: { kind: "cohort", id: rootCohort.id }, expectedStateVersion: session.stateVersion });
    setView({ filters: {}, label: "All trajectories" });
    setHistory([]);
    setSelectedId(undefined);
    setEventIndex(0);
  }

  if(!outcomes||!behaviors||!models||!rewards||!sample)return <main className="swarm-explorer" data-visual-capture-blocked="true" aria-busy={!remote?.error}><p role={remote?.error?"alert":"status"}>{remote?.error??"Resolving the pinned trajectory corpus…"}</p></main>;
  const outcomeMax = Math.max(1, ...outcomes.buckets.map((bucket) => bucket.count));
  const behaviorMax = Math.max(1, ...behaviors.buckets.map((bucket) => bucket.count));
  const modelMax = Math.max(1, ...models.buckets.map((bucket) => bucket.count));
  const rewardMax = Math.max(1, ...rewards.buckets.map((bucket) => bucket.count));
  return <SemanticSceneProvider scene={scene} dispatch={(action) => session.dispatch(action)}>
    <main className="swarm-explorer" data-visual-observation="template"
      data-visual-transport-state="terminal" data-visual-terminal="true"
      data-visual-rollout-count={sample.rows.length} data-visual-rendered-frame-count={0}
      data-visual-semantic-event-count={selectedEvent?1:0} data-corpus-revision={source.revision}>
      <header className="swarm-header">
        <div>
          <span className="swarm-eyebrow">Swarm legibility · exact {sharedClient?"indexed":"local"} query</span>
          <h2>{props.title ?? "Agent trajectory explorer"}</h2>
          <p>Move from population patterns to exact events without losing denominators or lineage.</p>
        </div>
        {!sharedClient && <div className="swarm-actions">
          <button onClick={toggleRecording} aria-pressed={recording}>{recording ? "Stop recording" : "Record"}</button>
          <button onClick={capture}>Snapshot</button>
        </div>}
      </header>
      {source.revision!==corpus.ref.revision&&<p role="status">Showing a pinned corpus revision. <button onClick={()=>{setSource(corpus.ref);resetToCorpus();}}>Use latest corpus</button></p>}

      <nav className="swarm-lineage" aria-label="Exploration lineage">
        <button onClick={resetToCorpus} disabled={!history.length}>Corpus · {rootCount}</button>
        {history.map((item, index) => <span key={`${item.view.label}:${index}`}>› {item.view.label}</span>)}
        <strong>› {cohort.name}</strong>
      </nav>

      <section className="swarm-summary" aria-label="Cohort summary">
        <div><span>Current cohort</span><strong>{cohort.count}</strong></div>
        <div><span>Root corpus</span><strong>{rootCount}</strong></div>
        <div><span>Prevalence</span><strong>{pct(cohort.count, rootCount)}</strong></div>
        <div><span>Within parent</span><strong>{pct(cohort.count, cohort.denominator)}</strong></div>
        <div><span>Excluded</span><strong>{rootCount - cohort.count}</strong></div>
        <div><span>Evidence</span><strong>Exact · complete</strong></div>
      </section>

      {history.length ? <button className="swarm-back" onClick={back}>← Return to prior aggregate state</button> : null}
      {snapshotId ? <p className="swarm-receipt" role="status">Snapshot receipt <code>{snapshotId}</code></p> : null}
      {persistenceError ? <p className="swarm-receipt" role="alert">Could not persist visual state: {persistenceError}</p> : null}

      <div className="swarm-grid">
        <section className="swarm-card" aria-labelledby="outcome-heading">
          <h3 id="outcome-heading">Outcomes</h3>
          <p className="swarm-note">Counts use the current cohort denominator ({cohort.count}).</p>
          {outcomes.buckets.map((bucket) => <Bar key={bucket.key} bucket={bucket} max={outcomeMax} onSelect={() => drill({ outcome: bucket.key as TrajectoryOutcome }, `Outcome: ${bucket.key}`)} />)}
        </section>
        <section className="swarm-card" aria-labelledby="behavior-heading">
          <h3 id="behavior-heading">Derived behaviors</h3>
          <p className="swarm-note">Heuristic labels v1; trajectories may have multiple labels.</p>
          {behaviors.buckets.map((bucket) => <Bar key={bucket.key} bucket={bucket} max={behaviorMax} onSelect={() => drill({ behavior: bucket.key as TrajectoryBehavior }, `Behavior: ${bucket.key}`)} />)}
        </section>
        <section className="swarm-card" aria-labelledby="model-heading">
          <h3 id="model-heading">Models</h3>
          <p className="swarm-note">Select a model to retain it as a query predicate.</p>
          {models.buckets.map((bucket) => <Bar key={bucket.key} bucket={bucket} max={modelMax} onSelect={() => drill({ model: bucket.key }, `Model: ${bucket.key}`)} />)}
        </section>
        <section className="swarm-card" aria-labelledby="reward-heading">
          <h3 id="reward-heading">Reward distribution</h3>
          <p className="swarm-note">Versioned Workshop bands; raw reward remains available per trajectory.</p>
          {rewards.buckets.map((bucket) => <Bar key={bucket.key} bucket={bucket} max={rewardMax} onSelect={() => drill({ rewardBand: bucket.key as RewardBand }, `Reward: ${bucket.key}`)} />)}
        </section>
      </div>

      <section className="swarm-card swarm-samples" aria-labelledby="samples-heading">
        <div className="swarm-section-header">
          <div><h3 id="samples-heading">Concrete examples</h3><p className="swarm-note">Every sample carries a strategy and source-cohort receipt.</p></div>
          <label>Sampling
            <select value={strategy} onChange={(event) => setStrategy(event.target.value as SamplingStrategy)}>
              {STRATEGIES.map((value) => <option key={value} value={value}>{sampleLabel(value)}</option>)}
            </select>
          </label>
        </div>
        <div className="swarm-sample-grid">
          {sample.rows.map((row) => <SemanticTarget key={row.id} ref={{ kind: "trajectory", id: row.id }}>
            <button className={`swarm-sample ${selected?.id === row.id ? "is-selected" : ""}`} onClick={() => selectTrajectory(row)}>
              <strong>{row.id}</strong><span>{row.outcome} · reward {row.reward}</span><small>{row.behaviors.join(" · ")}</small>
            </button>
          </SemanticTarget>)}
        </div>
        <p className="swarm-receipt"><code>{sample.receipt.id}</code> · {sample.receipt.returned}/{sample.receipt.requested} from {cohort.count}</p>
      </section>

      {selected ? <SemanticTarget ref={{ kind: "trajectory", id: selected.id }} className="swarm-card swarm-detail">
        <div className="swarm-section-header"><div><h3>{selected.id}</h3><p className="swarm-note">{selected.outcome} · reward {selected.reward} · {selected.steps} steps · {selected.toolCalls} tool calls</p></div><span className="swarm-context">1 example from {cohort.count} · {pct(1, cohort.count)} of cohort</span></div>
        <label className="swarm-timeline">Logical trajectory step
          <input type="range" min="0" max={Math.max(0, selected.events.length - 1)} value={eventIndex} onChange={(event) => seekTrajectory(Number(event.target.value))} />
        </label>
        {selectedEvent ? <SemanticTarget ref={{ kind: "trajectory_event", id: selectedEvent.id, parent: { kind: "trajectory", id: selected.id } }} className="swarm-event">
          <span>Step {selectedEvent.step}</span><strong>{selectedEvent.kind}</strong><p>{selectedEvent.summary}</p>
        </SemanticTarget> : null}
      </SemanticTarget> : null}
    </main>
  </SemanticSceneProvider>;
}

export default Shell;
