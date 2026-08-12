# Handoff: CUA best-QA — post-refactor Desktop vs current prod

**Date:** 2026-08-11 (CUA READY refresh 2026-08-12)  
**Audience:** Independent CUA / Computer Use tester (must **not** be the refactor implementer)  
**Goal:** Exercise the **refactor candidate** against the **current production friends download**, side-by-side where possible, and file a receipt that says what still works, what regressed, and what is new/expected.

**Candidate tip (refactor):** `josh/v02-architecture-refactor` (or `dev` after merge) — **committed tip only**  
**Architecture handoff:** [`HANDOFF_V02_REFACTOR_FINISH_2026-08-11.md`](./HANDOFF_V02_REFACTOR_FINISH_2026-08-11.md)  
**Prod friends artifact:** [`apps/synth_desktop/PROVENANCE.md`](../apps/synth_desktop/PROVENANCE.md) — served ZIP SHA `d31776…`  
**First CUA receipt (FAIL):** [`/Users/joshuapurtell/Documents/Codex/2026-08-11/f/outputs/CUA_REFACTOR_VS_PROD_RECEIPT_2026-08-11.md`](/Users/joshuapurtell/Documents/Codex/2026-08-11/f/outputs/CUA_REFACTOR_VS_PROD_RECEIPT_2026-08-11.md)  
**Manual Gate P (37-item):** `/Users/joshuapurtell/Documents/GitHub/evals/workshop/manual/CUA_MANUAL_GATE.md`  
**Fuzz invariants:** [`apps/synth_desktop/CUA_FUZZ_INVARIANTS.md`](../apps/synth_desktop/CUA_FUZZ_INVARIANTS.md)  
**Isolation runbook:** [`apps/synth_desktop/HANDOFF_ISOLATED_DEV_INSTANCES.md`](../apps/synth_desktop/HANDOFF_ISOLATED_DEV_INSTANCES.md)

A screenshot alone is not a pass. Every FAIL needs steps, expected vs actual, which app (prod vs candidate), and evidence path.

---

## CUA READY

**First CUA pass: FAIL.** This tip claims the blockers below are fixed in-tree. That is **not** Gate P. An **independent** CUA rerun is still required before promote.

| Prior blocker | Claimed status on this tip |
| --- | --- |
| CMP-01 duplicate user bubbles | Fixed — `clientMessageId` ownership through `sendTurn` / `startTurn` |
| Migration dropped `usage_ledger` (prod can’t open shared DB) | Fixed — migration 12 recreates empty ledger + fold-on-open. **Still isolate data roots.** |
| Malformed analysis visual crash | Hardened — skip invalid ranked-bars blocks |
| `PROVENANCE.md` ≠ served ZIP | Docs reconciled — live digest `d31776…` |

### Install sequence (after WIP is committed)

`npm run desktop:install` → `scripts/desktop.sh install` calls `require_clean_worktree` and **refuses a dirty tree** (same rule in root `README.md`: “No artifacts from dirty trees”). Commit first; then install.

```bash
cd /Users/joshuapurtell/Documents/GitHub/workshop
git fetch origin
git checkout josh/v02-architecture-refactor   # or: git checkout dev && git pull
git status --porcelain                          # must be empty
git rev-parse HEAD                              # record this SHA in the receipt

# Preflight (recommended; not a CUA substitute)
./scripts/desktop.sh conform
npm run desktop:check
(cd apps/synth_desktop/src-tauri && cargo test --lib)
NODE_PATH="$(pwd)/node_modules" node --test apps/synth_desktop/tests/*.test.mjs
(cd apps/synth_desktop && npx playwright test)

# Candidate → /Applications/Synth Desktop.app (default)
npm run desktop:install
npm run desktop:status
```

**Prod (friends ZIP)** — do **not** build from this tree:

```bash
open "https://www.usesynth.ai/download"
# Expect ZIP SHA-256 d317760fe414798c9c29ce3bb0db599beed25489f6a35f53650ac4d4ecac01a5
# Install to: /Applications/Synth Desktop PROD.app
```

Optional candidate install path (keeps another build at the canonical name):

```bash
SYNTH_DESKTOP_APP_PATH="/Applications/Synth Desktop CANDIDATE.app" npm run desktop:install
```

### Mandatory: isolated data roots

Sharing the canonical profile after candidate migration makes prod fail (`usage_ledger`). **Never** let prod and candidate open the same DB.

| Build | App path | Data root |
| --- | --- | --- |
| **Prod** | `/Applications/Synth Desktop PROD.app` | Default canonical: `~/.synth-desktop` + `~/Library/Application Support/Synth Desktop` |
| **Candidate (preferred)** | named instance via launcher | `~/.synth-desktop/instances/v02/<name>/data` |

**Option A — named instance for candidate** (isolates SQLite, Codex homes, config; sets env for you). See [`HANDOFF_ISOLATED_DEV_INSTANCES.md`](../apps/synth_desktop/HANDOFF_ISOLATED_DEV_INSTANCES.md) / `scripts/desktop-instance.sh`:

```bash
# Debug .app with LaunchServices identity (better for CUA than bare tauri dev)
./scripts/desktop-instance.sh cua candidate
# data → ~/.synth-desktop/instances/v02/candidate/data
# status → npm run desktop:instance:status -- candidate
```

Hot-reload equivalent (not an install artifact): `npm run desktop:dev -- candidate`.

Env the launcher sets (do not invent others): `SYNTH_DESKTOP_INSTANCE`, `SYNTH_DESKTOP_DATA_ROOT`, `SYNTH_DESKTOP_CONFIG`, `SYNTH_CODEX_HOME`, `SYNTH_DESKTOP_WORKSPACE`, `SYNTH_DESKTOP_APP_NAME`, `SYNTH_DESKTOP_INSTANCE_MANIFEST`. Named instances also get a per-name `SYNTH_LAGUNA_PORT`.

**Option B — two installed `.app`s with explicit roots.** Double-click / bare `open -a` ignores isolation. Launch candidate with a private root:

```bash
mkdir -p "$HOME/.synth-desktop/cua-candidate/data"
SYNTH_DESKTOP_DATA_ROOT="$HOME/.synth-desktop/cua-candidate/data" \
SYNTH_DESKTOP_CONFIG="$HOME/.synth-desktop/cua-candidate/data/config.toml" \
SYNTH_CODEX_HOME="$HOME/.synth-desktop/cua-candidate/data/codex" \
  open -n "/Applications/Synth Desktop.app"
# Leave prod on the default canonical roots (or give it its own SYNTH_DESKTOP_DATA_ROOT).
```

**Migration order:** if you ever open an old/migrated DB with the candidate, open **candidate once** before expecting prod to open that same profile (migration 12 repair). Prefer never sharing.

### Retest focus (this tip)

1. **CMP-01** — one user bubble per submit (duplicate gone).  
2. Open **candidate once** on any old migrated DB **before** prod opens the same profile.  
3. Analysis visual with **bad ranked-bars** — no crash; invalid blocks skipped.  
4. Then Tier A–C in §4; Gate P only if this receipt is the publish gate.

### Targeting

Never pick a window only by title “Synth Desktop”. Prefer exact paths / `desktop:status` / `desktop:instance:status -- <name>` / Settings → Runtime identity.

---

## 0. One-liner

> Install **two** Synth Desktops: **Prod** = current public friends ZIP; **Candidate** = clean install from the refactor tip. Run the research-engineering loop and account/billing surfaces on both. Diff behavior. Prefer the candidate for deep CUA; use prod as the honesty baseline (“did we break something users already have?”).

**Hard rule:** give each build its **own data root** (see **CUA READY**). Sharing the canonical profile after candidate schema migration makes prod fail to start.

---

## 1. Bring up the two apps (do this first)

Follow **CUA READY** for install + isolation. Short form:

### A — Prod

Download from `https://www.usesynth.ai/download` (or the public ZIP path in `PROVENANCE.md`). Record the ZIP SHA. Install to `/Applications/Synth Desktop PROD.app`. Leave it on the **default** canonical data roots unless you also isolate prod.

| Check | Currently served (2026-08-12) |
| --- | --- |
| ZIP SHA-256 | `d317760fe414798c9c29ce3bb0db599beed25489f6a35f53650ac4d4ecac01a5` |
| Public path | `v0.1.0` |
| Bundle | `0.1.0` |
| Signing | ad-hoc → **Open Anyway** |

### B — Candidate

Committed tip on `josh/v02-architecture-refactor` (or `dev`) → preflight → `npm run desktop:install` **or** `./scripts/desktop-instance.sh cua candidate`. Confirm with `npm run desktop:status` / `desktop:instance:status`.

### Shared services

| Need | Notes |
| --- | --- |
| Local Laguna | Required for local-model dogfood; named instances get per-name ports. Don’t fight one shared `:7333` with two apps unless intentional. |
| Craftax Rust | `:8098` — see [`HANDOFF_CONTAINERS_CRAFTAX.md`](../apps/synth_desktop/HANDOFF_CONTAINERS_CRAFTAX.md) |
| Network | Prod Clerk + `api.usesynth.ai` for cloud pairing / Synth Cloud models |
| Accounts | Prefer **two** throwaway accounts so device pairing doesn’t thrash |

---

## 2. What changed in the refactor (stress these)

Candidate tip includes structural work that can break UX without failing unit tests:

| Change | What to watch in CUA |
| --- | --- |
| **Inventory → Data** | Sidebar says **Data**; page H1 **Data**; testids may still be `open-inventory` / `inventory-*` |
| **Session store / thin App** | Streaming tokens, Stop/running state, multi-chat switching, no stuck “Working” |
| **Single `runtime:event` channel** | Approvals, unhealthy session, tool activity, transcript updates still appear once (no double toasts / missing events) |
| **RuntimeTarget / SessionKind** | Model picker targets (local Laguna, OpenRouter, Synth Cloud Laguna S, **hosted Muse**); no wrong-routing to Intern UI |
| **Credential broker injection** | Synth Cloud turns still work; no cleartext key in shell snapshots |
| **Composer ≤10 prop groups** | Model / effort / permissions / queue / slash still behave |
| **No `window.synth` in UI** | Settings, Visuals, Data, Terminal, Account still call into the host |

Intern chrome remains **dormant** on both — do not file “Intern missing” as a regression.

---

## 3. Comparison protocol

For each scenario below, run **Candidate first**, then **Prod** (or side-by-side if two machines/displays). Fill:

```text
ID:
Surface:
Prod result: PASS | FAIL | N/A | BLOCKED
Candidate result: PASS | FAIL | N/A | BLOCKED
Delta: same | candidate better | candidate worse | prod-only bug | candidate-only bug
Evidence: screenshot/video paths + timestamps
Notes:
```

Severity:

- **P0** — blocks friends/prod dogfood (auth, spend, data loss, crash, secrets)
- **P1** — broken primary loop (agent/turn/visual/container/trace)
- **P2** — polish / secondary surface

---

## 4. Scenario matrix (best QA, not the full 37 unless Gate P)

### Tier A — must compare (research loop)

| ID | Scenario | Prod | Candidate |
| --- | --- | --- | --- |
| CMP-01 | Cold launch → landing → start local Laguna chat → stream → Stop | ✓ | ✓ |
| CMP-02 | Mid-turn Stop; start another turn; no zombie Working | ✓ | ✓ |
| CMP-03 | Switch models mid-session (e.g. Laguna → Muse / OR); compact/rebind honesty | ✓ | ✓ |
| CMP-04 | Synth Cloud model turn after device pair (lease path; no key in UI) | ✓ | ✓ |
| CMP-05 | Approval / permission policy change in composer; persists | ✓ | ✓ |
| CMP-06 | Long prompt + composer/terminal open; active turn not under dock | ✓ | ✓ |
| CMP-07 | Data (Inventory) → Attach Craftax `:8098` → hydrate → open in pane | ✓ | ✓ |
| CMP-08 | Seal / import Trace V5 → Open visual → inspector usable | ✓ | ✓ |
| CMP-09 | Visuals library list / open / draft create | ✓ | ✓ |
| CMP-10 | Outputs / side panel / inference rail toggles; no stuck overlay | ✓ | ✓ |

### Tier B — account & money honesty

| ID | Scenario |
| --- | --- |
| CMP-11 | First-run: Local use vs Sign in equal weight |
| CMP-12 | Browser device pair → account flips; cancel mid-pair |
| CMP-13 | Account menu: plan / allowance; **UNKNOWN** not `$0.00` when missing |
| CMP-14 | Exhausted allowance: cloud blocked, local still works, Upgrade path |
| CMP-15 | Settings → Account facts before Advanced connection |

### Tier C — regression fuzz (CUA_FUZZ_INVARIANTS)

At **960×640, 1024×700, 1280×840, 1440×900** on **candidate** (spot-check prod if candidate fails):

- Model picker clears terminal / stays in viewport  
- Search dialog scrolls last result  
- Sidebar compact history / show all  
- Inference rail never shows impossible tok/s  

### Tier D — optional depth (timebox)

- Whisper voice mic → insert transcription  
- Optimizers page smoke (no crash)  
- Workspace scope chip / folder fence  
- Kill codex app-server child during a turn → session recovers / shows unhealthy honestly  

Full Gate P list: `CUA_MANUAL_GATE.md` (37 items) — only required if this receipt is meant for a publish gate; for refactor QA, Tier A–C is enough if evidence is solid.

---

## 5. Automated preflight (optional but recommended)

See **CUA READY** for the exact command list (conform, `desktop:check`, `cargo test --lib`, node tests, Playwright). Not a substitute for CUA. Prod ZIP will not run these.

---

## 6. Receipt template (return this)

```markdown
# CUA receipt — refactor vs prod

- Tester:
- Date (local + UTC):
- Candidate SHA:
- Candidate install path + CFBundle version:
- Prod ZIP SHA-256:
- Prod install path + CFBundle version:
- Machine: macOS version / chip / display scale
- Laguna: yes/no · Craftax: yes/no · Accounts used:
- Data roots (prod vs candidate):

## Summary
- Candidate overall: PASS / FAIL
- Regressions vs prod (P0/P1 list):
- Candidate-only improvements:
- Prod-only bugs still present:

## Matrix
(paste CMP-xx rows)

## Evidence index
| ID | Path | Notes |
| --- | --- | --- |

## Blockers for merging refactor to friends/prod
-
```

Attach videos/screenshots under a dated folder (e.g. `~/Desktop/cua-refactor-vs-prod-2026-08-12/`). Do not put API keys or `.env` contents in the receipt.

---

## 7. Known non-issues (do not FAIL these)

- Sidebar/product noun **Data** instead of Inventory on candidate  
- Intern / CloudDesk absent  
- Ad-hoc signing / Open Anyway on both friends builds  
- Public download still labeled **v0.1.0** while inner bundle may read **0.2.0**  
- Playwright testids still named `inventory-*` while UI says Data  

---

## 8. If blocked

| Blocker | What to do |
| --- | --- |
| Can’t tell which window is which | Rename prod `.app`; confirm paths via `desktop:status` / instance status |
| Port fight on Laguna | Named instance (per-name port) or set `SYNTH_LAGUNA_PORT` |
| Pairing thrash | Separate accounts / separate data roots |
| Candidate won’t install (dirty tree) | Commit or stash; `desktop:install` refuses dirty trees |
| Need craftax | Follow `HANDOFF_CONTAINERS_CRAFTAX.md` before CMP-07 |

---

## 9. Done when

1. Tier A + B completed on **both** apps with evidence.  
2. Tier C fuzz run on **candidate**.  
3. Receipt filled; P0/P1 list empty or filed with owners.  
4. Explicit statement: “Candidate is / is not worse than prod on the research loop.”  
5. Independent tester (not the implementer) — Gate P still open until that receipt lands.
