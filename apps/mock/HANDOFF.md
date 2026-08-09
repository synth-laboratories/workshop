# Synth Workshop Mock — Full Product + UX Handoff

**Date:** 2026-08-08  
**Location:** `workshop/apps/mock`  
**Status:** Fixture UX pin-down — titlebar labeled **MOCK**  
**Real app:** `workshop/apps/synth_desktop` (v0 + daemon)  
**Audience:** Engineers comparing mock chrome vs real runtime app  
**Source:** Product docs (`workshop/HANDOFF.md`, `handoff-package/`) + pin-down thread decisions

---

## 0. One-liner

> **Synth Desktop** is a local-first agent R&D workbench — not “another coding IDE.” Agents run locally (Laguna + LoRAs) or in Synth Cloud (Intern Live/Background); every run produces inspectable, replayable, quantitative, version-linked artifacts. Electron is the **viewer**; a local daemon is the **brain + vault**; cloud holds sealed publications.

This repo app (`apps/mock`) is a **fixture-only Electron UX pin-down** so we can settle IA and chrome before wiring `DesktopRuntime`.

---

## 1. How to run

```bash
cd workshop/apps/mock
npm install
npm run dev
```

| Command | Purpose |
|---------|---------|
| `npm run dev` | HMR + Electron window |
| `npm run build` | Production build → `out/` |
| `npm run preview` | Run built app |

Dev scenario bar (top) jumps fixture states. Main-process changes (dock icon, traffic lights, drag regions) need a full restart — renderer HMR is not enough.

---

## 2. Product thesis (authoritative)

From `workshop/HANDOFF.md` / `handoff-package/docs/00-PRODUCT-HANDOFF.md`:

**Center of gravity (not the editor shell):**

1. Universal agent sessions  
2. Universal rollout representation  
3. Rich artifacts / visuals  
4. Standardized metrics  
5. Exact harness / version provenance  
6. Local ↔ cloud execution parity  
7. Local fine-tuned model deployment (LoRAs first-class)  
8. Easy desktop-agent evaluation  

**Native loop:** observe → understand → modify → evaluate → fine-tune → deploy  

**V1 feel is closer to:** Codex Desktop + Claude Artifacts + PostTrainBench trajectory inspection + Synth eval visualizations + local inference + cloud Intern — **not** a VS Code/Cursor clone.

**Do not put orchestration in Electron.** Separate local runtime daemon owns sessions, tools, approvals, artifacts, metrics, inference, cloud delegation, persistence, provenance.

---

## 3. Architecture law (do not undo)

```text
Electron (renderer)  →  IPC / localhost  →  local-runtime daemon (Python / synth-ai)
                                              ├── Local Laguna adapter (MLX + LoRA)
                                              ├── Codex App Server / ACP → OpenRouter (Luna, Poolside S2.1, …)
                                              │     + local usage ledger
                                              └── Intern adapter (sync + async)
```

| Layer | Owns |
|-------|------|
| **Electron** | UI, mirrors status, opens visuals/traces/pools by digest/id; annotations overlay |
| **Local daemon** | Orchestration, SQLite, CAS, usage ledger, adapters |
| **Cloud** | Sealed Trace V5, Artifact Platform, Intern/Factory/Effort truth, remote pools |

**Never from renderer:** `/smr/research-intern/*`, MLX, secrets, second Intern mailbox invent.

**One Intern mailbox.** Desktop does not invent a parallel commander-queue. Dual lanes in Cloud desk = Intern mailbox (authority) vs Codex evidence stream — not two competing authorities.

---

## 4. Full product scope (this thread + docs)

In scope for Desktop as product:

| Pillar | Spec |
|--------|------|
| **Visual home for agent R&D** | Primary framing |
| **Local Laguna XS 2.1** | MLX on Mac; private/local-file reasoning, coding, research |
| **LoRAs first-class** | Local + remote adapters; picker + Settings → Finetunes |
| **Remote models via Codex App Server + ACP** | OpenRouter Luna, OpenRouter Poolside S2.1; **usage tracked locally** |
| **Intern Live (sync)** | Sessions managed like Codex manages Codex sessions |
| **Intern Background (async)** | Pinned panel; leave-safe; mailbox authority |
| **Artifacts-style visuals** | Trace V5 + rewards + annotations; Craftax-class panes; agent can “show” / user clicks chip |
| **Containers / pools** | Later Inventory surface (local + remote) |
| **Artifact Platform** | Cloud durability — Desktop tracks ids/digests, does not own sealed bytes |

### Architecture figure (condensed)

```text
╔══════════════════════════════════════════════════════════════════════════╗
║ SYNTH DESKTOP (Electron)                                                 ║
║  Sessions+targets │ Transcript+chips │ Visual pane │ Runs/Trace+annots │ Inventory ║
╚═══════════════════════════════╤══════════════════════════════════════════╝
                                │ Synth Protocol (SSE / IPC)
╔═══════════════════════════════╧══════════════════════════════════════════╗
║ LOCAL DAEMON — SQLite + CAS + usage + adapters (Laguna / ACP / Intern)   ║
║ Evidence: RuntimeEvent → optional seal Trace V5 + annotation overlay     ║
╚═══════════════════════════════╤══════════════════════════════════════════╝
                                │ publish / track (opt-in)
╔═══════════════════════════════╧══════════════════════════════════════════╗
║ CLOUD — Factory · Effort · Intern mailbox · Artifact Platform · pools    ║
╚══════════════════════════════════════════════════════════════════════════╝
```

### Data placement

| LOCAL ONLY | LOCAL + POINTER | CLOUD AUTHORITY |
|---|---|---|
| drafts / UI state | Intern session/job ids | sealed Trace V5 |
| Laguna weights / LoRAs | pool URLs + health | Artifact manifests |
| local usage ledger | visual revision ids | Trace catalog |
| unsynced CAS | publication digests | published Visuals |
| local containers | annotation overlay refs | Factory/Effort truth |

### Visualization loop

```text
agent / eval / container rollout
  → transcript chip when visual created
  → click / agent “show” → right Visual pane
  → pane binds TraceSource / publication / local CAS
  → scrubber + metrics
  → annotations ON TOP (never rewrite sealed trace)
```

Craftax reference: https://www.usesynth.ai/evals/craftax

---

## 5. IA decided in this pin-down (UX)

### Sidebar: **Chats/** and **Cloud/** only

User decision: keep IA simple — not a deep Local vs Cloud taxonomy.

| Section | Contents |
|---------|----------|
| **Chats** | Local Laguna conversations (Poolside-like chat list) |
| **Cloud** | Intern **sync sessions** (list, Codex-app-session metaphor) + **pinned Async Intern** panel |

Settings is separate (Finetunes / LoRAs). Landing remains first-run / download / ready.

### Visual language

- **Reference:** Poolside / Laguna desktop (light macOS shell) — layout reference, not a clone; chase **state clarity** over pixel parity  
- **Brand:** Synth MCMC / favicon mark (orange), **not** YC `$`  
- **Accent:** Synth orange (`#FF5C00` / `#F05F22` family in CSS)  
- **Light theme for M0** landing + chat; dark `smrChrome` workbench is M1+ inspector territory  

### Transcript UX (match Poolside)

- User bubbles: right, blue  
- Assistant text: left, plain  
- Activity / thoughts: left-aligned muted lines (`… Thought`, `… Searched once…`)  
- Expandable activity with wave/detail  
- **File reads:** left-justified **horizontal** row — file-type icon + `Read` + path (monospace). Not stacked/centered  
- File icons: Markdown `M↓`, Rust gear, TS/JS/Py/cfg/generic  
- Visual cue: `… Created visual · …` + Show/Hide; **click toggles** Visual pane (no auto-open)  
- Visual card under turn; Visuals rail icon also toggles  
- Permission cards (Allow Once / Always Allow / Deny) — Poolside pattern; **not mocked yet**  
- Composer: model chip with Synth mark, Always ask, mic/send; placeholder must not stick on “No model available” while downloading  

### Cloud desk UX

- Messaging panel + activity  
- Dual lanes: **Intern mailbox (authority)** vs **Codex evidence** (`/smr/internal/codex-activity/stream` shapes)  
- Toggle/filter: **All / Mailbox** (commander-queue only for instance + user)  
- Side-by-side polished layout (first stacked pass was rejected as ugly)  
- Sync: openable session desk; Async: leave-safe banner, phase chips, needs-input  

### Models / LoRAs

Composer menu groups:

| Group | Targets |
|-------|---------|
| **Local** | Laguna XS 2.1 (+ inline Laguna LoRAs) |
| **Remote** | OpenRouter Poolside S2.1, Luna (Codex/ACP · usage tracked) |
| **Cloud** | Intern Live / Background |

- Chip shows `Laguna · {LoRA}` when adapter active  
- “Manage finetunes in Settings…”  
- **Settings → Finetunes:** base + local/remote adapters (`AVAILABLE_LORAS`)  
- Menu must not clip (`overflow: visible` on composer)  

### Chrome

- macOS `hiddenInset` + drag regions (titlebar / non-interactive chrome) so the window is easily movable  
- Dock/app icon: `resources/icon.icns` / `icon.png` from Synth logo; `app.dock.setIcon` in main  
- Synth mark on composer model chip always  

---

## 6. What’s implemented in the mock (today)

### Surfaces

| Surface | Status | Key files |
|---------|--------|-----------|
| Landing + 5 scenarios | Yes | `LandingPage`, `ScenarioPicker`, fixtures |
| Sidebar Chats / Cloud | Yes | `Sidebar.tsx` |
| Local chat transcript | Yes | `ChatTranscript.tsx` |
| File-type icons (left row) | Yes | `FileTypeIcon.tsx` |
| Visual pane (Craftax-class) + toggle | Yes | `VisualPane.tsx`, `App.tsx` `toggleArtifact` |
| Cloud desk sync + async | Yes | `CloudDesk.tsx` |
| Mailbox vs All filter | Yes | Cloud desk |
| Composer Local/Remote/Cloud + LoRAs | Yes | `Composer.tsx` |
| Settings → Finetunes | Yes | `SettingsPage.tsx` |
| Synth logo + dock icon | Yes | `SynthLogo.tsx`, `resources/icon.*`, `main/index.ts` |
| Model download bar | Yes | `ModelDownloadBar.tsx` |
| Permission cards | **Not yet** | — |
| Runs / Trace scrubber + annotations | **Not yet** | — |
| Inventory (containers/pools) | **Stub / later** | — |
| Daemon / MockRuntime | **Out of scope for mock** | fixtures only |

### Execution targets (fixtures)

- Local Laguna (+ LoRA via Settings / picker)  
- OpenRouter Poolside S2.1, Luna  
- Intern sync / async  

### Notable fixtures

- Emerald porting chat: `PROGRESS.md` + `gold_rust/src/lib.rs` file reads  
- Craftax cost-vs-performance visual artifact  
- Sync session + async Intern pin with mailbox + Codex activity shapes  

### Refs / screenshots

`apps/mock/refs/` — Poolside captures, polish before/after. First-pass zip attempts in Downloads were **reviewed for implied UX only** — do not wire those builds in yet.

---

## 7. Repo layout

```text
workshop/apps/mock/
├── HANDOFF.md                 ← this file
├── README.md
├── package.json
├── electron.vite.config.ts
├── resources/                 # icon.icns, icon.png, iconset
├── refs/                      # visual references
└── src/
    ├── main/index.ts          # window chrome, dock icon, drag
    ├── preload/index.ts
    └── renderer/src/
        ├── App.tsx            # views: landing | chat | sync | async | settings
        ├── fixtures/landingScenarios.ts
        ├── types/landing.ts   # EXECUTION_TARGETS, AVAILABLE_LORAS, artifacts
        ├── components/
        │   ├── Sidebar, LandingPage, Composer, ChatTranscript
        │   ├── CloudDesk, VisualPane, SettingsPage, FileTypeIcon
        │   ├── SynthLogo, ModelDownloadBar, ScenarioPicker
        └── styles/app.css
```

**Planned later (not created):**

```text
workshop/packages/runtime-protocol/
workshop/packages/runtime-client/     # DesktopRuntime + MockRuntime
workshop/packages/desktop-ui/
workshop/fixtures/scenarios/          # JSON tapes
workshop/services/local-runtime/      # Python daemon
```

---

## 8. Session decisions log (user → product)

Chronological constraints from the pin-down thread — treat as requirements:

1. Iterate mock until it works and looks like **Laguna / Poolside** desktop.  
2. Stay **UI-focused**; fixtures only.  
3. Window must be **easily draggable** (hiddenInset + drag regions).  
4. Real **Synth MCMC favicon logo**, not YC `$`.  
5. IA: split **local vs cloud**; cloud = Intern; sync sessions like Codex manages sessions; **pinned async Intern**.  
6. Simplify labels to **`Chats/`** and **`Cloud/`**.  
7. Review first-pass Downloads builds for **implied UX** only — don’t adopt yet.  
8. Mock sync session + async content; ground streams in **backend mailbox + raw Codex activity** APIs.  
9. Local chats must **open transcript**.  
10. Show **model name** and align chrome.  
11. Cloud: **messaging panel** + Codex thinking/tool transcript from internal API.  
12. Toggle to show **only commander-queue mailbox** (instance + user).  
13. Fix ugly stacked Cloud layout → polished dual pane.  
14. Local Codex App Server + ACP should make OpenRouter Luna / Poolside S2.1 easy, with **local usage tracking**, alongside local Laguna.  
15. Desktop = **visual home**; Claude-Artifacts-like; agent can show / user clicks; Craftax-class.  
16. Visuals panel + icon → full Craftax visual on the right; **click in and out**.  
17. Local/remote **model picker**.  
18. **LoRAs first-class** in picker; Finetunes live in **Settings**.  
19. Full architecture figure + data placement (Electron / daemon / cloud) is in-scope product truth.  
20. Synth mark on model chip / dock.  
21. **File-type icons** on activity lines.  
22. File activity **left-justified** like Poolside (icon beside path, not centered stack).  

---

## 9. What NOT to do

- Don’t call Intern HTTP / research-intern from the renderer  
- Don’t fork Next.js BFF or Clerk auth into Electron  
- Don’t put MLX / orchestration inside this Electron app  
- Don’t invent a second Intern mailbox  
- Don’t chase Poolside pixel-perfect copy — chase state clarity + Synth brand  
- Don’t remove scenario picker until Playwright golden paths exist  
- Don’t wire Download first-pass Electron zips as dependencies yet  
- Don’t auto-open Visual pane; toggle only  
- Don’t rewrite sealed Trace V5 with annotations — overlay only  
- Don’t commit unless asked  

---

## 10. Recommended next work

### Still mock / UI (highest leverage)

1. **Permission cards** — Allow Once / Always Allow glob / Deny (+ shortcuts) in transcript  
2. Richer activity stream (more Poolside parity: wavy expanders, multi-file batches)  
3. Runs / Trace scrubber stub + annotation overlay mock  
4. Inventory stub: models / adapters / (later) containers & pools  
5. Playwright screenshot goldens for landing + chat + cloud desk  

### M1 — leave fixtures-only when UX settles

1. `DesktopRuntime` + `MockRuntime` event tapes  
2. Landing → active chat with fake streaming  
3. Extract `runtime-protocol` / `runtime-client` / `desktop-ui` when layout stabilizes  
4. Daemon skeleton later — renderer still only talks to `DesktopRuntime`  

### Intern wiring (M2+, via daemon)

Reuse patterns from:

- `handoff-package/excerpts/frontend/researchIntern.ts`  
- `handoff-package/references/CODEX_ACTIVITY_STREAM_HANDOFF.md`  
- Intern sync/async clarity plans under `handoff-package/plans/`  
- **Not** from renderer direct  

---

## 11. Key references

| Doc | Path |
|-----|------|
| Product thesis + V1 cut | `workshop/HANDOFF.md` |
| Packaged eng brief / architecture | `workshop/handoff-package/docs/01-ENG-BRIEF.md`, `03-V1-ARCHITECTURE.md` |
| Implementation sequence | `workshop/handoff-package/docs/04-IMPLEMENTATION-SEQUENCE.md` |
| Mock plan / gamebench parity | `workshop/mock_app_plan.md` |
| Intern interaction law | `handoff-package/plans/intern_frontend_interaction_spec.md` |
| Codex activity stream | `handoff-package/references/CODEX_ACTIVITY_STREAM_HANDOFF.md` |
| Craftax visuals reference | https://www.usesynth.ai/evals/craftax |
| Poolside layout refs | `apps/mock/refs/` |

---

## 12. Quick edit map

| Want to change… | Edit… |
|-----------------|-------|
| Scenario / fixture data | `fixtures/landingScenarios.ts` |
| Types / LoRAs / targets | `types/landing.ts` |
| Colors / alignment | `styles/app.css` |
| Views / artifact toggle | `App.tsx` |
| Sidebar IA | `Sidebar.tsx` |
| Local transcript / file icons | `ChatTranscript.tsx`, `FileTypeIcon.tsx` |
| Visual pane | `VisualPane.tsx` |
| Cloud desk / mailbox filter | `CloudDesk.tsx` |
| Model menu / LoRA chip | `Composer.tsx` |
| Finetunes | `SettingsPage.tsx` |
| Logo | `SynthLogo.tsx` + `assets/` |
| Window / dock | `src/main/index.ts`, `resources/` |

---

## 13. Open questions (carry forward)

1. Theme split — keep chat light forever, or dark workbench when Runs/inspector lands?  
2. Routing — React view state vs `react-router`?  
3. When to extract `desktop-ui` packages (recommend after permission cards + trace stub stabilize)?  
4. First oracle fixture — which Intern sync session to record from synth-dev?  
5. Inventory IA — top-level nav vs Settings subsection?  

---

## 14. Framing paragraph (shareable)

> Synth Desktop lets you work with a fast private **Laguna XS 2.1** (+ LoRAs) on your Mac, call remote models through **Codex App Server / ACP** with local usage tracking, and hand work to **Synth Intern** (live sessions or background). The app is a **visual home** for agent R&D: transcripts, Artifacts-style visuals (Craftax-class), and later traces/annotations/pools — all inspectable without becoming an IDE clone. This mock pins **Chats + Cloud** chrome and the visual/model/LoRA UX before the local daemon exists.
