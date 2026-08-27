# Handoff: visual admission + remaining visual work

**For:** Guy — test the attach/pin/seal admission cut, then continue visual-oriented leftover.  
**Date:** 2026-08-26  
**Do not commit unless asked.**

## 2026-08-26 signed-CUA result

The dynamic visual host is proved end to end in the signed debug app, using a
real local Craftax producer rather than an inline fixture:

| Identity | Value |
| --- | --- |
| Bundle | `com.synth.desktop.v08.dev.admission-cua` |
| App snapshot | `cua/visual-admission-dogfood` at `d908fd59` |
| Run | `opt_eval_c16be0cd0f85` |
| Visual | `vis_950d36ad0009423f8c26e5c2a388a60b` |
| Template | `optimizer.eval.live.v1` |
| Candidate set | `policy_set_438ee1d0aee244afa97c7f337a48dafc` |
| Local image | `craftax-eval-target@sha256:ef981d0d88e968dc470358607804d2a6abe218e760b5711a3dee3ea37fdb0195` |

Computer Use observed the same open right-pane visual remain `SUBSCRIBED` and
advance without reopen or rebind:

```text
5/20 trials, 5 valid, 2 running, raw event counter 40
13/20 trials, 13 valid, raw event counter 86
20/20 trials, 20 valid, 0 failed, TERMINAL / COMPLETED
```

The terminal manifest records `work.succeeded=20`, `work.failed=0`,
`rollouts=20`, `$0.00`, and visual subscription receipt
`synth.visual-subscription-receipt.v1` for that exact run and visual. The final
pane showed both scored policy rows (10 valid trials each), with no winner
because this recipe is deliberately report-only.

This is a local CUA proof, not a release benchmark receipt. The local image was
built from a dirty GameBench checkout and the image-only report-mode outer
container boundary is weaker than promotion-grade per-policy OS isolation.
Do not publish or cite the scores as portable benchmark results. No GHCR or
provider credentials were used.

Noun map (keep current): [`docs/qa/v08-visuals-data-model.md`](./qa/v08-visuals-data-model.md)  
CUA findings this cut targets: RP-CUA-014 / 053 / 060 (handoff chrome only).  
Sourced/compose CUA is already passed: [`HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md`](./HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md). Do not rebuild it.

Durable authority is the **local store**. Do not introduce `CoreRuntime` as a product noun. Specta command count is **264** after the landed `experiments_relate` command. Craftax is rust GameBench gold only (`env:craftax_gold`). Do not use intern / research-intern MCP.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/GitHub/workshop-v08-release` | `codex/v08-release-integration` | **Dirty.** Admission sits on top of Candidate + compose `optimizer_run` slot + shared `VisualPane` + `list_templates` components + Laguna-vs-Plugin pin. Do not commit this pile unless asked. |
| `/Users/joshuapurtell/GitHub/workshop-v08-admission-cua` | `cua/visual-admission-dogfood` | Signed CUA snapshot at `d908fd59`; this handoff update is intentionally uncommitted. |

Leave `containers`, `optimizers`, `optimizers-beta`, `synth-mlx-rl` alone unless a producer field is missing.

File work only under `/Users/joshuapurtell/GitHub`. Do not write under `Documents`.

---

## What landed (do not rebuild)

One Rust predicate for visual/diagram attach → pin → preflight → report seal. **No** `ArtifactRevision` sqlite class, **no** migration 40, **no** new MCP verb.

```
VisualRecord vis_ + revision
        │
        ▼
admit_visual_evidence(visual_id, revision, seals)
  ok iff VisualSeal exists for that exact vis_ + rev
  finding code: unresolved_visual_evidence
        │
   ┌────┴────┬──────────┬──────────┐
 attach    pin_all   validate   reports.seal
 live ok   needs ok  sealable   follows sealable
```

Two digest spaces stay labeled, never merged:

- `contentDigest` — authoring CAS, often null on blank canvas
- `receiptDigest` — VisualSeal receipt. Pin and report `sourceDigest` use this.

The unused `decideVisualEvidence` (`ready|reviewed|partial|failed`) parallel
model was removed during closeout. Reports use receipt-true admission only.

Pin is `reports_pin_all` / “Pin all evidence”, not `reports_promote`. Keep promote off agent visuals MCP.

The gate is **only** `report.visual.v1` / `report.diagram.v1`. Do not use `is_evidence_kind()` as the seal gate — that set includes experiment-records / research-log appendix and would make empty reports unsealable.

### Behavior you should see

| Surface | Blank `blank.canvas.v1` (no VisualSeal) | Same visual after pane Seal + pin |
| --- | --- | --- |
| Attach | succeeds as **live pointer** | still live until pin; copies `receiptDigest` into `sourceDigest` if a seal already exists |
| Preflight / “Ready to seal” | **not** ready (`unresolved_visual_evidence`) | sealable after pin (or after resolve finds the receipt) |
| Pin all | fails with the **same code** | pins to receipt digest |
| Report seal | blocked because `!sealable` | succeeds |
| Copy | **Live pointer** · `vis_…` · rev N · `digest —` | iframe / pinned+verified. Never “Frozen evidence attached to this revision” for a live unresolved block |

Identity chrome (title stays the human label):

- Visuals **Add to report** — `data-testid="visual-add-to-report-identity"`
- Pane header next to Seal — `data-testid="visual-pane-identity"`
- Reports Pin/Seal strip — `data-testid="reports-pin-seal-identity"`
- Reports block meta + live-pointer card — `data-testid="reports-visual-pointer"`
- Chat VisualCard — truncated `vis_` when the card is the open control

Claims picker labels an unresolved visual block `unresolved — not sealable` (cheap; claims still *can* point at it).

### Code

| Piece | Path |
| --- | --- |
| Predicate + `validate` / `pin_all` | `apps/synth_desktop/src-tauri/src/reports/registry.rs` (`admit_visual_evidence`, `UNRESOLVED_VISUAL_EVIDENCE`) |
| Finding fields | `apps/synth_desktop/src-tauri/src/reports/models.rs` (`visual_id`, `receipt_digest` on existing `ReportValidationFinding`) |
| Protocol | `apps/synth_desktop/src/renderer/src/generated/protocol.ts` — admission was regen-only; current tree is **264** after `experiments_relate` |
| Identity helper | `formatVisualAdmissionIdentity` in `apps/synth_desktop/src/renderer/src/types/landing.ts` |
| Attach | `VisualsPage.tsx` — full `visual-${id}` anchor, live + unresolved, copy receipt when present |
| Frozen fallback | `ReportsPage.tsx` + `reports/reader.js` |

---

## How to test (this is the job)

Use a **named instance** so you do not pollute prod data. Dev is enough; do not package unless you are doing CUA.

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-release
./scripts/desktop-instance.sh dev admission
```

### Happy / fail-closed path (RP-CUA-014)

1. Visuals → New visual (`blank.canvas.v1`, draft). Confirm pane header shows `vis_… · rev 1 · digest —` (or `content …` if a content digest exists).
2. **Add to report** (new report). Confirm identity under the button. Open Reports.
3. Block should say **Live pointer**, not Frozen. Integrity `unresolved`. Pin/Seal strip shows `vis_`.
4. **Run preflight** → `Resolve validation errors` / `unresolved_visual_evidence`. Seal report disabled.
5. **Pin all evidence** → same code in the error (`unresolved_visual_evidence: visual vis_… rev N has no seal receipt…`).
6. Open the visual pane → pass E1 quality gate if needed → **Seal** that revision (VisualSeal, not report seal).
7. Back on Reports → Pin all → preflight **Ready to seal** → Seal report.

A result-only pinned report (no visual block) remains sealable. A brand-new
report with no narrative/evidence, including one with only an automatic empty
appendix, now fails closed with `empty_report`; RP-CUA-009 is landed.

### Machine checks (already green on this tree)

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-release/apps/synth_desktop

# JS identity + Frozen-copy invariant
node --test tests/admission_identity.test.mjs

# Rust admission + specta lockstep (264)
cargo test --manifest-path src-tauri/Cargo.toml --lib blank_visual_evidence_is_not_sealable -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib sealed_visual_can_be_pinned -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib appendix_experiment_records_do_not_block -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib seal_inlines_visual_bytes -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib report_validation_persists_explicit_evidence -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib export_specta_protocol_bindings -- --nocapture
```

If you add fields to an existing specta type: regen only (`cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored`). If you add a Tauri command: `collect_commands!` + regen + bump the current 264 count. Admission itself did not add a command.

---

## Fail closed (do not start these)

- New sqlite `ArtifactRevision` / migration 40
- New MCP verb or specta command
- Reintroducing a parallel visual-evidence verdict / changing `VisualStatus`
- Writers for `forked_from` / `rerun_of`
- Candidate compare/promote
- `reports_promote` on visuals MCP
- Laguna as a plugin registry row (`plugin_id` stays `optimizers` + `computer-use`)
- Merging `contentDigest` and `receiptDigest`
- Intern / CloudDesk host
- Four-browser unification (Data catalog, experiment inspector, Outputs report rail) beyond showing digest **if** those surfaces grow an attach/pin/seal button

---

## Remaining visual-oriented work (ranked)

Closeout audit (2026-08-26): the renderer has moved past several items in the
original handoff. `admission_identity.test.mjs`, `visuals_page.test.mjs`,
`visual_pane_min_width.test.mjs`, and `visual_pane_shared.test.mjs` cover the
landed projection/chrome behavior (21 checks). Keep the completed items below
as invariants; do not rebuild them.

Admission and the listed projection/chrome closeout are done. Remaining cuts
must still refine projections, not introduce a new class.

1. **Experiment relations — landed.** Child writers accept `follow_up` / `forked_from` / `rerun_of`; member and Candidate compare/promote use `experiments_relate` and fail closed on mixed kinds. `ArtifactRevision` remains intentionally absent; keep using `admit_visual_evidence`.
2. **RP-CUA-009 — landed.** Empty reports return `empty_report` and cannot seal. This is a content-presence check, not a gate on `is_evidence_kind()`.
3. **Identity projection — landed.** Attach/pin/seal chrome, Data catalog, Chat VisualCard, and Outputs show `vis_` + revision + a labeled receipt/content digest. Report rows retain their `rep_` identity. Title remains the human label; there is still one visual registry.
4. **Filtered-empty copy — landed.** Visuals and Reports distinguish an empty filter from an empty registry and offer a clear-filter action. The Templates tab is deliberately labeled **Template visuals** while it remains a projection of VisualRecords with `rendererKind === "template"`; a shipped template catalog is a separate future cut.
5. **Pane lifecycle/chrome — landed for this refactor.** Chat, Visuals, Experiments, Optimizers, Data, and Reports share the window pane host. Settings joins it while a pane is open. Escape unwinds labeling/expanded state before closing, close restores focus to the workbench, and the inventory Back path restores its origin. “Open canvas” was removed; focus mode is explicitly review/presentation, not an authoring editor.
6. **Dead evidence verdict — removed.** `decideVisualEvidence` and its public exports/tests were deleted. Do not recreate it or wire a parallel verdict to Reports.
7. **Sourced/compose** — CUA proof already passed on packaged `sourced-cua`. Optional: compose `optimizer_run` CUA on that instance. Product `optimizer.*` chrome stays.

### Verification note

The renderer closeout checks above pass on `codex/v08-release-integration`.
The focused Rust admission, empty-report, experiment-lineage, member/Candidate
relation, and Specta export checks also pass. Specta remains 264; the
dead-verdict/projection closeout added no command or protocol shape.

For later projection work, keep the join key `id + revision + labeled digest`.
Title stays the human label.

---

## Parallel tracks (do not mix)

| Track | Handoff | Stay out of |
| --- | --- | --- |
| Candidate on experiment spine | [`HANDOFF_EXPERIMENT_CANDIDATE_2026-08-26.md`](./HANDOFF_EXPERIMENT_CANDIDATE_2026-08-26.md) | already in this dirty tree; do not redo migration 39 |
| Sourced / compose CUA | [`HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md`](./HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md) | already passed; package only if you need Computer Use |

If you must touch a shared file (`migrations.rs`, `specta.rs`, `lib.rs`, `protocol.ts`), keep the diff to the visual cut you are on and say so.
