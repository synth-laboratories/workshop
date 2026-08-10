# Workshop

> **Visibility note:** This repository is currently **private**. It is intended to become **public**.

Synth Desktop / Local Agent Workbench — a local-first agent research and development workbench where agents can run locally (Laguna XS 2.1) or in Synth Cloud (Intern sync/async), and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

## Branching

| Branch | Role |
| --- | --- |
| **`dev`** | Day-to-day integration branch. Open PRs against `dev`. Keep `dev` current. |
| **`main`** | Release branch. Merge `dev` → `main` only for cut releases (for example v0.1). |

Do not land feature work directly on `main`. After a release merge, fast-forward `dev` to `main` so they match again. Alignment checklist for the next cut: [`HANDOFF_DEV_MAIN.md`](./HANDOFF_DEV_MAIN.md).

## Status — v0 ready for review

| Surface | Path | Role |
| --- | --- | --- |
| **Real app** | [`apps/synth_desktop`](./apps/synth_desktop) | Tauri 2 + Rust CoreRuntime |
| **Visuals infra** | [`visuals/`](./visuals) | 9 genre templates, registry, MCP tools, TSX save |
| **Mock (UX pin-down)** | [`apps/mock`](./apps/mock) | Fixture-only; do not confuse with product |
| Runtime | `apps/synth_desktop/src-tauri` | Rust-owned sessions / runs / events / inventory / visuals |
| Agent runtime | `codex app-server` | Local/configured-provider coding-agent sessions |
| Inference | `services/laguna-daemon` | Responses-compatible Laguna → MLX boundary |

### Local Laguna XS 2.1

```bash
npm run laguna:setup    # once: mlx venv + NVFP4 weights (~21.6 GB)
npm run laguna:serve    # :7333 OpenAI-compatible daemon
source ~/.synth-desktop/laguna/env.sh
npm run dev --workspace @synth/synth-desktop
```

Desktop probes `http://127.0.0.1:7333` automatically. Details: [`services/laguna-daemon/README.md`](./services/laguna-daemon/README.md).

### Desktop development and acceptance

Use the repository-owned lifecycle commands instead of opening a build-tree
`.app` manually:

```bash
npm run desktop:dev      # primary hot-reload loop; isolated instance "codex"
npm run desktop:codex:status
npm run desktop:codex:stop
npm run desktop:check   # parallel typecheck + cargo check; normal checkpoint
npm run desktop:build   # parallel typecheck + Tauri release build; no tests
npm run cache:rust:stats # inspect Rust compiler-cache effectiveness
npm run desktop:verify   # full Rust + renderer acceptance battery; release/CI gate
npm run desktop:install  # standard build → atomic local /Applications install → launch
npm run desktop:install:release # full release gate → install → launch
npm run desktop:restart  # restart the installed canonical app
npm run desktop:status   # verify the one allowed process and install path
npm run desktop:stop
```

Named instances are the normal edit/test loop and never stop another instance.
Their exact source revision, executable, PID, data root, and manifest are shown
under Settings → Runtime → Desktop identity. The canonical lifecycle is
reserved for release acceptance.

Use the test batteries according to the scope of the change:

| Battery | Command | Run it when |
| --- | --- | --- |
| Focused | The relevant `npm`, Playwright, or Cargo test directly | During iteration and after a localized UI/runtime change. |
| Check | `npm run desktop:check` | Before handoff or when renderer/native contracts changed; parallel TypeScript and Rust compile checks. |
| Build | `npm run desktop:build` | Produce a local release bundle. It overlaps typechecking with the real Tauri build and runs no tests. |
| Full release | `npm run desktop:verify` | Before merging a release PR, cutting a release, or after broad runtime/integration changes. |

`desktop:install` runs the standard build (with no separate `cargo check` or test
battery), then signs and verifies the staged bundle, backs up the previous
install under `~/.synth-desktop/backups/app-builds`, and launches only
`/Applications/Synth Desktop.app`. Use `desktop:install:release` when the full
release battery must pass before installation. Acceptance testing and Computer Use must
target that full path. `desktop:stop` targets only that exact installed path (or
the canonical Cargo debug executable); it does not stop named instances or
arbitrary copied apps. Do not launch
`apps/synth_desktop/src-tauri/target/*/bundle/macos/Synth Desktop.app`; the
lifecycle commands never use a generic process-name match. Use the Runtime
identity receipt or the named instance manifest for CUA rather than relying on
whichever generic Synth window is focused.

Build acceleration is layered: Turborepo owns the npm-workspace task graph and
caches deterministic renderer tasks; Cargo remains authoritative for Rust;
`sccache` is detected automatically and caches eligible `rustc` invocations
under `~/.cache/synth-workshop/sccache`. The final macOS bundle, signing,
backup, and installation remain uncached and explicit.


### Dogfood gates (verified)

- Local Laguna XS 2.1 agent path through Codex app-server and the Responses-compatible MLX sidecar
- Configured Responses-compatible model APIs through Codex app-server
- Inventory: local + cloud containers, Trace V5 ingest, 9 visual templates, save-as-TSX
- Live dock/eval visual simulation
- Intern sync demo mailbox
- Accessibility surface testids + semantic eval hook
- Intern endpoint profiles (`prod`, `staging`, `local`) via `~/.synth-desktop/config.toml`

The runtime selects the production Intern endpoint by default. For local
dogfood, set `SYNTH_INTERN_DEMO=1`; the checked-in [`config.toml.example`](./config.toml.example)
shows the profile and endpoint shape.

## Product framing

> Synth Desktop is a local-first agent research and development workbench where agents can run locally or in Synth Cloud, and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

Core loop: **observe → understand → modify → evaluate → fine-tune → deploy**

## Docs

- [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](./WORKSHOP_QUALITY_STYLE_GUIDE.md) — unified visual, interaction, runtime-honesty, accessibility, and test quality bar
- [`workshop_style.md`](./workshop_style.md) — provisional categorical triage: unacceptable, fix-before-review, and expected-fail debt
- [`HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md`](./HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md) — current Rust core / visuals / Intern SDK handoff
- [`testing.md`](./testing.md) — Playwright, Bombadil, Rust, and runtime coverage map
- [`HANDOFF.md`](./HANDOFF.md) — full product + architecture
- [`synth_desktop_research_eng.md`](./synth_desktop_research_eng.md) — Trace V5 / visuals / containers
- [`apps/synth_desktop/README.md`](./apps/synth_desktop/README.md) — runbook
- [`visuals/README.md`](./visuals/README.md) — template + MCP agent flow
- [`handoff-package/`](./handoff-package/) — eng reuse bundle

## License / ownership

Owned by [synth-laboratories](https://github.com/synth-laboratories). Public release planned; treat contents as pre-release until then.
