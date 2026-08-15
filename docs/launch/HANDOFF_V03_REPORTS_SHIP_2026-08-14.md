# Workshop v0.3 Reports — ship handoff

Date: 2026-08-14
Audience: engineer closing **in-workshop / private hosted / public hosted** so Reports can ship.
Not this cut: Intern, E5, VisualsBench, Golden Reports as a separate product, Craftax eval data collection.

The older proofs doc `HANDOFF_V03_REPORTS_GOLDEN_REPORTS_2026-08-14.md` is **stale**. It said the Report aggregate did not exist. It does. Use this file.

Product noun: **Report**. ArtifactBundle / publication / collection are storage. Do not compete with Report in UI copy.

Target experience: a smaller, generalizable version of `https://www.usesynth.ai/evals/craftax` — narrative + exact results + interactive visuals + Trace V5 — opening the **same sealed revision** locally, via a private authenticated URL, or via an approved public route.

---

## Worktrees (do not mix)

| Surface | Path | Branch | Git state as of this handoff |
|---|---|---|---|
| Workshop Desktop | `/Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-reports` | `agent/v03-reports-complete` | **Dirty.** Reports crate is untracked. Origin already has visual-level seal/share (`5fbcd61`, `ade6dd2`). Remote: `https://github.com/synth-laboratories/workshop.git` |
| Backend Artifact Platform | `/Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/backend-v03-artifacts` | `agent/v03-artifact-platform` | **Dirty.** Report bundle + workshop report routes untracked. Origin already has ArtifactBundle v1 (`c07e4264c`, `32eb56415`, `78bdb0ebd`). Ahead of `origin/dev` by 3. |
| VisualsBench | `.../evals-v03-visualsbench` | `agent/v03-visualsbench` | Out of scope for this ship. |
| Proofs E2–E4 | `.../im/work/proofs` | `agent/v03-proofs-e2-e4` | Out of scope. Do not fold into the Reports PR. |

Canonical plan (still useful for nouns): `im/work/proofs/docs/PLAN_V03_REPORTS_PROOFS_INTEGRATION_2026-08-14.md`

Ship as **two PRs** (Workshop, Backend), then a **third** for public promote once private is proven on staging. Do not `git add -A`. Do not commit Craftax eval outputs, `.out/`, or standalone HTML from the eval worktree.

---

## Product modes

| Mode | Who | Behavior |
|---|---|---|
| **In-workshop** | Agent MCP + human UI | Draft, attach evidence, seal, offline reopen, compare. No hosted identity. |
| **Private hosted** | Human Share only | Exact sealed digest → one Report URL. Auth required. Partial upload = no URL. |
| **Public hosted** | Human promote only | Server policy promotes a **already-private** committed revision onto a stable public route. Client cannot flip visibility. |

Agent MCP **must not** advertise share, upload, or promote. Test already asserts this in `synth_visuals_mcp.rs`.

---

## Locked invariants (do not reopen)

- Missing evidence renders **`—`**, never `0`, never an empty chart that looks like a score.
- Sealed revisions are immutable. A later edit creates a **new draft revision**. Historical seals stay sealed.
- Frozen `index.html` CSP: `connect-src 'none'`. No `fetch(`, `XMLHttpRequest`, `EventSource`, `WebSocket`, `import(`. React cannot ship in the frozen reader; vanilla JS only.
- Live stream URLs / `s3://` / `gs://` / credentials are refused at seal.
- Report URL is `/reports/v1/publications/{id}`, **not** `…/index.html`.
- Failed/partial upload stays `state=failed` with `committed_url IS NULL` (SQLite CHECK).
- Overlay review comments do not change the receipt digest.
- `report.result.v1` is a known evidence kind. Live seal of a result block needs `access_state: accessible` + digest, or standalone HTML inlines the payload.
- Do not substitute ASCII for a missing PNG, or unlabeled pixels for a text-only model observation.
- Do not crown a winner on overlapping CIs (Craftax dogfood, not the rail).

---

## Track A — In-workshop (close this first)

### Already implemented (uncommitted)

Crate: `apps/synth_desktop/src-tauri/src/reports/`

| File | Role |
|---|---|
| `models.rs` | `synth.desktop-report.v1` / `synth.report-revision.v1` / `synth.report-bundle.v1`. Block kinds including `report.trace-v5.v1`, `report.result.v1`, experiment records, research log. |
| `registry.rs` | Create / update / seal / reopen / compare / attach Trace V5. Frozen runtime concat. |
| `hosting.rs` | Private share/open + overlay comments (Track B). |
| `reader.js` + `reader.css` | Frozen Report reader. |
| `rollout_inspector.js` + `.css` | `window.SynthRolloutInspector.mount` / `extractProjection`. |
| `compare_story.js` + `.css` | `window.SynthCompareStory.mount` for `craftax.compare-story.v1`. |
| `ReportsPage.tsx` | Research → Reports. Draft, seal, share, reopen, compare, attach trace, experiments, log, comments. |

Wiring (modified, not new files):

- SQLite **migration 15** (reports/revisions/blocks/sources/claims/limitations/experiments/log/seals) and **16** (uploads + review comments).
- CAS namespace `report_bundles` in `content_store.rs`.
- Tauri commands `reports_*` in `lib.rs`, specta, `protocol.ts`, `desktopBridge.ts` (`window.synthReports`).
- MCP in `synth_visuals_mcp.rs` → visuals IPC `/v1/reports…`.
- Sidebar / routes / `App.tsx`.
- Canonical live inspector: `visuals/templates/trace.rollout_inspector.v1/shell.tsx` (React, Workshop only).
- Visual frozen runtime helper: `apps/synth_desktop/src-tauri/src/visuals/frozen_runtime.js` (visual seals, not Reports).

Agent MCP tools (local only): `report_list`, `report_get`, `report_get_revision`, `report_create`, `report_update`, `report_attach_trace`, `report_seal`, `report_list_seals`, `report_get_seal`, `report_upsert_experiment`, `report_append_log`.

`report_attach_trace`: Desktop resolves the local rollout-inspector projection when `projection` is omitted. Frozen reader renders the canonical inspector. Missing projections stay `—`.

### Remaining close-out

1. **Commit the dirty tree as one Workshop PR.** Include `apps/synth_desktop/src-tauri/src/reports/`, `ReportsPage.tsx`, and the listed modified wiring. Keep `generated/protocol.ts` in lockstep with specta. Do not include eval HTML.
2. **Live UI vs frozen reader parity.** Frozen `reader.js` mounts Trace V5 + `craftax.compare-story.v1`. Live `ReportsPage` uses React `TraceInspector` (correct) but dumps other blocks as `<pre>` JSON. Mount compare-story / visual identity cards in the live page, or at least render `report.result.v1` instead of raw JSON.
3. **Embedded visuals/diagrams.** Seal resolves `report.visual.v1` / `report.diagram.v1` against local visual seals (`source_digest` = visual receipt). Frozen reader currently shows an identity card, not the visual’s sealed `index.html`. Close-out: iframe-or-inline the sealed visual HTML **without** giving the Report reader network (`connect-src` still none; inlined bytes only).
4. **Trace picker.** Frozen inspector supports optgroups (OSS-20B / OSS-120B / Nemotron). Live `<select>` is a flat list. Match frozen grouping if multiple traces are attached.
5. **Human smoke in Desktop.** Create draft → attach a real local Trace V5 → seal → quit/reopen → Open report rev N → Compare. Missing blocks stay `—`. Sealed fields disabled.
6. **Do not add MCP share/upload/promote** while closing the rest.

### Verify

```bash
cd /Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-reports
cargo test -p synth-desktop --lib reports::
cargo test -p synth-desktop --lib storage::migrations
# MCP contract: no share/upload/promote in advertised tools
cargo test -p synth-desktop --bin synth_visuals_mcp -- server_exposes_the_compact_facade_without_removing_legacy_tools
```

Known tests:

- `seal_freezes_heterogeneous_blocks_and_reopens_offline`
- `attach_trace_seals_the_rollout_inspector_instead_of_json`
- `research_log_is_append_only_and_corrections_link_history`
- `seal_refuses_live_stream_urls`
- `attach_trace_accepts_snake_case_mcp_fields`
- plus hosting tests under Track B

---

## Track B — Private hosted (close after A compiles/tests)

Workshop talks to backend:

```
POST {backend}/artifacts/v1/workshop/reports/prepare
PUT  {upload_url}                    # raw bytes for index.html, data.json, receipt.json
POST {backend}/artifacts/v1/workshop/reports/{publication_id}/finalize
GET  {backend}/reports/v1/publications/{publication_id}
     Accept: application/json  → publication metadata + asset_root
     Accept: text/html         → sealed index.html
```

Schema: `synth.workshop-report-upload.v1`. Bundle schema: `synth.report-bundle.v1`.

### Already implemented (uncommitted on both repos)

**Workshop** (`reports/hosting.rs` + `ReportsPage` Share / Open private Report URL):

- Human `reports_share` requires a signed-in Synth account (`api_key` from `synth_config`).
- Idempotent if `state == committed`.
- Local CAS digest re-check before upload.
- Open refuses a URL that is not the configured backend origin, and refuses a direct `index.html` asset.
- Overlay comments: `report_review_comments`; digest unchanged.

**Backend** (untracked):

- `packages/artifacts/report_bundle.py` — Report bundle ≠ ArtifactBundle visual identity.
- `packages/artifacts/workshop_report.py` + `services/artifact_platform/workshop_report.py`
- `app/api/v1/workshop_reports.py` — prepare / finalize / GET publication.
- Mounted in `smr_lean.py` and `app.py`.
- SMR `packages/smr/contracts/public_api/v1/reports.py` accepts ordered heterogeneous blocks (not one visual).
- Tests: `tests/units/test_workshop_report_upload.py`, `tests/units/test_report_revision_contract.py`.

**Local-slot loopback already proves** (Workshop, `127.0.0.1`, no staging):

- `private_share_against_a_local_slot_creates_a_report_url_not_index_html`
- `failed_local_slot_put_leaves_no_report_url`
- `open_shared_refuses_a_direct_index_html_asset`
- `review_comments_overlay_a_seal_without_changing_its_digest`

HTML GET without an API key is **local/dev only** (`APP_ENVIRONMENT` in `{local,dev}` uses `sk_dev_…`). Production private HTML still needs a real auth story (cookie/session or Workshop-only reopen). A raw browser tab against staging will 401. That is expected until you add authenticated HTML delivery.

### Remaining close-out

1. **Commit/PR the backend dirty files** listed in git status. Keep ArtifactBundle v1 unchanged; Reports are a parallel bundle type.
2. **Deploy or point Desktop at a staging Artifact Platform** that has these routes. Local-slot is not staging.
3. **Human path on staging:** sign in → Seal report → Share report → copy `/reports/v1/publications/{id}` → Open report in another signed-in Workshop. Same receipt digest. Failed PUT → no URL, `state=failed`.
4. **Private HTML in non-local env.** Do not silently use `sk_dev`. Options: Workshop `openShared` (already works with bearer) is the v0.3 bar; browser-tab private HTML can wait if documented. If you add cookie/session, keep assets authenticated and do not make the publication public.
5. **Do not store live stream URLs in the hosted bundle.** The sealed HTML is already self-contained (`connect-src 'none'`). Hosted traces for v0.3 are the inlined projection in `data.json`, not a cloud Trace V5 fetch from the frozen reader.

### Verify

```bash
cd /Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/backend-v03-artifacts
# existing unit tests for the new modules
python -m pytest tests/units/test_workshop_report_upload.py tests/units/test_report_revision_contract.py tests/units/test_smr_lean_routes.py -q

cd /Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-reports
cargo test -p synth-desktop --lib reports::hosting
```

---

## Track C — Public hosted (not started; this is the remaining product gap)

Private share is **not** public. There is no promote command, no `/evals/{slug}` route, and no disclosure policy.

Mirror the visual Artifact Platform rule: **server owns visibility**. Workshop may request promote; the API decides.

### Spec to implement

1. **Human-only** Desktop action: “Publish report” on a seal that already has `upload.state == committed`. Disabled without a committed private URL. **Never** on MCP.
2. **Backend** `POST /artifacts/v1/workshop/reports/{publication_id}/promote` (name bikesheddable) that:
   - requires the publication to be committed and owned by the org;
   - runs disclosure policy (strip credentials, refuse live locators — already sealed, re-check);
   - creates an immutable public route, e.g. `/evals/{slug}` or `/reports/public/{publication_id}`;
   - does not rewrite `index.html` / `data.json` / `receipt.json` bytes (same digests);
   - fails closed if policy rejects.
3. **Public GET** serves the same sealed `index.html` with `connect-src 'none'`. No authenticated APIs from that page. If traces or visuals are missing under public policy, the reader already shows `—`.
4. **usesynth.ai** mapping is routing + CDN in front of that public GET. Do not build a second Craftax-only renderer. The Craftax page is the *quality bar*, not the template.
5. **Slug / unpublish.** Promoting assigns a stable public identity. Unpublish may hide the route; it must not mutate the sealed revision.

### Explicitly out of scope for v0.3 public

- Factory SDK import/emit of Reports.
- Hosted Trace V5 collections fetched at read time from the frozen page.
- Agent-initiated promote.
- Treating a private Report URL as public because someone leaked it.

### Suggested first public proof

Seal a tiny Report (prose + one attached Trace V5 + one result block) privately on staging, promote, open the public URL in a logged-out browser, confirm inspector + `—` for missing Nemotron-style cells, confirm no network from the page.

---

## Dogfood (parallel, not a ship gate)

A Craftax OSS-20B/120B × effort contrast is running separately to fill a real Report. Do **not** block the rails PR on it. Do **not** kill gold `:8612` or the eval process if you are on this machine; it is unrelated close-out.

When that eval finishes, the contrast payload goes in `report.result.v1` with `schema_version: craftax.compare-story.v1`. Empty effort cells stay `—`. Env PNGs are reconstructed frames, captioned as env render not model input (OSS on OpenRouter is text-only).

---

## Suggested close-out order

1. Workshop PR: Track A files + tests green.
2. Backend PR: Track B files + pytest green.
3. Staging private share round-trip (two signed-in Desktops or one Desktop + `openShared`).
4. Public promote PR (Track C) on top of those.
5. Optional: live UI parity for compare-story and inlined visual HTML.

Stop when: a researcher can author a Report in Workshop, seal it, share it privately, reopen the same digest, and (after C) publish it to a stable public URL that still honors missing=`—` and `connect-src 'none'`.
