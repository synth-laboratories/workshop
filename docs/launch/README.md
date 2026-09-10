# Workshop launch docs

**v0.7:** [v0.7-scope.md](./v0.7-scope.md) · runtime plan [v0.7-optimizers-runtime.md](./v0.7-optimizers-runtime.md) · LoRA catalog + dual-family inference [v0.7-lora-catalog.md](./v0.7-lora-catalog.md) · release folder [v0.7-release/](./v0.7-release/) (RELEASE_NOTES, KNOWN_ISSUES, ACCEPTANCE, TEST_REPORT, PROVENANCE, PACKAGE, ROLLBACK, READY, POST_RELEASE, COMMIT_MAP) · HealthBench Tinker evidence [v0.7-cispo-healthbench-canary.md](./v0.7-cispo-healthbench-canary.md). Precedent: [v0.4-release/](./v0.4-release/).

## Fresh-worktree build prerequisites (v0.7)

A worktree created from `origin/v0.7` does not build or test until these are done, in this order. Every lane hits them; none are in `npm run build`.

1. `npm ci --ignore-scripts` at the repo root. No workspace `package.json` in this tree declares a lifecycle script (checked 2026-08-20), so the flag changes nothing today; it is the form the lanes use so a future `postinstall` cannot reach out of the worktree.
2. `scripts/stage-packaged-cookbooks.sh` — copies `banking77_container` and `crafter_container` from a sibling `synth-cookbooks-public` checkout (or `SYNTH_COOKBOOKS_SOURCE_ROOT`) into `apps/synth_desktop/src-tauri/generated-resources/cookbooks/`, which `tauri.conf.json` lists under `bundle.resources`. Missing → the Tauri resource check fails before compiling.
3. `scripts/build-computer-use-helper.sh ensure-dev` — `tauri.conf.json` also bundles `helpers/synth-computer-use/target/bundle/Synth Computer Use.app`; the bundle must exist even for `cargo test`/`cargo check` of the desktop crate. `ensure-dev` keeps a valid existing bundle and otherwise builds + ad-hoc signs one (TCC grants do not survive a rebuild; a Developer ID `all` is the release path).
4. macOS has no GNU `timeout`. Do not wrap commands in `timeout …` in scripts or PR test plans; use the tool's own deadline flags (`cargo test` has none — run suites in the background and poll) or `gtimeout` only if coreutils is installed locally and never in committed scripts.
5. Then: `cargo test -p synth-desktop --lib optimizers::` (231 passed / 4 ignored at `905ef812`), `npm run typecheck`, `npm run test:visuals`, `npm run test:a11y`, and Playwright via `npm run test:ui`.
6. Packaged instances: `scripts/desktop-instance.sh cua <name>` builds a signed debug `.app` under `~/.synth-desktop/instances/v07/<name>/` (the script hard-fails on any release line other than `v0.7`; a `-dirty` tree disqualifies `assert-identity`). `scripts/test-desktop-instance.sh` covers the contract.
7. `scripts/release-artifact.sh` additionally needs a clean tree and a clean sibling `containers` checkout — see [v0.7-release/PACKAGE.md](./v0.7-release/PACKAGE.md).

v0.1 friends-release contract remains in this folder. **v0.2 launch status and E2E plan:** [v0.2-launch.md](./v0.2-launch.md). **v0.3 launch notes and integration status:** [v0.3-launch.md](./v0.3-launch.md). **v0.3 themes:** [v0.3-themes.md](./v0.3-themes.md). **v0.2 second-pass review + address plan (not started):** [v0.2-second-pass-2026-08-13.md](./v0.2-second-pass-2026-08-13.md). **v0.2 finish handoff (receipts / `v02golden`; dirty snapshot stale):** [V0.2_FINISH_HANDOFF_2026-08-13.md](./V0.2_FINISH_HANDOFF_2026-08-13.md). **Harbor GameBench code-policy DEO + Codex Luna med (visual-first):** [HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md](./HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md).

## v0.1 (frozen)

Frozen contract and remaining Gate F / Gate P work.

| Doc | Purpose |
|---|---|
| [V01_SCOPE_AND_OWNERS.md](./V01_SCOPE_AND_OWNERS.md) | Product contract, owner matrix, candidate SHAs |
| [LAUNCH_OPS.md](./LAUNCH_OPS.md) | Monitoring, flags, rollback, no-go, post-publish smoke |
| [GATE_SEQUENCE.md](./GATE_SEQUENCE.md) | Deterministic / integration / fault-injection sequence |
| [CLEAN_USER_REHEARSAL.md](./CLEAN_USER_REHEARSAL.md) | Download / signup / sign-in / checkout rehearsal |
| [CRAFTAX_LUNA_010.md](./CRAFTAX_LUNA_010.md) | Blocking Luna xhigh → 10 Luna-low Craftax scenario |
| [AUTH_WEB_HANDOFF.md](./AUTH_WEB_HANDOFF.md) | Clerk, device-init, download, upgrade deep link |
| [UPDATES_AND_CHANNELS.md](./UPDATES_AND_CHANNELS.md) | Passive v0.1 check, stable/nightly isolation, updater plan, rollback |
| [LAUNCH_READINESS_STATUS.md](./LAUNCH_READINESS_STATUS.md) | Live status vs Gate F / Gate P blockers |

Helper: `scripts/run_launch_gates.sh` runs the deterministic subset.
