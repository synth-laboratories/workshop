# Optimizer Workshop Final Plan

**Confirmed:** 2026-08-09  
**Scope:** make GEPA, GELO, and SFT durable first-class Workshop objects, with a real Craftax GPT-OSS SFT run as the first non-fixture SFT acceptance test.  
**Source plan:** `HANDOFF_FIRST_CLASS_OPTIMIZERS_GEPA_GELO_SFT.md`

## 1. Final product decision

Optimizers are a first-class product noun beside Traces, Containers, and Visuals.

An optimizer is not only a job row or chart. It is one durable object containing:

- stable run identity and lifecycle;
- algorithm and objective;
- input datasets, prompts, traces, and configuration;
- execution bindings, including cloud providers, containers, and local slots;
- replayable events and state slices;
- candidates, checkpoints, evaluations, usage, and artifacts;
- typed relationships to the objects it uses and produces;
- a linked visual that can be reopened after completion or restart.

Synth Cloud is authoritative for hosted execution. Workshop's Rust `CoreRuntime` owns the durable local mirror, event cursor, relationships, offline projection, and Visual Registry binding. React renders these projections and does not own optimizer state or call providers directly.

```text
 Chat / Agent               Optimizers home              Visuals vault
      |                           |                            |
      +-------------+-------------+----------------------------+
                    |
                    v
             optimizer_run.v1
                    |
       +------------+-------------+
       |            |             |
       v            v             v
    Events       State slices   Relationships
       |            |             |
       +------------+-------------+
                    |
                    v
        shared optimizer.run.v1 visual
          +---------+---------+
          |         |         |
         GEPA      GELO       SFT
```

## 2. Experience to ship

### Sidebar and home

`Optimizers` appears alongside the other durable inventory/research nouns. Its home supports search and filtering by status, algorithm, source, project, and recency. Rows show objective, status, progress, cost, execution binding, and last update.

### Chat

Starting or discovering a run creates one optimizer card and one linked visual card. Cards resolve the same `optimizer_run_id` and `visual_id` used everywhere else. Agents operate the object through the same Rust service as the UI.

### Inspector and visual

Selecting a run opens a useful inspector immediately and the dedicated visual in the existing right `VisualHost`. Shared chrome contains status, timeline, usage, artifacts, events, execution, and relationships. GEPA, GELO, and SFT add algorithm-specific overlays.

### Persistence

Closing and reopening Workshop restores the run, its last durable cursor, relationships, and linked visual. Live reconciliation must not duplicate events. Completed visuals retain a sealed, reproducible revision.

## 3. Shared architecture and contract

```text
 TypeScript / React
 cards | home | inspector | VisualHost | accessible projections
                         |
                  typed Tauri bridge
                         |
 Workshop Rust CoreRuntime
 OptimizerService | SQLite mirror | journal | cursor | MCP | Visual Registry
                 /                              \
                /                                \
       Synth Cloud authority                  Local slot
   scheduling | billing | events        bounded leased capability
                |
         algorithm workers
       GEPA | GELO | SFT/Tinker
```

The shared record is `optimizer_run.v1`. `algorithm_id` and slice IDs are forward-compatible strings rather than closed product enums. All algorithms use `optimizer_event.v1` with stable per-run sequence numbers, immutable resource IDs, bounded replay via `after_seq`, and secret-free payloads.

Common slices:

```text
run.summary       run.timeline       run.usage
run.logs          run.artifacts      run.execution
run.relationships
```

Algorithm slices:

```text
gepa.candidates       gepa.frontier          gepa.reflections
go-ex.board           go-ex.themes           go-ex.data_engine
sft.training_curves   sft.checkpoints        sft.checkpoint_evaluations
sft.dataset           sft.compute            sft.examples
```

Capabilities—not algorithm names—control whether cancel, pause, resume, checkpoint evaluation, inference, and local-slot binding are available.

Typed relationships include:

```text
optimizer --uses--------> dataset | prompt | trace | container | local_slot
optimizer --produces----> prompt | adapter | checkpoint | trace | report
optimizer --visualized_by> visual
optimizer --started_from-> chat/session
```

## 4. Shared visual

Use one `visuals/templates/optimizer.run.v1` shell with GEPA, GELO, and SFT overlays. Do not build three unrelated dashboards.

Required shared behavior:

- follow live events until the user scrubs;
- one global sequence/time control rewinds every panel consistently;
- reconnect from the last cursor with deduplication;
- accessible textual equivalents for every chart;
- direct links to typed traces and artifacts;
- the same visual opens from Chat, Optimizers home, and Visuals vault;
- completion seals a reproducible revision bound to artifact digests.

Algorithm overlays:

- **GEPA:** candidate lineage, Pareto frontier, reflections, rollout/eval evidence.
- **GELO:** phase/tick board, themes, checkpoint map, proposer/agent activity, acceptance and heldout results.
- **SFT:** training curves, dataset summary, checkpoint rail, selection evaluation matrix, per-example comparisons, compute, and model lineage.

Raw JSON remains available as a debug fallback, not as the primary artifact experience.

## 5. Confirmed implementation order

### Phase 0 — Complete the first-class noun

1. Finish and stabilize the versioned record, events, extensible slices, capabilities, and relationships.
2. Finish Rust storage, transactional cursor advancement, reconciliation, IPC, and the `synth-optimizers-mcp` service.
3. Provision an optimizer skill and MCP configuration in the same installed-app path as Containers and Visuals.
4. Finish Optimizers home, a substantive inspector, chat cards, and `optimizer_run` visual bindings.
5. Ensure UI and MCP mutations converge on `OptimizerService`.

**Gate:** one fixture run has one identity across home, chat, inspector, MCP, VisualHost, restart, and offline reopen.

### Phase 1 — GEPA proves the shared visual

1. Render a bounded GEPA fixture.
2. Consume a real local GEPA event stream.
3. Consume a hosted GEPA run through canonical replay/live APIs.
4. Prove candidate lineage, frontier, traces, historical scrub, reconnect, restart, and vault persistence.

**Gate:** no GEPA-specific network or event ownership exists in TypeScript.

### Phase 2 — Hosted GELO through a real local slot

1. Normalize `events.optimizer.jsonl` and state projections into the shared contract.
2. Create the hosted run from Workshop and lease a selected real local slot.
3. Surface slot health and lease state as execution metadata.
4. Prove disconnect/reconnect, cursor recovery, artifact publication, and no duplicate work.

**Gate:** a real Craftax GELO run is started, observed, interrupted/recovered, completed, and reopened from Workshop.

### Phase 3 — SFT compatibility and real Craftax bridge

1. Keep the synthetic SFT fixture for deterministic UI and replay tests.
2. Add generic training metric, checkpoint, dataset, evaluation, provider-operation, and model-lineage items.
3. Add a thin adapter to the real Craftax SFT script so it emits canonical `optimizer_event.v1` events and state slices.
4. Import/replay an existing completed Craftax run at zero compute cost.
5. Run a small live Craftax smoke and then the full GPT-OSS acceptance experiment below.

At this phase, a locally imported Craftax run proves the shared SFT object. It does not imply that hosted standalone SFT or its billing/control semantics are generally available.

**Gate:** the real run renders without fixture fallback, survives restart, and explains behavioral uplift even when scalar reward is flat.

### Phase 4 — Hosted standalone SFT with Tinker

1. Register hosted `algorithm_id = sft` behind honest feature availability.
2. Implement a provider-neutral `SftBackend`, with Tinker first.
3. Stream normalized training, checkpoint, evaluation, usage, and terminal events.
4. Enforce explicit `train`, `selection`, and `heldout` dataset roles.
5. Add capability-gated cancel/pause/resume and immutable adapter/checkpoint artifacts.
6. Add the OpenAI-compatible fine-tuning façade only as an adapter over the canonical optimizer run.

**Gate:** a real Tinker run is identical through canonical and compatibility APIs, and Cloud/Desktop agree on cursor, status, checkpoint, usage, and artifacts.

### Phase 5 — Installed-app hosted dogfood

Run the pinned Banking77 hosted SFT acceptance from the installed Workshop app, including real Tinker compute, checkpoint selection, a leased local evaluation slot, restart recovery, and CUA evidence.

Craftax is the earlier algorithm/UI truth test; Banking77 remains the final hosted platform and installed-app release gate.

## 6. Craftax GPT-OSS SFT acceptance test

### Experiment topology

```text
gpt-oss-120b via Groq
   strong champion prompt
            |
            v
 real Craftax rollouts on seeds 101-108
            |
            v
  observation -> JSON action demonstrations
            |
            v
        train.jsonl
            |
        Tinker LoRA SFT
            |
            v
 gpt-oss-20b + rank-32 adapter
            |
       +----+----+
       |         |
       v         v
 base 20b     SFT 20b
       |         |
       +----+----+
            |
 identical weak prompt + heldout seeds 501-506
            |
            v
 reward | achievements | steps | actions | artifacts
```

The 120B model is the demonstration teacher. The 20B model is the trained and evaluated student. The base and adapted students receive the same weak evaluation prompt, so measured improvement must come from the adapter rather than prompt text.

### Reproduction

Prerequisites:

```bash
set -a
source ~/Documents/GitHub/synth-ai/.env
set +a

cd ~/Documents/GitHub/gamebench/tasks/craftax-singleplayer/gold_rust
cargo build --release --bin craftax_gold
./target/release/craftax_gold --port 8098 --host 0.0.0.0
curl -sf http://127.0.0.1:8098/health
```

Full run:

```bash
cd ~/Documents/GitHub/optimizers-beta/.out/craftax_sft_uplift

~/Documents/GitHub/synth-ai/.venv/bin/python run_craftax_sft_uplift.py \
  --output-dir . \
  --collect-seeds 101,102,103,104,105,106,107,108 \
  --eval-seeds 501,502,503,504,505,506 \
  --train-steps 48 \
  --batch-size 4 \
  --rank 32 \
  --lr 1e-3
```

Required source artifacts:

```text
train.jsonl          collected demonstrations and source metadata
train_result.json    model/config plus immutable Tinker sampler/state paths
eval_summary.json    paired base/SFT heldout results and per-seed evidence
```

### Canonical event bridge

The current script's `collected`, `dataset`, `train_step`, `eval_seed`, and `summary` messages must be normalized into at least:

```text
optimizer.run.created
sft.dataset.collection_started
sft.dataset.example_collected
sft.dataset.validated
sft.training.started
sft.step.metrics
sft.adapter.saved
sft.base_eval.started
sft.eval.seed_completed
sft.base_eval.completed
sft.adapter_eval.started
sft.eval.seed_completed
sft.adapter_eval.completed
sft.heldout_eval.completed
optimizer.artifact.created
optimizer.run.completed | failed
```

Each event must contain a stable sequence number, run ID, algorithm ID, occurrence time, immutable artifact references where applicable, and enough absolute snapshot data for deterministic historical replay. Training-step events may be coalesced, but lifecycle, adapter-save, evaluation, artifact, and terminal events may not be dropped.

The adapter must also project:

```text
run.summary
run.timeline
run.usage
run.artifacts
run.execution
sft.dataset
sft.training_curves
sft.checkpoints
sft.checkpoint_evaluations
sft.examples
sft.compute
```

### Scientific acceptance

The known reference run contains 62 training rows and the following heldout behavior:

| Metric | Base GPT-OSS 20B | SFT GPT-OSS 20B | Interpretation |
| --- | ---: | ---: | --- |
| Mean progress reward | 0.67 | 0.67 | Saturated; not sufficient alone |
| Achievements per seed | 0.83 | 1.67 | Approximately 2x |
| Mean episode steps | 13.0 | 52.3 | Approximately 4x |

The visual must therefore treat reward, achievement count, survival/episode length, and per-seed behavior as separate facts. It must not label the run a failure merely because aggregate progress reward is flat.

Pass conditions:

- collection uses the LM-capable Craftax binary and records nonzero policy LLM turns;
- at least four valid SFT rows are available, with the full reference target of 62;
- training identifies `openai/gpt-oss-20b`, rank, step count, learning rate, sampler path, and state path;
- base and SFT use identical heldout seeds and the same weak prompt;
- heldout seeds never appear in training data;
- the visual pairs base and adapted evidence by seed;
- achievements and episode steps are primary outcome panels alongside reward;
- clicking a seed exposes actions and achievements for both arms;
- provenance shows teacher, student base model, adapter, dataset digest, configuration, and evaluation seed set;
- the completed run can be reopened after Workshop restarts;
- no API keys or signed provider URLs enter events, SQLite payloads, visual bindings, or evidence.

### Three test levels

```text
Replay fixture       Live smoke                Full acceptance
existing artifacts   2-3 collect/eval seeds    seeds 101-108 / 501-506
no provider spend    4 training steps          48 steps, rank 32
       |                    |                         |
       +--------------------+-------------------------+
                            |
                            v
                  same optimizer object path
```

Replay is the fast deterministic UI/restart test. Smoke proves live ingestion. Full acceptance proves the actual behavioral story and artifact lineage.

## 7. SFT visual acceptance

The SFT visual must match the operational baseline of a provider training dashboard and then add optimizer-specific evidence.

Operational baseline:

- status and lifecycle messages;
- base model, dataset, hyperparameters, progress, and errors;
- real training metrics rather than fixture curves;
- immutable adapter/checkpoint identities;
- usable output artifact and honest supported controls.

Workshop differentiators:

- paired per-seed baseline versus adapter evidence;
- separate selection and heldout measurement;
- dataset, trace, container/slot, and artifact relationships;
- global historical scrub across all panels;
- model lineage from teacher/data through base model and adapter;
- cost/compute evidence and durable offline reopen;
- agent/MCP inspection and operation through the same object.

## 8. Test and release gates

### Contract

- golden Rust/JSON fixtures for records, events, capabilities, relationships, and slices;
- unknown future algorithms/slices round-trip without data loss;
- zero metrics remain zero rather than becoming missing;
- replay/live overlap deduplicates;
- secret scanning covers events and visual bindings.

### Desktop

- Rust transaction commits event normalization and cursor advancement atomically;
- list/search/filter, inspector, chat card, visual binding, and restart tests;
- MCP and UI use the same service;
- installed-app provisioning includes the optimizer skill and MCP binary;
- Playwright/a11y covers deterministic fake-cloud and replay paths.

### Algorithm

- real GEPA local and hosted replay;
- real GELO event normalization and slot interruption;
- synthetic SFT fixture plus real Craftax replay, smoke, and full run;
- Tinker provider translation and train/selection/heldout enforcement.

### Final release

- no fixture-only SFT panel is presented as live product support;
- no algorithm-specific fallback displays another algorithm's fixture;
- no renderer-owned optimizer state or direct provider call;
- no separate SFT job database behind the compatibility API;
- Cloud and Desktop reconcile exactly after restart;
- installed-app Banking77 dogfood and evidence packet pass;
- the completed optimizer and linked visual remain useful offline.

## 9. Definition of done

The work is complete when an operator can discover, start, watch, inspect, compare, reopen, and hand an optimizer to an agent exactly as they can with Traces, Containers, and Visuals.

The staged proofs are:

```text
GEPA proves shared visualization and replay
  -> GELO proves hosted execution through a real local slot
  -> Craftax GPT-OSS SFT proves real training and behavioral evidence
  -> Hosted Tinker SFT proves provider abstraction and lifecycle
  -> Banking77 installed-app CUA proves the complete product
```

Craftax is successful only when Workshop makes the true result legible: flat scalar reward, materially better achievements, much longer episodes, and inspectable per-seed evidence tied to a real GPT-OSS adapter.
