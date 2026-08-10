# Workshop v0.1 launch-gate implementation plan

**Scope:** testing, evaluations, QA, fuzzing, orchestration, and release evidence only  
**Explicit non-goal:** improving or expanding Workshop product functionality  
**Product contract:** [`launch_v0p1.md`](launch_v0p1.md)  
**Scenario catalog:** [`qa_cua_end_to_end.md`](qa_cua_end_to_end.md)  
**Auth input:** `/Users/joshuapurtell/Documents/Codex/2026-08-09/pl/outputs/HANDOFF_AUTH_CLOSEOUT_ERGONOMICS.md`

## Implementation status — 2026-08-09

The initial gate framework is implemented under `/Users/joshuapurtell/Documents/GitHub/evals/workshop` without changing Workshop product behavior:

- `gate:pr`: readiness audit plus typecheck, gate unit/property tests, Workshop static checks, Playwright, Bombadil, and Rust contracts.
- `gate:preflight`: read-only named Workshop eval-driver, local frontend, device-init JSON, local slot health/ownership/contract, authenticated MLX health, and Craftax health checks.
- `gate:local`: PR gate plus real independently graded coding tasks through Synth slot and local MLX, real Craftax catalog/rollouts, live visual creation, and required Trace correlation proof.
- `gate:release`: deterministic + live gates plus exact artifact identity and a matching, fresh 37-item CUA/manual receipt.
- `gate:verify`: revalidates receipt status, summary, source revisions, artifact hash, evidence presence, evidence hashes, receipt freshness (24 h), and evidence redaction.
- `gate:negative-control`: plants one fake canary per secret pattern class and requires the scanner to detect every class; `NEGATIVE-CONTROL-SECRET` failing (`RED_SECURITY`) means the scanner itself is broken.
- Seeded state-machine fuzzing runs 10,000 session/run/panel transitions per gate unit pass.
- The coding fixture starts red by construction, preventing a no-op agent from receiving credit.

### Hardening pass — 2026-08-09 evening

A review found integrity holes that could let a green receipt lie; all were closed without changing Workshop product code:

- **Live grading is real.** The provider-parity check now byte-compares the fixture's test files against the originals (agent tampering fails) and runs the fixture's own `npm test` in the post-agent workspace; exit 0 is required. Evidence files are sha256-hashed into the receipt.
- **Skip-dodging is caught.** `PRODUCT-NO-XFAIL` matches `test.fail`, `test.skip`, `test.fixme`, `describe.skip`, `it.skip`, `xit`, `xdescribe`, and `todo` markers, reporting per-marker counts.
- **Hollowed suites are caught.** Suites with parseable runner output enforce minimum pass-count floors calibrated from real prior logs (Playwright ≥ 75, Rust ≥ 160, static ≥ 40, harness unit ≥ 8). Bombadil reports no count and is honestly exempt.
- **Artifact trust is automated.** `RELEASE-ARTIFACT-SIGNATURE` runs `codesign --verify --deep --strict`, `spctl` assessment, and `xcrun stapler validate`, fail-closed on missing tools or non-macOS.
- **Dirty trees block release.** `SOURCE-CLEAN-*` checks fail the release lane when either repo is dirty or its revision is unresolvable (informational in pr/local lanes).
- **Manual evidence is verifiable.** All 37 CUA items require evidence that is an existing absolute path or URL; files are hashed at grade time and `gate:verify` re-hashes them. Artifact/revision binding fails loudly instead of silently skipping. Five new items cover the former coverage gaps: web funnel (CUA-033), GEPA visualization (CUA-034), deferred-Intern absence/isolation (CUA-035), release signing (CUA-036), and performance/memory (CUA-037).
- **Secret scanning is broader.** AWS, GitHub (classic + fine-grained), GitLab, Slack, JWT, and npm token patterns added; the negative control proves detection of every class (13/13).
- **MLX auth boundary.** `INFRA-MLX-AUTH-BOUNDARY` requires unauthenticated requests to the MLX loopback to be rejected (401/403); an unconfigured bearer is a blocking fail.
- **Craftax rollouts must differ.** Two seeds must produce substantively different rollout payloads (ids stripped, per-rollout step/reward evidence required), not merely distinct id strings.
- **Receipts are honest about themselves.** Live lanes report actual session/container cleanup via `LIVE-CLEANUP-*` checks instead of a hardcoded "read-only" claim; a crash writes an `INTERRUPTED` receipt with a DO-NOT-SHIP verdict instead of no receipt; the markdown receipt carries source revisions, dirty flags, and artifact identity.

Remaining known limits: receipts are unsigned (trusted-operator model); Bombadil has no pass-count floor; the Trace V5 correlation route is still an intentional hardcoded blocker; performance budgets and the website funnel are covered by manual CUA items, not automation.

The first run against the current Workshop correctly returned `RED_PRODUCT`. It found unresolved expected-failing product tests, renderer launch-debt markers, a real Playwright failure in the Set up agent path, and a Bombadil watchdog/harness failure. The consolidated current readiness run also remains red for missing exact artifact qualification and 32 pending native CUA checks. Live preflight separately returned `RED_INFRA`: the local frontend was absent, the MLX health route required an unprovided bearer, the selected eval driver was stale/unreachable, and slot1 was actively claimed plus contract-degraded. This separation is intentional; infrastructure failure cannot erase the independent product-readiness failures.

## 0. Desired outcome

Build a small set of gates such that a green release candidate gives us strong evidence that a real customer will have a great first experience with the core product:

1. Open the local usesynth.ai frontend and sign in or continue locally.
2. Open a local Workshop development instance.
3. Run a Codex research-engineering agent through the local Synth slot or local MLX Responses sidecar.
4. Use Containers, Visuals, and Traces beside the active agent without losing state or context.
5. Stop, recover, restart, and reopen the work successfully.

The gates must be reproducible, evidence-producing, aggressively adversarial, and strict about false success. They should test the system we ship—not a parallel mock application—but should use deterministic fixtures where determinism is the property being tested.

## 1. The test topology

```text
                         ┌──────────────────────────┐
                         │ launch-gate orchestrator │
                         │ evals/workshop           │
                         └────────────┬─────────────┘
                                      │ run manifest + evidence
             ┌────────────────────────┼─────────────────────────┐
             │                        │                         │
             ▼                        ▼                         ▼
  ┌───────────────────┐    ┌────────────────────┐    ┌─────────────────────┐
  │ local frontend    │    │ local Workshop     │    │ local synth-dev slot│
  │ auth/device/API UI│◀──▶│ debug instance     │◀──▶│ backend/API/Intern  │
  │ dedicated E2E auth│    │ eval-driver v1     │    │ provider services   │
  └───────────────────┘    └─────────┬──────────┘    └─────────────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    ▼                ▼                ▼
            ┌──────────────┐ ┌──────────────┐ ┌─────────────────┐
            │ MLX Responses│ │ Craftax     │ │ local code/work │
            │ sidecar      │ │ container   │ │ fixture repo    │
            └──────────────┘ └──────┬───────┘ └─────────────────┘
                                    │
                             Visuals + Traces
```

### Canonical local lane

- **Frontend:** the local `frontend` checkout, configured against the chosen local slot and a dedicated Clerk development/E2E instance.
- **Slot:** one explicitly claimed and healthy `synth-dev` slot. The orchestrator records the slot ID, stack contract, source identities, service health, and claim owner. It must not assume slot1 is free; slot1 is the preferred launch topology only when safely available.
- **Workshop:** a named local debug instance started with its own data root and `synth.eval-driver.v1` enabled.
- **Local inference:** the real MLX Responses-compatible sidecar, with a supported prepositioned model for the full lane and a fake backend only for deterministic lower-layer tests.
- **Containers:** the real registered loopback Craftax service for live/eval gates; a contract-faithful fault server for malformed/recovery tests.
- **Workspace:** a generated, disposable git repository containing a bounded research-engineering task, tests, known files, and no secrets.
- **Runner:** owned by `/Users/joshuapurtell/Documents/GitHub/evals/workshop`; it consumes public/loopback contracts and never imports Workshop internals.

### Configuration contract

One checked-in, non-secret lane file should describe logical endpoints and requirements; one generated run manifest resolves exact ports, paths, revisions, and secret *names/hashes only*.

```toml
id = "workshop-v0p1-local"
frontend_mode = "local"
slot = "slot1"
workshop_instance = "launch-gate"
inference = ["synth_slot", "local_mlx"]
container = "craftax"
auth = "clerk_dev_e2e"

[budgets]
wall_clock_minutes = 60
cloud_usd = 10
local_model_gb = 30
craftax_rollouts = 40
```

No command may print credential values. Preflight reports presence, source, expiry where safe, and a stable redacted fingerprint.

## 2. Gate hierarchy

The system has four required lanes. A higher lane never replaces a lower lane; each catches a different class of defect.

### Gate A — deterministic change gate

**When:** every relevant PR/change; target ≤10 minutes.  
**System:** built renderer + faithful bridge fixtures, Rust/service unit/contract tests.  
**Purpose:** fast, exhaustive state and UI invariants.

Required:

- Typecheck, static accessibility/debt locks, visual registry tests.
- Rust storage, session lifecycle, eval-driver, trace, container, and credential-custody contract tests.
- Playwright directed workflows for agent lifecycle, provider state, panels, auth states, recovery, keyboard, and geometry.
- Short deterministic Bombadil seed corpus.
- Schema compatibility between `evals/workshop` vendored driver types and `synth.eval-driver.v1`.

Green means the expected product state machine is internally coherent. It does not claim that native/local services work.

### Gate B — local integration gate

**When:** merge/nightly and before a release candidate; target ≤25 minutes.  
**System:** local frontend + claimed local slot + named local Workshop + real MLX sidecar + real Craftax container.  
**Driver:** existing authenticated loopback eval driver; browser automation for frontend; service APIs for preflight/evidence only.

Required:

- Auth through dedicated Clerk dev/E2E instance.
- One real agent task against the local slot/Synth API route.
- Same task against local MLX Responses.
- Containers → real Craftax rollout → Visuals → Traces path.
- Restart/reconnect/cancel recovery.
- Medium Bombadil/fault campaign against the real local lane where safe.
- Redaction scan and evidence-bundle validation.

Green means the local multi-process system works end to end with real inference and environments.

### Gate C — installed-app release-candidate gate

**When:** mandatory for every release artifact, within 24 hours of ship; target ≤45 minutes.  
**System:** actual packaged artifact plus local frontend/slot/sidecar/container.  
**Driver:** CUA/native UI for visible/native actions. Because `synth.eval-driver.v1` is debug-only, the release artifact must not expose it.

Required:

- Install/launch/pair/continue-local.
- One hosted/slot-backed Codex task and one local-MLX Codex task.
- Side-panel switching and real Craftax evidence.
- App/daemon/container interruption and recovery.
- Visual/responsive/keyboard/adversarial CUA sweep.
- Signed artifact identity, logs, video/screenshots, trace/run export.

Green means the artifact a customer receives behaves correctly. This lane must not be replaced with a debug instance merely because automation is easier.

### Gate D — production smoke

**When:** immediately before and after publish; target ≤10 minutes.  
**System:** production frontend/auth/API and published artifact/download.

Required:

- Device-init route returns JSON rather than redirect.
- One semi-manual pairing with a real standing test mailbox.
- Badge/account state flips and one low-cost cloud action succeeds.
- Download/version/checksum/signing links match the promoted artifact.
- Local-only path remains usable without auth.

Production Clerk cannot use `+clerk_test`/OTP `424242`; do not pretend this lane is fully automated.

## 3. What “green” must prove

### Core customer promises

- **Agent:** a real Codex task can inspect, edit, run, and explain work; events remain ordered and durable.
- **Inference:** local MLX and local-slot/Synth API paths preserve the same core session semantics where capabilities overlap.
- **Panels:** Visuals, Containers, and Traces remain correctly bound to the active agent/run through live updates, switching, resizing, collapse, restart, and reopen.
- **Recovery:** stop, cancellation, disconnect, process death, restart, and retry never create false success or duplicate paid work.
- **Trust:** local/cloud/provider identity, cost/usage, fixture/live data, permissions, and terminal states are honest.
- **Ergonomics:** signup/pair/first action is short, keyboard-usable, visually proportionate, and has one-action recovery.
- **Evidence:** every live run produces enough identity and provenance to reproduce or diagnose it.

### Hard fail rules

Any of these makes the gate red regardless of aggregate pass rate:

- Crash, hang, corruption, migration loss, secret leak, workspace escape, or remote loopback exposure.
- First-run, pairing, first real agent task, or reopen cannot finish.
- “Completed” without authoritative terminal evidence.
- Provider/model/local/cloud identity or cost state is wrong.
- Side panel shows data from the wrong session, run, seed, container, or trace.
- Composer/primary controls are clipped, covered, inaccessible, or focus-trapped.
- Cancellation leaves compute running beyond its defined reconciliation bound.
- Retrying duplicates a charged run or destructive action.
- Craftax visual uses a fixture or mismatched frame while claiming to be live.
- Release artifact differs from the artifact/revision tested.

## 4. Test assets to build

### 4.1 `evals/workshop` launch orchestrator

Extend the current runner rather than adding a second driver.

Proposed structure:

```text
evals/workshop/
  cases/
    launch-smoke.toml
    agent-provider-parity.toml
    panels-craftax-trace.toml
    auth-local-dev.toml
    recovery-chaos.toml
  runner/
    preflight.ts
    topology.ts
    run.ts
    evidence.ts
    redact.ts
  scenarios/
    agent-task.ts
    auth.ts
    panels.ts
    craftax.ts
    recovery.ts
  faults/
    responses-server.ts
    container-server.ts
    stream-proxy.ts
  schemas/
    run-manifest.schema.json
    scenario-result.schema.json
    evidence-index.schema.json
  results/<run-id>/
```

Runner responsibilities:

- Resolve and validate topology without mutating unhealthy/claimed shared infrastructure automatically.
- Read the Workshop eval-driver descriptor and assert protocol/source revision.
- Generate unique run/case/session IDs and a disposable workspace.
- Enforce wall-clock, token/cost, rollout, disk, and retry budgets.
- Execute ordered scenarios with cleanup in `finally`/signal handlers.
- Capture exact redacted requests/results, terminal evidence, screenshots references, traces, metrics, and service health.
- Emit machine-readable JUnit/JSON plus a concise human report.
- Exit nonzero on hard failure, missing evidence, stale evidence, budget overrun, or cleanup leak.

### 4.2 Disposable research-engineering workspace

Create a tiny deterministic repo that feels like actual coding work:

- A small typed package with one behavioral bug, unit tests, README, and git history.
- Task: diagnose the bug, edit the implementation, run tests, explain the cause, and produce a small result artifact.
- Grader checks the repository and tests independently; it never grades the agent's self-report.
- Seeded variants change names/data/order while preserving the same underlying skill.
- Workspace is copied to a per-case temp directory and destroyed only after evidence is sealed.

This becomes the parity task for local slot vs local MLX. We compare outcomes and lifecycle semantics, not exact prose.

### 4.3 Auth gate

Implement the handoff's two-target law:

- **Automated local/staging lane:** local frontend wired to a dedicated Clerk dev E2E instance; disposable `e2e-*+clerk_test` users; OTP `424242`; existing user, new user, redirect preservation, expiry, denial, `ORG_MISSING`, duplicate completion, local-only choice, local-only sign-out semantics.
- **Production smoke:** real test mailbox, device-init JSON check, one pairing, badge flip, one action. No fake OTP.
- Drive Workshop through the existing eval driver in the debug/local lane; use native CUA for the installed artifact.
- Measure action count and time-to-first-action with the counting rule from the auth handoff.
- Add a janitor with a dry-run default, exact `e2e-*` scope, maximum deletion count, and receipt.

### 4.4 Provider parity gate

For the same disposable task, run:

1. Codex through local slot/Synth API-compatible route.
2. Codex through local MLX Responses sidecar.

Assert:

- Session create/send/stream/terminal/export/reopen works.
- User → activity/tool → assistant chronology is valid.
- Stop/cancel is authoritative and bounded.
- Commands and file changes appear and persist.
- Trace includes provider/model identity and tool/artifact evidence.
- Grader passes task outcome.
- No secret or internal credential enters transcript/export.
- Capability differences are declared, not silently degraded.

Do not require identical tokens, wording, tool count, or latency.

### 4.5 Side-panel coherence gate

Build a directed state machine around the real session:

```text
chat only
  → attach healthy Craftax container
  → open Containers
  → launch rollout
  → open live Visual
  → switch to Trace
  → select correlated event/frame
  → return to chat
  → collapse/reopen panel
  → restart Workshop
  → reopen session and evidence
```

At every transition assert:

- Active session/run/container/visual/trace IDs are correct.
- Composer and primary actions remain visible and usable.
- Focus and selection behavior is intentional.
- Stream freshness/staleness and terminal status are truthful.
- Counts/events/frames cannot cross-bind between two concurrent sessions or seeds.
- Side-panel state either persists by contract or resets visibly; never ghost state.

### 4.6 Evidence validator

A run is invalid—not merely incomplete—if required evidence is missing.

Required index:

- Run ID, timestamps, lane, status, failure classification.
- Workshop/frontend/backend/evals/synth-dev/container/model revisions or immutable identities.
- Slot ID and redacted stack/health contract.
- OS/arch, release/debug artifact identity, viewport/display scale.
- Scenario seeds, workspace hash, provider/model, prompt/task hash, budgets.
- Eval-driver protocol and Workshop source revision.
- Session export, trace/artifact hashes, Craftax rollout IDs/seeds/terminal records.
- Screenshots/video pointers for native/CUA scenarios.
- Test logs and cleanup receipt.
- Redaction-scan result.

The validator rejects secrets, absolute temporary paths without a portable mapping, inconsistent revisions, incomplete terminal records, or evidence older than the run.

## 5. Playwright plan

Use Playwright for deterministic state breadth and frontend browser flows. Do not claim native coverage from it.

### P0 directed specs

1. `agent-core-gate.spec.ts`
   - create/send/stream/activity/tool/result/stop/retry/reload/reopen
   - long prompt/output, multi-turn queue, partial stream, malformed/out-of-order events
2. `research-panels-gate.spec.ts`
   - Visuals/Containers/Traces open/switch/resize/collapse/reopen
   - correct binding across two sessions/runs, live updates, stale/error/terminal states
3. `provider-parity-ui.spec.ts`
   - same capability/status/cancel/provenance presentation for Synth API and local MLX
   - explicit, tested handling for declared capability differences
4. `auth-first-run-gate.spec.ts`
   - local choice, sign-in, pairing/expiry/denial/error/account states, local-only sign-out copy
5. `recovery-gate.spec.ts`
   - app-server loss, stream break, daemon/container loss, restart reconciliation, duplicate terminal events
6. `launch-visual-gate.spec.ts`
   - all P0 surfaces across 960×640, 1280×840, 1440×900 plus a narrow continuous-resize sample
7. `trust-gate.spec.ts`
   - fixture banners, provider/local/cloud badge, cost/usage source, disabled/dead controls, raw internal errors, redaction

### Fixture contract

- Every fixture event must validate against production protocol types.
- Every real failure found gets a minimized deterministic Playwright reproduction where possible.
- Fixture success cannot satisfy the live gate; specs declare `fixture`, `contract`, or `live` evidence class.
- Random fixture generation records seed and minimized counterexample.

## 6. Bombadil/stateful fuzzing plan

Bombadil should model dangerous interaction sequences, not just resize the page.

### Model state

- Current route/session/provider/model/run status.
- Composer/queue/stream state.
- Panel mode/open/width/selection/binding/freshness.
- Container health and rollout counts.
- Trace/visual existence and selected evidence.
- Auth/account/local-only state.
- Terminal open/process state.

### Generated actions

- Create/switch/rename/search session.
- Type/send/queue/edit/remove/stop/retry.
- Switch provider/model and simulate availability changes.
- Open/switch/resize/collapse panels; select visual/container/trace item.
- Attach/probe container; start/cancel rollout; interrupt/resume stream.
- Toggle terminal/settings/sidebar; resize viewport; keyboard navigation.
- Reload renderer; inject duplicate/late/out-of-order/malformed events.
- Expire auth, revoke provider, kill health, recover service.

### Always invariants

- No uncaught/console errors.
- Composer and primary controls remain visible, reachable, and non-overlapped.
- At most one authoritative active run per session unless contract explicitly permits more.
- A terminal run never becomes running again without a new run ID.
- Panel object belongs to the displayed session/run or is clearly pinned as external.
- Live frames/events match the selected rollout/seed.
- Counts never go negative and aggregate counts reconcile.
- No success state without terminal evidence.
- No fixture/live or local/cloud identity confusion.
- Focus remains within visible UI; Escape/back behavior does not discard work.
- Secret canaries never render or enter captured logs.

### Campaigns

- **PR:** fixed corpus of minimized regression seeds, 30–60 seconds.
- **Nightly:** rotating deterministic seeds, 10–20 minutes, failure shrinking enabled.
- **Release:** fixed certified seed corpus plus fresh random seeds, ≥30 minutes across state/viewport variants.
- Persist seed, action trace, minimized trace, screenshot, DOM/state extraction, and environment identity.

Bombadil's intermittent post-time-limit hang is itself a harness defect. Add a parent watchdog and distinguish `VIOLATION`, `HARNESS_HANG`, and `PRODUCT_HANG`; none counts as green.

## 7. Protocol and service fuzzing

Use property-based/unit fuzzing beneath the UI and fault injection at service boundaries.

### Rust/property targets

- Journal event append/replay/dedupe/order and restart reconciliation.
- Trace V5 parse/import/hash/path handling.
- Eval-driver HTTP parser: body/header limits, auth, method/path, malformed JSON, protocol mismatch.
- Container/visual binding identifiers and loopback URL validation.
- Responses event compilation, cancellation, terminalization, usage counters.
- Migration from prior schema with interrupted/repeated execution.

Prefer `proptest` for structured Rust state machines and `cargo-fuzz`/libFuzzer for byte-level parsers where the crate boundary supports it. Every discovered crash gets a checked-in minimized corpus case.

### Fault proxies

Create reusable loopback proxies/servers that can deterministically:

- Delay first byte or arbitrary chunks.
- Split SSE/JSON at every byte boundary.
- Duplicate, reorder, omit, or corrupt nonterminal events.
- Disconnect before/after terminal event.
- Return 401/403/404/408/409/429/500/502/503 and invalid bodies.
- Stall cancellation, then report authoritative completion/failure.
- Serve stale/wrong rollout frame IDs.
- Become unhealthy between preflight and launch.

Each fault has an expected product state and bounded recovery time. The gate grades authority/recovery, not whether an error toast exists.

## 8. Native CUA plan

CUA is the final judge of the installed experience and should be short, scripted, and evidence-rich.

### Required scripts

- **CUA-1 First-time user:** local frontend → signup/pair or continue local → choose inference → first real agent task.
- **CUA-2 Research loop:** agent → Containers → Craftax launch → Visuals real frame → Traces correlated evidence → agent summary.
- **CUA-3 Recovery:** stop, kill sidecar/container/app at named phases, reopen/reconcile, verify no false success/duplicate spend.
- **CUA-4 Adversarial polish:** keyboard, focus, continuous resize, long content, multiple queue entries, panel transitions, errors, settings/diagnostics.
- **CUA-5 Upgrade:** install over prior seeded data, migrate, reopen historical session/trace, then run fresh task.

### CUA pass criteria

- Every action has an observable expected result and timeout.
- Automation records a screenshot before/after important transitions and video for the golden paths.
- Native dialog/browser/app focus changes are included.
- Image/OCR-only matching is avoided for IDs/state when accessibility or exported evidence exists.
- A human reviews the final visual/ergonomic evidence; CUA success alone is not sufficient for taste.

## 9. Chaos and soak gates

### Bounded chaos matrix

Apply one failure at a time first, then pairwise failures for high-risk boundaries:

| Boundary | Failures | Required recovery |
|---|---|---|
| Frontend ↔ auth | redirect loss, expiry, denial, 5xx | one visible retry; local path intact |
| Workshop ↔ Codex | child exit, malformed/late event, restart | durable interruption; safe resume/new run |
| Workshop ↔ local MLX | refused, OOM, stall, disconnect | honest state; cancel/reload/unload |
| Workshop ↔ slot/API | 401, 429, timeout, 5xx | distinct error; no duplicate paid retry |
| Workshop ↔ container | health flip, stream break, stale frame | block/reconnect; correct binding |
| Journal/CAS | duplicate event, missing/corrupt blob | idempotence or explicit corruption |

### Soak

- Two-hour local lane: repeated short agent tasks alternating local slot/local MLX, side-panel activity, and periodic cancellation/restart.
- Forty-rollout Craftax representation with supported concurrency and bounded frame rate.
- Record Workshop, MLX, container, and slot CPU/memory/file-descriptor/process counts.
- Gate monotonically growing memory, orphan processes, stuck slots, journal growth anomalies, stale health, and Bombadil/watchdog hangs.

Soak is nightly/release-candidate evidence, not a per-PR requirement.

## 10. Preflight, cleanup, and shared-slot safety

### Preflight is read-only

It must:

- Resolve repository revisions and dirtiness without modifying them.
- Check slot claim/health/source identity and refuse a foreign/active claim.
- Confirm frontend points to the intended slot and E2E auth instance.
- Confirm Workshop instance/data root are unique and eval-driver source revision matches.
- Confirm local MLX model/health, Craftax catalog/version, free disk/RAM, ports, credentials, and budgets.
- Verify all evidence/result directories are new and writable.

It must not silently reconcile/restart a shared slot, overwrite env files, switch branches, deploy, or delete accounts/data.

### Cleanup

- Stop only processes/rollouts created by the run and prove exact IDs.
- Release only the claim acquired by the run.
- Preserve failed workspaces and evidence; delete successful temp workspaces after sealing.
- Janitor operations are separately scoped, dry-run by default, capped, and receipted.
- On SIGINT/SIGTERM, terminalize the manifest as interrupted and attempt bounded cleanup.

## 11. Implementation sequence for you and me

### Phase 0 — freeze the contracts (half day)

Deliverables:

- [ ] Agree on Gate A/B/C/D definitions and hard-fail rules in this file.
- [ ] Choose the canonical local slot for the first implementation run after a read-only availability check.
- [ ] Pin frontend, Workshop, evals, synth-dev/backend, Craftax, and MLX revisions for the initial baseline.
- [ ] Define run-manifest, scenario-result, and evidence-index JSON schemas.
- [ ] Define the disposable coding task and independent grader.
- [ ] Record which release artifact is being qualified.

Division:

- **You:** confirm product-level hard-fail decisions, budget, supported hardware/model, and acceptable use of the local slot.
- **Me:** draft schemas, case files, exact command graph, and coverage-to-scenario map.

Exit: a dry-run manifest can fully describe the topology without starting or mutating anything.

### Phase 1 — orchestrator and evidence spine (day 1)

- [ ] Add topology/preflight/manifest/evidence/redaction modules in `evals/workshop`.
- [ ] Wrap existing driver client; do not add Workshop endpoints.
- [ ] Add bounded cleanup and signal handling.
- [ ] Add JUnit + JSON + Markdown report generation.
- [ ] Add evidence validator and canary-secret scan.
- [ ] Run a no-inference health/session/export smoke.

Exit: one command connects to a named Workshop instance, proves all identities, creates/exports a session, validates evidence, and cleans up.

### Phase 2 — deterministic P0 suite (day 1–2)

- [ ] Implement the seven Playwright P0 specs.
- [ ] Expand Bombadil model/actions/invariants and watchdog.
- [ ] Add protocol/property targets with initial regression corpus.
- [ ] Tag every test by scenario ID and evidence class.
- [ ] Produce a coverage report proving each hard-fail rule has at least one directed test.

Exit: Gate A is green and intentionally turns red for seeded representative defects.

### Phase 3 — real agent/provider parity (day 2)

- [ ] Generate disposable research repo/task variants and grader.
- [ ] Run via local slot/Synth API route.
- [ ] Run via local MLX Responses.
- [ ] Grade outputs and compare lifecycle/provenance semantics.
- [ ] Add cancel/restart/partial-stream variants.

Exit: Gate B proves the same core research task through both inference paths with valid traces/artifacts.

### Phase 4 — panels + Craftax + trace coherence (day 2–3)

- [ ] Register/probe real Craftax and verify stable seed catalog.
- [ ] Execute agent-led single rollout and small eval.
- [ ] Verify real frame/event/seed binding and Trace correlation.
- [ ] Execute side-panel state machine and two-session cross-binding adversary.
- [ ] Add SSE interruption/resume and unhealthy-between-preflight/launch faults.

Exit: one evidence bundle shows the complete agent → Container → Visual → Trace loop and restart/reopen.

### Phase 5 — auth/local frontend lane (day 3)

- [ ] Start local frontend against the claimed slot and dedicated Clerk dev/E2E instance.
- [ ] Automate existing/new/expiry/denial/org/redirect/local-only cases.
- [ ] Add action/time ergonomics measurements.
- [ ] Add disposable-account janitor safety tests.
- [ ] Define the thin production smoke separately.

Exit: automated local auth gate is green; production smoke runbook uses a real mailbox and cannot accidentally use dev OTP.

### Phase 6 — native CUA and release wrapper (day 3–4)

- [ ] Implement the five bounded CUA scripts and evidence capture.
- [ ] Qualify the actual packaged artifact; confirm debug eval driver is absent.
- [ ] Add a single release command that runs preflight → gates → evidence validation → summary.
- [ ] Make release receipt name exact artifact/revision and expire after 24 hours or any product/content revision change.
- [ ] Run defect-seeding audit to demonstrate the gates fail for wrong binding, false success, auth redirect loss, overlap, stale frame, and secret leak.

Exit: Gate C can issue a signed green/red receipt for one exact artifact.

### Phase 7 — chaos, soak, and release rehearsal (day 4+)

- [ ] Run fault matrix and two-hour soak.
- [ ] Minimize and promote all failures into deterministic regressions.
- [ ] Execute Gate A → B → C → D rehearsal with rollback/cleanup.
- [ ] Have a non-implementer review the evidence packet and attempt the golden path.

Exit: launch decision can be made from evidence, without trusting verbal “it worked” reports.

## 12. Proposed commands and receipts

Commands are names to implement; exact flags become stable only after Phase 0 schemas are pinned.

```bash
# Read-only topology validation
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:preflight -- \
  --lane local --slot slot1 --workshop-instance launch-gate

# Fast deterministic gate
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:pr

# Local frontend + slot + Workshop + MLX + Craftax integration
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:local -- \
  --manifest cases/launch-local.toml

# Installed artifact with CUA evidence
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:release -- \
  --artifact /absolute/path/to/SynthDesktop-v0.1.zip

# Validate an existing evidence bundle without executing anything
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run evidence:verify -- \
  results/<run-id>
```

Receipt statuses:

- `GREEN`: every required scenario/evidence item passed for the exact artifact and revisions.
- `RED_PRODUCT`: product assertion failed.
- `RED_INFRA`: required topology unavailable or unhealthy; never reported as product pass.
- `RED_HARNESS`: driver/fuzzer/CUA/evidence machinery failed or hung; never reported as product pass.
- `RED_SECURITY`: secret/boundary/signing violation; release blocked.
- `INTERRUPTED`: bounded cleanup attempted; rerun required.

## 13. Coverage accounting

Maintain one generated matrix joining:

```text
customer promise
  → risk / hard-fail rule
  → QA scenario ID
  → directed Playwright/contract test
  → Bombadil/property invariant
  → local live scenario
  → native CUA scenario
  → evidence fields
```

No percentage is sufficient by itself. Release confidence comes from:

- Every P0 promise having directed deterministic coverage.
- Dangerous cross-feature sequences having stateful/property exploration.
- Every external boundary having deterministic fault injection.
- Both inference paths completing a real independently graded task.
- The actual artifact completing native CUA.
- Failures producing reproducible counterexamples and never being hidden as infrastructure noise.

## 14. Decisions to make before implementation

These are the only meaningful choices still needed:

1. **Local slot:** prefer slot1 for the documented topology, but select only after read-only claim/health inspection; never displace active work.
2. **Supported local model/hardware:** name the exact MLX model revision and Mac class that make Gate B/C authoritative.
3. **Release budget:** approve cloud dollar/token and Craftax rollout ceilings; default proposal is $10 and 40 rollouts for the full release gate.
4. **Auth environment:** identify/create the dedicated Clerk dev E2E instance and account janitor authority.
5. **Artifact:** decide whether tonight's gate qualifies the ad-hoc alpha artifact or waits for signed/notarized packaging; the receipt must say which.
6. **Human bar:** name the non-implementer who reviews the final evidence and subjective CUA polish.

## 15. Definition of done for the gate project

The gate project is complete when one documented command can safely validate topology, exercise the exact core customer journey across the local frontend, local slot, local Workshop, local MLX sidecar, real Craftax, Visuals, and Traces, then emit a redacted self-validating receipt tied to the exact artifact and revisions—and when deliberately injected defects in lifecycle, binding, recovery, auth, visuals, and security reliably turn that receipt red.
