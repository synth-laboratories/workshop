# Synth Desktop / Local Agent Workbench — Product + Architecture Handoff

Below is a full handoff you can give to an agent to combine with the local codebase/context and turn into a concrete first-pass implementation plan.

---

## 0. Purpose

Design a first-pass native desktop application for Synth that combines:

* local OSS model inference on Mac
* cloud agent execution
* Codex App Server compatibility
* ACP compatibility
* local and remote agent sessions through one UI
* LoRA-aware model serving
* first-class rollout inspection
* first-class visual artifacts and quantitative statistics
* versioned harnesses, prompts, code, models, adapters, environments, and renderers
* a benchmark/evaluation surface that makes the desktop app as easy to evaluate as a browser or CUA environment

The product should not be framed primarily as “another coding IDE.”

The stronger framing is:

> Synth Desktop is a local-first agent research and development workbench where agents can run locally or in Synth Cloud, and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

The product should make the loop:

**observe → understand → modify → evaluate → fine-tune → deploy**

feel native.

---

## 1. Core Product Thesis

The distinctive product is not the editor shell itself.

The center of gravity should be:

1. universal agent sessions
2. universal rollout representation
3. rich artifacts
4. standardized metrics
5. exact harness/version provenance
6. local/cloud execution parity
7. local fine-tuned model deployment
8. easy desktop-agent evaluation

The UI shell should support coding well, but avoid becoming a full VS Code/Cursor clone in V1.

A good V1 is closer to:

* Codex Desktop
* Claude Artifacts
* PostTrainBench-style trajectory inspection
* Synth eval visualizations
* local inference
* cloud execution

combined into one workbench.

---

## 2. High-Level Architecture

Do not put orchestration logic directly into Electron.

Use a separate local runtime daemon.

```text
                    Synth Desktop
                      Electron
                         │
        ┌────────────────┼─────────────────┐
        │                │                 │
     Agents            Runs            Artifacts
        │                │                 │
     Diffs           Metrics           Viewers
     Files          Rollouts          Reports
     Terminal       Compare           Charts
        └────────────────┬─────────────────┘
                         │
                  Synth Protocol
             bidirectional event stream
                         │
                Synth Runtime Daemon
                         │
        ┌────────────────┼────────────────┐
        │                │                │
      LOCAL           HYBRID            CLOUD
        │                │                │
 Local inference   local routing    Synth Cloud runtime
 local tools       remote workers   Codex instances
 local shell       cloud escalation GPU jobs
 local files                       long-running agents
```

The desktop should act as a client to the runtime.

The runtime should own:

* sessions
* turns
* agent lifecycle
* tool calls
* approvals
* files/workspaces
* checkpoints
* artifacts
* metrics
* rollout construction
* local inference
* cloud delegation
* model/adaptor selection
* persistence
* harness/version provenance

---

## 3. Local Model Stack

### 3.1 Laguna XS 2.1

Primary local heavyweight model.

Intended uses:

* local data agent
* coding
* repository exploration
* dataset analysis
* private/local-file reasoning
* longer reasoning
* visual analysis
* report generation
* local research

Preferred Mac inference path:

```text
Laguna XS 2.1
      ↓
MLX-compatible weights
      ↓
MLX inference runtime
      ↓
DFlash speculative decoding if compatible
      ↓
Metal / Apple GPU
```

The reason to favor MLX for Laguna-on-Mac is performance.

The recent Laguna XS 2.1 Mac optimization work has targeted approximately 200 tok/s-class decoding on high-end Apple Silicon using MLX-family inference and speculative decoding.

Do not blindly copy Magnitude’s llama.cpp backend if maximum Mac performance is the objective.

Magnitude is still useful as an architecture reference because it separates:

* inference control plane
* model lifecycle
* HTTP/OpenAI-compatible serving
* low-level backend

The desired equivalent is:

```text
Synth Local Inference Daemon
    │
    ├── model lifecycle
    ├── request scheduling
    ├── adapter selection
    ├── prefix/KV management
    ├── usage/timing
    ├── streaming
    └── MLX backend
             │
             ├── Laguna XS 2.1
             └── DFlash
```

Potential fallback backend:

```text
GGUF
 ↓
llama.cpp
 ↓
Metal
```

Useful for portability and broad LoRA support, but likely secondary to MLX on Mac.

### 3.2 Qwen ~4B Local Policy Model

Keep a smaller model resident as a local routing/control model.

Potential responsibilities:

* intent classification
* deciding which agent/model handles a task
* tool routing
* context filtering
* decomposition
* short/simple coding tasks
* deciding when Laguna is required
* deciding when cloud execution is warranted
* choosing local vs cloud
* potentially controlling subagents

Conceptually:

```text
user
 ↓
Qwen 4B policy
 ├── answer locally
 ├── use local tool
 ├── invoke Laguna
 └── invoke Synth Cloud / Codex
```

Think of Qwen as the low-latency local control plane.

Laguna is the larger local reasoning/data agent.

---

## 4. LoRA Support

LoRAs should be a first-class model primitive.

Do not represent a fine-tuned model only as a new opaque model name.

Represent:

```text
base model identity
+
adapter identity
```

Example request:

```text
model = laguna-xs-2.1
adapter = sha256:abc123
```

rather than:

```text
model = laguna-xs-2.1-my-finetune-v7
```

Desired model registry:

```text
Laguna XS 2.1
 ├── base
 ├── company-code.lora
 ├── data-agent.lora
 ├── customer-X.lora
 └── user-custom-r17.lora
```

Local inference should support:

* base model
* adapter selection
* adapter install
* adapter versioning
* preferably adapter hot-swapping
* adapter fusion as an optimization path
* quantized base + LoRA where practical

Important caveat:

DFlash speculative decoding may lose acceptance rate after significant target-model fine-tuning.

Correctness should remain unaffected, but speed may fall.

Therefore benchmark:

```text
base + DFlash
vs
base + LoRA + same DFlash
```

and potentially support:

* LoRA-specific speculator
* re-trained/re-aligned speculator
* speculative decoding disabled for incompatible adapters

---

## 5. Local / Cloud Execution Model

A major design goal is parity.

The same conceptual session should be executable:

```text
Local
Cloud
Hybrid
```

CLI examples:

```bash
synth run agent
synth run agent --cloud
```

Desktop:

```text
Run on:
○ Local
○ Synth Cloud
○ Auto
```

The UI should not need fundamentally separate logic.

Both local and cloud runtimes should emit the same domain events:

```text
Session
Turn
Message
ToolCall
Approval
Artifact
Metric
RolloutStep
Checkpoint
Outcome
```

Remote/cloud execution can include:

* Codex worker
* Synth sandbox
* long-running agent
* GPU worker
* browser environment
* RL/eval worker
* training job

Hybrid execution is especially interesting:

```text
Local Qwen policy
       ↓
    delegate
    ↙      ↘
Laguna      Cloud Codex
local       Synth runtime
```

---

## 6. Codex App Server Compatibility

Codex App Server should be treated as a compatibility protocol, not as the core internal architecture.

Desired shape:

```text
Synth Runtime
 ├── native Synth protocol
 ├── Codex App Server adapter
 └── ACP adapter
```

Why:

The Synth runtime needs richer domain concepts than Codex provides.

Codex compatibility is still valuable because it enables:

* Codex instances in Synth Cloud
* drop-in integration
* tooling reuse
* ecosystem compatibility
* possible compatibility with other Codex clients

Important runtime features that should map naturally:

* threads
* turns
* streaming events
* approvals
* tool-call notifications
* resumability
* interrupt/cancel
* workspaces
* diffs
* execution state

---

## 7. ACP Compatibility

Also support Agent Client Protocol.

ACP should be treated as another compatibility surface.

Potential use cases:

* expose Synth agents to external ACP clients
* invoke local ACP agents inside Synth Desktop
* connect Poolside/local agents
* make the runtime interoperable with agent-aware editors

Internal Synth protocol should remain richer.

Synth-specific concepts that likely exceed ACP:

* rollouts
* metrics
* checkpoints
* environment states
* renderers
* training artifacts
* LoRA versions
* harness revisions
* eval summaries
* cross-run comparison

---

## 8. Universal Artifact System

Artifacts should be a core protocol primitive.

Do not treat them as generic file attachments.

Example:

```typescript
Artifact {
  id: string
  type: string
  mimeType?: string

  title?: string
  sourceStepId?: string

  content?: unknown
  uri?: string

  renderer?: {
    kind: string
    config?: unknown
  }

  provenance: {
    runId: string
    harnessRevision?: string
    modelRevision?: string
    adapterRevision?: string
    codeRevision?: string
  }
}
```

Built-in artifact types should include:

```text
text
markdown
source_code

html
react_app
svg

image
gif
video

table
dataframe
json

timeseries
vega_lite
plotly

diff
terminal

environment_frame
environment_state

model_checkpoint
lora

report
```

Desired UI:

```text
┌──────────────────────┬──────────────────────────┐
│ Agent conversation   │ Artifact                 │
│                      │                          │
│ I analyzed run 482…  │ Craftax trajectory      │
│                      │                          │
│ > inspect failure    │ [interactive viewer]     │
│ > compare            │                          │
│ > rerun              │ reward 0.58              │
│                      │ health 7                  │
└──────────────────────┴──────────────────────────┘
```

This is inspired by Claude Artifacts, but extended to quantitative/agentic research outputs.

---

## 9. Universal Rollout Format

The rollout format should probably be the canonical artifact type.

Define a standard representation that works across:

* coding agents
* browser agents
* games/environments
* data agents
* RL
* post-training
* research agents
* multi-agent systems

Example:

```typescript
Rollout {
  id: string
  task: TaskRef
  seed?: number

  versions: VersionManifest

  steps: Step[]

  outcome?: Outcome
  metrics?: Metric[]
  artifacts?: Artifact[]
}

Step {
  index: number
  timestamp?: string

  observations?: Event[]
  messages?: Message[]
  actions?: Action[]

  artifacts?: Artifact[]
  metrics?: Metric[]

  stateRef?: string
}
```

Examples of how the same representation applies:

### Coding

```text
reasoning
shell command
diff
test result
```

### Craftax

```text
reasoning
action
environment frame
inventory
reward
health
```

### Browser

```text
reasoning
click
screenshot
DOM/accessibility state
reward
```

### Data Agent

```text
reasoning
python
dataframe
chart
metric
```

### RL / post-training

```text
prompt
completion
reward
advantage
logprob
KL
loss
checkpoint
```

---

## 10. Rollout Viewer

Build one excellent viewer rather than many one-off viewers.

Important capabilities:

* timeline
* step scrubber
* message/reasoning pane
* tool/action pane
* synchronized artifacts
* synchronized environment frames
* synchronized metrics
* event markers
* reward trajectory
* failure markers
* search
* filters
* jump-to-anomaly
* compare two runs
* compare two harness revisions
* aggregate statistics
* cohort comparison

Possible layout:

```text
┌──────────────────────────────────────────────┐
│ Rollout 482 | Harness r93 | reward 0.58      │
├──────────────┬───────────────────────────────┤
│ Timeline     │ Environment / Artifact        │
│              │                               │
│ step 31      │ [Craftax frame]               │
│ step 32      │                               │
│ step 33      │ Health: 7                     │
│ step 34 ●    │ Food: 4                       │
│ step 35      │ Reward: +0.1                  │
├──────────────┴───────────────────────────────┤
│ Reasoning / actions / tool output            │
└──────────────────────────────────────────────┘
```

This should become the standard viewer for Synth-produced trajectories.

---

## 11. Visuals + Statistics

Visuals and metrics should be emitted directly from environments/harnesses.

Example:

```python
step(action) -> {
    observation,
    reward,

    artifacts: [
        Frame(...),
        Inventory(...),
        Map(...)
    ],

    metrics: {
        "health": 7,
        "food": 4,
        "drink": 6,
        "achievement/wood": 1
    }
}
```

The desktop should automatically produce:

```text
timeline
frame scrubber
metric charts
reward chart
event markers
reasoning
actions
```

without requiring a bespoke frontend.

Custom renderer modules should still be allowed for advanced cases.

Example:

```text
craftax.viewer.tsx
```

But standardized artifact + metric rendering should handle most use cases.

Goal:

The quality of visuals currently seen in polished Synth eval pages should become a natural product output, not a manually built reporting layer.

---

## 12. Harness Versioning

Version the entire executable agent system.

Introduce a first-class Harness Revision.

Example:

```yaml
harness: craftax-research-agent

revision:
  git: 92ad18b

agent:
  entrypoint: ./agent.py

prompts:
  system: sha256:...
  planner: sha256:...

models:
  policy:
    base: qwen-4b
    adapter: sha256:...

  data_agent:
    base: laguna-xs-2.1
    adapter: sha256:...

tools:
  manifest: sha256:...

environment:
  image: sha256:...

dependencies:
  lockfile: sha256:...

renderers:
  craftax: sha256:...
```

A rollout should point to an exact Harness Revision.

Then comparison becomes:

```text
Run 419
  ↓
Harness r92

Run 482
  ↓
Harness r93
```

and the UI can display:

```diff
Harness r92 → r93

prompt/planner.md
- Explore carefully...
+ Prioritize acquiring wood...

policy.adapter
- sha256:8839...
+ sha256:b821...

src/planner.py
+12 -4

environment
unchanged
```

This is more than Git.

Git does not natively version:

* model adapters
* model identities
* prompt blobs
* container images
* environment schemas
* renderers
* tool manifests
* evaluation config

The product should.

---

## 13. Core Desktop Surfaces

Strong V1 surfaces:

### Agents

* session list
* local/cloud/hybrid
* streaming messages
* tool calls
* approvals
* diffs
* terminal
* files
* branch/worktree context

### Runs

* rollout viewer
* run list
* filter
* compare
* metrics
* outcomes
* traces

### Artifacts

* generated reports
* charts
* images
* HTML
* interactive apps
* environment frames
* tables
* dataframes
* code
* checkpoints

### Harnesses

* revisions
* prompts
* code
* model bindings
* adapter bindings
* environment version
* renderer version
* diffs
* provenance

### Models

* local base models
* adapters
* download/install
* quantization
* residency
* model status
* inference backend
* performance
* adapter activation

### Projects

* repo
* environment
* sessions
* runs
* harnesses
* artifacts
* local/cloud status

---

## 14. Keep V1 Out of Full-IDE Scope

Do not build a full IDE initially.

Include:

* repo/file browser
* basic code viewer/editor
* terminal
* diff viewer
* search
* command palette
* open in external editor

Support:

```text
Open in Cursor
Open in VS Code
Open in Zed
```

The differentiation is not editor widget completeness.

---

## 15. Accessibility / CUA / Playwright Evaluation

A major requirement:

Evaluating Synth Desktop should be almost as easy as evaluating a browser app.

Expose three parallel surfaces:

```text
                Synth Desktop
                     │
       ┌─────────────┼─────────────┐
       │             │             │
  Playwright/CDP   AX tree       pixels
       │             │             │
 deterministic   generic CUA   vision CUA
```

Because the app is Electron, exploit Chromium aggressively.

---

## 16. Semantic UI Contract

Every meaningful control/object should have strong semantics.

Prefer:

* semantic HTML
* ARIA roles
* accessible names
* descriptions
* stable IDs
* deterministic test IDs

Examples:

```tsx
<button
  aria-label="Run agent"
  data-testid="run-agent"
>
  Run
</button>
```

Domain objects should be machine-legible.

Example run row:

```text
role=button
name="Run 482"
description="Craftax, harness r93, reward 0.58, completed"
selected=true
```

Example rollout step:

```text
role=group
name="Step 37"
description="Action move north, reward 0.1"
```

The accessibility tree should effectively act as a structured projection of application state.

---

## 17. First-Class CUA Driver

Ship a tiny eval/control CLI.

Example:

```bash
synth-ui snapshot
synth-ui screenshot
synth-ui click @e42
synth-ui fill @e81 "Analyze this rollout"
synth-ui press Enter
```

Example snapshot:

```text
[e1] window "Synth"
  [e2] navigation
    [e3] button "Agents"
    [e4] button "Runs" selected=true

  [e5] main
    [e6] heading "Runs"
    [e7] button "Run 482"
         description="Craftax, reward 0.58"
```

Implementation can sit on top of:

* CDP
* Playwright
* Chromium accessibility nodes

This gives a generic structured-action interface for:

* Codex
* Claude
* Qwen policy
* CUA agents
* benchmark agents

---

## 18. Native OS Accessibility

Do not rely only on a private CDP path.

Preserve real macOS accessibility.

Desired chain:

```text
React DOM
   ↓
Chromium accessibility tree
   ↓
Electron accessibility bridge
   ↓
macOS AXUIElement
   ↓
generic desktop CUA agent
```

Goal:

A third-party accessibility-based Mac agent should be able to interact with Synth without special integration.

---

## 19. Canvas / Visualization Accessibility

Important visuals will often involve:

* game frames
* charts
* maps
* diagrams
* interactive environments

Do not expose only opaque canvas elements.

Separate:

```text
visual rendering
+
semantic projection
```

Example Craftax visualization:

```text
Visual:
[canvas frame]

Accessible state:
Craftax environment step 37.
Player at x=12, y=17.
Health 7.
Food 4.
3 wood in inventory.
Nearest enemy: zombie, four tiles east.
```

Chart accessibility:

```text
Reward over 100 episodes.
Mean reward 0.58.
Previous harness 0.43.
Improvement 34.9%.
Maximum 0.72.
```

This helps:

* actual accessibility
* CUA agents
* automated eval
* structured agent reasoning

---

## 20. Artifact Accessibility Metadata

Extend artifact schema.

```typescript
Artifact {
  ...

  accessibility?: {
    role?: string
    name: string
    description?: string
    textSummary?: string
    tree?: AccessibleNode[]
  }
}
```

Example:

```json
{
  "type": "environment_frame",
  "accessibility": {
    "name": "Craftax environment step 37",
    "textSummary": "Player at 12,17 with 7 health. Zombie four tiles east. Three wood in inventory."
  }
}
```

Renderers should automatically map this to DOM/ARIA.

---

## 21. Desktop Eval Mode

Add an explicit evaluation mode.

Example:

```bash
synth-desktop --eval-mode
```

Expose a local-only endpoint such as:

```text
http://127.0.0.1:<port>/__synth_eval
```

Possible endpoints:

```text
GET  /snapshot
GET  /screenshot
POST /action
GET  /state
POST /reset
```

Definitions:

`/snapshot`

* accessibility/semantic tree
* stable refs

`/screenshot`

* current desktop screenshot

`/action`

* click
* type
* press
* select
* scroll
* possibly drag/drop

`/state`

* internal ground-truth application state

`/reset`

* deterministic fixture reset

This enables:

```text
reset fixture
→ launch app
→ give task to agent
→ collect actions
→ inspect internal state
→ score
```

---

## 22. Eval Task Format

Define a desktop eval task format.

Example:

```yaml
task:
  id: compare-craftax-runs

setup:
  project: craftax
  fixture: eval-001

instruction:
  "Find the higher-performing of the two latest harness revisions and open its worst rollout."

success:
  - selected_run.harness_revision == "r93"
  - active_rollout.reward == minimum_reward(r93)

allowed_interfaces:
  - playwright
  - accessibility
  - cua

timeout_seconds: 120
```

Same task can be run against:

```text
Playwright agent
AX-tree agent
vision CUA agent
Claude Computer Use
Codex
local Qwen policy
```

Do not score by DOM state alone.

Score against internal domain state.

Example:

```text
selectedHarnessId == "r93"
activeRolloutId == "482"
```

This makes scoring robust to unusual but valid interaction paths.

---

## 23. Semantic State Architecture

Best architecture:

```text
                 DOMAIN STATE
                      │
               semantic UI model
                      │
      ┌───────────────┼────────────────┐
      │               │                │
      ▼               ▼                ▼
   React DOM      AX projection     screenshot
      │               │                │
      ▼               ▼                ▼
 Playwright      accessibility      vision CUA
```

Do not derive semantics from rendered pixels.

Derive:

* DOM
* accessibility
* labels
* test selectors
* evaluation state

from the same underlying domain state.

---

## 24. Example End-to-End Workflow

User says:

```text
Improve this Craftax agent.
```

Qwen policy model locally:

```text
I can inspect recent runs locally.
```

Desktop opens:

```text
Runs
#411   reward .42
#412   reward .39
#413   reward .47
```

Laguna analyzes local trajectories.

Produces:

```text
Finding:
73% of failures occur immediately after first wood acquisition.

[Open visualization]
```

Viewer shows:

* clustered failure states
* aggregate metric charts
* synchronized trajectories
* step-level reasoning/actions

User:

```text
Try improving it.
```

Qwen decides to escalate.

```text
local analysis
    ↓
Synth Cloud Codex worker
```

Codex edits:

* prompt
* harness code

Runs 100 evals.

Returns:

```text
Harness r92 → r93

reward       .43 → .58
wood         74% → 91%
stone        22% → 41%

[Compare rollouts]
[View changes]
[Accept revision]
```

User:

```text
Make this my local agent.
```

Synth trains:

```text
Laguna XS 2.1
+
craftax-r93.lora
```

Desktop installs it.

Now the improved agent can execute locally.

This is the full product loop.

---

## 25. Suggested Internal Protocol Objects

At minimum:

```text
Project
Workspace

Session
Turn
Message

Agent
AgentInstance

Tool
ToolCall
ToolResult
Approval

Artifact
Metric

Rollout
RolloutStep
Outcome

Harness
HarnessRevision

Model
ModelRevision
Adapter
AdapterRevision

Environment
EnvironmentRevision

Renderer
RendererRevision

Checkpoint

ExecutionTarget
LocalRuntime
CloudRuntime

EvalTask
EvalRun
EvalResult
```

All important objects should have stable IDs and provenance.

---

## 26. Persistence

Prefer a local durable store owned by the runtime daemon.

Likely:

```text
SQLite
+
content-addressed artifact store
+
Git/repo references
```

Use content hashes for:

* prompts
* adapters
* generated reports
* environment snapshots
* model artifacts
* renderers
* tool manifests
* large artifacts

The desktop should be restart-safe.

Sessions/runs should resume.

---

## 27. Model Inference Service Requirements

Local inference API should be backend-neutral.

Potential interface:

```text
POST /v1/chat/completions
POST /v1/responses
GET  /models
POST /models/load
POST /models/unload
GET  /models/status
GET  /adapters
POST /adapters/install
POST /adapters/activate
```

Request-level fields might include:

```json
{
  "model": "laguna-xs-2.1",
  "adapter": "sha256:...",
  "reasoning_effort": "...",
  "speculator": "auto"
}
```

Eventually support:

* Qwen
* Laguna
* GGUF fallback
* MLX
* LoRA
* multiple local models
* model residency policy

---

## 28. Potential Local Residency Policy

Example:

```text
Always resident:
Qwen 4B

Conditional:
Laguna XS 2.1

If memory pressure:
evict Laguna
retain Qwen

If heavy local task:
load Laguna

If task too large:
cloud escalation
```

The policy model can help choose.

---

## 29. Cloud Runtime

Synth Cloud should expose the same semantic events.

Potential execution implementations:

```text
Codex instance
Synth native agent
remote Laguna
GPU sandbox
browser agent
RL worker
training worker
eval farm
```

Cloud sessions should appear indistinguishable from local sessions at the UI level except for:

* target
* latency
* resource usage
* network status
* cost
* persistence semantics

---

## 30. Renderer Model

Renderer packages should be versioned and sandboxed.

Basic standardized renderer kinds:

```text
markdown
code
image
video
json
table
chart
html
react
environment
rollout
diff
terminal
```

Custom renderers:

```text
renderer package
+ schema
+ version
+ accessibility projection
```

Example:

```text
craftax-viewer@sha256:...
```

The renderer used for a historical rollout should be recoverable.

---

## 31. First-Pass Engineering Priorities

The first implementation pass should optimize for architectural truth, not completeness.

Recommended order:

### Phase 1: runtime skeleton

* Electron shell
* local daemon
* session RPC
* persistence
* streaming
* tool events
* artifact event
* metric event
* rollout event

### Phase 2: agent compatibility

* Codex App Server adapter
* ACP adapter
* local subprocess agent support
* Synth Cloud remote agent connection

### Phase 3: local inference

* Qwen 4B
* Laguna XS 2.1
* MLX backend
* OpenAI-compatible serving
* basic adapter loading
* model status

### Phase 4: workbench surfaces

* Agents
* Runs
* Artifacts
* Models
* Harnesses
* Projects

### Phase 5: rollout viewer

* timeline
* step view
* synchronized artifacts
* metrics
* compare two runs

### Phase 6: versioning

* Harness Revision
* prompt hashes
* code revision
* adapter revision
* environment revision
* renderer revision

### Phase 7: eval surface

* semantic DOM
* ARIA
* Playwright launch
* CDP snapshot
* eval CLI
* reset/state endpoints
* native accessibility verification

---

## 32. Likely V1 Scope

A credible V1 should support:

* one local project
* local Qwen
* local Laguna
* one LoRA per active model
* one Codex cloud execution path
* ACP
* agent sessions
* terminal
* diffs
* artifact pane
* rollout viewer
* metrics
* harness revision metadata
* deterministic desktop eval mode

Do not require:

* full IDE parity
* giant multi-model scheduler
* dozens of LoRA hot swaps
* perfect speculative decoding with tuned LoRAs
* arbitrary custom renderers on day one
* multi-user cloud collaboration
* comprehensive multi-agent orchestration

---

## 33. Strategic Differentiation

Features likely to commoditize:

```text
chat UI
coding shell
Codex compatibility
ACP compatibility
local OSS models
basic artifacts
terminal
diff viewer
```

Harder-to-copy differentiation:

```text
universal rollout format
universal rollout viewer
metrics + visuals attached to steps
harness version graph
exact model/adapter/environment provenance
local/cloud execution parity
research → eval → tune → local deploy loop
first-class evaluability of the desktop itself
```

The rollout/artifact/version system should therefore be treated as the core product platform.

---

## 34. Questions the Reviewing Agent Should Resolve Using Local Context

The next agent should inspect the local Synth codebase and answer:

1. Which existing Synth object models already overlap with:

   * Session
   * Run
   * Artifact
   * Metric
   * Harness
   * Environment
   * Model
   * Checkpoint?

2. Which current backend should become the local runtime daemon:

   * Rhodes?
   * Horizons runtime?
   * another existing service?
   * a new thin daemon?

3. Which existing protocol/API is closest to the required event model?

4. How does existing Managed Research persistence map onto Desktop sessions?

5. What can be reused from existing:

   * rollout serialization
   * eval data
   * Craftax visualizers
   * report generation
   * model registry
   * LoRA handling
   * container/environment abstractions?

6. Is there already a content-addressed artifact model?

7. What is the most natural Harness Revision representation in the current ontology?

8. Where should Codex App Server adaptation live?

9. Where should ACP adaptation live?

10. What is the local inference integration boundary?

    * separate daemon
    * child process
    * embedded Rust/native module
    * HTTP service?

11. Is Electron already used anywhere?

12. Which parts can be implemented as web UI shared with existing usesynth.ai views?

13. How much of the existing Craftax eval page can become a reusable renderer?

14. Which existing cloud execution abstractions can make Local vs Cloud a single `ExecutionTarget`?

15. What is the minimum subset needed for a first shippable prototype?

---

## 35. Requested Output From Reviewing Agent

After reviewing this handoff and the local repository/context, produce:

### A. Existing-system mapping

For every major concept above:

```text
Concept
→ existing Synth equivalent
→ reusable code path
→ gap
```

### B. Recommended architecture

Concrete packages/services/modules.

Example:

```text
apps/desktop
packages/runtime-protocol
packages/rollout
packages/artifacts
services/local-runtime
services/local-inference
...
```

Do not assume these names; derive them from repository conventions.

### C. Reuse plan

Identify specific existing code/modules to reuse.

### D. Missing primitives

List primitives that genuinely need to be invented.

### E. V1 cut

Specify the smallest coherent V1.

### F. Implementation sequence

Order work by dependency.

### G. Rough LOC / effort

Estimate major components.

### H. Major risks

Especially:

* Laguna MLX support
* LoRA compatibility
* speculative decoding after LoRA tuning
* Codex protocol drift
* ACP limitations
* Electron/runtime process lifecycle
* accessibility semantics
* rollout schema over-design
* custom renderer security
* local/cloud consistency

### I. First concrete coding pass

Recommend exactly what to implement first in the current repository.

Prefer something that establishes the central abstraction rather than a disposable UI demo.

A likely good candidate is:

```text
runtime protocol
+
minimal Electron client
+
one local agent
+
Rollout/Artifact/Metric stream
+
Playwright-evaluable UI
```

but the repository context should determine the final choice.

---

## 36. Guiding Principles

1. **Local-first, cloud-capable.**

2. **Same runtime semantics everywhere.**

3. **Models are replaceable; runs and provenance are durable.**

4. **Artifacts are protocol objects, not attachments.**

5. **Rollouts are first-class data.**

6. **Metrics belong next to trajectory steps.**

7. **Visuals should emerge automatically from structured data.**

8. **Everything executable should be versionable.**

9. **Base models and LoRAs remain separate identities.**

10. **Electron is a client, not the runtime.**

11. **Use Codex and ACP as compatibility layers, not ontology.**

12. **Do not build a full IDE until the workbench proves it needs one.**

13. **Accessibility semantics are part of the product API.**

14. **The desktop app itself should be benchmarkable.**

15. **Score agent behavior against internal state, not brittle DOM conditions.**

16. **Optimize first for the research/eval/fine-tuning loop that Synth uniquely owns.**

The most useful next step is for the reviewing agent to annotate this against the actual Synth repo and return a **reuse/gap matrix plus a concrete package-level V1 architecture**, rather than re-specifying the product from scratch.

---

## Appendix: Narrowed V1 Cut (authoritative for first ship)

That’s a much cleaner first cut. Narrow the handoff around **two execution targets only**:

1. **Local Laguna XS 2.1**
2. **Synth Intern**, with both **sync** and **async** execution as first-class modes

Everything else—Qwen policy routing, generalized ACP ecosystem support, broad Codex compatibility, LoRA fleet management, rich custom renderers—can sit behind that.

The revised V1 should look like this:

```text
                    Synth Desktop
                         │
                  Synth Runtime API
                         │
             ┌───────────┴───────────┐
             │                       │
           LOCAL                   INTERN
             │                       │
      Laguna XS 2.1          Synth Intern agent
       MLX / Metal             │          │
             │                 │          │
          synchronous        sync       async
          session             run        job
```

The important thing is that **sync vs async should not be modeled as two unrelated products**. They should be two execution modes of the same Intern abstraction.

Something like:

```ts
type ExecutionTarget =
  | {
      kind: "local";
      model: "laguna-xs-2.1";
    }
  | {
      kind: "intern";
      mode: "sync" | "async";
      intern: InternRef;
    };
```

And both should produce the same stream/domain objects:

```ts
Session
Turn
Message
ToolCall
Artifact
Metric
RolloutStep
Outcome
```

For synchronous Intern execution:

```text
user submits task
      ↓
Intern starts
      ↓
events stream live
      ↓
artifacts / tool calls / rollout
      ↓
complete
```

For async:

```text
user submits task
      ↓
job created
      ↓
desktop can disconnect
      ↓
Intern continues remotely
      ↓
persistent events/artifacts/checkpoints
      ↓
desktop reconnects
      ↓
resume viewing live or completed run
```

So an Intern run needs a lifecycle more like:

```ts
InternRun {
  id
  sessionId

  mode: "sync" | "async"

  status:
    | "queued"
    | "starting"
    | "running"
    | "waiting_for_input"
    | "completed"
    | "failed"
    | "cancelled"

  createdAt
  startedAt?
  completedAt?

  latestCursor
  checkpoint?
  outcome?
}
```

The **cursor/event-log semantics are especially important**. Async should not require a second representation. The client should effectively be able to say:

```text
subscribe(run_id, after_event=1827)
```

and reconstruct the same UI that it would have seen if it had remained connected the whole time.

### Local Laguna V1

Scope the local side quite tightly:

```text
Laguna XS 2.1
    ↓
MLX
    ↓
Metal
    ↓
local inference daemon
    ↓
OpenAI-ish streaming API
    ↓
Synth Runtime
```

Initial requirements:

* download/install model
* hardware/memory check
* load/unload
* token streaming
* cancellation
* basic usage/timing statistics
* conversation/session state
* tool-call support required by the local agent
* eventually LoRA adapter parameter, even if V1 only supports base Laguna
* deterministic model/version identity

**Design the adapter field now**, even if actual LoRA installation lands in V1.1:

```json
{
  "model": "laguna-xs-2.1",
  "adapter": null
}
```

That avoids changing the identity model later.

### Intern should be the main cloud primitive

The desktop surface should say something like:

```text
Run with
─────────────
Local
  Laguna XS 2.1

Synth
  Intern
    ○ Live
    ○ Background
```

Intern should feel substantially better than generic “remote agent” integration.

For async runs especially, first-class UI should include:

* running jobs
* queued jobs
* notifications when state changes
* elapsed runtime
* most recent activity
* checkpoint / progress indication where available
* open live run
* stop/cancel
* resume/retry
* results/artifacts when completed

This makes the product useful even before you build the entire harness/eval platform.

### Revised implementation order

**Milestone 1 — Runtime contract**

Define:

```text
ExecutionTarget
Session
Run
Event
Artifact
Metric
Outcome
```

and sync/async lifecycle semantics.

**Milestone 2 — Local Laguna**

Get one real Laguna XS 2.1 session working through the daemon into a minimal Electron UI.

```text
prompt → Laguna → stream → UI
```

Then tool use.

**Milestone 3 — Intern sync**

Same UI, but execution target becomes Intern.

This tests whether the abstraction really works.

**Milestone 4 — Intern async**

Persistent remote run IDs, cursor-based event replay, reconnect, background jobs, completion handling.

This is probably the most important architectural milestone.

**Milestone 5 — Run/artifact viewer**

Once local Laguna and Intern both produce events, make their output inspectable through the same Run representation.

**Milestone 6 — evaluation/accessibility**

Bake in Playwright/ARIA semantics from the start, but build the explicit `synth-ui`/eval mode after the primary interaction loop works.

### Explicitly deferred

For this first scope, explicitly defer:

```text
Qwen policy model
automatic local/cloud routing
general multi-agent orchestration
full ACP compatibility
full Codex App Server compatibility
arbitrary local OSS models
multi-LoRA hot swapping
training UI
custom renderer SDK
full Harness Revision system
full eval comparison UI
full IDE/editor
```

But preserve enough schema room that they don't require architectural replacement.

The resulting first product is much easier to explain:

> **Synth Desktop lets you work with a very fast private Laguna XS 2.1 agent on your Mac, and seamlessly hand work to Synth Intern when you want a live or long-running cloud agent. All work remains inspectable in the same session/run interface.**

That’s a tight enough wedge to actually ship, while still laying the foundation for the rollout/artifact/versioning system we discussed.
