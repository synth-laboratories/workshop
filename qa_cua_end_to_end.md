# Synth Workshop v0.1 end-to-end QA plan

**Parent product contract:** [`launch_v0p1.md`](launch_v0p1.md)  
**Implementation plan:** [`launch_gate_implementation_plan.md`](launch_gate_implementation_plan.md)  
**Purpose:** executable launch checklist across Playwright, Bombadil, native Computer Use (CUA), service contracts, and scientific evals  
**Rule:** a screenshot alone is not proof of function; a passing API test alone is not proof of usable product behavior.

The consolidated native checklist and machine-graded receipt live at `/Users/joshuapurtell/Documents/GitHub/evals/workshop/manual/CUA_MANUAL_GATE.md`. A checked Markdown box is not release evidence: each JSON receipt item must include tester, timestamp, evidence, exact artifact SHA-256, exact Workshop revision, and must remain within its 24-hour validity window.

## 0. v0.1 quality priority

The release target is one exceptionally reliable research-engineering loop:

```text
Codex agent
  ├─ Synth API or local MLX Responses sidecar
  ├─ Container inspector: executable environment and rollout state
  └─ Shared visual/trace inspector: interpretation, evidence, provenance
```

QA should spend depth here before broadening feature coverage. Visuals, Containers, and Traces remain the three product primitives, but the shipped UI implements them as `ContainerPane` plus a shared `VisualPane` for visuals and Trace V5. A failure in provider parity, agent lifecycle, or that inspector relationship is a launch blocker even if secondary pages pass.

## 1. Test system and ownership

### Layer responsibilities

| Layer | Proves | Must not substitute for |
|---|---|---|
| Unit/static/type | local invariants, schemas, redaction, state transitions | rendered behavior or native integration |
| Playwright | deterministic browser UI behavior, geometry, keyboard, state/error matrices | native dialogs, packaged app, real daemon |
| Bombadil | visual alignment, clipping/overlap, density, style invariants | semantic correctness or paid/live execution |
| CUA | installed Mac app, browser handoff, native surfaces, real services, human-visible quality | deterministic exhaustive state coverage |
| Contract/integration | API/daemon/container/eval/trace/optimizer interoperability | user comprehension and polish |
| Scientific eval | model/prompt/harness quality and reproducibility | application UX or infrastructure health |

### Result statuses

- **PASS:** assertions and required evidence complete on the release candidate.
- **FAIL-P0:** launch blocked.
- **FAIL-P1:** fix before launch unless the feature is removed or explicitly relabeled/isolated.
- **XFAIL:** historical planning vocabulary only. The implemented launch gate rejects expected-fail, skip, fixme, and todo markers through `PRODUCT-NO-XFAIL`; release debt must be fixed, removed from the supported surface, or isolated outside v0.1.
- **N/A:** feature absent from build and marketing; must include why.

### Evidence packet per scenario

- Scenario ID, time, tester/automation layer, build revision/version.
- OS/architecture, viewport/display scale, account state, provider/model, service/container versions.
- Fixture/real-data flag, exact seed/task/split, prompt/harness hashes, budget/concurrency.
- Steps, assertions, terminal state, screenshots/video, redacted logs, trace/artifact/run IDs.
- Performance/usage/cost where relevant.
- Failure severity, owner, issue link, and retest evidence.

## 2. Environments and fixtures

### Release environments

- **PW-fixture:** deterministic renderer with controlled bridges and all state/error fixtures.
- **Native-clean:** signed release candidate installed with fresh app data and no resident Laguna model.
- **Native-existing:** upgrade over prior schema with conversations, traces, model registration, and failed/in-progress histories.
- **Native-local:** healthy local Laguna on supported Apple Silicon hardware.
- **Native-cloud:** production-like Synth account/API credential with capped launch-test budget.
- **Native-eval:** Craftax container and evals checkout pinned to recorded revisions.
- **Offline/degraded:** DNS/network off, daemon absent, container unhealthy, stream interrupted, disk constrained.

### Canonical fixture states

- Empty/new account; existing free/paid account; expired/revoked auth; quota/rate-limited account.
- Zero, one, many, and very-long-title conversations.
- Short, long, tool-heavy, failed, cancelled, and resumed sessions.
- No models; partial download; one compatible resident model; incompatible/oversized model; corrupted artifact.
- No containers; healthy Craftax; unhealthy Craftax; capability mismatch; interrupted SSE.
- No runs; active/queued/completed/failed/cancelled eval; GEPA lineage; GELO/SFT alpha fixture.
- No traces; valid V5; large V5; missing blob; corrupt hash; unsupported version.

## 3. Release-candidate command gate

Run the canonical root wrappers from `/Users/joshuapurtell/Documents/GitHub/workshop` on the exact release revision and retain complete output:

```bash
npm run desktop:check
npm run desktop:verify
npm run desktop:build
npm run desktop:install:release

# Gate harness and launch policy (from the evals checkout)
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop test
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:pr
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:current
```

`desktop:check` is the fast static/type lane; `desktop:verify` is the product verification wrapper; build/install commands qualify packaging behavior. `gate:current` is intentionally red without healthy live topology, an exact artifact, and a fresh 37-item manual receipt. For live and release lanes use the concrete `gate:local`/`gate:release` invocations in the implementation plan. Record exact commands and logs; do not summarize “tests passed” without receipts.

## 4. Scenario catalog

### WEB — usesynth.ai, auth, download, blog, docs

#### WEB-001 Landing comprehension and CTA

- **Playwright:** hero, primary/secondary CTA, nav, local/cloud explanation, API snippet, absence of Intern launch claims, requirements, pricing/privacy/docs/support, desktop/mobile widths.
- **Bombadil:** hero crop, typography hierarchy, CTA prominence, screenshots/video aspect, no overflow at 320/768/1440 px.
- **CUA/browser:** new visitor reads page, plays hero clip with captions, follows Download.
- **Assert:** no unsupported claim, dead link, staging URL, layout shift, inaccessible media, or mobile dead end.

#### WEB-002 Signup and return-to-download

- Test email/OAuth paths actually offered; validation, duplicate account, cancellation, expired state, back/forward, refresh.
- Preserve campaign and download intent without putting secrets in URL/storage.
- Return to correct architecture/version download page after success.
- CUA verifies browser/account/app handoff in real signed-in session.

#### WEB-003 Binary download

- Correct architecture, filename, version, MIME, content length, checksum, signing/notarization information.
- Retry/range/interrupted download; duplicate click; expired signed URL; CDN 4xx/5xx.
- Mobile explains desktop requirement and supports “send link” only if real.
- CUA installs the downloaded artifact, not a local developer build.

#### WEB-004 Blog and Cap usage videos

- All required embeds load, have poster/captions/transcript, keyboard controls, mobile crop, and compressed fallback.
- Video content matches release build and feature labels; no secrets, internal paths/hosts, fixture passed as live, or misleading time cut.
- Links from blog to download, quickstart, API, Craftax, known issues, privacy, and support are valid.

#### WEB-005 API documentation copy/paste

- Run documented curl, Python, and TypeScript examples with a fresh credential.
- Cover streaming/non-streaming, invalid auth, rate limit, timeout, and model-not-found.
- Assert response/usage schemas and redact secrets from copied diagnostics.

### APP — install, first run, account, shell

#### APP-001 Clean install and launch

- CUA: download → mount/install → Gatekeeper/notarization → launch → welcome.
- Assert one app instance, correct icon/name/version, no dev console, no unexpected permission request, usable first paint within budget.

#### APP-002 Device pairing

- Happy path, code expiry, denial, wrong browser profile/account, offline, backend 5xx, app closed during pair, duplicate callback, sign-out/re-pair.
- Assert short-lived state, secure credential storage, authoritative account snapshot, and no local-data surprise on sign-out.

#### APP-003 First-run choice and first real response

- Verify the initial equal choices are **Continue locally** and **Sign in to Synth**; assert the removed setup-agent modal/card never appears.
- Continue locally into the normal landing/composer, then exercise local Laguna and Synth Cloud launch-picker targets independently. Pairing is initiated through Settings → Account when selected.
- Send an editable first prompt, stop generation, retry, and assert success only after a real provider response plus durable conversation restore after relaunch.

#### APP-004 Conversation lifecycle

- Create, rename, send, stream, stop, retry, queue several long turns, remove/edit queue where supported, search, reopen, archive/delete.
- Kill/restart mid-stream and after completion; switch provider/model for future turn.
- Assert chronology/provenance, no duplicates, correct partial/terminal status, and no queue/composer/inspector overlap.

#### APP-004A In-session research side panels

- Start a real Codex task, open a container in `ContainerPane`, then open visual and Trace V5 records in their shared `VisualPane` while output is streaming and after completion.
- Close, expand/collapse, reopen, switch objects rapidly, exercise the resizable Inventory split, navigate to related evidence, restart the app, and restore the session.
- Assert each inspector stays bound to the correct session/run/object; selection and transcript state survive; no inspector covers the composer or steals focus. Do not test or document a nonexistent three-tab peer panel switcher.
- Assert live updates are bounded, stale/disconnected state is explicit, and fixture or cross-session data never appears as current.
- Playwright covers deterministic lifecycle/geometry; Bombadil covers proportions/overflow; CUA covers real installed behavior and subjective usability.

#### APP-005 Tool/workspace/terminal/output

- Grant/revoke workspace; attempt out-of-scope access; open terminal; run/stop a harmless command; resize/copy/scroll.
- Produce an artifact, open from transcript and Inventory, move/delete externally, then reopen.
- Assert permission boundary, process state, file provenance, missing-file recovery, and no unprompted expansion of scope.

#### APP-006 Search, Inventory, Visuals, Settings

- Cover the actual navigation: Chats, Connectors, Research → Visuals/Optimizers, Inventory → Containers · Traces · Usage, and Settings. Exercise empty/loading/populated/error states, back/restart, and open relationships.
- Keyboard and screen-reader names for tabs, lists, buttons, dialogs, search results, and panes.
- No fixture content without persistent demo labeling.

#### APP-007 Responsive and visual matrix

Playwright + Bombadil + CUA at minimum:

- 1280×720, 1440×900, 1728×1117; narrow app width; 100% and enlarged text/zoom.
- Sidebar expanded/collapsed; terminal open/closed; inference inspector open/closed; queue empty/multiple; long tool event; settings forms; live visual.
- Assert no overlap/clipping/horizontal page scroll, minimum target size, readable hierarchy, stable composer, proportional right rail, bounded queue, and usable dialogs.

#### APP-008 Accessibility/keyboard

- Traverse every primary journey without pointer.
- Escape closes transient layers without losing work; focus returns logically.
- Streaming/download/run status announcements are useful and non-spammy.
- Contrast/reduced motion/text expansion audited.

#### APP-009 Data upgrade and corruption recovery

- Upgrade from last shipped schema; restart during migration; duplicate events; missing/corrupt content blob; unsupported trace.
- Assert backup/recovery behavior, idempotence, explicit error, and no silent deletion.

### LAG — local Laguna

The local implementation under test is the MLX-backed Responses-compatible sidecar powering Codex agents. Tests must validate agent behavior, not merely direct model chat.

#### LAG-001 Hardware recommendation and download

- No daemon/model; supported and insufficient RAM/disk; partial/corrupt download; cancel/retry; checksum; storage-location display.
- Assert model/license/source/size/quantization/context and privacy boundary.

#### LAG-002 Load and first response

- Start/load stages, health probe, real streamed prompt, stop, second prompt, unload.
- Record model revision, hardware, load time, TTFT, TPS, tokens, context, peak/resident memory.
- Assert metric calculations exclude invalid near-zero windows and state follows daemon authority.

#### LAG-003 Stress and recovery

- Long context, repeated Codex-sized prompt cache, concurrent requests, cancellation at multiple phases, daemon kill/restart, app kill/restart, memory pressure, unload/reload.
- Assert bounded cache/memory, fair/error-safe concurrency, no zombie running state, released memory within recorded budget, no duplicate response.

#### LAG-004 Local Responses compatibility

- Documented curl/Python calls against enabled loopback endpoint; streaming event order; usage; tool/structured capability behavior; invalid request/auth/origin.
- Assert loopback binding and no accidental network exposure.

#### LAG-005 Codex provider parity

- Run the same bounded research-engineering task through Synth API and the local MLX Responses sidecar.
- Compare session lifecycle, streaming event order, cancellation, tool/command behavior, partial output, retry, provenance, trace formation, artifact opening, and restart recovery.
- Capability differences may be declared; silent semantic divergence, false success, or broken evidence binding is FAIL-P0.

### API — Synth API/cloud provider

#### API-001 Credential lifecycle

- Manual key and account-derived path where offered; validate, save, replace, revoke, sign out.
- Renderer/log/export/crash/support bundle redaction checks.

#### API-002 Responses compatibility

- Streaming and non-streaming text, long input, cancellation, timeout, retry, model capability selection, tool/structured cases explicitly supported.
- Assert event order, IDs, terminal status, usage/cost reconciliation, and error taxonomy.

#### API-003 Degraded cloud

- Offline, DNS, TLS, 401/403, 404 model, 429, quota, 5xx, malformed stream, disconnect/reconnect.
- Assert actionable non-deceptive state; retries do not duplicate paid execution.

#### API-004 Environment integrity

- Package release and inspect all configured hosts/model aliases.
- Assert no staging/local endpoint is accidentally selected and provider identity is visible.

### CTR — containers and live telemetry

#### CTR-001 Craftax registration/catalog

- Register healthy loopback service; inspect `/info`, capabilities, task catalog, and stable instances.
- Assert 32 train (`1001–1032`) and 8 test (`2001–2008`) seed IDs with split/seed/world/rules/readout metadata and exact rollout payload.

#### CTR-002 Unhealthy/capability mismatch

- Wrong port, service dies, stale registration, unsupported schema/transport, invalid metadata.
- Assert preflight blocks spend, health is current, remediation/re-register works.

#### CTR-003 Single real rollout

- Launch one held-out seed with telemetry; stream lifecycle/progress/action/reward/state/frame/completion.
- Assert image is the actual changing Craftax world, event IDs/order/schema are valid, and terminal trace/result correlates.

#### CTR-004 SSE resume and fallback

- Interrupt after known cursor; reconnect with `Last-Event-ID`; slow subscriber; dropped intermediate frames; polling fallback.
- Assert no duplicate/gap in required events, terminal never drops, heartbeats/backpressure bounded, UI discloses fallback/freshness.

#### CTR-005 Multi-rollout live visual

- Launch configured concurrency across train/test sample; monitor queued/running/completed/failed, ETA range, throughput, reward, cost, real-frame grid.
- Cancel one rollout and whole eval; retry failure.
- Assert counts sum, ETA says estimated, frames cannot cross-bind seeds, and completion opens correct evidence.

### TRC/VIS — traces, artifacts, visual integrity

#### TRC-001 V5 import/open

- Valid, large, corrupt, missing-blob, duplicate, and unsupported bundle.
- Assert CAS/hash, idempotent import, schema/version warning, generic inspector, environment overlay, and source provenance.

#### TRC-002 Timeline correlation

- Craftax observation/action/reward/state/frame plus model/tool events.
- Assert step/time ordering, frame binding, filters, selection, scroll/zoom, and raw-data escape hatch.

#### VIS-001 Template binding

- Empty/loading/live/completed/failed and malformed-data states for live rollouts and optimizer visual.
- Assert template/schema compatibility, no stale/cross-run data, accessible summary, labeled axes/units/legends.

#### VIS-002 Export/reopen

- Export run evidence, reopen/import in fresh app data, compare hashes/metrics/visual binding.
- Assert self-description and no absolute machine-only dependency without warning.

### EVA — Craftax eval harness

#### EVA-001 Preflight

- Healthy/unhealthy container, model unavailable, bad credential, invalid seeds/splits, budget exceeded, output unwritable, concurrency invalid.
- Assert nothing launches/spends before all hard requirements pass.

#### EVA-002 Smoke run

- Small pinned seed set, one model/prompt/harness, low concurrency.
- Assert exact manifest, terminal accounting, metrics, usage/cost, traces, frames, failures, export.

#### EVA-003 Reproducibility

- Rerun identical deterministic components; compare configuration hashes and allowed metric variance.
- Assert environment/model/prompt/harness/code revisions and seeds are never omitted.

#### EVA-004 Failure/resume

- Kill app, runner, container, network at controlled phases; reopen and resume/reconcile.
- Assert no duplicate paid rollouts, correct unknown/failed state, partial evidence retained, safe retry.

#### EVA-005 Full held-out comparison

- Baseline and finalist on identical held-out seeds/repetitions.
- Assert paired analysis, mean/median/distribution/confidence, achievements, invalid actions, latency, cost, failure rate, and no train/test leakage.

### OPT — GEPA, GELO, SFT, final promotion

#### OPT-001 GEPA import/visualization

- Import real GEPA evidence with several generations/candidates and one failed candidate.
- Assert lineage, parent, prompt diff, score/metric evidence, configuration, selected candidate, raw artifacts, and baseline comparison.

#### OPT-002 GEPA live run (only if supported)

- Preflight, launch, progress, cancel, restart/reconcile, terminal artifacts.
- If runner is not real, remove launch CTA or label fixture Preview; never animate fake progress.

#### OPT-003 GELO [alpha]

- Alpha disclosure/consent; real local slot/hosted boundary; lifecycle, cost, candidate comparison, failure, cancel/reconnect.
- Assert shared optimizer contract and isolation from supported conversations/runs.

#### OPT-004 SFT [alpha]

- Dataset manifest, dedup/filter, train/validation/test separation, token budget, checkpoint schedule, training status/metrics, cancellation, checkpoint eval.
- Assert model/dataset lineage, held-out integrity, no secret/raw-user-data leak, and explicit provider/cost.

#### OPT-005 Fixed-token recipe comparison

- Train several recipes with the same token budget; evaluate checkpoints on same validation suite.
- Visualize loss/training metrics and Craftax uplift vs tokens/checkpoint.
- Assert fair comparison and retention of negative results.

#### OPT-006 Final model + prompt + harness promotion

- Combine finalist candidates; run held-out Craftax matrix; compare to frozen baseline.
- Promotion gate requires quality uplift without unacceptable cost/latency/reliability regression.
- Export immutable bundle with model/checkpoint, prompt, harness/config, manifests, versions, metrics, confidence, traces, and invocation.

### INT — Intern deferred in v0.1

#### INT-001 Surface removal

- Picker, sidebar, search, setup, status, run, trace, docs, captures, and marketing expose no Intern entry point or launch claim.
- Dormant catalog, protocol, bridge, fixture, and component code is acceptable only when unreachable from the shipped UI.

#### INT-002 v0.2 Sync re-entry (not a v0.1 gate)

- Create, attach scoped workspace, prompt, stream authoritative events, tool/artifact, stop, resume/reopen, failure/retry.
- Assert no stale optimistic state, correct chronology, durable artifacts, and permission revocation.

#### INT-003 v0.2 Async re-entry (not a v0.1 gate)

- Configure objective/budget, launch, close app, reopen/reconnect, observe progress, cancel, inspect result/partial failure.
- Assert idempotency and authoritative cloud reconciliation.

#### INT-004 Dormant-code isolation

- Make dormant Intern bridges unavailable or failing.
- Assert no Intern surface appears and Chats, local Laguna, Synth API, storage, Inventory, and ordinary artifacts remain intact and usable.

### SEC/REL/PERF — cross-cutting

#### SEC-001 Secret redaction

- Inject recognizable canary secrets into provider auth and test errors.
- Search renderer/native/daemon logs, traces, exports, clipboard helpers, analytics, screenshots, crash/support bundles.
- Any leak is FAIL-P0.

#### SEC-002 Workspace/loopback boundary

- Attempt path traversal, symlink escape, unapproved folder, remote bind/origin, cross-rollout data access.
- Assert least privilege, loopback defaults, and explicit approval/rejection.

#### REL-001 Crash/restart matrix

- Kill app/daemon/container/runner/browser during pairing, download, generation, write, eval, optimizer, import, migration.
- Assert recovery truth, durability, idempotency, and no phantom completion.

#### PERF-001 Launch/UI performance

- Measure cold/warm launch, navigation, large transcript, search corpus, streaming, live 40-rollout visual, trace inspection.
- Capture CPU/memory/frame responsiveness and compare to provisional budgets in parent spec.

#### PERF-002 Local inference memory

- Measure initial, loaded, post-short, post-large-cache, post-concurrency, post-cancel, and post-unload memory.
- Gate unexplained growth or failure to release; attach model/hardware/revision.

## 5. Playwright implementation checklist

Current deterministic coverage is distributed across the real checked-in specs; do not create parallel “gate” filenames merely to satisfy this plan:

- [x] `account-sign-in.spec.ts` and `get-started.spec.ts`: local/sign-in first run, browser pairing, cancellation, and absence of the removed setup card.
- [x] `session-lifecycle.spec.ts`: process exit, interruption, terminal envelopes, and failed-turn truthfulness.
- [x] `runtime-regressions.spec.ts`: provider switching, compaction, workspace, real model-download bridge, cold local warm-up, container and visual inspector behavior.
- [x] `layout-invariants.spec.ts`, `poolside-polish.spec.ts`, and `sidebar-navigation.spec.ts`: geometry, picker containment, navigation, composer/rail relationships, and major route polish.
- [x] `visuals-registry.spec.ts`, `optimizer-banking77.spec.ts`, and `gaps.spec.ts`: visual lifecycle, GEPA/Craftax SFT recipes, Inventory Rust storage, container/trace access, and Intern absence.
- [x] `synth-cloud-provider.spec.ts`, `slash-voice.spec.ts`, and `design-debt.spec.ts`: cloud provider, slash/skills/voice behavior, launch-debt locks, and removed-control absence.
- [ ] Add the still-missing breadth: real auth/download return errors, local download pause/cancel/corruption and unload state machine, Synth API error taxonomy, SSE resume/cross-seed binding, Trace V5 corruption, GEPA lineage/diff depth, reduced-motion/text expansion, and deterministic two-session inspector cross-binding.
- [ ] Keep assertions on user-observable and authoritative state. The source tree currently contains no allowed expected-fail/skip/fixme/todo escape hatch.

## 6. Bombadil implementation checklist

- [ ] Golden geometry for shell, sidebar, transcript, composer, queue, terminal, inspector at required viewport matrix.
- [ ] No overlap, clipping, accidental horizontal scroll, offscreen menu/dialog, or undersized primary control.
- [ ] Visual alignment for first-run choices/landing, settings, Inventory, live eval, trace, optimizer, error/empty/loading states; there is no setup modal to cover.
- [ ] Long localization-like strings, long model/task names, large numbers, and dense tool events.
- [ ] Semantic color plus icon/text; contrast and disabled/hover/focus states.
- [ ] Snapshot/reference update requires a human-reviewed screenshot and explanation.
- [ ] Launch-debt spec fails on fixture leakage, internal copy, placeholder controls, staging hosts, missing `[alpha]` labels, and any v0.1 Intern entry point.

## 7. Native CUA runbooks

### CUA-A — new-user golden path

1. Start screen recording.
2. Open usesynth.ai in a signed-out browser.
3. Read/play primary content; create account; download release candidate.
4. Install/open app; choose **Sign in to Synth** and pair the account. In a separate pass, choose **Continue locally** and verify it reaches the normal landing without an auth loop.
5. Select the Synth Cloud target from the normal launch picker; send a real prompt, stop/retry, and open usage. No setup-agent modal should appear.
6. Restart app; search and reopen conversation.
7. Export redacted evidence.

### CUA-B — Poolside-like local Laguna path

1. Fresh app/model state; choose **Continue locally**, then open Settings → On-device models.
2. Observe hardware/storage recommendation.
3. Download or use prepositioned verified model while recording truthful progress.
4. Load; send real coding/research prompt; watch inference metrics.
5. Cancel; send again; restart daemon; recover; unload.
6. Inspect memory/diagnostics and support guidance.

### CUA-C — Craftax research path

1. Begin in a Codex agent session with a concrete Craftax research objective.
2. Open the Container inspector beside the agent (or register/discover it through Inventory) and inspect catalog/held-out seeds.
3. Have the agent configure a small eval with explicit model/prompt/harness/concurrency/budget; review and approve.
4. Pass preflight and launch; open the rollout visual in the shared visual/trace inspector and watch actual frames plus aggregate progress without losing the agent conversation.
5. Interrupt/reconnect the live stream; inspect truthful reconnect/staleness state.
6. Open the Trace V5 record in the same inspector and correlate an environment observation, agent action, reward, frame, and model/tool event.
7. Complete; inspect distributions and a failed/slow rollout if available; ask the agent to summarize grounded in the selected evidence.
8. Export, restart Workshop, reopen the session/run, and confirm provenance and panel bindings.

### CUA-D — optimizer/promotion path

1. Open baseline and GEPA run.
2. Inspect lineage, prompt diffs, metrics, failures, and choose a candidate.
3. Open GELO/SFT `[alpha]`; verify disclosure and safe exit/failure.
4. Compare finalist vs baseline held-out evidence.
5. Export final model + prompt + harness bundle.

### CUA-E — Intern v0.1 absence

1. Verify picker, sidebar, search, setup, status, docs, captures, and marketing contain no Intern surface or launch claim.
2. Make dormant Intern bridges unavailable and verify supported workflows remain intact with no surface appearing.
3. Record the v0.2 re-entry pointer from `launch_v0p1.md` §4.8.

### CUA-F — adversarial polish sweep

- Visit every primary page with empty, busy, long, failed, and restored state.
- Resize continuously between narrow and large; toggle sidebar, terminal, inspector, queue.
- Keyboard through all controls; inspect focus, labels, menus, dialogs, toasts, scroll regions.
- Flag dead/deceptive controls, weak hierarchy, raw internal errors, stale status, visual crowding, and inconsistent nouns.
- Every finding gets a fix + regression, removal from supported v0.1, or explicit isolation outside the launch surface; the gate does not permit XFAIL/skip markers. Screenshots go in the evidence packet.

## 8. Scientific Craftax protocol

### Partition and leakage rules

- Training/search uses declared train seeds only.
- Validation/checkpoint selection uses a distinct fixed validation slice.
- Final selection is frozen before held-out test execution.
- Test seeds are never fed back into prompt, harness, data recipe, or checkpoint choice.
- All exceptions invalidate the claimed held-out result and are recorded.

### Baseline manifest minimum

- Task/container/version/config hashes and seed list/split.
- Model provider/revision/quantization/inference parameters.
- System/user prompt and harness/tool/environment versions/hashes.
- Repetitions, max steps/tokens, concurrency, timeout/retry, telemetry detail.
- Evaluator/scorer version, metrics, cost accounting, code/worktree revision.

### Required comparisons

- Baseline vs prompt-only GEPA.
- Baseline vs GELO harness/program candidate `[alpha]`.
- Baseline vs each SFT checkpoint/recipe `[alpha]`.
- Component ablations and combined finalists.
- Paired seed outcomes, reward and achievement distributions, invalid actions/deaths, failures, latency, token usage, and cost.

### Promotion rule

A candidate cannot become the final v0.1 recommended Craftax bundle unless:

- Held-out primary metric improves with reported uncertainty/variance.
- No important achievement/safety/reliability regression is hidden by the aggregate.
- Failure and invalid-action rates are acceptable.
- Cost/latency tradeoff is documented and within the declared budget.
- Run is reproducible from the exported bundle.
- Trace sampling confirms the behavior is real rather than evaluator or harness exploitation.

## 9. Launch sign-off matrix

| Area | Automation | Native/CUA | Evidence owner | Status |
|---|---|---|---|---|
| Landing/signup/download/blog/videos | Playwright + visual web checks | CUA browser/install | TBD | ⬜ |
| Pairing/account | Playwright + auth integration | CUA | TBD | ⬜ |
| Conversation/workspace/terminal/output | Playwright + Rust | CUA | TBD | ⬜ |
| Local Laguna | service/integration/stress | CUA | TBD | ⬜ |
| Synth API | contract + provider UI | CUA + external client | TBD | ⬜ |
| Containers/live Craftax | contract + Playwright | CUA | TBD | ⬜ |
| Trace/visual/export | Rust + Playwright + Bombadil | CUA | TBD | ⬜ |
| Craftax eval harness | eval smoke/acceptance | CUA | TBD | ⬜ |
| GEPA | contract + visual tests | CUA | TBD | ⬜ |
| GELO/SFT [alpha] | contract + safe-state tests | CUA | TBD | ⬜ |
| Intern deferred | absence/isolation | CUA | TBD | ⬜ |
| Accessibility/responsive/style | Playwright + Bombadil | CUA | TBD | ⬜ |
| Security/privacy/recovery | unit/integration/adversarial | CUA | TBD | ⬜ |
| Package/sign/notarize/upgrade | release scripts | CUA | TBD | ⬜ |

## 10. Final go/no-go checklist

- [ ] Exact release build has one complete, linked evidence packet.
- [ ] All P0 scenarios pass; no P0 XFAIL/waiver.
- [ ] P1 failures are fixed, removed, or explicitly isolated outside supported v0.1.
- [ ] Marketing/support boundary matches the build; every alpha label is present.
- [ ] No unresolved secret/privacy/workspace/signing issue.
- [ ] Real cloud and real local first responses pass.
- [ ] Real Craftax smoke and held-out comparison evidence can be opened in Workshop.
- [ ] Local memory/stress results are acceptable on declared supported hardware.
- [ ] Final CUA polish sweep has been rerun after the last code/content change.
- [ ] Rollback artifact/path, release owner, incident channel, and post-launch smoke are ready.
