# Handoff: shared agent traces and RuneBench visual

## Four-part follow-on delivered

The requested RuneBench integration, annotation persistence/review, replay synchronization and retained Craftax reuse are now published as **revision 18**. See the [updated RuneBench scope](../../../evals/workshop/runebench-ma-demo/ma/IMPLEMENTATION-SCOPE.md) for exact completion evidence and limits. The older revision-15 account below is historical.

New shared files include `EvidenceClient.tsx` and `CraftaxTraceView.tsx`; `TraceViews.tsx` now supplies replay following and common event navigation. Containers `annotation_store.py` persists canonical evidence revisions through `attach_many`, exact-selector resolution and validation. Evals `evidence_api.py` adapts the local capability-scoped HTTP endpoint; `build_web.mjs` rebuilds the standalone viewer with the same shared components. `build_traces.py` folds saved evidence into portable bindings. `import_craftax.py` records the retained source digest and honest partial coverage. Native screenshots cover RuneBench and Craftax, with background navigation preserving the foreground application. No new inference ran.

Validation: Workshop TypeScript, 14 shared tests, one real-recording Playwright test, two annotation-store tests, and the explicit browser save/reload/review/supersede/replay/Craftax walkthrough. The notes are model-authored demo reviews, not user-authored judgments. Full source-body inspection is not claimed for projection-based notes.

## Scope reconciliation after the annotation-aware follow-on

The RuneBench delivery scope is consolidated in [IMPLEMENTATION-SCOPE.md](../../../evals/workshop/runebench-ma-demo/ma/IMPLEMENTATION-SCOPE.md). The architecture is General trace → ReAct/Codex app-server presentation → container/app extension, with shared annotation identity and controls. `TraceViews.tsx`, `annotations.ts` and `RuneBenchTraceView.tsx` are now identified as the follow-on from the same user conversation; that follow-on passed a separate focused TypeScript check and four tests, including browser view-switch identity checks. These checks do not replace this handoff's real-recording checks or a final native review.

A current scope review confirmed five projections with 696 items and zero annotation records. RuneBench's editable viewer does not yet register the RuneBench extension or wire annotation persistence. Revision 15 remains the confirmed published baseline. Treat the original implementation/validation account below as historical evidence and the linked scope as the combined remaining work; do not mark these newer integration gaps complete based on component exports alone.

## User intent

Fix Workshop Visuals so the library is on the left and one usable visual is on the right, without the clipped duplicate preview below the list. Then improve RuneBench inspection beyond world replay and chat: show each agent's recorded inputs, thinking, responses, tool calls/results, reward events, annotations, and other events. Build on containers V5 / ATIF foundations and reuse components across evals rather than growing a RuneBench-only renderer.

The user authorized implementation. This handoff is requested so another engineer can take over. No new task, branch, commit, or PR was created by this handoff.

## Current state and evidence

The initial implementation is saved locally across three repositories. RuneBench visual revision **15** was published successfully to the local Workshop instance. This is a working foundation, not a claim that all cross-eval UX, streaming, or annotation workflows are finished.

Verified during this task:

- Five existing RuneBench recordings imported to sealed V5 traces and separate evidence bundles; existing validators reported no errors. Latest build reports 696 projected items total.
- Workshop TypeScript check passed.
- Ten focused tests passed: sourced module allowlist, V5 bindings, concurrent decision grouping, and evidence target time/actor resolution.
- Playwright with the real local recordings passed: inspector loads, agent selection works, retained observation expands, rewards show cumulative values, and the first model run reports that chat was not recorded.
- Native app displayed the new inspector. Native accessibility inspection was very slow; closed payload fields were subsequently changed to lazy rendering and the browser check rerun. Final revision 15 publication is confirmed by the saved receipt; there was no full native regression pass after that final update.
- Earlier layout work passed 23 focused checks and the CSS lint check.
- No provider-backed evaluation or new inference was run.

## Workspace and ownership

Use these actual checkouts (not the Documents/Synth Prod working directory):

- Workshop: `/Users/joshuapurtell/GitHub/workshop`
- Containers: `/Users/joshuapurtell/GitHub/containers`
- Evals: `/Users/joshuapurtell/GitHub/evals`

Read applicable AGENTS.md files first. Repositories contain substantial unrelated uncommitted work, especially Workshop optimizers/CISPO and containers runtime changes. Do not reset or broadly stage the checkout. RuneBench's demo directory is untracked and has also been edited by another task.

At handoff inspection, `visuals/components/agent_trace.v1/TraceViews.tsx` and `annotations.ts` were present. These were NOT created or verified by the implementation described here; they appear to be concurrent follow-on work for protocol views and annotation matching. Inspect and coordinate before editing. Do not assume the prior passing checks cover those newer files.

## Architecture implemented

Native RuneBench recordings → containers adapter → sealed TraceDocumentV5 plus TraceEvidenceBundleV5 → existing rollout projection / `synth.trace-visual.v1` → shared Workshop inspector.

V5 remains the authority. Eval-specific parsing belongs in adapters; common rendering consumes the projection. Preserve source identities and selectors. Missing source material must stay explicitly unavailable: an observation is not an exact provider request, and reasoning token counts are not reasoning text.

### Containers

- `src/synth_containers/tracing/adapters/runebench.py` (new): reads manifest, job result, run/episode event streams and engine state. Deterministic identities and source file/line/digests. Maps model requests/completions, policy calls/results, observed communication; retains other events. Builds canonical messages and model-call spans. Groups by native actor plus decision ID rather than a single open call. Derives XP deltas from engine ticks and records authoritative terminal XP rewards separately.
- `src/synth_containers/tracing/projections/visual.py`: expands canonical span input/output messages; preserves usage fallback, annotation detail/provenance/selectors, reward provenance/evidence/units; exposes completeness reasons as capture coverage.

### Workshop shared UI

- `visuals/components/agent_trace.v1/AgentTraceInspector.tsx`: Messages, Agent rollout, Rewards & annotations, All events; actor filter, comparison, search, grouped decisions, expandable payloads and provenance. Lazy payload rendering. `actorId`/`onActorChange`, `cursorMs`/`onSelectEvent`, optional environment renderer and native source-link callback.
- `model.ts`: projection types, classification, concurrency-safe grouping, target-based actor/time resolution.
- `archive.ts`: bounded gzip/base64 inline binding with SHA256 verification, loading/error state. No new arbitrary network permission.
- `README.md`: component contract.
- `visuals/families/analysis/trace.rollout_inspector.v1/shell.tsx`: uses the same inspector for the trace tab; retains Craftax comparison and other outer tabs.
- `visuals/package.json`, `visuals/runtime/sourcedValidate.ts`, `visuals/runtime/sourcedVisual.ts`: export and allowlist `@synth/visuals/components/agent_trace.v1`.
- Tests: `visuals/tests/agent_trace_model.test.mjs`, updated `sourced_visual.test.mjs`, and `apps/synth_desktop/tests/playwright/agent-trace-shared.spec.ts`.

### Earlier Workshop layout fix

Inspect the scoped diff in:

- `apps/synth_desktop/src/renderer/src/components/VisualsPage.tsx`
- `apps/synth_desktop/src/renderer/src/routes.tsx`
- `apps/synth_desktop/src/renderer/src/styles/app.css`
- `apps/synth_desktop/src/renderer/src/preferences/schema.ts`
- `apps/synth_desktop/scripts/lint-app-css.mjs`
- `apps/synth_desktop/tests/playwright/visuals-registry.spec.ts`
- `apps/synth_desktop/tests/visual_pane_min_width.test.mjs`

Removed the duplicate preview/dock path, fixed hidden library/focus sizing, aligned persisted library minimum to 240px, and fixed the CSS lint regex falsely treating `var(...)` as a selector. These files may contain other changes; review hunks rather than assuming whole-file ownership.

### RuneBench integration

All below are under `/Users/joshuapurtell/GitHub/evals/workshop/runebench-ma-demo/ma`:

- `build_traces.py`: invokes containers adapter/validators; writes `trace-v5/*.trace.json`, `*.evidence.json`, `*.projection.json`, plus `trace-bindings.json`.
- `query.py --build`: runs trace build and regenerates compact world/timeline data and `viewer.built.tsx`.
- `viewer.tsx`: editable source; imports shared inspector and archive hook, binds actor and replay selection, and links original evidence records. World presentation remains eval-specific.
- `serve.py`: local read-only query/media/evidence server. Added engine evidence scope. Concurrent work added frame/image handling; preserve it.
- `publish_trace_view.py`: updates existing visual through local Workshop visual MCP. Compresses trace binding to avoid IPC broken-pipe failures with the full ~2.7MB JSON. Saves `trace-publish-receipt.json`.

Do not hand-edit `viewer.built.tsx`. Rebuild from source. Do not restore old temp paths; the previous demo location was moved/deleted.

## Local instance and commands

- App bundle ID: `com.synth.desktop.v09.dev.hitlqa`
- App: `/Users/joshuapurtell/.synth-desktop/instances/v09/hitlqa/build/target/debug/bundle/macos/Synth Workshop v0.9 · hitlqa.app`
- Renderer: `http://127.0.0.1:14338/`
- Query/media server: `http://127.0.0.1:8118/`
- Visual ID: `vis_12c0bfef3cc34b1ea0537f1043cc04c0`
- Title: `RuneBench · swarm query to synchronized replay`
- Display name: `RuneBench Swarm Explorer`
- Session: `eval_92d9cfd2-ef37-44fc-a980-81c8bf707634`
- Confirmed publication: revision 15, 2026-09-07T19:24:18Z.

The dev server stopped during verification and was restarted. Check ports/processes before starting another. Do not depend on old agent terminal session IDs. To start the renderer if needed, from Workshop/apps/synth_desktop:

```sh
npm run frontend:dev -- --port 14338 --strictPort
```

From Evals:

```sh
python workshop/runebench-ma-demo/ma/query.py --build
python workshop/runebench-ma-demo/ma/publish_trace_view.py
```

The publisher is hardcoded to this existing local visual/instance. Review the current visual before publishing if another task is changing it. Its console may report revision null because of receipt nesting; the actual revision is in decoded response `event.payload.revision`. The saved receipt confirms success.

From Workshop:

```sh
npm run typecheck --workspace @synth/synth-desktop
node --test visuals/tests/agent_trace_model.test.mjs visuals/tests/sourced_visual.test.mjs visuals/tests/trace_v5_binding.test.mjs
RUNEBENCH_DEMO_ROOT=/Users/joshuapurtell/GitHub/evals/workshop/runebench-ma-demo/ma npx playwright test --config apps/synth_desktop/playwright.config.ts agent-trace-shared.spec.ts
```

Browser log: `artifacts/agent-trace-browser.log`. Screenshot: Evals `workshop/runebench-ma-demo/ma/shared-inspector-check.png` (empty Messages state, not a comprehensive visual review). The browser fixture emitted an existing native `transformCallback` warning while the test passed.

## Remaining work / critique for the next engineer

1. Reconcile concurrent `TraceViews.tsx` / `annotations.ts` work. Decide how protocol-specific general/ReAct/Codex views plug into the shared inspector without creating another parallel renderer.
2. Validate real reuse with a second eval/ATIF trace, preferably existing Craftax data. Generic V5 shell integration is implemented, but a second real-eval browser proof was not completed. Review visibility/redaction when expanding message references.
3. Review generic-inspector regressions: its old local focus/full/play/jump controls and inline analysis-finding rendering were removed with the old trace list. Outer Craftax/metrics/evidence views remain. Restore useful behavior through shared controls rather than duplicating the old implementation.
4. Improve synchronization: clicks seek replay, but playback does not automatically follow/highlight the active trace event. Native timeline still emphasizes actions/chat; model, reward and annotation markers need a common event timeline. Terminal actor-level rewards may have no execution timestamp; do not guess from annotation production time or proximity.
5. Complete evidence integration: annotation rendering exists, but the RuneBench importer currently creates rewards, not external annotation imports or an annotation authoring/persistence flow. Test existing annotations, supersession, exact selector attachment and unresolved targets.
6. Strengthen canonical model: span usage currently falls back to retained detail; populate typed UsageV5 and aggregates. Review completeness/session status for startup failures. Observed chats preserve native facts but are not a fully populated coordination graph. Avoid claiming receipt implies causality or consumption.
7. UX/performance: initial limit is 60 groups, not virtualization. Generic canonical messages are expandable JSON rather than polished message-part rendering. Check narrow/wide panes, long tools, comparison, large recordings, and native app responsiveness. Leave the user in list-left/visual-right layout after verification.
8. Add meaningful regression coverage in Workshop/containers: second-eval rendering, missing/corrupt archive, messages-present recording, replay selection, annotation linkage, provisional updates/identity stability. Evals AGENTS prohibits adding/extending tests there unless explicitly requested.
9. Review and commit only task-owned hunks across the three repos; no commit or PR was made. Do not stage generated recordings/large data blindly.

## Constraints

No macOS Keychain or Workshop Keychain-backed Secrets registry without explicit operation-specific authorization. Existing local IPC access is sufficient here. No paid inference is needed for this scope. Preserve missing-data labels; never synthesize provider reasoning or pretend source observations are exact input messages. The latest request is a handoff, not authorization to launch a new evaluation.

## Follow-on: revision 19 / Craftax decision evidence

Added existing sealed Luna-medium capture: ten independent single-agent episodes, 66 retained prompts/replies, executed actions and reward records. Evals `ma/import_craftax_calls.py` verifies and promotes through the canonical Craftax adapter; source and receipt are retained alongside derived authority. No inference. This does not fulfill Craftax-MA or DungeonGrid MA collection.

Craftax's source selector keeps old frame-only authority/annotation separate. Its Focus view groups interleaved actors by actor+session, renders stated rationale/planned actions/outcomes, preserves annotated hidden records, and keeps full source access. General model-call message parts are readable. Larger Craftax authority loads via existing local evidence service (inline publication exceeded IPC limit); local service dependency is explicit.

Validated four shared tests (including Craftax actor grouping and annotation identity), TypeScript, real-data browser editor/selection/source switching and compact/wide screenshots. Saved revision 19. Native capture explicitly refused because renderer had `none` open, so native revision 19 is not verified; did not activate/restart the user's app to obtain it.

### Revision 19 native confirmation after authorized restart

User requested Workshop restart. Restarted only `hitlqa` with `desktop-instance.sh cua-live hitlqa`; Vite healthy on 14338. Native `capture_review` now confirms revision 19 for both RuneBench and Craftax. Craftax screenshot visibly includes 66-decision source, seed labels, rationale, planned actions, executed harvest and achievement. Background navigation preserved foreground identity. Left Craftax open. Receipts/screenshots: Evals `ma/craftax-r19-restarted-native.{json,png}` and `ma/craftax-r19-confirmed-native.{json,png}`. This resolves the prior native-verification gap.

### Horizontal timeline scrubbing

Shared trace timeline now selects the leading visible marker during horizontal scrolling (last marker at the right edge), reveals the exact corresponding transcript card, and updates annotation/context selection. Selection also loads groups beyond the initial 60. Active markers are highlighted. Verified forward/backward scrolling with a 150-decision regression and the real Craftax capture; existing shared view/annotation checks and TypeScript pass.

### Explicit agent scope (revision 22)

Added All agents / One agent modes, persistent scope banner, directly selectable roster, and actor labels on timeline markers. RuneBench defaults to all agents; event selection preserves that mode. Roster, transcript and replay identify actor + team + role, with matching actor colors. The episode header includes its unique rollout id. All-agent context renders all four labeled player recordings; one-agent context follows the selected player. Existing shared comparison remains available.

Real browser verification covered all-agent default, four player recordings, event selection preserving scope, one-agent mode, roster switching and restoring all. TypeScript and three shared regression tests passed. No authority/annotation identities changed.

### Luna low/high experiment (revision 23)

User authorized sequential Luna low then high rollouts and visual comparison. Ran two four-agent RuneBench episodes with fixed starting saves, unseeded engine RNG, 150s deadline, ten calls/actor and equal 8192 completion-token caps. Batch `luna-low-high-20260907`: 80 calls, $0.0634919 actual, zero unknown-cost calls; dedicated MA container stopped. Both episodes passed all seven runtime gates, no policy errors.

- Low `20260907T214644Z-luna-low`: 40 calls; 575 combined XP (Moss275/Ember300); 2.283s mean response; 3319 output tokens, 1665 reasoning tokens; 38 chop/1 walk/1 say.
- High `20260907T215116Z-luna-high`: 40 calls; 500 combined XP (Moss275/Ember225); 3.262s mean response; 6538 output tokens, 4885 reasoning tokens; 36 chop/2 walk/1 say/1 wait.
- High scout reports on decision0, low on decision9; neither shows sustained communication. This pair does not establish an effort treatment effect.

Runner now retains effort, complete request messages and returned reasoning fields. Canonical adapter preserves returned reasoning parts and reports source availability conditionally, preserving prior annotation identities. UI uses API `reasoning.summary` label where present; encrypted-only content is not presented as a readable summary. Readable reasoning exists for 36/40 low and27/40 high responses, so summary length is not reasoning-token usage.

Viewer comparison buttons and transcript headings identify low/high, show aggregate response/token/XP metrics and open first response. Explicit reasoning-summary markers connect to canonical selections. New large traces load through the existing local evidence API; episode DATA moved from component source into compressed trace binding after hitting source size cap. Both real traces passed browser switching/summary selection checks with distinct canonical digests. TypeScript + shared tests pass. Native revision23 comparison capture verified. Artifacts in Evals MA: `luna-effort-comparison.json`, `luna-{low,high}-review.png`, `luna-comparison-native.{json,png}`. Prior initial browser attempt ran before rebuilt source was ready; rerun on completed build passed.


### Terra low comparison
Added actual run `20260907T220646Z-terra-low` (openai/gpt-5.6-terra, low): 475 XP, 39 calls, 1.6398s mean response, 2230 output tokens including 764 reasoning tokens, $0.271059. All seven environment gates passed. The final mab call was rejected by floating-point reservation arithmetic (7.8 + .2 exceeded 8); future reservations now round to 8 decimals. Retained the original episode and policy error; the UI explicitly discloses unequal call counts. Three columns and selectors distinguish model plus effort, with deltas against Luna low. One episode per arm, unseeded engine RNG; descriptive only. Headless browser verified each arm opens its own digest and selects a reasoning summary. Shared trace selection now preserves explicit marker selection through replay synchronization and click-triggered scrolling. Native revision 25 capture confirmed the three-column table and selected Terra arm. Dedicated run container stopped.


### Luna medium addition
Run `20260907T221533Z-luna-medium`: 40 calls, 650 XP (325 per team), 2.4375s mean response, 3886 output tokens including 2236 reasoning tokens, $0.02958495 actual cost. No policy errors; all seven environment gates passed. Same four actors, 150s deadline, ten calls per actor, 8192 output cap; fixed saves with unseeded engine RNG. Actions: 37 chop, 3 walk, no say. Added medium to runner choices and comparison ordering: Luna low / medium / high, Terra low. Container stopped after completion.


### Two-run investigation workflow — revision 28
RuneBench custom visual now opens Compare two rollouts: independent selectors defaulting Luna low/medium, actor contribution table, filter/search over the two runs’ retained policy results/actions/observed messages, exact source-event navigation, two shared AgentTraceInspector panes, optional game replay, independent / elapsed-time / same actor-decision alignment, canonical annotation create/review. Narrow panes stack. No additional renderer/schema; shared inspector gained an external selection input (itemId + revision) so query hits select the exact event despite timestamp ties and clear hidden search/section filters.

Actual investigation: low→medium actor XP changes maa −25, mab +75, mba −25, mbb +50 = +75 total. mab decision1 targets (3200,3246) in both runs: low loses tree; medium succeeds. Paired notes stored on exact result events, author Codex comparison review, model, needs_review. Reviewed native action/result records and terminal scores; no causal claim about effort or coordination. Medium has no say action.

Verification: browser exercises contribution drilldown, correct result target/digest in each pane, independent and elapsed cursors, decision alignment to recorded counterpart time, both annotations save and reopen after reload, wide/narrow layouts with no horizontal overflow. Shared annotation/view-switch and horizontal timeline tests pass; Workshop TypeScript passes. Native background button opened comparison with frontmostUnchanged=true. Evidence: ma/two-run-comparison-wide.png, two-run-comparison-narrow.png, two-run-evidence.png; Workshop artifacts/run-comparison-review.log. The browser annotation walkthrough writes real notes; do not blindly rerun it.
