# Handoff: visual admission + remaining visual work

**For:** Guy — test the attach/pin/seal admission cut, then continue visual-oriented leftover.  
**Date:** 2026-08-26  
**Do not commit unless asked.**

Noun map (keep current): [`docs/qa/v08-visuals-data-model.md`](./qa/v08-visuals-data-model.md)  
CUA findings this cut targets: RP-CUA-014 / 053 / 060 (handoff chrome only).  
Sourced/compose CUA is already passed: [`HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md`](./HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md). Do not rebuild it.

Durable authority is the **local store**. Do not introduce `CoreRuntime` as a product noun. Specta command count stays **263**. Craftax is rust GameBench gold only (`env:craftax_gold`). Do not use intern / research-intern MCP.

---

## Tree

| Path | Branch | Git |
| --- | --- | --- |
| `/Users/joshuapurtell/GitHub/workshop-v08-release` | `codex/v08-release-integration` | **Dirty.** Admission sits on top of Candidate + compose `optimizer_run` slot + shared `VisualPane` + `list_templates` components + Laguna-vs-Plugin pin. Do not commit this pile unless asked. |

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

`decideVisualEvidence` (`ready|reviewed|partial|failed`) still has no consumers. Do not drive Reports with it.

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
| Protocol | `apps/synth_desktop/src/renderer/src/generated/protocol.ts` — regen only, **263 unchanged** |
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

A result-only pinned report (no visual block) must still be sealable. An empty report with only auto appendix experiment-records must still be sealable. **RP-CUA-009 is still open:** a brand-new empty report with no visual block can still say Ready to seal. That is not this gate.

### Machine checks (already green on this tree)

```bash
cd /Users/joshuapurtell/GitHub/workshop-v08-release/apps/synth_desktop

# JS identity + Frozen-copy invariant
node --test tests/admission_identity.test.mjs

# Rust admission + specta lockstep (263)
cargo test --manifest-path src-tauri/Cargo.toml --lib blank_visual_evidence_is_not_sealable -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib sealed_visual_can_be_pinned -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib appendix_experiment_records_do_not_block -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib seal_inlines_visual_bytes -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib report_validation_persists_explicit_evidence -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib export_specta_protocol_bindings -- --nocapture
```

If you add fields to an existing specta type: regen only (`cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored`). If you add a Tauri command: `collect_commands!` + regen + bump 263. This cut must not bump.

---

## Fail closed (do not start these)

- New sqlite `ArtifactRevision` / migration 40
- New MCP verb or specta command
- Driving Reports from `decideVisualEvidence` / changing `VisualStatus`
- Writers for `forked_from` / `rerun_of`
- Candidate compare/promote
- `reports_promote` on visuals MCP
- Laguna as a plugin registry row (`plugin_id` stays `optimizers` + `computer-use`)
- Merging `contentDigest` and `receiptDigest`
- Intern / CloudDesk host
- Four-browser unification (Data catalog, experiment inspector, Outputs report rail) beyond showing digest **if** those surfaces grow an attach/pin/seal button

---

## Remaining visual-oriented work (ranked)

Admission is done. Next visual cuts are **projections and chrome**, not a new class.

1. **Still leftover on the noun map.** CHECK leftovers `rerun_of` / `forked_from` (writer is `follow_up` only); compare/promote including Candidate. `ArtifactRevision` as a sqlite class is explicitly **not** built — keep using `admit_visual_evidence`.
2. **RP-CUA-009** — empty report (no visual block) still “Ready to seal”. Separate product decision. Do not “fix” it by gating on `is_evidence_kind()`.
3. **Identity outside the handoff.** Attach/pin/seal chrome now shows `vis_` + labeled digest. Data catalog and Outputs report rail are still title-first (RP-CUA-060 remainder). Do not invent a fourth registry.
4. **Filter empty copy** — Visuals Live (RP-CUA-001) and Reports Sealed (RP-CUA-015) still say “no visuals/reports yet” when the registry is non-empty. Templates tab is still `rendererKind === "template"` pretending to be a catalog (RP-CUA-050/051/052).
5. **Pane chrome leftovers.** Escape closes instead of restoring split (RP-CUA-004). Settings/Reports still unmount the shared `VisualPane` host. “Open canvas” is not an editor (RP-CUA-013).
6. **`decideVisualEvidence`** — still unused. Leave it or delete it; do not wire it to Reports.
7. **Sourced/compose** — CUA proof already passed on packaged `sourced-cua`. Optional: compose `optimizer_run` CUA on that instance. Product `optimizer.*` chrome stays.

If you pick (3)–(5), keep the join key `id + revision + labeled digest`. Title stays the human label.

---

## Parallel tracks (do not mix)

| Track | Handoff | Stay out of |
| --- | --- | --- |
| Candidate on experiment spine | [`HANDOFF_EXPERIMENT_CANDIDATE_2026-08-26.md`](./HANDOFF_EXPERIMENT_CANDIDATE_2026-08-26.md) | already in this dirty tree; do not redo migration 39 |
| Sourced / compose CUA | [`HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md`](./HANDOFF_SOURCED_VISUALS_CUA_2026-08-26.md) | already passed; package only if you need Computer Use |

If you must touch a shared file (`migrations.rs`, `specta.rs`, `lib.rs`, `protocol.ts`), keep the diff to the visual cut you are on and say so.
