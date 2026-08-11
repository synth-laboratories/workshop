# Handoff: one release trunk (`main`), keep `dev` synced

**Audience:** whoever next aligns local WIP and cuts a Desktop release  
**Date:** 2026-08-11  
**Policy owner:** SYN-3196 (H4 Workshop release trunk hygiene)

## Policy

- **`origin/main` is the release source of truth.** Cut friends ZIPs / notarized / installed acceptance builds only from a **clean** tree at that tip (or a tag on it).
- **Work on `dev`.** Feature PRs target `dev`.
- **Release via `main`.** Merge `dev` → `main` only when cutting a release (merge commit preferred so the release boundary stays visible).
- After each release merge, **fast-forward `dev` to `main`** so `dev` is never a second, lagging trunk.
- **Never publish artifacts from a dirty worktree.** Lifecycle scripts refuse dirty trees for `build` / `verify` / `install` / `install-release`. Dev instances may append `-dirty` to the revision label; that is not a release seal.

## Snapshot (2026-08-11 audit)

| Ref | Tip (at audit) | Note |
| --- | --- | --- |
| `origin/main` | `0e8af0a` | Includes v0.1 release merges (#3, #4) and macOS 14 release fix |
| `origin/dev` | `a94fda7` | **Ancestor of `origin/main`** — 93 commits behind; no unique commits |
| Merge-base | `a94fda7` | Same as `origin/dev` → sync is a pure fast-forward |

`origin/dev` is not divergent in content; it was simply not fast-forwarded after the v0.1 cut. Sync it; do not rebase released `main` history.

Local checkouts may still hold **unpushed** Muse / account WIP on a private `dev` tip ahead of `origin/dev`. That WIP is **not** on either remote trunk — triage onto topic branches off the synced tip, or discard. Do not push a divergent local `dev` over the FF sync without an explicit review.

## Immediate sync (retire the lag)

Because `origin/dev` is a strict ancestor of `origin/main`:

```bash
git fetch origin
git checkout dev
# Park or commit any local dirty files first — do not mix WIP into the FF.
git status --porcelain   # must be empty on the branch tip you will push
git merge --ff-only origin/main
git push origin dev
```

Verify alignment:

```bash
git fetch origin
git rev-list --count origin/main..origin/dev   # expect 0
git rev-list --count origin/dev..origin/main   # expect 0 after FF
git merge-base --is-ancestor origin/dev origin/main && echo ok
```

If someone already pushed unique commits onto `origin/dev` that are not ancestors of `main`, stop: open an integration PR into the synced tip instead of force-pushing.

### Optional: retire long-lived `dev` later

If the team decides Workshop should be **`main`-only** (like some other repos), the retirement path is:

1. FF-sync `dev` to `main` one last time (commands above).
2. Update README branching table to drop `dev` as integration.
3. Protect `main`; open feature PRs against `main` (or short-lived release branches).
4. Leave `origin/dev` as a historical pointer or delete after a grace period — do not keep merging into a zombie `dev`.

Until that decision is explicit, keep the dual-branch policy above with **mandatory post-release FF**.

## Dirty worktrees (do not cut from these)

Before any friends ZIP / notarized / `desktop:install:release` cut:

```bash
git fetch origin
git checkout main
git merge --ff-only origin/main
git status --porcelain   # must be empty
git rev-parse HEAD       # record as the artifact source SHA
```

Named Codex/Claude worktrees with local edits are fine for iteration. They are **forbidden** as the packaging root. If `./scripts/desktop.sh build|verify|install|install-release` reports a dirty tree, stop and move to a clean checkout of `origin/main`.

## Careful `dev` → `main` release checklist (next cut)

1. Confirm `origin/dev` and `origin/main` already match (post-sync), or freeze the `dev` tip you intend to release and note the SHA.
2. CI green / local: `npm run desktop:verify` on a **clean** tree.
3. Diff the release:
   ```bash
   git fetch origin
   git log --oneline origin/main..origin/dev
   git diff --stat origin/main...origin/dev
   ```
4. Open PR `dev` → `main` with release notes (what ships, what is out).
5. On conflicts: merge `main` into `dev` first on an integration branch; never force-push `main`.
6. Merge with a normal merge commit (avoid squash for the integration PR if `dev` history matters).
7. Post-merge FF:
   ```bash
   git fetch origin
   git checkout dev
   git merge --ff-only origin/main
   git push origin dev
   ```
8. Tag if this cut is a named release (`v0.1.x`). Bind published ZIP provenance to source SHA + executable digest + backend/gateway SHAs.

## Known footguns

- **Skipped post-release FF** left `origin/dev` 93 commits behind `origin/main` after v0.1 — the exact failure mode this handoff prevents.
- **Parallel histories / add/add:** both branches grew the same paths independently in earlier cuts. Prefer one product line of record (`dev` until merge, then `main`).
- **Do not** open feature work against `main` “to go faster” — it recreates divergence.
- **Dirty local `dev` ahead of `origin/dev`:** park Muse/account WIP on topic branches before pushing the FF sync.

## Ready-to-go definition

- [ ] `origin/dev` and `origin/main` share the same tip (or `main` is exactly the merge of that `dev` tip, then FF’d)
- [ ] No open conflicting PR targeting `main` with a second Desktop history
- [ ] Local stashes / dirty worktrees triaged onto topic branches or discarded with a written note
- [ ] README branching policy still accurate
- [ ] Artifact builds only from clean trees at the release tip

## Owner ask

Fast-forward `origin/dev` to `origin/main` now, keep tips matched after every future cut, and refuse dirty-tree Desktop artifacts.
