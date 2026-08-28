# Workshop release tiers

`contracts/release-tiers-v1.toml` is the single source of truth for two
independent questions:

1. **What maturity level is this Workshop build?** — the feature envelope.
2. **Which verification gates belong at that level?** — the release plan.

Tiers form a strict envelope progression, narrowest to widest:

```text
core ⊂ stable ⊂ beta ⊂ alpha ⊂ dev
```

`dev` contains everything in development; `alpha` what is ready for internal
and design-partner use; `beta` what is suitable for broader testing; `stable`
the supported public product; `core` the especially durable subset expected to
work across all distributions. Selecting a tier includes every feature whose
`min_tier` is at or below it — a beta build carries core, stable, and beta
features and is **structurally unable** to expose alpha or dev ones.

## How the envelope is structural

| Layer | Mechanism | Where |
| --- | --- | --- |
| Host (Rust) | `tier-*` cargo feature chain: `tier-dev` ⊃ `tier-alpha` ⊃ `tier-beta` ⊃ `tier-stable` ⊃ `tier-core`; gated code is compiled out with `#[cfg(feature = "tier-…")]` | `src-tauri/Cargo.toml`, `src-tauri/src/release_tier.rs` |
| Renderer (TS) | Vite `define` injects `__WORKSHOP_TIER__` and literal `__TIER_HAS_BETA__` / `__TIER_HAS_ALPHA__` / `__TIER_HAS_DEV__`; statically-false branches and the modules only they import are eliminated from the production bundle | `vite.config.ts`, `src/renderer/src/flags/tier.ts` |
| Cross-check | The host reports its compiled tier over `release_tier_get`; the renderer compares it to its own bundle tier at startup and reports a mismatch as a packaging defect | `runtime/desktopBridge.ts` |
| Incompatible configs | Rejected at compile time (`compile_error!`): a tier-less build, or the `eval-driver` QA control plane inside a stable/core envelope | `release_tier.rs` |

Packaging defaults are stable on both layers: cargo `default = ["tier-stable"]`
and Vite builds default `WORKSHOP_TIER=stable` (the dev server defaults to
`dev`). Dev instances (`scripts/desktop-instance.sh`) build
`--features eval-driver,tier-dev` with `WORKSHOP_TIER=dev`.

## Local builds

`scripts/build-tier.sh` is the supported way to build any tier locally with
both layers aligned in one command:

```bash
scripts/build-tier.sh beta            # one packaged app at the beta tier
scripts/build-tier.sh all --debug     # all four channel tiers, debug profile
npm --prefix apps/synth_desktop run build:tier -- alpha   # same, via npm
```

`all` builds stable, beta, alpha, and dev sequentially into
`work/tier-builds/<tier>/`. Non-stable tiers get a tier-suffixed product name
and bundle identifier (`Synth Workshop Beta` · `com.synth.desktop.beta`) so
the four apps install and run side by side; stable keeps the canonical
identity. Each output directory carries a `manifest.json` binding the app to
its tier, profile, and source commit, and every app reports its own envelope
in Settings → Build (with the pre-release badge on beta/alpha/dev). `core`
builds individually the same way (the script handles the
`--no-default-features` cargo dance it needs); it is a durability
classification, not a channel, so `all` omits it.

The raw two-knob form underneath, when you need it:

```bash
# host                                            # renderer
cargo build --features tier-beta                  WORKSHOP_TIER=beta npm run frontend:build
```

If the knobs ever disagree, the startup cross-check reports the mismatch as a
packaging defect. Changing tiers is always a rebuild/reinstall — there is no
runtime switch, which is what keeps the envelope structural.

## Feature records

Each `[[feature]]` declares `name`, `summary`, `owner`, `min_tier`, and an
`enforcement` class:

- `compiled` — a cargo feature (named in `cargo_feature`) removes the host
  code from excluded tiers.
- `bundled` — a `__TIER_HAS_*__` define removes the renderer code from
  excluded bundles.
- `declared` — classification only: pre-envelope code, not yet gated. Allowed
  **only** with `grandfathered = true` and only at beta-or-below; the contract
  loader refuses an alpha/dev feature without a structural gate, so new
  pre-stable features cannot ship ungated.

**Included vs present.** A grandfathered feature classified above the build
tier (for example `hosted_workflows`, beta) is still *present* in a stable
binary until its gate lands. `release_tier::included_at` answers the
classification question, `present_at` the artifact question, and the
included/present gap — listed by `workshop-tier-plan` as
`grandfatheredAboveEnvelope` — is the visible gating backlog a promotion
review must read.

**Runtime flags** (`runtime_flag = { key, default }`) control rollout *within*
the envelope: resolution is the `WORKSHOP_FLAG_<KEY>` environment override,
else the contract default. A flag can disable an included feature; nothing at
runtime can enable an excluded one — the code is not in the artifact.

## Verification records

Each `[[verification]]` item (`kind` = test | eval | drill | manual) carries a
disposition per tier:

- `required` — must pass to release that tier.
- `recommended` — should run; omission is recorded with a reason in the receipt.
- `optional` — investigative coverage, selected when relevant.
- `excluded` — invalid, unsafe, or irrelevant there (for example the CUA
  instance harness drives the QA control plane, which stable builds
  structurally lack).

Feature maturity and verification importance evolve independently: a feature
can enter alpha with its smoke test immediately required while its expensive
eval stays recommended until the beta promotion makes it required.

## Release flow

```text
choose tier → resolve envelope → reject incompatible config (compile time)
  → build → select required/recommended verification → run
  → bind evidence to the exact revision → promote only if required all pass
```

- `cargo run --bin workshop-tier-plan -- <tier>` prints the resolved envelope
  and plan as JSON.
- `scripts/release-gate.sh <tier> [--required-only]` runs the plan and writes
  a receipt to `work/release-gates/` binding results to the commit. Manual
  items land as `needs-human` and block the promote verdict until a human
  attests them in the receipt; skipped recommended items carry a recorded
  reason.

**Promotion is explicit.** Moving a feature between tiers means editing its
contract record — maturity, enforcement, user-facing expectations — and the
matching verification dispositions in the same change. Two pinned test suites
keep the contract honest: `release_tier` tests in the host (envelope
monotonicity, stable hard gates, structural exclusion of QA surfaces) fail on
a contract edit that weakens the rules, and the specta gate pins the
`release_tier_get` boundary.

## Typical channel usage

- `dev` — fast local iteration; the whole feature surface, mostly recommended checks.
- `alpha` — internal/design-partner builds; incomplete features allowed, their
  core journeys (CUA instance harness) required.
- `beta` — externally testable; compatibility, packaging, recovery, and the
  relevant evals become hard gates.
- `stable` — supported public release; only mature features and the complete
  release-candidate pipeline (`auth-pre-release`, provenance) required.
- `core` — a durability classification more than a marketing channel; its
  checks stay required everywhere.
