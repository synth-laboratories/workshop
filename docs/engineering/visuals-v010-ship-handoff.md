# Ship handoff: v0.10 visuals

Written 2026-09-09. For whoever takes this from certified to released.

The engine work is done and certified. What is left is publication, two steps
after `mark_ready`, and three decisions that are not an implementer's to make
alone. Expect a day, not a week — most of it waiting on builds.

## What you are inheriting

Branch **`codex/v0.10.0-visuals-release`**, six commits on `7a099b0c`:

```
35fa18a4  Keep this release's acceptance receipts in the repository
df99a25b  Add the acceptance gates the release evidence rests on
f8cbecbf  Make the production-source guards scan the production source again
9c5358ee  Give the container-eval tests the credential and routes their recipes need
a9de8817  Extract the reusable visuals engine and close its trace/media acceptance
7a099b0c  (base) Clear stale sign-in status and pairing responses on account switch
```

The commits are in `workshop-v010`'s object store, so they are safe. **The
worktree they were built in is under `/tmp` and is not.** Get your own:

```sh
git -C ~/GitHub/workshop-v010 worktree add ~/GitHub/wt-v010-visuals-release codex/v0.10.0-visuals-release
cd ~/GitHub/wt-v010-visuals-release && npm ci        # see the node_modules trap below
```

Nothing is pushed. No remote has this branch, and `codex/v0.10.0` does not
contain it.

### What is already proven on this exact branch

- Renderer build, both TypeScript projects, and the native build all pass.
- Visual JS suite 433 passed / 1 skipped; native `visuals::` 125/0/3;
  `data::` 22/0; `optimizers::frames` 3/0.
- The binary stamps `f8cbecbf1849` with **no `-dirty`**, and
  `./scripts/desktop-instance.sh print <name>` resolves the same clean
  revision — so the launcher's own `cua-build` clean-tree gate is satisfied.
- **`mark_ready` passes.** Two reviews at 1440x900 and 760x760 with the full
  required check set, and `validate_certification_build_identity` accepted.
  This was the release blocker; it is closed.
- 40/40 family rows carry evidence: 30 paint-verified domain cuts, 2 nested
  cursors, 8 explained static. Receipts in `docs/receipts/2026-09-09/`.

Read `visuals-v010-final-handoff.md` for how each of those was established.
Read this file for what to do next.

## Do these, in this order

### 1. Publish the branch (5 min)

```sh
git push -u origin codex/v0.10.0-visuals-release
```

Open the PR against `codex/v0.10.0`. The four commit messages are written to be
the PR body; you should not need to invent a narrative.

### 2. Finish the certification tail (about an hour, mostly builds)

Two steps remain after `mark_ready`. Stand up an instance on a binary built
from this branch — the identity is what the gates check.

```sh
# renderer first: the app answers "renderer did not answer in time" without it
(cd apps/synth_desktop && node --preserve-symlinks ../../node_modules/vite/bin/vite.js \
   --host 127.0.0.1 --port 1420 --strictPort) &
TAURI_CONFIG="$(<path/to/instance-config.json)" cargo build \
  --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --bin synth-desktop --bin synth-visuals-mcp
```

**a. Seal.** `visual_seal` compiles an exact revision into a local immutable
ArtifactBundle v1. Route `POST /v1/visuals/<id>/seal`, MCP operation `seal`:

```sh
node visuals/tests/native_visual_mcp.mjs <instance-root> accept-diagram-mermaid-v1 \
  manage:seal '{"revision":<current>}'
```

**b. Packaged smoke.** Everything above ran against a debug build serving the
renderer from Vite. Build the release-profile packaged app and repeat: mount a
visual, capture, `capture_review` at both viewports, restart, confirm the
restored state. This is the one gap between "certified" and "shipped", because
it is the only configuration a user actually runs.

Record both in `docs/receipts/2026-09-09/` beside the existing receipts. Do not
leave evidence in `/tmp`; the receipts commit exists because that mistake was
already made once.

### 3. Decide the three open questions (see below), then merge.

## The three decisions

These are judgement calls with real trade-offs. They were deliberately left
open rather than made unilaterally.

**Reports renders a family shell unhosted.** `ReportsPage.tsx` imports
`@synth/visual-templates/analysis/trace.rollout_inspector.v1/shell` and renders
it directly. It works — a shell falls back to ordinary local React state when
unhosted, and a report is read-only — but that pane gets no session, no
retained reads, no capture barrier and no observation. Routing it through
`VisualHost` would give it those guarantees and change report rendering
semantics, so it needs its own acceptance. Shipping as-is is defensible;
shipping while *believing* reports have the same guarantees as panes is not.

**The dirty `workshop-v010` worktree.** It still has 407 changed entries. That
content is now committed on the release branch, so the worktree is redundant —
but other sessions may be working in it. Do not clean it without checking. If
you do reconcile it, the boundary is documented: everything is visuals-lane
except `session/codex/{event_pump,manager,proto,tests}.rs` and
`tests/fixtures/fake_codex_app_server.py`.

**The containers `artifact_ids` linkage.** `EventV5.artifact_ids` was never
populated for an application-event capture, so every consumer projection
reports no artifacts per event and Workshop's sealed-media branch cannot fire.
A tested fix is on `feat/link-events-to-their-artifacts` in the containers repo
(collector, detached capture service, finalizer; 2 new tests plus 247 existing
tracing tests pass). Adopting it means bumping Workshop's exact pin
(`synth-containers-version.txt`, currently `0.4.2.dev20260903`) and
re-registering the dev build, which cascades into every lane on that pin. It is
**not** required for this release: sealed frames are reachable by digest today,
they are just not linked to their step.

## Not yours: four optimizer assertions

A clean checkout of `7a099b0c` failed 27-29 `optimizers::` tests. The harness
causes are fixed on this branch (a test service that never installed the
secrets proxy its recipes require; a craftax mock two routes behind the
protocol) and so are three source-scan guards. **Four remain:**

```
optimizers::container_eval::tests::non_codex_workspace_eval_uses_chat_completions_capability
optimizers::container_eval::tests::final_record_projection_preserves_authoritative_provider_receipt
optimizers::container_eval::tests::a_running_craftax_rollout_relays_its_whole_journal_before_it_settles
optimizers::container_eval::tests::cancelling_in_flight_trials_settles_each_as_cancelled
```

They are product-intent assertions the optimizers lane owns: proxy capability
classification (`responses.create` vs `chat.completions.create`), a telemetry
token count (116385 vs 100471), a projection field (`Null` vs `0`), and
cancellation settlement (`failed` vs `cancelled`). Each is a real disagreement
between the code and a test about what should happen, not a broken harness.

**Measure against 4, not against 0.** If you see 4 failures in `optimizers::`,
that is the expected state of this branch, and it is 23-25 better than the base
commit. Do not edit an assertion to get a green suite; that is deleting the
question, not answering it.

## Traps that will cost you an afternoon

**A release checkout needs its own `node_modules`.** Symlinking another
worktree's makes esbuild resolve React twice, and two managed-frame browser
tests fail in a way that looks exactly like a missing changeset. `npm ci` in
your worktree.

**Start Vite before the app.** The isolated app starts happily without it and
then answers `renderer did not answer in time`, which reads like a hung
renderer.

**`apps/synth_desktop/src-tauri/target` gets cleared by another lane.** If
`native_visual_mcp.mjs` reports `ENOENT`, rebuild `synth-visuals-mcp` before
concluding anything about the instance.

**Per-family acceptance needs `--refresh-fixture` for any `optimizer_run`
binding.** Without it the visual keeps a binding to a run id that no longer
resolves, and capture correctly fails closed with "Capture visual is
unresolved, loading, or invalid" — which reads like a capture bug.

**`capture.pixels` is fixed to the mounted pane's surface.** It cannot reach
below the fold. The review window is the adjustable surface; that is what
`native_visual_capture_viewport.mjs` exercises.

**Trace import needs the pinned format authority registered.** If
`resolve_trace_cli()` reports none, build it from a *clean* worktree of
`containers@a5743ef` — not from `~/GitHub/containers`, which is another lane's
dirty checkout:

```sh
git -C ~/GitHub/containers worktree add --detach /tmp/containers-0.4.2 a5743ef
/tmp/containers-0.4.2/scripts/register-local-dev-build.sh
```

## Still open, and honestly small

- **Recording-speed UI ergonomics and detached/offscreen panes.** Human checks
  on surfaces this harness only drives through committed state. Native MCP
  cadence and stepping already pass; what is unverified is the UI affordance.
- **Gate 7, consolidation.** Deprecating the `visuals/` compatibility symlinks
  and legacy migration-72 storage was always gated on the other gates passing.
  They now do, so this is newly unblocked rather than newly open. Keep the
  required legacy import/read adapters; do not delete persisted data or schema
  for cosmetic tidiness.
- **Eraser stays excluded**, by design, as it has been throughout.

## One thing not to lose

Five of the eight defects this lane fixed were invisible because a fallback
kept working: `show` was a silent no-op but review-capture still selected
visuals; the optimizer frame lane served nothing but the legacy spelling still
matched in tests; the source guards scanned an empty prefix and still passed.
Every one of them was found by feeding the system *real data* and looking at
the result, not by reading code.

If something here "was never exercised", check what the tests actually spell
before you trust that the code path works.
