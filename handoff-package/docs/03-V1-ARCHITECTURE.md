# V1 Architecture — Concrete Packages

Derive names from Synth monorepo habits (`apps/`, `packages/`, `services/`). Suggested home: new **`workshop`** repo (this one) or a `desktop` app folder inside an agreed monorepo. Do not stuff orchestration into `frontend`.

---

## Process model

```text
┌──────────────────────────────────────────────────────────┐
│  Synth Desktop (Electron)                                │
│  ┌─────────────┐  IPC/HTTP  ┌──────────────────────────┐ │
│  │  Renderer   │◄──────────►│  Main process            │ │
│  │  React UI   │            │  windowing, keychain,    │ │
│  │  Run viewer │            │  spawns daemon           │ │
│  └─────────────┘            └────────────┬─────────────┘ │
└──────────────────────────────────────────┼───────────────┘
                                           │ localhost
                                           ▼
┌──────────────────────────────────────────────────────────┐
│  synth-local-runtime  (Python daemon)                    │
│  ├── SessionStore (SQLite)                               │
│  ├── EventLog (per-session sequence)                     │
│  ├── adapters/                                           │
│  │     ├── local_laguna.py  → MLX inference service      │
│  │     └── intern.py        → synth-ai SynthClient       │
│  └── IPC / JSON-RPC or local HTTP :port                  │
└───────────────┬──────────────────────────┬───────────────┘
                │                          │
                ▼                          ▼
     ┌──────────────────┐      ┌────────────────────────────┐
     │ local-inference  │      │ Synth Backend              │
     │ MLX / Metal      │      │ /smr/research-intern/*     │
     │ OpenAI-ish HTTP  │      │ (hosted or synth-dev slot) │
     └──────────────────┘      └────────────────────────────┘
```

---

## Recommended package layout

```text
workshop/   (or desktop monorepo root)
├── apps/
│   └── desktop/                 # Electron + Vite/React
│       ├── electron/            # main, preload, IPC
│       └── src/                 # renderer UI
├── packages/
│   ├── runtime-protocol/        # shared TS types (Session, Run, Event, ExecutionTarget)
│   ├── runtime-client/          # renderer → daemon HTTP/IPC client
│   └── intern-ui/               # optional extract of presentational Intern React
├── services/
│   ├── local-runtime/           # Python daemon (primary)
│   └── local-inference/         # MLX Laguna server (separate process)
└── contracts/
    └── research-v1.json         # vendored from this handoff / synth-ai
```

Python daemon depends on published/local `synth-ai` — do not vendor the whole SDK.

---

## Domain types (daemon owns; UI mirrors)

```ts
type ExecutionTarget =
  | { kind: "local"; model: "laguna-xs-2.1"; adapter: string | null }
  | { kind: "intern"; mode: "sync" | "async"; binding?: InternRuntimeBinding };

type Session = {
  id: string;                    // desktop uuid
  target: ExecutionTarget;
  remoteId?: string;             // sync_session_id | "async" singleton key
  createdAt: string;
  status: SessionStatus;
  stateGeneration?: number;      // Intern only
  latestCursor: number;          // after_sequence for remote; local seq for local
};

type Run = {
  id: string;
  sessionId: string;
  mode: "local" | "sync" | "async";
  status:
    | "queued" | "starting" | "running"
    | "waiting_for_input" | "completed" | "failed" | "cancelled";
  latestCursor: number;
  checkpoint?: unknown;
  outcome?: unknown;
};

type RuntimeEvent = {
  sequence: number;
  eventKind: string;
  payload: unknown;
  commandId?: string;
  createdAt: string;
  source: "local" | "intern";
};
```

**Cursor rule:** never resume an Intern stream with a local sequence, or vice versa. Store `(sessionId, source, cursor)`.

---

## Intern adapter (reuse, don’t reinvent)

```python
from synth_ai import SynthClient

client = SynthClient(api_key=..., base_url=...)

# Sync
session = client.research.intern.sync_.create(...)
client.research.intern.sync_.send_message(session.sync_session_id, ...)
page = client.research.intern.sync_.tail(session.sync_session_id, after_sequence=cursor)

# Async
rt = client.research.intern.async_.ensure(...)
client.research.intern.async_.send(...)
page = client.research.intern.async_.tail(after_sequence=cursor)
```

Map Intern projections → desktop `Session`/`Run` status. Map ledger events → `RuntimeEvent`.

Optimistic concurrency: every command needs `command_id`, `idempotency_key`, `expected_generation`.

---

## Local Laguna adapter

```text
prompt
  → daemon opens/creates Session(target=local)
  → POST local-inference /v1/chat/completions (stream)
  → daemon appends RuntimeEvents (message.delta, message.completed, usage)
  → UI subscribes via daemon SSE/WS/IPC
```

V1 requirements: install/load/unload, hardware check, stream, cancel, usage/timing, `model` + `adapter: null` identity.

---

## UI surfaces (V1)

| Surface | Behavior |
|---------|----------|
| **New session** | Pick Local / Intern Live / Intern Background |
| **Transcript** | Optimistic local bubbles; reconcile on receipt + agent events |
| **Jobs** | Async list: running / sleeping / blocked; reconnect; do not auto-pause |
| **Run timeline** | Ordered events; jump to artifacts |
| **Models** | Laguna status (loaded?, tok/s); Intern = “cloud” badge |

Reference layout: frontend SyncCockpit (left transcript + composer, right context). Async primary product in web is Effort board — Desktop V1 can ship a simpler Async ops desk first, then Effort board.

---

## Auth

| Secret | Storage |
|--------|---------|
| `SYNTH_API_KEY` | OS keychain via Electron main → daemon env |
| Backend URL | User setting / `SYNTH_BACKEND_URL` |
| Local only | No cloud key required for Laguna-only mode |

---

## Eval / accessibility (bake in, don’t block)

- Stable `data-testid` on session list, composer, run timeline, target picker
- Prefer roles/names over CSS selectors
- Full `synth-ui` eval mode is Milestone 6, not day one

---

## What not to put where

| Don’t | Do instead |
|-------|------------|
| Intern HTTP from renderer | Daemon adapter |
| Second command vocabulary | Frozen Intern `command_kind`s |
| Treat receipt HTTP as “agent finished” | Wait for events |
| Pause Async on window close | Leave running; show reconnect |
| Embed full backend | Call remote `/smr/research-intern` |
| Fork Next BFF into Electron | Direct API key auth |
