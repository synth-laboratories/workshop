# v0.10 integrated release-readiness closeout

Date: 2026-09-09  
Branch: `codex/v010-complete`  
Runtime source commit certified: `d54f1d876166`  
Outcome: **locally release-ready; external publication remains held**

## Executive assessment

The previously separate v0.10 shell/native-fix and visuals-release lanes are now integrated in one clean branch. The integration resolves the behavioral conflicts exposed by combining them, restores stale tests to the current architecture, stages every packaged runtime from its exact source pin, builds the macOS release artifacts, and completes the packaged native visual tail through launch, interaction, capture, restore, two-view review, readiness, and immutable sealing.

No production state was changed. The branch was not pushed or merged; the DMG was not Developer-ID signed or notarized; the four coordinated Python packages were not published; and no paid provider run was started. Those actions remain outside the authority preserved by the source handoffs.

## What was integrated

The integration commit `d54f1d87` merges `codex/v0.10.0-visuals-release` into the complete v0.10 shell/native-fix base. It includes the packaged browser lane, trace policy-call and Harbor repair work, ACP ordering and terminal attribution fixes, fullscreen/native geometry fixes, plugin/annotation empty-state fixes, provider-labelled limit errors, and the full visuals engine and acceptance corpus.

Conflicts were resolved according to ownership and recency rather than by choosing one branch wholesale:

- the shell lane's current training workspace, desktop-instance launcher, optimizer recovery implementation, and provider-limit handling were retained;
- the visuals registry, runtime bindings, styling, managed templates, and acceptance suites were retained;
- container-eval fixtures now support both policy-config routing and the scoped Workshop proxy contract;
- child-session projections keep a standalone child visible while excluding it from the parent;
- terminal failure activity is keyed to its terminal sequence so a retry cannot inherit an earlier run's summary;
- compatibility-shim source tests now inspect the shared adapter implementations they delegate to;
- paid-compute tests use distinct preparation digests when they intend distinct reservations, preserving the production rule that a retry of one preparation replaces its earlier hold;
- lock-wait diagnostics are tested as monotonic process-global deltas, avoiding a race with other database tests.

## Verification results

The integrated renderer passed TypeScript checking, the visual suite, accessibility acceptance, focused parent/child conversation tests, and the production graph build. The accessibility run completed 722/722. The graph build compiled 625 renderer modules and emitted production assets; its only notable message is the already documented Reports static/dynamic import warning.

The first parallel native run reported 1,820 passing tests and 19 failures. Four failures were stale compatibility/version assertions and were corrected. The remaining cluster was isolated to process-global or resource-contention behavior. A one-thread run then reached 1,837 passing, 2 failing, and 16 intentionally ignored. The final two tests were not runtime failures: both reused `sha256:spec` while asserting that separate paid-compute requests accumulated, contradicting the deliberate retry-replacement contract. After assigning distinct preparation digests, both focused tests pass. The formerly racy lock-wait test also passes in isolation. Thus every observed deterministic failure on the integrated tree is closed; the 16 ignored tests still declare their external model/service prerequisites in source.

The relevant durable evidence already present in this repository remains authoritative for the broader 40-family visuals certification: `docs/engineering/visuals-v010-final-handoff.md` and `docs/receipts/2026-09-09/visuals-certification.json`. This closeout adds evidence for the integration/package tail rather than replacing those receipts.

## Packaged-runtime proof

Packaging initially failed closed three times, each for a useful reason: the live Containers checkout was older than Workshop's pin, the live MLX checkout differed from the sealed source revision, and the Optimizers distribution had not been staged. Those dirty producer trees were left untouched. Clean detached worktrees were created at the catalog revisions and used to build verified embedded distributions:

| Runtime | Packaged identity | Exact source |
|---|---:|---:|
| synth-containers | `0.4.2.dev20260908` | `be173da` |
| synth-optimizers | `0.2.21` | `137fe713fc485687d9176ea4d50cddeba2f5fb44` |
| synth-mlx-rl | `0.6.0` | `5d6db14330babcff170d2afbb8535de2138385a9` |
| browser runtime | Node 24.18.0, Chromium 151.0.7922.34 | Playwright revision 1234 lock |

The release-profile build produced `Synth Workshop.app` and `Synth Workshop_0.10.0_aarch64.dmg`; the DMG hash is `97157d690ae93752466dbb7c1df7d23ccd303696415b854c12873eabde69449e`. This unsigned release bundle is a build artifact, not a publication candidate until the production signing/notarization lane runs.

The isolated signed CUA bundle launched as `com.synth.desktop.v010.dev.v010-complete`. Runtime health reported source/build revision `d54f1d876166` and executable digest `sha256:422e21538b8957a59206d25c4ff56d80075e9132935b465aaf96070da1644541`.

## Packaged visual acceptance and seal

The packaged app imported and mounted the networkless managed-session fixture. MCP actions captured state 0, advanced to state 3, restored state 0, and captured verified pixels at each point. A native UI click then advanced the same committed state from 0 to 1, proving that human and MCP actions share the packaged visual session.

The visual was captured at 1440×900 and 760×760. Both PNGs were inspected directly: the primary surface and control are visible, hierarchy is clear, and no collision, clipping, crossing, or overflow is present. Two screenshot-backed reviews were recorded against the exact renderer, executable, bindings, content, template, source, and build digests. `mark_ready` passed. The subsequent seal produced `synth.artifact-bundle.v1`, 20,812 bytes, with receipt digest `424beec9628beaed5f59f19623bd5fc8d65d3e0d6374aef672c257adae0ff9f4`.

Machine-readable details are in `docs/receipts/2026-09-09/v010-integrated-packaged-closeout.json`.

## Explicit release decisions

Reports remains on its documented read-only unhosted shell for v0.10. It functions correctly but does not claim the retained-session and capture guarantees of a VisualHost pane. Moving it now would change report semantics and require a new acceptance surface; the existing ship handoff explicitly treats shipping as-is as defensible.

The Containers `EventV5.artifact_ids` linkage is not adopted. Sealed frames remain addressable by digest, and taking that branch would force an exact Containers repin across every package lane. The visuals ship handoff marks it as non-required for v0.10.

Gate 7 consolidation is deferred. Compatibility symlinks and legacy migration-72 read/import adapters remain because deleting persistence compatibility is unnecessary release risk. Eraser remains excluded by design.

## Remaining external actions

Local implementation and certification are complete. Releasing to users still requires human release authority for:

1. push this branch and open/merge the PR into the v0.10 release line;
2. publish the four coordinated Python package cuts in their dependency order;
3. Developer-ID sign, notarize, and verify the final distribution;
4. run any still-desired paid training experiment under a separately stated bounded budget.

Until those actions are explicitly authorized and completed, the precise status is **certified local candidate, publication held**—not “released.”

## Source trail

- `docs/engineering/visuals-v010-ship-handoff.md` — required tail and the three release decisions.
- `docs/engineering/visuals-v010-final-handoff.md` — 40-family evidence and original certification details.
- `docs/receipts/2026-09-09/visuals-certification.json` — prior mark-ready certification receipt.
- `docs/receipts/2026-09-09/v010-integrated-packaged-closeout.json` — this integration's package, runtime, UI, review, and seal identities.
- `apps/synth_desktop/src-tauri/src/session/paid_compute_budget.rs` — retry-replacement reservation contract.
- `apps/synth_desktop/src-tauri/src/storage/database.rs` — process-global lock-wait diagnostics contract.
