# Handoff: sourced / compose visuals — CUA dogfood

**For:** an engineer who will package Workshop Desktop and try **dynamic visuals** with Computer Use.  
**Updated:** 2026-08-26 (CUA targeting — `desktop:dev` is not a CUA target)  
**Do not commit the original tree unless asked.** Do not mix this into optimizer plugin QA, Laguna, or Craftax SFT.

Noun map: [`docs/qa/v08-visuals-data-model.md`](./qa/v08-visuals-data-model.md)  
Skill: [`apps/synth_desktop/skills/use-synth-visuals/SKILL.md`](../apps/synth_desktop/skills/use-synth-visuals/SKILL.md)  
Launcher: [`scripts/desktop-instance.sh`](../scripts/desktop-instance.sh)

---

## Status of the last attempt

Automated acceptance **passed**. Live CUA **was correctly blocked**.

| What | Result |
| --- | --- |
| `npm install` | passed |
| Sourced validator + Playwright compose/sourced | passed |
| `npm run desktop:dev` (`codex` instance) | unsigned `tauri dev` binary — **not a CUA target** |
| Computer Use vs `com.synth.desktop.v08.dev.codex` | refuses: raw process has no LaunchServices identity |
| `./scripts/desktop-instance.sh cua` on the dirty original | correctly refuses: `cua-build requires a clean checkout` |

Do **not** retry `desktop:dev` for this proof. Do **not** bypass the dirty-tree check. Do **not** target `com.synth.desktop.v08.dev.codex`. Do **not** commit `/Users/joshuapurtell/GitHub/synth-mlx-rl` to satisfy packaging — `cua-build` stages the **pinned** mlx-rl (`5d6db143` + lock `7f14b704…`), not current HEAD.

The launcher says this explicitly: raw `tauri dev` binaries have no LaunchServices app identity, so accessibility clients cannot address a named instance. Only a signed debug `.app` registers `BUNDLE_ID`.

---

## What you are proving

The agent authors a visual **inside the packaged instance**, Desktop **runs it in the right pane**, and ingest stays host-owned. Computer Use then reads that pane (`list_apps` → the packaged bundle id → `get_app_state` / click).

Two dialects, same kit:

| Dialect | Template | Agent writes | Pane mounts |
| --- | --- | --- | --- |
| Compose spec | `compose.visual.v1` | JSON placements | shipped `event_stream.v1` + `detail_modal.v1` |
| Sourced TSX | `sourced.visual.v1` | allowlisted TSX as `content` | compiled Shell using those same parts |

`blank.canvas.v1` is HTML/SVG with **no scripts**. It is not the TSX path.  
`visual_save_tsx` on other templates still writes a frozen wrapper around a registered family shell. **Executed custom TSX is only `sourced.visual.v1`.**

If the agent dumps TSX into `blank.canvas.v1`, guesses `/events`, or `fetch`es a stream URL, we have not won.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/GitHub/workshop-v08-release` | `codex/v08-release-integration` | **Leave dirty.** Sourced visuals plus other WIP. Do not commit here. |

Leave `containers`, `optimizers`, `optimizers-beta` alone. Leave `/Users/joshuapurtell/GitHub/synth-mlx-rl` dirty (perf WIP). Packaging uses `/Users/joshuapurtell/GitHub/synth-mlx-rl-v08-pinned` (`5d6db143`, already clean).

File work only under `/Users/joshuapurtell/GitHub`. Do not write under `Documents`.

---

## Required path: snapshot worktree → packaged CUA

The snapshot **already exists** at `/Users/joshuapurtell/GitHub/workshop-v08-sourced-cua` on `cua/sourced-visuals-dogfood`. Do not delete it and re-rsync unless the original tree moved. From that tree:

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-sourced-cua
./scripts/desktop-instance.sh cua sourced-cua
```

`cua-build` stages mlx-rl from `/Users/joshuapurtell/GitHub/synth-mlx-rl-v08-pinned` (clean `5d6db143`). It will **not** use the dirty sibling `/Users/joshuapurtell/GitHub/synth-mlx-rl`. Do not `git add` that sibling to unstick this.

If `uv build` is `Killed: 9`, do not rebuild mlx. This tree already has a verified `runtime-distributions/mlx-rl` for that pin; the stager reuses it. `SYNTH_MLX_RL_REBUILD=1` is the only way to force a wheel rebuild.

If you must recreate the snapshot:

`cua-build` needs a **clean git status in the tree it builds from**. Snapshot the dirty working copy into a sibling worktree, commit **there only**, then package. Do not push that branch. Do not amend or commit `workshop-v08-release`.

Instance name: `sourced-cua`  
Bundle id: **`com.synth.desktop.v08.dev.sourced-cua`**

```bash
SRC=/Users/joshuapurtell/GitHub/workshop-v08-release
WT=/Users/joshuapurtell/GitHub/workshop-v08-sourced-cua

# Stop the unsigned dev instance if it is still around.
npm --prefix "$SRC" run desktop:codex:stop

# Fresh worktree from current HEAD (original tree stays dirty).
rm -rf "$WT"
git -C "$SRC" worktree remove --force "$WT" 2>/dev/null || true
git -C "$SRC" branch -D cua/sourced-visuals-dogfood 2>/dev/null || true
git -C "$SRC" worktree add -b cua/sourced-visuals-dogfood "$WT" HEAD

# Copy the dirty working tree, not node_modules / cargo targets.
rsync -a --delete \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude '**/node_modules/' \
  --exclude 'target/' \
  --exclude '**/target/' \
  --exclude 'apps/synth_desktop/src-tauri/target/' \
  "$SRC/" "$WT/"

cd "$WT"
git add -A
git status
# Review: sourced visuals + lineage/experiments WIP is expected. No secrets.
git commit -m "$(cat <<'EOF'
snapshot: sourced visuals for local CUA dogfood

Do not push. Original workshop-v08-release stays uncommitted.
EOF
)"

# Confirm this copy is clean for cua-build.
git status --porcelain --untracked-files=no
# must print nothing

df -k . | awk 'NR==2 {print $4}'   # need ≥ 5242880 KiB (~5 GiB)

./scripts/build-computer-use-helper.sh ensure-dev
# if signing identity missing:
#   ./scripts/setup-desktop-dev-signing.sh

npm install
# mlx-rl: do not use the dirty sibling. Launcher defaults to
# /Users/joshuapurtell/GitHub/synth-mlx-rl-v08-pinned when that tree exists.
./scripts/desktop-instance.sh cua sourced-cua
```

Need ≥5 GiB free, helper at `helpers/synth-computer-use/target/bundle/Synth Computer Use.app`, and the Workshop development signing identity in the keychain.

If the helper / signing / disk preflight fails, **stop** and report that error. Do not fall back to `desktop:dev`.

Re-run the already-built app:

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-sourced-cua
./scripts/desktop-instance.sh cua-run sourced-cua
```

Confirm identity before chatting:

```bash
jq '{name, bundleId, sourceRevision, runtime}' \
  ~/.synth-desktop/instances/v08/sourced-cua/instance.json
```

Expect `bundleId` = `com.synth.desktop.v08.dev.sourced-cua`.

Login is automatic from machine-local ChatGPT / OpenRouter / Synth settings. No credential flags. Contract: [`docs/TEST_INSTANCE_LOGIN_CONTRACT.md`](./TEST_INSTANCE_LOGIN_CONTRACT.md).

### After the app is up

1. Human-install plugin id `computer-use` in **this** instance (agent cannot install it).
2. **New chat** in this instance. Existing Codex homes will not pick up `use-synth-visuals` until `ensure_home` runs again.
3. Computer Use `list_apps` must show `com.synth.desktop.v08.dev.sourced-cua`. Drive **that** id. Never `com.synth.desktop.v08.dev.codex`. Never a generic `synth-desktop` pid.

### Automated proof (already green — does not replace CUA)

Run from either tree; Playwright does not need a signed app:

```bash
node --experimental-strip-types --test visuals/tests/sourced_visual.test.mjs
./node_modules/.bin/playwright test sourced-visual.spec.ts --config apps/synth_desktop/playwright.config.ts
```

---

## CUA prompts (copy these)

New chat **in the packaged `sourced-cua` app**. Do not QA from Visuals-library create-only. Product path is **agent MCP in the conversation**, then Computer Use reads the **right pane**.

### 1. Compose (JSON kit)

> Load `use-synth-visuals`. Create `compose.visual.v1` with an inline `spec` that places `event_stream.v1` then `detail_modal.v1` from that log. Bind slot `stream` as `inline` with a few eval envelopes including `stream.subscribed` and `rollout.finished` with marker `CUA-SOURCED-1`. Then `show`. Do not use `blank.canvas.v1`. Do not guess `/events`. Click the rollout row and confirm the marker in the overlay.

### 2. Sourced TSX (the new product)

> Same skill. Now create `sourced.visual.v1`. Put allowlisted TSX in `arguments.content`: import `VisualChrome`, `EventStream`, `DetailModal`, and `useLiveEvalStream` only. Default-export a Shell that uses host `replay` / `props.stream.events` (no `fetch`, no `EventSource`, no guessed URLs). Bind the same kind of inline `stream` with marker `CUA-SOURCED-2`, then `show` that `visual_id`. The pane must execute this module, not wrap a registered family shell.

### 3. Fail closed (same visual family, new id)

> Create another `sourced.visual.v1` whose TSX imports `lodash` or calls `fetch`. `show` it. I should see a visible fail-closed error, not an event log.

Follow-up after (2) lands:

> Revise the **same** `visual_id`: add a short lede in the chrome. `update` `content`, then `show` again. Do not fork.

---

## Pass

1. Agent calls `visual_manage` (`create` / `create_with_bind` / `bind` / `show`). Tool name is `mcp__synth_visuals__visual_manage`. No `resources/list`.
2. Compose: pane `data-testid="visual-compose"`, event log, **no** `stream.subscribed` text, click opens `compose-detail-modal` with the marker.
3. Sourced: pane `data-testid="visual-sourced"` (not the compose shell, not blank canvas). Same event-log + overlay behavior.
4. Chat card and right pane are the **same** `visual_id`.
5. Forbidden module: `data-testid="visual-sourced-invalid"` naming the unknown import or `fetch`. No `compose-event-stream`.
6. `update` on sourced bumps revision and recompiles once (register-then-show, not per seed).
7. Computer Use targeted `com.synth.desktop.v08.dev.sourced-cua` (from `list_apps`), and a screenshot of the pane exists.

Allowlisted imports only:

```
react / react-dom / react/jsx-runtime
@synth/visuals/chrome
@synth/visuals/chrome/useLiveEvalStream
@synth/visuals/components/event_stream.v1
@synth/visuals/components/detail_modal.v1
```

Host still builds `ReplayClient`. The module must not discover URLs.

## Fail (stop and say which)

| Symptom | Likely cause |
| --- | --- |
| Computer Use cannot target `…dev.codex` / unregistered process | Still on `desktop:dev`. Package from the snapshot worktree. |
| `cua-build requires a clean checkout` on `workshop-v08-release` | Building the dirty original. Use `$WT`. |
| HTML/SVG in `blank.canvas.v1` | Skill still treating TSX as canvas / evidence-only |
| Pane loads a registered family shell, ignores `content` | `VisualHost` did not branch on `sourced.visual.v1` |
| `visual-sourced-invalid` on a good module | Import specifier not exact; or `content` not stored / `visuals_content` missing |
| Event log shows `stream.subscribed` | Ingest not dropping control envelopes |
| Agent `fetch`es or binds `http://127.0.0.1:…/events` | Guessed URL — fail closed |
| Two different ids in chat vs pane | Did not `show` the created id |
| Helper / signing / disk preflight | Report the exact launcher error. Do not fall back to `dev`. |
| `[mlx-runtime] release source must be clean` / wrong revision | Staging used dirty `/Users/joshuapurtell/GitHub/synth-mlx-rl`. Use the pin at `synth-mlx-rl-v08-pinned`. Do not commit mlx-rl WIP. |
| `uv build` `Killed: 9` | Do not rebuild. Stager must reuse `runtime-distributions/mlx-rl`. Pull latest `cua/sourced-visuals-dogfood` if this snapshot still rebuilds every time. |
| Live Craftax invented as a 5×5 stub | Wrong env — gold is rust GameBench only; fail closed if gold is down |

`desktop:dev` remains valid for **human eyeball** of Codex MCP. It is **not** this CUA proof.

---

## What landed (uncommitted on the original tree)

Kind `sourced_visual`, protocol `whole_file.v1`, `rendererKind: tsx`. Template `sourced.visual.v1`. Compiler: `visuals/runtime/sourcedValidate.ts` + `sourcedVisual.ts` (sucrase, allowlist). `VisualHost` loads CAS `content` and mounts the compiled Shell. Create without `content` fails closed. Skill + MCP copy no longer say “TSX is never executed.”

Compose `optimizer_run` (GEPA/SFT/CISPO event log on the same kit) is **not** in this cut. Product `optimizer.*` chrome stays. Candidate is still missing on the experiment spine.

---

## Live streams (second pass only)

First CUA pass should use **inline** events so you are not blocked on Harbor/Craftax. If you then bind live SSE:

- Ground the URL from the container / create-rollout receipt.
- Bind `live_sse` + declared `poll_url`. Never invent `/events`.
- Craftax is rust GameBench gold only (`env:craftax_gold`). If gold is down, fail closed.

---

## Operator checklist

1. Snapshot worktree + local commit on `cua/sourced-visuals-dogfood` only.
2. `./scripts/desktop-instance.sh cua sourced-cua` from `$WT`.
3. Confirm `bundleId` in `~/.synth-desktop/instances/v08/sourced-cua/instance.json`.
4. Human-install `computer-use`. New chat.
5. Prompt 1 → Computer Use reads the pane, click a row.
6. Prompt 2 → `visual-sourced`, marker in overlay, record `visual_id`.
7. Prompt 3 → fail closed.
8. Return `visual_id`s + screenshot. Do not “fix” by switching to `blank.canvas.v1`.
