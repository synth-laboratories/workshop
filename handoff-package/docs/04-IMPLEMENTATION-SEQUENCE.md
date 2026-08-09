# Implementation Sequence

Optimize for **architectural truth**, not feature completeness.

---

## Milestone 0 — Repo bootstrap (1–2 days)

- [ ] Create `apps/desktop` Electron + React skeleton
- [ ] Create `services/local-runtime` Python package (`uv` / pyproject)
- [ ] Create `packages/runtime-protocol` shared types
- [ ] Vendor `contracts/research-v1.json`; script to generate TS types
- [ ] Dev script: start daemon + Electron against mock events

**Exit:** Hello-world IPC roundtrip.

---

## Milestone 1 — Runtime contract (3–5 days) ← do this first

Define and fixture-test:

```text
ExecutionTarget · Session · Run · RuntimeEvent · ArtifactRef · Metric · Outcome
```

- [ ] SQLite (or sqlite-wal) SessionStore + EventLog in daemon
- [ ] `subscribe(sessionId, afterSequence)` replay + live append
- [ ] Normalize fixtures for (a) local fake stream (b) recorded Intern SSE page
- [ ] Document cursor rules (no cross-target resume)

**Exit:** CLI or thin UI can replay a recorded Intern event page into the Run viewer.

**Why first:** locks the central abstraction before UI or MLX rabbit holes.

---

## Milestone 2 — Local Laguna path (parallelizable)

### 2a Stub (2–3 days)
- [ ] Fake streaming completions in daemon
- [ ] Electron prompt → stream → transcript
- [ ] Cancel + usage events

### 2b Real MLX (2–4 weeks, risky — can trail M3)
- [ ] `services/local-inference` MLX Laguna XS 2.1
- [ ] Load/unload, memory check, OpenAI-compatible stream
- [ ] Record `model` + `adapter: null` on every Run

**Exit (2a):** End-to-end local loop in UI.  
**Exit (2b):** Real tokens at usable latency on Apple Silicon.

---

## Milestone 3 — Intern sync (3–5 days)

- [ ] Daemon `InternAdapter` via `SynthClient().research.intern.sync_`
- [ ] create → send_message → tail/stream → map to RuntimeEvent
- [ ] Persist `sync_session_id`, `state_generation`, `after_sequence`
- [ ] Handle receipts: `applied` / `conflict` / `refused`
- [ ] UI: target picker “Intern · Live”

**Exit:** Same transcript UI works for Local stub and Intern sync against a real backend (synth-dev slot or hosted).

**Proof borrowed from web:** reconnect from cursor after restart (`fe_intern_sync_live_reconnect` spirit).

---

## Milestone 4 — Intern async (3–5 days)

- [ ] `async_.ensure` / `get` / `send` / `tail`
- [ ] Jobs panel: status from projection (`phase` / `resume` / `leave_safe` if present)
- [ ] Disconnect app → runtime keeps going → reconnect from cursor
- [ ] pause / resume / cancel **explicit** only
- [ ] `provide_input` for parked questions (minimal UI)

**Exit:** Background job survives Electron quit/relaunch.

This is the most important architectural milestone for cloud parity.

---

## Milestone 5 — Run / artifact viewer (1–2 weeks)

- [ ] Unified timeline for local + Intern events
- [ ] Follow `resource_refs` → open artifact/visual when available
- [ ] Extract/adapt `ArtifactFrame` patterns
- [ ] Basic metrics strip (tokens, elapsed, spend if Intern)

**Exit:** One Run page explains “what happened” without reading raw JSON.

---

## Milestone 6 — Eval hooks (3–5 days)

- [ ] `data-testid` / ARIA on primary controls
- [ ] Playwright can launch Electron or daemon+headless UI
- [ ] Document scoring against internal state (session status), not brittle DOM copy

Full desktop-as-benchmark environment stays later.

---

## First concrete coding PR (recommended)

**Title:** `runtime-protocol + local-runtime skeleton + event replay`

**Scope:**
1. `packages/runtime-protocol` types
2. `services/local-runtime` with in-memory/SQLite event log
3. Fixture JSON from a real Intern `events` page (capture once)
4. Minimal Electron window that renders the timeline from daemon subscribe
5. README: how to point `SYNTH_BACKEND_URL` for M3

**Out of scope for PR1:** MLX, Intern live calls, styling polish, Effort board.

---

## Parallelization

```text
          M1 runtime contract
           /        |        \
     M2a stub    M3 Intern sync   (types/codegen)
         |           |
     M2b MLX     M4 Intern async
           \        /
         M5 Run viewer → M6 eval hooks
```

---

## Definition of “first pass complete”

An external eng can:

1. Clone workshop + install desktop deps + `synth-ai`
2. Run Local stub chat in Electron
3. Configure API key → create Intern sync session → see live events
4. Start Async job → quit app → reopen → resume timeline
5. Explain architecture from `docs/03-V1-ARCHITECTURE.md` without inventing a second mailbox
