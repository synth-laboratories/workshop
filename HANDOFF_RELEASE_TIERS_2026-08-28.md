# Handoff: release-tier maturity envelope (2026-08-28)

Everything below is merged into `codex/v08-release-integration` (the v0.8
branch) and **nothing is pushed to any remote**. Merges, in order:
`8280a6be` (envelope), `23f77de1` (build tooling), `600a4097` (titlebar badge
+ channel-bound updates + README). Topic branch: `release/tier-envelope-v08`
(worktree `~/GitHub/workshop-v08-auth-pairing`; the v0.8 branch itself is
checked out at `~/GitHub/workshop-v08-final`). A concurrent eval agent also
commits to this branch — merge, don't rebase, and never `git stash` in these
worktrees.

## What this is

A build-maturity flagging system answering two independent questions from one
contract, `contracts/release-tiers-v1.toml`:

1. **What maturity level is this build?** `core ⊂ stable ⊂ beta ⊂ alpha ⊂ dev`.
2. **Which verification gates belong at that level?** per-item dispositions
   (`required` / `recommended` / `optional` / `excluded`) per tier.

The envelope is compile-time on both layers — a stable build is structurally
unable to expose dev/alpha features:

- **Host**: cargo feature chain `tier-dev ⊃ tier-alpha ⊃ tier-beta ⊃
  tier-stable ⊃ tier-core`, `default = ["tier-stable"]`. Logic in
  `apps/synth_desktop/src-tauri/src/release_tier.rs` (embeds the TOML,
  `BUILD_TIER` const, `compile_error!` on tier-less or incompatible configs —
  notably `eval-driver` requires tier-beta+).
- **Renderer**: Vite `define` injects `__WORKSHOP_TIER__` and literal
  `__TIER_HAS_BETA__/ALPHA__/DEV__` booleans (`vite.config.ts`); gate JSX and
  imports on the raw globals for true dead-code elimination. Helpers in
  `src/renderer/src/flags/tier.ts`.
- **Cross-check**: `release_tier_get` (specta command #272) reports the host
  tier; `desktopBridge.ts` compares it to the bundle tier at startup and logs
  a packaging defect on mismatch.

User-visible: Settings → Build card (`settings-build-tier`) and, on
beta/alpha/dev bundles only, a tier badge in the titlebar top right
(`titlebar-tier-badge`) — both statically absent from stable/core bundles.
`update_check::CHANNEL` now defaults to `BUILD_TIER`, so each tier app polls
only `/releases/<tier>/latest.json` (`SYNTH_DESKTOP_CHANNEL` still overrides).

## Key semantic: included vs present

`included_at` = classified inside the envelope (`min_tier ≤ tier`).
`present_at` = actually in the binary. Pre-envelope features marked
`enforcement = "declared"` + `grandfathered = true` (e.g. `hosted_workflows`,
`containers_computer_use`, both classified beta) ship in **every** build until
someone lands a structural gate; the included/present gap is the visible
gating backlog (`workshop-tier-plan` prints it as
`grandfatheredAboveEnvelope`). The contract loader refuses any *new*
alpha/dev feature without a compiled/bundled gate, so the backlog can only
shrink. Runtime flags (`WORKSHOP_FLAG_<KEY>` env → contract default) narrow
the envelope, never widen it.

## Tooling

```bash
cargo run --bin workshop-tier-plan -- <tier>   # envelope + verification plan, JSON
scripts/release-gate.sh <tier> [--required-only]  # runs the plan → receipt in work/release-gates/
scripts/build-tier.sh <tier|all> [--debug]     # tier-aligned app build(s)
npm --prefix apps/synth_desktop run build:tier -- <tier>   # same via npm
```

`build-tier.sh all` stages four side-by-side apps into
`work/tier-builds/<tier>/` with tier-suffixed product names and bundle ids
(`Synth Workshop Beta` · `com.synth.desktop.beta`; stable keeps the canonical
identity) plus a `manifest.json` per app binding tier/profile/commit. A
debug-profile set built 2026-08-28 sits in the auth-pairing worktree's
`work/tier-builds/` (~315 MB each; the stable app's manifest records
`e9a0c05f`+dirty because it built moments before the tooling commit —
content-identical, rebuild if you want a clean receipt). `work/` is
gitignored. Release-profile: same command without `--debug` (first run pays a
cold release compile, ~15+ min). Dev instances (`scripts/desktop-instance.sh`)
now build `--features eval-driver,tier-dev` and export `WORKSHOP_TIER=dev`.

## Verification state

On the merged head: `cargo test --lib -- release_tier update_check` 14/14;
specta export gate green (271→272 documented in `contract/specta.rs`);
renderer typecheck; `lint:app-css`; Playwright `tier-envelope.spec.ts` (3
tests) plus `theme6-privacy` and the titlebar-adjacent suites
(layout-invariants, design-debt, poolside-polish, get-started) — the only
failure is the pre-existing "design debt (expected fail until fixed)" lane.
DCE was proven directly: `build-tier-badge` appears in 0 stable-bundle files
and 1 dev-bundle file; the four built host binaries hash distinctly.

## Not done / next steps

1. **Frontend channel half** (`~/GitHub/frontend-v08-release`, its own
   `codex/v08-release-integration`): widen `desktopRelease.ts` `channel` to
   the four tiers, add `/releases/<channel>/latest.json`, per-channel env
   conventions; `/download` stays stable-only, beta gets its own page,
   alpha/dev never advertised. Design recorded in `docs/RELEASE_TIERS.md`
   § "Releases: channels in the frontend". Deliberately deferred until the
   first non-stable line is actually published — the desktop side already
   degrades correctly (missing manifest = "no update known"). Note the
   frontend repo currently has the unmerged `auth/device-pairing-v2` branch
   checked out.
2. **Gating backlog**: decide whether `hosted_workflows` and
   `containers_computer_use` really are beta; if yes, land compiled/bundled
   gates and drop `grandfathered`; if no, reclassify to stable. Either way is
   a contract edit + the pinned tests will force the paired updates.
3. **Verification matrix curation**: current items/dispositions are a
   sensible seed (host tests, typecheck, playwright, ui-gates, crash drill,
   auth live driver, provenance, CUA harness) — owners should review them,
   and the NanoHorizon five-seed eval (frontend repo) is not yet represented.
4. **CI**: nothing runs `release-gate.sh` automatically yet.
5. A stable-tier `release-gate.sh stable` full run has never been executed
   end to end (it includes the live auth driver and provenance, both manual).

## Gotchas for the next person

- `lint:app-css` flags ANY added `font-size`/`border-radius` line — regex
  backtracking defeats its `var()` exemption. Write `font-size:var(--x)`
  with **no space after the colon**, or avoid the properties.
- MLX staging (`stage-mlx-runtime-distribution.sh`) refuses the dirty
  `~/GitHub/synth-mlx-rl`; `build-tier.sh` and `desktop-instance.sh` resolve
  the clean pinned sibling `synth-mlx-rl-v08-compat` automatically.
- Adding a specta command: append in `contract/specta.rs`, bump the count
  with a comment, regenerate via
  `cargo test --lib regenerate_protocol_bindings -- --ignored`.
- `core` builds need `--no-default-features` (build-tier.sh handles it);
  `all` deliberately excludes core — it is a classification, not a channel.

## Adjacent in-flight work (same session lineage)

- **Telemetry** (merged, working): see `docs/WORKSHOP_V06_IDENTITY_TELEMETRY_USAGE.md`
  and backend `dba6d8fa9`+; the slot1 backend still needs a rebuild/restart to
  serve `/api/v1/product/usage-events` live.
- **Auth device pairing v2** (NOT merged to v0.8): branch
  `auth/device-pairing-v2` in workshop + frontend, based on
  `eval/inline-first-admission` — needs rebase/cherry-pick or it drags ~21
  eval commits. QA handoff: `HANDOFF_AUTH_PAIRING_V2_QA_2026-08-28.md`.
