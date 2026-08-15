# v0.3 frontend / Desktop handoff

For whoever is changing the Workshop UI after the v0.3.0 friends cut. This is how to edit the renderer and visual shells without breaking the packaged app.

## Where to work

Do **not** use `Documents/GitHub/workshop`. Use the v0.3 worktree (or a fresh checkout of `josh/v03` / `origin/dev`):

```text
/Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-gemini
```

| Ref | Value |
| --- | --- |
| Branch | `josh/v03` (tracks `origin/josh/v03`) |
| Landed | `origin/dev` and `origin/main` via PRs #21–#25 |
| Friends ZIP | GitHub [v0.3.0](https://github.com/synth-laboratories/workshop/releases/tag/v0.3.0) |
| Product SHA for the current ZIP | `c146e83` on `origin/dev` — see [PACKAGE.md](./PACKAGE.md) |
| App identity | Desktop `0.3.0` / `v0.3` |

Unnotarized, adhoc-signed. Do not treat this as an Apple-notarized ship.

## Two trees, not one

Frontend work is split. Most “visual” bugs are **not** in `apps/synth_desktop`.

```text
apps/synth_desktop/src/renderer/     Chrome, chat, settings, Reports, VisualHost
visuals/families/                    Template shells (Craftax, Harbor, GEPA, Trace inspector, …)
visuals/chrome/                      Shared visual chrome / live SSE hooks
visuals/registry/index.ts            Catalog + shell importers (Vite glob)
apps/synth_desktop/src-tauri/        Rust host, MCP adapters, approvals, Reports seal
```

Vite root is `apps/synth_desktop/src/renderer`. Aliases:

| Alias | Resolves to | Use for |
| --- | --- | --- |
| `@/` | `apps/synth_desktop/src/renderer/src/` | App components |
| `@synth/visuals` | `visuals/registry/index.ts` **(a file)** | `resolveTemplate`, `getShellImporter` |
| `@synth/visual-templates` | `visuals/families/` | Direct shell imports |

`@synth/visuals` is a **file alias**. `@synth/visuals/families/...` will not work. Direct template imports must go through `@synth/visual-templates/<family>/<id>/shell`.

Example (Reports):

```ts
import TraceInspector from "@synth/visual-templates/analysis/trace.rollout_inspector.v1/shell";
```

The old flat path is gone:

```ts
// BROKEN — visuals/templates/ was deleted in SYN-3217
import TraceInspector from "@synth/visual-templates/trace.rollout_inspector.v1/shell";
```

Keep `vite.config.ts` and `tsconfig.json` pointed at `visuals/families`, not `visuals/templates`. The first v0.3 mint failed on that exact import.

## How a visual actually renders

1. Desktop opens a visual id → `VisualHost` (`apps/synth_desktop/src/renderer/src/components/VisualHost.tsx`).
2. Native diagrams (`diagram.mermaid.v1`, systems) bypass TSX and use the Rust renderer.
3. Everything else: `loadVisualShell(templateId)` in `runtime/visualsLoader.ts` → `getShellImporter(id)` in `visuals/registry/index.ts` → `visuals/families/**/shell.tsx`.
4. Template **ids stay stable** (`live.craftax.v1`, `optimizer.gepa.live.v1`, …). Only the directory moved under a family folder.

Add or change a live visual:

1. Edit `visuals/families/<family>/<templateId>/shell.tsx` (and `template.json` if slots change).
2. Do not recreate `visuals/templates/<id>/`.
3. Run `npm run test:visuals` (registry asserts ids and refuses `optimizer.dag.live.v1`).
4. If you add a template, add the id to `visuals/tests/registry.test.mjs` `EXPECTED_IDS`.

`optimizer.dag.live.v1` is **v0.4**. Overlay math still lives under `visuals/families/optimizers/_shared/optimizer.run.v1/overlays/dag/` for `optimizer.run.v1`. Relative imports there must reach `visuals/runtime/liveStream.ts` (six `../`, same as GEPA/SFT overlays). Four `../` resolves under `families/optimizers` and breaks `test:visuals`.

## App chrome (chat, settings, Reports)

| Surface | Start here |
| --- | --- |
| Shell / routes | `src/renderer/src/App.tsx`, `routes.tsx` |
| Chat | `ChatTranscript.tsx`, `Composer.tsx`, `ComposerDock.tsx` |
| Visual pane | `VisualHost.tsx`, `VisualsPage.tsx`, `PaneResizeHandle.tsx` |
| Settings / About changelog | `SettingsPage.tsx`, `ContextSettings.tsx` |
| Reports | `ReportsPage.tsx` |
| Data / Trace inspect | `DataPage.tsx` (opens `trace.rollout_inspector.v1`) |
| Mander | `src/renderer/src/components/mander/` |
| CSS | `src/renderer/src/styles/` (`app.css`, `tokens.css`, `primitives.css`) |
| Preferences | `src/renderer/src/preferences/` |
| Tauri bridge | `src/renderer/src/runtime/desktopBridge.ts`, `bridge/types.ts` |

User-visible 0.3.0 notes: `v0p3_changelog.md` **and** the `CHANGELOG` array in `SettingsPage.tsx`. Update both.

## Dev loop

From the worktree root:

```bash
npm run desktop:dev          # named instance via scripts/desktop-instance.sh
# or
npm run frontend:dev --workspace @synth/synth-desktop   # Vite only, port 1420
```

Checks that catch the v0.3 footguns:

```bash
npm run frontend:build --workspace @synth/synth-desktop
npm run typecheck --workspace @synth/synth-desktop
npm run test:visuals
cargo check --manifest-path apps/synth_desktop/src-tauri/Cargo.toml
```

UI gates (need a built app): `npm run desktop:ui-gates:bombadil` / `desktop:ui-gates:playwright`.

## Remint / republish after frontend changes

`scripts/release-artifact.sh` refuses a dirty tree. Commit first.

```bash
# 1. clean tree
git status --porcelain   # must be empty

# 2. Containers pin must be a real git repo (a worktree .git *file* fails the script).
#    If SYNTH_CONTAINERS_ROOT points at a worktree, clone it to a normal repo first.
export SYNTH_CONTAINERS_ROOT=/path/to/containers-clone   # .git directory, clean

# 3. fresh output dir (script dies if stage/ already exists)
rm -rf /tmp/synth-desktop-v0.3-release
export SYNTH_RELEASE_ROOT=/tmp/synth-desktop-v0.3-release

# 4. do not clobber /Applications unless you mean to
export SYNTH_RELEASE_INSTALL_APP="$SYNTH_RELEASE_ROOT/installed/Synth Desktop.app"

./scripts/release-artifact.sh all
```

Then update [PACKAGE.md](./PACKAGE.md) + [PROVENANCE.json](./PROVENANCE.json) from the new `PROVENANCE.json`, commit those docs, push `josh/v03`, PR into `dev` then `main`, retag / replace GitHub `v0.3.0` assets only if Josh wants a new friends ZIP.

`all` installs. Default install path is `/Applications/Synth Desktop.app` unless `SYNTH_RELEASE_INSTALL_APP` is set.

## Merge traps already hit on this cut

Keep these if you merge leftover branches:

- **Families vs flat templates.** `origin/dev` once still had `visuals/templates/`. Always keep families + the `@synth/visual-templates` → `visuals/families` alias.
- **Typed broker vs Reports.** Keep `SessionPersistence::append_boundary_event` as the broker version (`Result<()>`, persist-before-publish). Do not reintroduce a second copy that returns `Option<AppEvent>`. Same for `list_events_of_kinds_after` in `event_journal.rs`.
- **Sidecar + Craftax GEPA.** Dev sidecar `0.2.9` and `gepa.craftax.smoke.v1` both stay. Do not drop `authorize_sidecar` on install/start/stop.
- **Plugins MCP + session MCP.** Both bins: `synth-plugins-mcp` and `synth-session-mcp`. Both skills. Both IPC paths (`/v1/plugins` and `POST /v1/sessions/present`).
- **Prompt materialization.** `optimizers/service.rs` should accept both `stage2_system` (Banking77) and `react_system_prompt` (Craftax).

Do **not** merge `agent/v03-proofs-e2-e4` (E2–E4 dump). DAG live visual stays v0.4.

## Stashes on this worktree (older than the ZIP)

`git stash list` still has pre-mint SYN-3222 / SYN-3224 parks (`thread-read`, Harbor SSE, `app.css`, etc.). The published ZIP is from later `origin/dev` merges that already include SYN-3222 / SYN-3224 Workshop surfaces. **Do not pop those stashes onto a clean tree** without diffing against HEAD — they are likely stale and will recreate duplicates.

## Not done (do not claim)

- Apple notarization
- Installed-app CUA / Bombadil / Playwright on the friends ZIP
- Harbor 3×2×5 DEO matrix (SYN-3224 Workshop surface ≠ matrix evidence)
- Intern, E2/E3/E5, GELO+OHCO iteration, DAG live visual
- SYN-3224 matrix evidence — reopened In Progress in Linear; the other v0.3 tickets were closed Done on 2026-08-15
