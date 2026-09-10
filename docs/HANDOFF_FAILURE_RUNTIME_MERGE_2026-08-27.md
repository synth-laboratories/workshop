# FailureRuntime merge plan — 2026-08-27

## Objective

Integrate `codex/workshop-failure-runtime` into
`eval/inline-first-admission` without losing the proven NanoHorizon fixes, then
dogfood the unified failure/log lifecycle against the real J instance.

## Current topology

- Target branch: `eval/inline-first-admission`
- Target head when this plan was written: `c947051c76601fe194e5533430b252e39fa87c09`
- Failure branch: `codex/workshop-failure-runtime`
- Failure head: `c9cb827da51fd41ce1ad0ee18b1d9bd3277c98fe`
- Exact merge base: `61cb243abfca9b24503cb2b28b4ed9f777f4f50a`
- Target-only changes after the base: NanoHorizon dogfood documentation only
- Failure-branch changes after the base: 91 files
- Current overlapping changed paths: none

The failure branch already includes the experiment migration repair and Trace V5
runtime/payload fixes through `61cb243a`. Do not cherry-pick those fixes again.

## Integration method

1. Confirm both worktrees are clean and preserve their exact heads.
2. In `/Users/joshuapurtell/GitHub/workshop-failure-runtime`, rebase
   `codex/workshop-failure-runtime` onto the then-current
   `eval/inline-first-admission` head.
3. Re-run the failure branch's focused validation after the rebase.
4. In `/Users/joshuapurtell/GitHub/workshop-v08-release`, merge with
   `--ff-only`. Do not create an integration merge commit when a linear,
   reviewed series is available.
5. Keep the three logical commits intact unless conflict resolution materially
   changes their boundaries:
   - failure/log lifecycle specification
   - FailureRuntime, persistence, and domain settlement
   - FailureView and Errors-pane projection

## Conflict policy

No conflicts are currently predicted. If the target moves, treat these paths as
high-risk rather than resolving mechanically:

- `apps/synth_desktop/src-tauri/src/storage/migrations.rs`
- `apps/synth_desktop/src-tauri/src/visuals_ipc.rs`
- `apps/synth_desktop/src-tauri/src/contract/specta.rs`
- generated protocol TypeScript
- session restart/resume code
- optimizer admission and terminal reconciliation

Preserve these invariants during conflict resolution:

- Migration 48 repairs the legacy experiment/session unique index.
- Migration 49 owns the FailureRuntime tables.
- Canonical Trace V5 frame fields come from `event.payload`; no legacy fallback.
- `synth-trace` readiness requires an executable exact-version probe.
- Missing measurements remain unavailable and never become numeric zero.
- No Keychain credential path is introduced or enabled.
- No fixture or synthetic result may count as real-system acceptance.

## Static and unit validation gate

Run, in this order:

1. `scripts/check-failure-runtime.sh`
2. Focused FailureRuntime, migration, container-unhealthy, admission, session
   restart, and specta protocol tests from the failure handoff
3. Trace authority and canonical portable-frame regression tests
4. Complete Rust library suite
5. Renderer typecheck and production build
6. Generated-protocol cleanliness check
7. Git status and diff audit for generated or test-created debris

The existing warning backlog is not acceptance evidence. Every command must have
an explicit exit status and test count.

## Real J acceptance gate

The failure work has unit coverage but no real-system proof. After building J
from the integrated head, launch it through the canonical CUA path with the
already-authorized file-backed ChatGPT OAuth context. Never access Keychain.

Prove these cases using real persisted state:

1. J opens authenticated and can resume the existing task after restart.
2. The existing NanoHorizon run and five trusted traces remain readable after
   migration 49.
3. A real container-health failure creates one canonical failure identity,
   appears in Errors & Logs, and links to the affected container/run/session.
4. Repeating the same unchanged failure increments or relates occurrences
   according to the specification instead of creating split identities.
5. Repairing the condition settles the failure without deleting its history.
6. The chat card, experiment visual, trace workstation, and Errors pane expose
   the same failure/evidence-quality state.
7. Session restart preserves active/terminal failure identity and does not
   resurrect a settled failure as `Working`.
8. Emergency-sink import and diagnostics-index degradation are visible and
   explicitly labeled; they do not silently disappear.

Do not launch new paid rollouts merely to validate the failure ledger. Reuse the
existing terminal run and retained evidence wherever possible. Any new paid run
requires a separately bounded approval.

## Dogfood-gap mapping required before sign-off

Map every P0/P1 item in
`docs/DOGFOOD_NANOHORIZON_INLINE_EVAL_2026-08-27.md` to one of:

- canonical `FailureKind` and lifecycle state;
- evidence-quality gap with a typed reason;
- producer defect that Workshop records but cannot repair;
- renderer defect with no new domain failure identity.

The FailureRuntime must not turn missing reward, cost, step, or token telemetry
into generic strings. Each gap needs a durable code, authority, subject IDs,
first/last observation, occurrence count, cause chain, settlement rule, and
user-safe remediation.

## Release decision

Merge is acceptable only when:

- the rebased series is clean and reviewable;
- all static/unit gates pass;
- J migrates and launches authenticated;
- the Errors pane shows real failure records from J;
- existing NanoHorizon evidence remains intact;
- no second error lifecycle or string classifier was reintroduced;
- the exact integrated SHA and acceptance evidence are recorded.

