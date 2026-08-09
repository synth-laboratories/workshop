# Mock Desktop App Plan — UX/UI Pin-Down

**Goal:** Ship a mock-only desktop renderer to pin down layout, states, density, and flows with **second-scale iteration** — no daemon, no MLX, no API key.

**Philosophy:** Same as gamebench — fast mock lane first, production lane later, oracle fixtures for truth. The mock is for speed; the real runtime plugs in via a swappable provider.

---

## What we're building

```text
┌─────────────────────────────────────────┐
│  Mock Desktop (Vite + React → Electron) │
│  ┌─────────┐  ┌──────────┬────────────┐ │
│  │ Nav     │  │ Chat     │ Inspector  │ │
│  │         │  │ (fake    │ (Run /     │ │
│  │         │  │  stream) │  Artifact) │ │
│  └─────────┘  └──────────┴────────────┘ │
│  [Scenario ▼]  [▶ Walkthrough]  [⏩ 2x] │
└─────────────────────────────────────────┘
         │
         ▼
   MockRuntimeProvider (fixtures + tape player)
   — no network, no Python, no Electron main logic (initially)
```

This is a **UX pin-down rig**, not a product backend.

---

## M0: Landing page (first deliverable)

**Scope:** Mock **only** the empty / first-run landing screen — similar-ish to Poolside Desktop Assistant’s new-chat state (light macOS shell, sidebar + centered hero + bottom composer). No active conversation, no inspector pane, no streaming yet.

**Reference:** Poolside landing capture (Aug 2026). Use for **layout and density**, not branding or copy. Synth identity, model names, and CTAs are ours.

### Layout

```text
┌──────────────────────────────────────────────────────────────────┐
│ [tab: New session]  [+]                              [window ctl] │  ← title / tab bar
├──────────────┬───────────────────────────────────────────────────┤
│ SIDEBAR      │ MAIN (landing)                                      │
│              │                                                     │
│ + New conv   │              [Synth logo]                           │
│   Connectors │                                                     │
│   Search     │     Start a new conversation using                  │
│              │     [ Laguna XS 2.1 ▼ ]   ← model / target picker   │
│ ▼ Chats      │                                                     │
│   · recent…  │   ┌─────────────────┐  ┌─────────────────┐      │
│              │   │ Add a project     │  │ Set up another  │      │
│ ▼ Projects   │   │ (dashed card)     │  │ agent (dashed)  │      │
│   [+ Add     │   └─────────────────┘  └─────────────────┘      │
│    Project]  │                                                     │
│              │  ┌─────────────────────────────────────────────┐  │
│              │  │ No model available / placeholder…    [↑][🎤] │  │  ← composer
│ ──────────── │  │ [+]  Always ask ▼                            │  │
│ ▓▓▓░░ 42%    │  └─────────────────────────────────────────────┘  │
│ Downloading  │                                                     │
│ Laguna XS…   │                                                     │
│ ⚙ Settings   │                                                     │
└──────────────┴───────────────────────────────────────────────────┘
```

### Regions to implement

| Region | Components | Notes |
|--------|------------|-------|
| **Tab bar** | `TabBar`, active tab, `+` new tab | macOS-style; Electron chrome later |
| **Sidebar** | `SidebarNav`, `ChatList`, `ProjectList`, `ModelDownloadBar`, `SettingsLink` | Collapsible Chats / Projects sections |
| **Landing hero** | `LandingHero`, `ModelPicker` | Centered logo + “Start a new conversation using” + model pill |
| **Quick actions** | `QuickActionCard` × 2 | Dashed border: “Add a project”, “Set up another agent” (Synth wording TBD) |
| **Composer** | `Composer` (disabled variant) | Placeholder, permission dropdown, mic, send |
| **Download strip** | `ModelDownloadProgress` | Sidebar footer: progress + pause; drives composer enabled/disabled |

### Landing states (scenario picker)

Pin these before building conversation UI:

| Scenario ID | State |
|-------------|-------|
| `landing-first-run` | No chats, no projects, model not installed |
| `landing-downloading` | Progress bar active, composer disabled (“No model available”) |
| `landing-ready` | Model installed, composer enabled, placeholder text |
| `landing-with-history` | One+ chats in sidebar, still on landing hero |
| `landing-with-project` | Projects section has one folder |

Fixture: `fixtures/scenarios/00-landing-*.json` — static UI state only (no event tapes yet).

### Landing interactions (mock only)

| Action | Mock behavior |
|--------|---------------|
| Click model pill | Dropdown: Local Laguna XS 2.1, Intern · Live, Intern · Background |
| Click “Add a project” | Toast or modal stub — no real FS |
| Click “Set up another agent” | Stub — deferred |
| Click “New conversation” | Navigate to `01-active-chat` (later milestone) |
| Type in composer (when ready) | No-op or toast until M1 |
| Settings | Stub panel or placeholder route |

### Visual direction (landing)

Poolside reference uses a **light** shell (white main, light gray sidebar). For M0, match that aesthetic for the landing page — do **not** default to `smrChrome` dark operator console yet. Define `tokens-light.ts` for landing; dark workbench can come with the agent/inspector surfaces in M1+.

```text
packages/desktop-ui/
├── tokens-light.ts      # M0 landing (Poolside-ish reference)
├── tokens.ts            # later: smrChrome-derived workbench
├── shell/
│   ├── AppShell.tsx
│   ├── Sidebar.tsx
│   ├── TabBar.tsx
│   └── ModelDownloadBar.tsx
└── surfaces/
    └── LandingPage.tsx
```

### M0 done when

- [ ] All 5 landing scenarios render via scenario picker
- [ ] Model picker shows Synth execution targets (Local / Intern Live / Background)
- [ ] Composer disabled vs enabled states are obvious
- [ ] Download progress bar animates from fixture (`landing-downloading`)
- [ ] `data-testid` on: sidebar, model picker, composer, quick-action cards, settings
- [ ] Readable in browser at ~1280×800 without layout breaks

**Out of scope for M0:** active chat transcript, inspector, jobs panel, real downloads, Electron, runtime protocol beyond a minimal `LandingState` type.

---

## Stack progression

| Phase | Stack | When |
|-------|-------|------|
| **1** | Vite + React only | Day 1 — fastest HMR |
| **2** | Electron shell wrap | Layout stabilizes on 3–4 screens |
| **3** | Real daemon provider | UX freeze on mock scenarios |

Electron doesn't change early UX decisions. Pin the shell in the browser first.

---

## Core abstraction: `DesktopRuntime`

UI components **only** talk to a runtime interface. Mock and real daemon implement the same contract.

```ts
type DesktopRuntime = {
  listSessions(): Promise<Session[]>;
  createSession(target: ExecutionTarget): Promise<Session>;
  subscribe(sessionId: string, afterSequence: number): AsyncIterable<RuntimeEvent>;
  sendMessage(sessionId: string, body: string): Promise<void>;
  cancel(sessionId: string): Promise<void>;
  getProjection(sessionId: string): Promise<SessionProjection>;
  listJobs(): Promise<InternRun[]>;  // async panel
};
```

App root swaps implementation:

```tsx
const runtime = import.meta.env.VITE_MOCK
  ? new MockRuntime(fixtures)
  : new DaemonRuntime("http://127.0.0.1:PORT");
```

Everything polished in mock phase transfers directly to production.

---

## Repo layout

```text
workshop/
├── apps/
│   └── mock/                       # Vite + React + Electron (M0 landing)
│       ├── src/
│       │   ├── App.tsx
│       │   ├── dev/
│       │   │   ├── ScenarioPicker.tsx
│       │   │   ├── WalkthroughPlayer.tsx
│       │   │   └── StateInspector.tsx
│       │   ├── surfaces/
│       │   │   ├── AgentView.tsx
│       │   │   ├── JobsPanel.tsx
│       │   │   └── RunInspector.tsx
│       │   └── main.tsx
│       ├── index.html
│       └── vite.config.ts
├── packages/
│   ├── runtime-protocol/           # Session, Run, RuntimeEvent, ExecutionTarget
│   ├── runtime-client/             # DesktopRuntime interface + useDesktopRuntime
│   └── desktop-ui/                 # primitives + surfaces
├── fixtures/
│   └── scenarios/                  # JSON initial states + event tapes
└── product/
    ├── walkthroughs/
    └── surfaces/                   # 1-pagers per screen
```

**Bootstrap:**

```bash
cd workshop/apps/mock && npm create vite@latest . -- --template react-ts
npm run dev   # → localhost:5173, instant HMR
```

---

## Mock data: scenarios, not improvised state

Don't hand-write fake state inside components. Use **scenarios** (gamebench tape shape):

```text
fixtures/scenarios/
├── 00-landing-first-run.json  # M0: empty landing
├── 00-landing-downloading.json
├── 00-landing-ready.json
├── 00-landing-with-history.json
├── 00-landing-with-project.json
├── 01-empty.json              # M1+: no sessions, first-run (post-landing)
├── 02-local-streaming.json    # mid-stream Laguna response
├── 03-intern-sync-live.json   # tool calls + approval pending
├── 04-intern-async-jobs.json  # running / queued / completed jobs
├── 05-run-timeline.json       # event log for inspector
└── tapes/
    └── intern-sync-001.jsonl  # step-by-step events for replay
```

Each scenario = initial state + optional event tape. Mock runtime **plays tapes** on user actions:

```text
User clicks "New → Intern Live"
  → load scenario 03
  → play tape: session.created → message.delta × N → tool_call → approval.requested
```

Streaming simulated via `setInterval` emitting `message.delta` — speed configurable (instant / 2x / 1x / 0.5x).

### Scenarios by milestone

**M0 — landing only**

| Scenario | Exercises |
|----------|-----------|
| `landing-first-run` | Empty sidebar, disabled composer, hero + quick actions |
| `landing-downloading` | Sidebar progress, “No model available” |
| `landing-ready` | Composer enabled, model pill selected |
| `landing-with-history` | Chat list populated, landing hero unchanged |
| `landing-with-project` | Projects section has entry |

**M1+ — conversation / workbench**

| Scenario | Exercises |
|----------|-----------|
| `empty` | Post-landing, target picked, empty transcript |
| `local-streaming` | Laguna tokens streaming, cancel button |
| `intern-sync-tools` | tool_call rows, approval panel |
| `intern-async-jobs` | jobs list + reconnect banner |
| `run-inspector` | timeline with artifacts + metrics strip |

Example tape events:

```json
{"sequence": 4, "eventKind": "message.delta", "payload": {"delta": "Analyzing"}, "createdAt": "..."}
{"sequence": 5, "eventKind": "tool_call.started", "payload": {"tool": "read_file", "path": "agent.py"}}
{"sequence": 6, "eventKind": "approval.requested", "payload": {"kind": "shell", "command": "pytest"}}
```

Record real oracle tapes later when backend exists. Promote to `fixtures/oracle/` per gamebench ladder.

---

## Surfaces to mock (build order)

| # | Milestone | Surface | States to pin |
|---|-----------|---------|---------------|
| **0** | **M0** | **Landing page** | first-run, downloading, ready, with-history, with-project |
| 1 | M1 | **App shell** (active session) | nav, session list, tab bar |
| 2 | M1 | **Model / target picker** | Local / Intern Live / Background (in hero + composer) |
| 3 | M1 | **Agent view** | empty, streaming, waiting_for_input, failed, completed |
| 4 | M1 | **Composer** (active) | idle, sending, disabled-while-streaming |
| 5 | M2 | **Inspector** | empty run, event timeline, artifact card |
| 6 | M2 | **Jobs panel** | running, queued, reconnect banner |
| 7 | M2 | **Approval chip** | pending approve/deny |

**Defer:** file tree, editor, harness diff, real model download backend (M0 uses fixture-driven progress only).

---

## Dev tools (build into mock app)

### 1. Scenario picker (top bar)
Dropdown jumps instantly to any scenario state. No clicking through flows every time.

### 2. Walkthrough player
Load `product/walkthroughs/*.md` as steps; **Next** advances scripted actions and highlights expected region.

```text
Step 3/7: "Target picker shows Intern · Live selected"
[Next] [Prev] [Jump to step...]
```

### 3. State inspector drawer (dev only)
Live JSON: `session`, `events[]`, `cursor`, `selectedRunId`. Confirms UI is driven from domain state, not local React fiction.

### 4. Stream speed control
`Instant | 2x | 1x | 0.5x` for fake token streaming.

### 5. Chaos toggles
- Simulate disconnect mid-stream
- Simulate 409 conflict
- Simulate slow projection refresh

---

## Design system

```text
packages/desktop-ui/
├── tokens.ts          # fork smrChrome from frontend → desktop density
├── WorkbenchShell.tsx # 3-column layout
├── SplitPane.tsx
├── SessionList.tsx
├── Transcript.tsx
├── Composer.tsx
├── RunTimeline.tsx
├── TargetPicker.tsx
└── StatusBadge.tsx
```

- **M0 tokens:** `tokens-light.ts` — light shell per Poolside landing reference (white main, gray sidebar)
- **M1+ tokens:** `frontend/src/lib/smrChrome.ts` — operator-console dark for workbench/inspector
- **Layout reference:** Poolside landing capture + `electron-clones/poolside-spec.md` — proportions only, not pixel-copy
- **Semantics:** every component gets `data-testid` + `aria-label` from day one

---

## Daily iteration loop

```text
1. Pick walkthrough step or scenario
2. Adjust layout/tokens in desktop-ui
3. HMR shows change instantly
4. Scenario picker → verify all states still work
5. Update product/surfaces/*.md if behavior intentionally changed
6. (Optional) screenshot diff on golden scenario
```

If you're waiting on builds or APIs, the mock isn't mock enough.

---

## UX freeze criteria

### M0 (landing)

- [ ] All 5 landing scenarios render without layout bugs
- [ ] Model picker + composer disabled/enabled states are clear
- [ ] Sidebar download strip reads well at a glance
- [ ] Team can demo landing mock without caveats

### M1+ (full mock phase)

- [ ] Conversation scenarios render without layout bugs
- [ ] Walkthroughs 01–03 playable end-to-end in mock
- [ ] Target picker → transcript → inspector flow feels right
- [ ] Async jobs panel states are clear without explanation

Then: Electron wrap → real daemon provider → oracle tape parity (gamebench ladder).

---

## First PR scope

**Title:** `desktop-mock: landing page + scenario picker`

1. Vite + React app under `apps/mock`
2. `tokens-light.ts` + `AppShell` + `Sidebar` + `LandingPage` + `Composer` (disabled variant)
3. `LandingState` fixture type + 5 `00-landing-*.json` scenarios
4. Scenario picker in dev toolbar
5. README: `npm run dev` → pick landing scenario → see UI

**Out of scope:** active chat, inspector, `DesktopRuntime`, Electron, real downloads, Intern/Laguna backends.

### Second PR (M1 preview)

**Title:** `desktop-mock: active chat shell`

1. `runtime-protocol` types (`ExecutionTarget`, `Session`, `RuntimeEvent`)
2. `MockRuntime` + `01-empty` / `02-local-streaming` scenarios
3. Navigate landing → active session on “New conversation”

---

## What NOT to do in mock phase

- Don't call Intern HTTP from the renderer "just to test"
- Don't build MLX or Python daemon yet
- Don't fork `SyncCockpit.tsx` — extract patterns, rewrite thin
- Don't chase Poolside pixel parity — chase **state clarity**
- Don't add features not in a walkthrough or scenario

---

## Bridge to gamebench-style parity (later)

| Gamebench | Mock desktop → production |
|-----------|---------------------------|
| gold_python | `MockRuntime` + scenarios |
| gold_rust | real daemon + Electron |
| mGBA oracle | recorded `fixtures/oracle/*.jsonl` |
| `--mode rust` | mock invariant fuzz (cursor, idempotency) |
| `--mode oracle` | mock ↔ production ↔ fixture diff |
| IMPLEMENTATION_MAP | `product/IMPLEMENTATION_MAP.md` per surface |

**Operating rule:** mock for speed, production for integration, oracle fixture for truth.

---

## Related docs

- [`HANDOFF.md`](./HANDOFF.md) — product thesis + V1 cut
- [`handoff-package/docs/03-V1-ARCHITECTURE.md`](./handoff-package/docs/03-V1-ARCHITECTURE.md) — process model
- [`handoff-package/docs/04-IMPLEMENTATION-SEQUENCE.md`](./handoff-package/docs/04-IMPLEMENTATION-SEQUENCE.md) — milestone order
- [`electron-clones/poolside-spec.md`](../electron-clones/poolside-spec.md) — layout reference only
