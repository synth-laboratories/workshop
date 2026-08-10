# Synth Workshop v0.1 launch specification

**Target:** public v0.1 launch tonight  
**Product:** coding agents for research engineering, powered by Synth API or a local MLX Responses sidecar, with first-class Visuals, Containers, and Traces  
**Status:** living launch contract; unchecked items are not implied complete  
**Detailed QA plan:** [`qa_cua_end_to_end.md`](qa_cua_end_to_end.md)
**Gate implementation plan:** [`launch_gate_implementation_plan.md`](launch_gate_implementation_plan.md)

## 0. Launch decision in one sentence

Ship a trustworthy Mac desktop for research-engineering coding agents. A new user can discover Synth on usesynth.ai, create an account, download and open Workshop, run a Codex agent through either Synth API or the local MLX Responses API sidecar, and use first-class Visuals, Containers, and Traces alongside the agent to inspect and improve real work—without hidden setup, deceptive controls, data loss, or visual breakage.

## 0.1 Core product thesis

Workshop v0.1 is not a generic chat client, model dashboard, eval website, or collection of disconnected research tools. Its core loop is:

> Give a coding agent a research-engineering objective, let it work against code and executable environments, and keep the environment, evidence, and interpretation visible beside the agent.

The foundation has five parts:

1. **Codex coding agent:** the primary actor and conversational surface for planning, editing, running, debugging, and reporting research-engineering work.
2. **Inference choice:** the same agent workflow runs against Synth API or a local MLX-backed OpenAI Responses-compatible sidecar.
3. **Containers side panel:** executable environments, tasks, datasets/evals, rollouts, health, and live state are attachable to the current agent session.
4. **Traces side panel:** agent, model, tool, environment, reward, timing, and artifact evidence can be inspected without leaving the work.
5. **Visuals side panel:** live and completed research state—rollouts, metrics, comparisons, lineage, charts, and specialized environment views—is rendered next to the agent and bound to real data.

The launch priority is depth and reliability of this loop. New top-level nouns, broad orchestration features, and additional research workflows must not dilute or destabilize it.

## 1. Release vocabulary and scope

Every surface must carry one of these labels in product, documentation, and launch copy:

- **v0.1:** supported launch behavior. It must pass the required automated and CUA gates.
- **[alpha]:** visible and usable by invited users, but explicitly unfinished. Failures must not corrupt supported v0.1 data or imply production readiness.
- **Preview:** demonstrable read-only or fixture-backed behavior. It must say when data is a fixture.
- **Deferred:** not shown as available and not promised in launch copy.

### v0.1 supported

- usesynth.ai landing, account entry, download, install, first run, and device pairing.
- Codex agent sessions: create, instruct, stream, stop, queue, resume, search, rename, reopen, and preserve code/tool/artifact provenance.
- Local MLX Responses sidecar discovery, model download/setup, health, generation, cancellation, unload/recovery, and inference telemetry.
- Synth API configuration and hosted Responses use for the same Codex agent workflow, plus a copyable external API example.
- Honest provider/model selection and capability presentation.
- Workspace access, terminal, code changes, commands, approvals, outputs, and artifacts as one durable agent session.
- First-class session side panels for Visuals, Containers, and Traces, including attach/open/close/resize/reopen and correct binding to the active work.
- A reproducible Craftax evaluation path through the Codex agent and Workshop harness, including container control, live rollout visuals, and trace inspection.
- GEPA run import/visualization and a coherent optimizer information architecture.
- Packaging, update/download guidance, documentation, support/recovery guidance, and privacy/security basics.

### [alpha]

- Hosted GELO execution.
- Standalone hosted SFT and checkpoint evaluation.
- Advanced trace annotation, branching, and collaboration.
- Any feature that depends on an unstable backend contract or cannot complete the launch acceptance matrix tonight.

### Deferred unless proven tonight

- General Projects as a top-level product noun.
- Additional top-level dashboards or inventory taxonomies that compete with the agent + side-panel loop.
- Unsupported operating systems.
- Claims of arbitrary container/eval compatibility.
- **Intern Sync/Async and cloud orchestration.** Intern is completely absent from the v0.1 product UI, launch copy, screenshots, videos, docs, and acceptance paths. Dormant implementation code may remain behind no reachable v0.1 entry point.
- Billing automation or upgrade flows that are not end-to-end connected.
- ReAct/DCRBench campaigns whose runner still lacks implemented `launch`/`run` behavior.
- **Legacy Python `runtime.sqlite3` migration UI** — removed from Runtime settings for v0.1; migration tooling is not part of the launch loop.
- Titlebar Account-menu chevron and Expand chrome — removed (Account avatar still opens Settings → Account).

### [alpha] but stay in scope if proven tonight

- **GELO**, **SFT**, and **advanced trace annotation/branching/collaboration** remain `[alpha]` labels, but are **not** deferred further — land them for launch if the runners and CUA evidence can be completed.

## 2. Product principles and non-negotiable requirements

- **Agent first.** The coding agent and its current research objective dominate the information hierarchy. Supporting systems stay contextual and adjacent.
- **One agent contract, two inference paths.** Synth API and local MLX Responses must preserve the same Codex session semantics, event ordering, cancellation, tool behavior, and provenance wherever their capabilities overlap.
- **Three first-class research primitives.** Visuals, Containers, and Traces are not secondary admin pages. Each can be opened beside the agent, bound to the current session/run, and used without losing conversational context.
- **Truth over theater.** Never label fixtures, cached results, inferred state, or partial failures as live success.
- **Local-first clarity.** The user can always tell what runs on their Mac, what calls Synth Cloud, what model is resident, and what may cost money.
- **One continuous workflow.** Marketing, signup, download, pairing, model setup, prompting, evals, and artifacts use the same nouns and visual language.
- **Recoverable by default.** Restart, reconnect, cancel, retry, and resume paths are first-class. A crash or network loss must not silently lose durable work.
- **Evidence-bearing output.** An eval or optimizer run is not complete until its configuration, versions, seeds/splits, trace/artifacts, metrics, and terminal status are inspectable.
- **No dead controls.** Anything clickable must work, be disabled with a reason, or be visibly labeled Preview/[alpha].
- **Accessible, keyboard-usable, and proportionate.** No clipped primary controls, trapped focus, unlabeled icons, overlapping panes, or unreadable density.

## 3. Personas and launch usage stories

### A. Curious first-time user

> I arrive at usesynth.ai, understand that Workshop gives me coding agents for research engineering, create an account, download the correct Mac build, choose Synth API or local MLX inference, and complete a real agent task in under ten minutes.

Pass conditions:

- [ ] Hero states the concrete job: run research/code agents, local models, evals, and optimizers in one desktop workshop.
- [ ] Primary CTA leads to the correct signup/download sequence; secondary CTA shows a real product workflow.
- [ ] Apple Silicon requirements, download size, model size, disk/RAM expectations, privacy, and pricing/credits are visible before download.
- [ ] Download is signed/notarized or the exact exception path is documented honestly.
- [ ] First-run checklist reaches a real successful response; no fixture is presented as the result.

### B. Local-model user

> I want Poolside-like simplicity: Workshop detects my hardware, recommends a local MLX model that fits, downloads it with progress and disk checks, starts the Responses sidecar, powers the same Codex agent experience, shows speed/memory, and helps me recover if it cannot load.

Pass conditions:

- [ ] Hardware and free-space preflight runs before a large download.
- [ ] Recommended model explains fit and tradeoffs; advanced users can choose another supported model.
- [ ] Download supports visible progress, pause/cancel/retry, checksum verification, and cleanup of failed partial data.
- [ ] Loading has stage/status/ETA; cancellation works; out-of-memory failure has actionable recovery.
- [ ] Resident model, local/cloud badge, TTFT, tokens/sec, context usage, and memory are truthful.
- [ ] Large prompt caches do not cause unbounded resident-memory growth; unload releases memory within a defined window.

### C. API developer

> I create or paste a Synth API key, verify it, choose a model, send an OpenAI-compatible Responses request, stream output, inspect usage, and reproduce the same request from curl/Python/TypeScript.

Pass conditions:

- [ ] Key entry is masked, stored in OS-secure storage, never logged, and has validate/replace/revoke guidance.
- [ ] Base URL, model name, auth header, streaming, errors, timeouts, and usage fields are documented with copyable commands.
- [ ] Workshop's Synth Cloud provider uses the same public contract as the documented API.
- [ ] Invalid/expired key, quota, rate limit, model unavailable, timeout, and offline states are distinguishable.
- [ ] Usage/cost source and freshness are explicit.

### D. Evaluation researcher

> I ask a Codex agent to work on Craftax, attach the container, configure a model/prompt/harness and seeds, watch the resulting environments in Visuals, inspect decisions in Traces, and export a reproducible run without abandoning the agent session.

Pass conditions:

- [ ] The registered Craftax catalog exposes stable task instance IDs and splits (currently 32 train seeds `1001–1032`, 8 test seeds `2001–2008`).
- [ ] Preflight validates container health, model/provider, credentials, prompt/harness version, budget, concurrency, and output destination.
- [ ] Live view reports queued/running/completed/failed, ETA with uncertainty, throughput, reward/success aggregates, and real per-rollout frames.
- [ ] SSE reconnect/resume works; polling fallback is honest; terminal events are never dropped.
- [ ] Results retain seed, task/version, model, prompt, harness, code revision, environment/config hashes, timing, usage/cost, trace, and artifacts.

### E. Optimizer user

> I use prior Craftax traces to run or inspect GEPA, GELO, or SFT, understand what changed, evaluate checkpoints/candidates on held-out seeds, and promote a stronger final model + prompt + harness only when evidence supports it.

Pass conditions:

- [ ] **GEPA:** import and visualize candidates, parentage, scores, metric deltas, prompts, and evidence; launching is supported only if the real runner is connected.
- [ ] **GELO [alpha]:** run through a real local slot/hosted service, show lifecycle and cost, and compare candidates using the shared optimizer contract.
- [ ] **SFT [alpha]:** show dataset recipe, train/eval split, checkpoints, token budget, training metrics, held-out eval results, and model lineage.
- [ ] Promotion compares baseline and candidate on the same held-out Craftax suite with confidence/variance, not a cherry-picked rollout.
- [ ] Final bundle includes model/checkpoint reference, prompt, harness/config, manifests, metrics, traces, and reproducible invocation.

### F. Intern user — Deferred to v0.2

Intern has no v0.1 usage story. The shipped app must expose no Intern picker targets, sidebar sessions, pinned Async card, search results, CloudDesk route, setup action, status copy, or marketing promise.

Re-entry criteria for v0.2 are recorded in §4.8 and must be satisfied before any Intern surface is restored.

## 4. End-to-end product workflows

### 4.1 Discover → understand → signup → download

1. User opens the updated usesynth.ai landing page.
2. Hero shows Workshop in a real end-to-end use, not an abstract dashboard.
3. Page explains: local Laguna, Synth API/cloud, evals/optimizers, and artifacts/visuals.
4. User chooses **Download Workshop**.
5. If signed out, user creates/signs into a Synth account; marketing attribution and intended return path survive auth.
6. Site detects supported platform/architecture but allows explicit selection.
7. Download page shows version, release notes, checksum/signing status, requirements, file size, and install steps.
8. Download starts once; retries do not create confusing duplicate state.
9. User sees **Open Workshop and pair** plus manual recovery instructions.

Landing acceptance:

- [ ] Strong headline, one-sentence explanation, primary download CTA, secondary watch-demo CTA.
- [ ] Real screenshots/video of conversation, local Laguna, live Craftax, trace inspection, and optimizer comparison.
- [ ] “Runs locally” claims specify exactly which data/model is local.
- [ ] API section includes a short real Responses example and links to full docs.
- [ ] Intern is absent; future functionality is not mixed with launch claims.
- [ ] Pricing/credits, system requirements, privacy/security, docs, release notes, and support are reachable.
- [ ] Mobile layout remains useful even though the binary is desktop-only.
- [ ] SEO/social metadata, favicon, analytics consent, CTA instrumentation, 404, and download failure states work.

### 4.2 First launch → device pairing → first success

1. App opens to an uncluttered welcome state with account and local-only choices.
2. Pairing launches a browser device flow with short-lived code/state and returns to the app.
3. Account snapshot shows plan/credits/usage only from authoritative backend data.
4. User selects **Start local** or **Use Synth API**.
5. Setup tests the selected path and displays a real success check.
6. A starter prompt is editable and sends into a normal durable conversation.
7. The user can reopen that conversation after app restart.

Requirements:

- [ ] Pairing expiry, denial, wrong account, offline, and browser-return failure are recoverable.
- [ ] Local-only use is possible where intended and never silently authenticates/uploads.
- [ ] Secrets never pass through renderer logs or analytics.
- [ ] Setup can be skipped and later resumed from Settings.
- [ ] Sample content is labeled; completion requires a real provider response.

### 4.3 Everyday conversation and agent work

- [ ] Create a conversation; title is sensible and editable.
- [ ] Send/stream/stop/retry work; partial output is represented honestly.
- [ ] Queue supports multiple turns without covering the composer; reorder/edit/remove behavior is explicit.
- [ ] Tool calls, reasoning/activity, files, terminal commands, failures, and approvals have distinct readable treatments.
- [ ] Search opens the correct session and match.
- [ ] Restart preserves chronology, messages, provider/model, and artifacts.
- [ ] Switching provider/model does not rewrite historical provenance.
- [ ] Workspace access is explicit, scoped, reversible, and visible.
- [ ] Terminal dimensions, input, copy, scrolling, process exit, and relaunch work.
- [ ] Outputs open from the producing event and from Inventory; missing files show recovery guidance.
- [ ] Narrow and large windows keep sidebar, transcript, composer, terminal, and inspector separate.

### 4.4 Local Laguna setup and operation

- [ ] Auto-probe local Laguna; distinguish absent, starting, healthy, busy, unloading, failed.
- [ ] Offer guided install/download with hardware compatibility and storage checks.
- [ ] Show model source/license, quantization, size, context, capabilities, and expected memory.
- [ ] Verify artifact integrity and avoid loading an incomplete download.
- [ ] Stream a real response with stop/cancel and correct final state.
- [ ] Support app restart and daemon restart without stale “running” state.
- [ ] Measure TTFT, decode throughput, total tokens, context use, memory, and errors without misleading near-zero-duration percentiles.
- [ ] Exercise long context, concurrent requests, cancellation, unload, reload, and memory-pressure recovery.
- [ ] Provide a copyable local Responses endpoint example when enabled.

### 4.5 Synth API setup and use

- [ ] Account-paired credential path and manual API-key path are coherent.
- [ ] Verify credential before saving it as active.
- [ ] Fetch model/capability list or use a clearly versioned supported catalog.
- [ ] Send non-streaming and streaming Responses requests.
- [ ] Tool calls and structured output either work or are labeled unsupported per model.
- [ ] Usage and cost reconcile with server records.
- [ ] Copy-as-curl/Python/TypeScript redacts secrets.
- [ ] Staging/local/prod endpoints cannot be confused in a release build.

### 4.6 Container → eval → live visual → trace

1. Register or discover a loopback container and inspect its declared capabilities.
2. Choose Craftax and task instances/seeds.
3. Configure model, prompt, harness, repetitions, concurrency, budget, telemetry, and output path.
4. Run preflight; block on unhealthy dependencies rather than failing after spend begins.
5. Launch; show aggregate and per-rollout live state using canonical events.
6. Render actual Craftax PNG/SVG state and retain enough frames for replay.
7. Recover SSE via cursor/`Last-Event-ID`; use bounded polling fallback where required.
8. Finish with terminal status, metrics, distribution, failures, traces, and artifacts.
9. Open a rollout trace and correlate observation, action, reward, environment state, frame, model/tool events, and timing.
10. Export/share a self-describing evidence bundle.

### 4.7 Craftax improvement loop through Workshop harness

The scientific objective is a stronger combined **model + prompt + harness**, not a prettier single trace.

1. Freeze baseline model, prompt, harness, container version, train seeds, and held-out test seeds.
2. Run baseline with repetitions sufficient to expose variance.
3. Diagnose failures from traces, achievements, invalid actions, resource curves, deaths, and latency/cost.
4. Run GEPA prompt search on train seeds; visualize lineage and score evidence.
5. Run GELO [alpha] for program/harness policy improvement where supported.
6. Build SFT [alpha] recipes from validated trajectories; checkpoint through training.
7. Evaluate every checkpoint/candidate on a fixed validation slice; never train on held-out test seeds.
8. Run finalists on the same held-out suite as baseline.
9. Compare mean/median, success and achievements, variance/confidence, cost, latency, invalid actions, and failure rate.
10. Promote only a reproducible bundle; retain negative and failed runs.

Required visualizations:

- [ ] Live rollout grid with real frames, status, step, reward, key resources, and latest achievement.
- [ ] Aggregate progress, queue/running/completed/failed, throughput, ETA range, and cost.
- [ ] Reward/achievement distributions by seed and run.
- [ ] Baseline vs candidate paired comparison.
- [ ] Prompt/program lineage graph for GEPA/GELO.
- [ ] SFT loss/metrics by token/checkpoint plus held-out performance curve.
- [ ] Trace timeline synchronized with environment frames and model actions.
- [ ] Final model + prompt + harness provenance graph.

### 4.8 Intern — Deferred to v0.2

**v0.1 removal contract:** Intern is fully de-scoped. No Intern Sync/Async target, Cloud/Sync Sessions sidebar section, Async Intern pin, search result, CloudDesk, setup prompt, health/status label, screenshot, video, documentation path, or marketing copy may be reachable or visible in the shipped build. Dormant catalog, protocol, bridge, fixture, and component code may remain only to make the later re-entry recoverable.

**Where it comes back:** restore Intern in the v0.2 launch contract and its matching gate first, then re-enable the currently dormant renderer routes (`LAUNCH_PICKER_TARGETS`, sidebar/search navigation, CloudDesk, and native `synthIntern` session loading) only after all of these pass:

- [ ] Proper public-cloud Sync/Async mailbox contracts, authentication, authoritative lifecycle events, and reconnect/recovery are production-ready.
- [ ] Permissions, workspace/cloud upload boundary, spend, retention, persistence, and stopping semantics are disclosed before execution.
- [ ] Sync can create, attach a workspace, prompt, observe live events, stop, resume/reopen, and inspect artifacts.
- [ ] Async can configure objective/budget, launch, survive app closure, reconnect, report authoritative progress, cancel, and expose results.
- [ ] Partial backend/schema/network failure cannot damage ordinary conversations, local models, or artifacts or fabricate completion.
- [ ] Branch/merge/chat controls appear only when their backend contracts are implemented.
- [ ] Dedicated v0.2 automation and CUA evidence qualify the entire restored surface.

## 5. Core interaction model and UX requirements

### Agent workspace composition

The default screen is an agent workspace, not a dashboard:

- The center is the durable Codex transcript and current research objective.
- The composer is the primary action surface and supports instructions, files/context, provider/model choice, queueing, and cancellation without visual overload.
- Code/tool activity and approvals appear in chronological context and can open their evidence.
- The side panel has three peer modes: **Visuals**, **Containers**, and **Traces**.
- Opening or switching a side-panel mode never destroys agent state, scroll position, current selection, or an in-progress run.
- The side panel can be resized, collapsed, reopened, and deep-linked to a specific visual/container/trace.
- The active side-panel object clearly identifies the agent session, run, task, or artifact to which it is bound.
- A panel may update live, but it must never steal focus, cover the composer, or cause transcript reflow that makes active work unusable.

### Visuals panel contract

- Render live or stored data produced by the current research workflow.
- Support generic charts/tables/timelines and specialized views such as real Craftax frames.
- Expose data source, freshness, run/trace binding, units, filters, and raw-data escape hatch.
- Preserve selection across live updates where possible and distinguish loading, stale, disconnected, completed, failed, and fixture states.
- Let the agent open, focus, or explain a visual through a stable tool/MCP contract.

### Containers panel contract

- Discover/register executable research environments and show authoritative health/capabilities.
- Inspect task definitions, instances/seeds/splits, rollout parameters, active rollouts, logs/readouts, and artifacts.
- Permit validated agent-initiated operations with clear scope, budget, lifecycle, and cancellation.
- Stream environment events and frames into Visuals and correlate them into Traces.
- Keep container identity/version/config hashes attached to all resulting evidence.

### Traces panel contract

- Open the current agent or evaluation trace without export/import ceremony.
- Correlate messages, reasoning/activity, tool calls, commands, environment observations/actions, rewards, frames, timing, usage/cost, errors, and artifacts.
- Filter and select events while preserving chronology and provenance.
- Navigate bidirectionally: trace event → transcript/tool/artifact/visual and agent output → trace evidence.
- Store/import/export Trace V5 truthfully, validate hashes/schema, and expose missing/corrupt evidence.

### Primary navigation

- **Chats:** normal durable conversations and agent sessions.
- **Inventory:** secondary management/recovery view for models, containers, traces, datasets/artifacts—not the main way to perform agent work.
- **Runs:** evaluations and optimizers, or a single coherent entry if the present build uses another noun.
- **Visuals:** reusable views bound to real data, while the primary usage remains the in-session Visuals side panel.
- **Intern:** absent in v0.1; reserved for the v0.2 re-entry defined in §4.8.
- **Settings:** account, providers/API, local models, workspace permissions, appearance/preferences, diagnostics.

Avoid introducing Projects until persistence, ownership, reopening, and cross-surface relationships have a complete contract.

### State and copy

- Every asynchronous operation has idle, validating, queued, starting, running, stopping, completed, failed, cancelled, and reconnecting states where applicable.
- Success appears only after authoritative confirmation.
- Errors say what failed, whether work/charges may have occurred, and the next safe action.
- Local, Synth Cloud, and third-party provider badges are always visible at selection and execution.
- Fixture/demo data has a persistent banner and cannot contaminate real Inventory.
- Destructive actions name the object and recovery behavior.

### Visual language

- Follow [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](WORKSHOP_QUALITY_STYLE_GUIDE.md) and [`workshop_style.md`](workshop_style.md).
- Dark, restrained research-workbench character; hierarchy comes from spacing and typography before borders/chrome.
- One coherent spacing/radius/type scale; no dashboard cards inside narrow chat inspectors.
- Sidebar, main transcript, composer, terminal, and right inspector have stable proportional relationships.
- Queue is compact and bounded; inference inspector is inset; composer never extends beneath it.
- Use semantic status colors plus icon/text; never color alone.
- Charts have labeled axes/units, readable legends, empty/error/loading states, and accessible summaries.
- Real screenshots and videos use representative data, clean window framing, legible scale, and no secrets/internal endpoints.

### Accessibility and interaction

- [ ] Full keyboard traversal with visible focus and no traps.
- [ ] Semantic roles/names for icon buttons, tabs, panes, dialogs, charts, and progress.
- [ ] Contrast meets WCAG AA for text and controls.
- [ ] Reduced motion respected; animations never block operation.
- [ ] Screen-reader announcements for streaming completion, errors, downloads, and long operations.
- [ ] Resize/zoom/text expansion do not clip primary controls.
- [ ] macOS shortcuts and window behaviors are consistent.

## 6. Functional and system requirements

### Reliability and data integrity

- [ ] Durable session/event storage survives app kill, daemon restart, and upgrade.
- [ ] Event ordering and terminal states are deterministic and idempotent.
- [ ] Retrying does not duplicate paid runs or corrupt histories.
- [ ] Content-addressed artifacts verify hashes and report missing/corrupt content.
- [ ] Migrations are forward-safe, tested from the last shipped schema, and never silently discard data.
- [ ] Export contains enough metadata to reproduce; import validates schema/version.

### Security and privacy

- [ ] Signed/notarized release and documented provenance/checksum.
- [ ] Credentials in OS-secure storage; redacted from UI logs, traces, exports, screenshots, analytics, and crash reports.
- [ ] Loopback services bind to `127.0.0.1` by default with origin/auth controls appropriate to their surface.
- [ ] Workspace/file access uses least privilege and clear revocation.
- [ ] External URLs and downloaded artifacts are validated.
- [ ] Privacy policy and telemetry controls match actual behavior.
- [ ] Account sign-out clears/revokes appropriate device credentials without deleting local work unexpectedly.

### Performance budgets (provisional launch gates)

- [ ] Warm app launch to usable shell: target ≤3 s on supported launch hardware.
- [ ] Common navigation response: target ≤100 ms perceived; no multi-second unmarked stalls.
- [ ] Conversation restore/search: target ≤1 s for the launch fixture corpus.
- [ ] Streaming text updates smoothly without transcript-wide rerender/jank.
- [ ] Live eval UI remains responsive with at least 40 Craftax rollouts represented and configured concurrency active.
- [ ] Telemetry is bounded; frames may coalesce, terminal events may not drop.
- [ ] Local model memory/load/unload budgets are recorded per supported model/hardware rather than guessed.

### Observability and support

- [ ] Diagnostics screen shows app/build, OS/arch, daemon health, provider status, storage paths/sizes, and redacted recent errors.
- [ ] One-click copy/export support bundle is safe by default and previews included data.
- [ ] Correlation/run IDs connect UI errors to logs without exposing secrets.
- [ ] Website/app funnel events cover landing CTA, signup, download, pair, setup choice, first real response, first eval, and failure category.
- [ ] Support/docs links work offline where practical and never strand the user in an auth loop.

## 7. Launch content requirements

### usesynth.ai landing update

- [ ] New Workshop-led hero and real product capture.
- [ ] “Choose your compute”: local Laguna vs Synth API explained in plain language.
- [ ] Workflow section: prompt → tools/workspace → eval → trace → optimize → compare.
- [ ] Live Craftax section with real rollout video and result visual.
- [ ] Developer API snippet with real endpoint/model names and error-free copy/paste path.
- [ ] Intern absent from landing copy, navigation, captures, and launch promises.
- [ ] Download requirements, version, release notes, docs, privacy, pricing/credits, and support.
- [ ] CTA and attribution analytics verified without leaking prompt/content data.

### Launch blog post

Working thesis: **“Synth Workshop v0.1: local models, reproducible evals, and optimization in one research desktop.”**

Required outline:

1. The problem: research workflows fracture across chat, local inference, scripts, traces, and dashboards.
2. What Workshop v0.1 is, with an honest support/alpha boundary.
3. First run: download, pair, choose local Laguna or Synth API, send first prompt.
4. Local Laguna: hardware-aware setup, privacy boundary, metrics, and recovery.
5. Craftax: configure and run real rollouts, see live environments, inspect traces.
6. Improve: GEPA today; GELO and SFT clearly `[alpha]`; compare against held-out seeds.
7. API: reproduce the same workflow outside the app.
8. What is deferred: Intern Sync/Async returns only after the v0.2 re-entry gates in §4.8.
9. Known limitations and next steps.
10. Download CTA, system requirements, docs, and support.

Usage videos to record with Cap:

- [ ] 30–45 s hero: open Workshop → select provider → real streamed response → artifact/visual.
- [ ] 60–90 s first-run: site/signup/download/pair/setup/first prompt.
- [ ] 60–90 s local Laguna: recommendation/download/load/chat/telemetry/unload.
- [ ] 90–150 s Craftax: configure seeds → launch → live real frames → results → trace.
- [ ] 60–90 s optimizer: baseline → GEPA lineage/comparison → promoted candidate; label fixtures/alpha precisely.
- [ ] 45–60 s API: create/configure credential → curl/Python streamed response → usage.

Capture checklist:

- [ ] Use a clean release account/workspace and synthetic non-secret data.
- [ ] Record the signed release build at realistic scale; hide notifications and unrelated apps.
- [ ] Rehearse each exact path; capture failures separately for docs if useful.
- [ ] Cursor, zoom, audio, captions, captions-safe crop, and export dimensions are consistent.
- [ ] No staging hosts, keys, personal paths, internal IDs, or misleading edits.
- [ ] Final embeds have poster frames, captions/transcript, compressed fallback, and mobile behavior.

### Documentation set

- [ ] Install/upgrade/uninstall and macOS security recovery.
- [ ] First-run quickstart.
- [ ] Local Laguna requirements, model selection, download, storage, troubleshooting, unload/remove.
- [ ] Synth API auth, models, Responses streaming, tools/structured output support, usage/errors.
- [ ] Craftax eval quickstart and reproducibility/export.
- [ ] GEPA visualization and optimizer concepts; GELO/SFT `[alpha]` boundaries.
- [ ] Deferred-features note points Intern to the v0.2 re-entry criteria in §4.8 without advertising a usable v0.1 surface.
- [ ] Privacy/security/telemetry and support bundle guide.
- [ ] Known issues and release notes.

## 8. Evals repository and Workshop harness requirements

All product dogfood runs live under `/Users/joshuapurtell/Documents/GitHub/evals` with checked-in manifests/configuration and pointers to immutable or content-addressed evidence. Do not commit credentials or huge raw artifacts.

- [ ] Add a v0.1 Workshop product suite under `evals/` covering setup, API, local inference, Craftax, traces, visuals, optimizers, and the absence of deferred Intern surfaces.
- [ ] Define machine-readable preflight and terminal result schemas.
- [ ] Pin task/container versions, seeds/splits, model revision, prompt/harness hash, concurrency, budgets, and evaluator versions.
- [ ] Store pass/fail, metrics, timings, usage/cost, error taxonomy, artifact hashes, and trace references.
- [ ] Separate smoke (minutes), launch acceptance (≤1 hour), and deeper scientific/nightly suites.
- [ ] Make resume/retry idempotent and never silently rerun paid work.
- [ ] Surface the suite and evidence in Workshop rather than requiring terminal archaeology.

Craftax experiment matrix:

- [ ] Baseline model + prompt + harness on train, validation, and held-out test partitions.
- [ ] Prompt-only GEPA candidates.
- [ ] GELO [alpha] program/harness candidates.
- [ ] SFT [alpha] checkpoints for fixed-token data recipes.
- [ ] Combined finalists, evaluated under identical held-out conditions.
- [ ] Ablations for model vs prompt vs harness contribution.
- [ ] Repetitions/paired seeds sufficient to report variance and confidence.
- [ ] Failure and cost/latency regressions are gates, not footnotes.

## 9. QA strategy and release gates

The executable scenario catalog, ownership, fixtures, evidence packets, and detailed matrices are in [`qa_cua_end_to_end.md`](qa_cua_end_to_end.md).

### Required layers

- **Static/type/unit:** schemas, reducers/state machines, storage/migrations, API clients, redaction, event ordering.
- **Playwright:** deterministic renderer workflows, keyboard/a11y, state transitions, geometry, responsive behavior, error recovery.
- **Bombadil:** visual alignment, overlap/clipping, density, fixture comparison, launch-debt assertions.
- **CUA:** signed/installed macOS app, browser handoff, native dialogs, real daemon/provider/container, subjective polish, recovery.
- **Contract/integration:** Rust/Python services, Responses compatibility, SSE resume, container/eval/trace/optimizer contracts.
- **Scientific eval:** Craftax baseline/candidates/checkpoints and evidence comparison.

### P0 ship blockers

- [ ] Signup/download/pairing or first real response cannot complete.
- [ ] App crashes, hangs, loses durable work, duplicates paid work, or corrupts migrations.
- [ ] Secret/workspace/privacy boundary violation.
- [ ] Provider/local/cloud identity or cost state is deceptive.
- [ ] Stop/cancel/recovery leaves false running/success state.
- [ ] Primary UI is clipped, overlapping, inaccessible, or unusable at supported viewport.
- [ ] Craftax launch claims use fixture frames or non-reproducible results.
- [ ] Release binary is unsigned/unverifiable without explicit launch decision.

### Required tonight acceptance sequence

1. [ ] Clean-machine-ish install and first-run path.
2. [ ] Signup/device pairing and sign-out/re-pair.
3. [ ] One real Codex research-engineering task through Synth API: inspect/edit/run/produce evidence, stop/retry, and reopen.
4. [ ] Repeat the same core Codex task through local MLX Responses; verify semantic parity, restart/unload, and memory.
5. [ ] During an active agent task, open/resize/switch/collapse/reopen Visuals, Containers, and Traces without losing state or obscuring the composer.
6. [ ] Attach Craftax from Containers; let the agent launch one real rollout; watch the real frame in Visuals; inspect correlated decisions in Traces.
7. [ ] Small multi-seed Craftax eval, SSE reconnect, result/trace open, export, and agent summary grounded in evidence.
8. [ ] Conversation restart/search/queue/terminal/output workflow.
9. [ ] GEPA result visualization and candidate comparison.
10. [ ] GELO/SFT `[alpha]` entry, disclosure, safe failure/exit behavior; Intern absence across picker/sidebar/search/setup/copy.
11. [ ] Full Playwright, Bombadil, Rust/service tests, package/install, and final CUA visual sweep.
12. [ ] Website signup/download/API/docs/blog/video link sweep on desktop and mobile widths.
13. [ ] Evidence packet reviewed by one person who did not implement the path.

## 10. Launch operations checklist

### Product/engineering freeze

- [ ] Name/version/build/channel consistent in app, artifact, site, docs, API, and release notes.
- [ ] Supported features and `[alpha]` flags reconciled against the actual build.
- [ ] No unexplained dirty/generated/demo state enters the packaged artifact.
- [ ] Database/schema migrations and rollback/recovery tested.
- [ ] Production endpoint allowlist and environment configuration audited.
- [ ] Crash/error/usage telemetry, sampling, retention, and consent verified.
- [ ] Known issues triaged: fix, explicit limitation, expected-failing test, or remove surface.

### Release artifact

- [ ] Clean build from pinned revision; tests and evidence linked to that revision/build.
- [ ] Code signing, notarization, checksum, malware scan, install, launch, upgrade, uninstall tested.
- [ ] Download CDN headers, filename, MIME, redirects, range/retry, and cache invalidation correct.
- [ ] Version/about/diagnostics and release notes match artifact.
- [ ] Emergency rollback and previous-download path are documented.

### Web/content

- [ ] Landing copy, screenshots, Cap videos, blog, docs, API examples, pricing, privacy, and support are published.
- [ ] All CTAs and deep links tested signed-out, signed-in, new account, existing account, and expired session.
- [ ] Analytics funnel and error dashboards visible before announcement.
- [ ] Social cards and launch posts preview correctly.

### Go/no-go room

- [ ] Named release driver and owners for desktop, web/auth, API, Laguna, evals, and content.
- [ ] Single incident channel and status update cadence.
- [ ] P0/P1 issue template includes build, environment, exact path, evidence, logs, and user impact.
- [ ] No-go authority and rollback trigger are explicit.
- [ ] First-hour smoke after publish and next-morning regression are assigned.

## 11. Evidence packet required for “v0.1 shipped”

- Release revision/build identity, signed artifact URL, checksum, and notarization result.
- Full test summary with exact commands and failures/waivers.
- CUA screenshots/video for each required tonight journey.
- Website signup/download/pair and API response evidence with secrets redacted.
- Local Laguna hardware/model/version, TTFT/TPS/memory/load/unload results.
- Craftax run manifest, live telemetry/frame evidence, results, traces, and export.
- GEPA visualization evidence; explicit `[alpha]` evidence or removal for GELO/SFT; explicit absence evidence for Intern.
- Accessibility, responsive viewport, and visual regression results.
- Known limitations, rollback plan, support contacts, and monitoring links.

## 12. Current implementation facts to preserve

These are current repository/live-task facts, not blanket launch approval:

- The installed-app polish pass has added geometry coverage for the compact queued-turn tray, inset inference inspector, and composer/inspector separation.
- A recent packaged pass reported 76 Playwright checks plus Rust/typecheck/build/install/CUA verification; rerun against the final revision.
- The live Craftax container has demonstrated real PNG rendering and canonical SSE/WebSocket events for a rollout, with a 40-instance deterministic catalog.
- The public Responses route has produced a successful Workshop-compatible Laguna response with metered usage in local integration; final app/provider validation is still required.
- Local inference stress work is actively investigating resident-memory growth and misleading decode percentile telemetry; these remain launch risks until the final gate passes.
- Final stack/lifecycle validation and Craftax optimization work are still active. Do not convert active-thread progress into a shipped claim without an evidence packet from the final build.

## 13. Definition of done

v0.1 is done only when a first-time user can traverse the public landing page through a real useful response, a researcher can run and inspect a reproducible Craftax evaluation, an API developer can reproduce the hosted request outside the app, supported local Laguna behavior is stable under load/recovery, `[alpha]` surfaces are unmistakable and isolated, and the final signed build passes the full automation plus human CUA evidence matrix with no open P0.
