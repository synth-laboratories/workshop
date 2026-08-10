# Handoff: keep `dev` current, merge to `main` for releases

**Audience:** whoever next aligns local WIP and cuts a `dev` → `main` release  
**Date:** 2026-08-10  
**Current tips (at handoff write):** both should be at `71e0046` (`v0.1: Desktop stack + compact-on-send model switch (#2)`) after you push the README/`dev` sync below.

## Policy (also in README)

- **Work on `dev`.** Feature PRs target `dev`.
- **Release via `main`.** Merge `dev` → `main` only when cutting a release.
- After each release merge, **fast-forward `dev` to `main`** so `dev` stays the newest tip.

## What already landed (do not re-litigate)

- [PR #2](https://github.com/synth-laboratories/workshop/pull/2) merged `dev` → `main` for the v0.1 Desktop stack cut.
- That merge resolved ~70 **add/add** conflicts by preferring the **`dev` Desktop/Codex/Laguna line**. Auth pairing from `main` #1 was already represented on the `dev` history; do not “take theirs” wholesale if histories diverge again.
- Compact-on-send model switch is in tree:
  - Explicit decision tree: `apps/synth_desktop/src/renderer/src/runtime/modelSwitchPlan.ts`
  - Send path: `App.tsx` (`planComposerSend` / no landing kick on model chip)
  - Rust: `compact_before_model_switch` + `thread/compact/start` wait in `codex.rs`
  - Tests: `tests/model_switch_plan.test.mjs`, Rust `turn_send_compacts_on_source_model_before_rebind`, Playwright mid-chat provider switch

## Immediate sync (make `dev` newest)

```bash
git fetch origin
git checkout dev
git merge --ff-only origin/main   # should be a no-op once pushed
git push origin dev               # if local was ahead of origin/dev by the merge commit only
```

`dev` must be an ancestor of nothing unique on `main` after sync — ideally:

```bash
git rev-list --count origin/main..origin/dev   # expect 0 for “fully released”
git rev-list --count origin/dev..origin/main   # expect 0 after ff sync
```

If `main` is one merge-commit ahead of `dev`, fast-forward `dev` (do not rebase released history).

## Local leftovers (not on either branch — triage carefully)

These were stashed while shipping the v0.1 cut. **Do not dump them onto `main`.** Review on a topic branch off `dev`, then PR into `dev`.

| Stash | Notes |
| --- | --- |
| `stash@{0}` `wip: model_performance leftovers` | Small `storage/` edits + untracked `model_performance.rs` |
| `stash@{1}` `wip: pre-v0.1 local leftover 20260810` | Large mixed WIP: whisper, Composer images, App, laguna, polish, handoff md, scripts, etc. |
| `stash@{2}` duplicate of model-switch era WIP | Likely obsolete relative to merged `d491ee9`; diff before applying |

Also untracked locally: `work/` (ignore unless you know it is intentional).

Suggested triage:

```bash
git stash list
git stash show --stat stash@{1} | less
# For each coherent slice:
git checkout -b topic/<name> origin/dev
git stash apply stash@{N}   # or path-limited checkout from stash
# prune unrelated files, run desktop:verify, PR → dev
```

## Careful `dev` → `main` release checklist (next cut)

1. **Freeze `dev` tip** you intend to release; note the SHA.
2. **CI green on `dev`** (or run locally):
   - `npm run desktop:verify`
   - `node --test apps/synth_desktop/tests/model_switch_plan.test.mjs`
   - `cargo test --lib turn_send_compacts_on_source_model_before_rebind` (from `apps/synth_desktop/src-tauri`)
3. **Diff the release:**
   ```bash
   git fetch origin
   git log --oneline origin/main..origin/dev
   git diff --stat origin/main...origin/dev
   ```
4. **Open PR `dev` → `main`** with release notes (what ships, what is explicitly out).
5. **If GitHub reports conflicts:**
   - Prefer merging `main` *into* `dev` first on a integration branch, resolve there, then PR.
   - For add/add Desktop duplicates: default to **`dev` (ours)** unless `main` has a hotfix not cherry-picked.
   - Never force-push `main`.
6. **Merge with a normal merge commit** (keeps release boundary visible); avoid squash for the integration PR if history on `dev` matters.
7. **Post-merge:**
   ```bash
   git fetch origin
   git checkout dev
   git merge --ff-only origin/main
   git push origin dev
   ```
8. **Tag** if this cut is a named release (`v0.1.x`).

## Known footguns from the last cut

- **Parallel histories / add/add:** both branches grew the same paths independently. Resolving by “take ours everywhere” is only safe when `dev` is the product line of record.
- **Huge PR surface:** Desktop + Laguna + auth already mixed; keep release PRs as integration cuts, not drive-by refactors.
- **Do not** open feature work against `main` “to go faster” — it recreates the divergence.
- Renderer chip policy: model chip change must **not** kick to landing; compact only on send when pending ≠ bound model (`modelSwitchPlan.ts`).

## Ready-to-go definition

`dev` → `main` is ready when:

- [ ] `origin/dev` and `origin/main` share the same tree for the release SHA (or `main` is exactly the merge of that `dev` tip)
- [ ] No open conflicting PR targeting `main` with a second Desktop history
- [ ] Stashes triaged onto topic branches or discarded with a written note
- [ ] README branching policy still accurate
- [ ] Smoke: Desktop boots; model chip fiddle stays in chat; send on new model continues same thread

## Owner ask

Align leftover stashes onto `dev` via small PRs, keep `dev` tip newest daily, and only open the next `dev` → `main` PR when cutting the next release.
