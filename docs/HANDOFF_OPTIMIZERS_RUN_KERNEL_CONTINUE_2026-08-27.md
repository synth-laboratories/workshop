# Optimizers run-kernel implementation status

- Date: 2026-08-27
- Status: production authority cutover implemented and focused validation green; work remains uncommitted
- Original design: `/Users/joshuapurtell/GitHub/workshop-v08-release/docs/HANDOFF_OPTIMIZERS_RUN_KERNEL_REFACTOR_2026-08-27.md`

## Location

| Item | Value |
| --- | --- |
| Worktree | `/Users/joshuapurtell/GitHub/workshop-v08-run-kernel` |
| Branch | `codex/optimizers-run-kernel` |
| Base | `a506e26c21a6f98a983bf88e8eb906b5f70bdb7b` |
| Commits | None; all changes remain in the working tree |
| Release repo | `/Users/joshuapurtell/GitHub/workshop-v08-release` unchanged |
| Producer repo | `/Users/joshuapurtell/GitHub/optimizers-v08-release` unchanged |

Another task is editing credential, approval, secrets, and failure-contract files in this same worktree. Preserve those edits when splitting or committing the run-kernel work.

## Implemented cutover

- One canonical `optimizer_run` aggregate now owns Eval, GEPA, GO-EX, SFT, and CISPO lifecycle, work, evidence, terminal state, and result semantics.
- The kernel has typed admission, algorithm, driver, sequencing, work, evidence, commit, persistence, projection, and V2-view modules under `apps/synth_desktop/src-tauri/src/optimizers/kernel/`.
- Run creation passes through an admission draft/spec/projection transaction. Approved execution consumes its draft atomically; imports and cloud attachments are explicitly `NotRequired`. A run is not minted by staging a draft.
- Producer events have distinct producer and aggregate sequences. Replay is idempotent, gaps and digest collisions fail closed, and projection persistence participates in the event transaction.
- Terminal state seals once. Post-terminal evidence is accepted only as an append-only amendment tied to the sealed terminal sequence.
- Missing cost, token, reward, step, plan, or evidence data remains unavailable rather than becoming zero.
- `OptimizerRunViewV2` is a generated discriminated union with common run metadata and typed Eval, GEPA, GO-EX, SFT, and CISPO projections.
- The desktop bridge and shared subscription always request V2 for production run progress. Live cards, optimizer workspaces, and GEPA sibling comparison format V2 and fail closed if it is absent. Raw event reduction is reachable only for explicit historical cursors or transports intentionally injected without V2 in legacy adapter tests.
- Optimizer visual projection code and `VisualHost.tsx` no longer use `@ts-nocheck`.
- Experiments accept canonical optimizer-run members only and verify the referenced run exists. Retired campaign/direct member coverage is ignored as legacy behavior.
- The campaign Rust module, campaign DataStore methods, `/v1/campaigns` IPC, reconciliation paths, and campaign MCP tools are deleted.
- Legacy evaluation run/progress persistence and campaign dual writes are deleted.

## Algorithms and placements

The driver registry fails closed on unsupported pairs:

| Algorithm | Supported placement |
| --- | --- |
| Eval | local Python process; direct container evaluation |
| GEPA | local Python process; hosted Optimizers service |
| GO-EX (displayed as GELO) | hosted Optimizers service |
| SFT | local training sidecar; remote training service |
| CISPO | local training sidecar; remote training service |

GO-EX is the only accepted wire name. `gelo`, `hosted_gelo`, `go_ex`, `goex`, and case variants fail with `AlgorithmAliasRejected`.

## Migrations

- Migration 50 installs kernel tables and columns.
- Migration 52 converts historical campaign execution and experiment membership to canonical optimizer runs and refuses identity collisions.
- Migration 53 is the one-way cut: it drops `eval_campaign_rollouts`, `eval_campaigns`, `evaluation_rollouts`, `evaluation_run_drafts`, and `evaluation_runs`.
- Required-schema healing installs kernel tables separately from ALTER statements and no longer recreates retired evaluation tables.

## Validation on 2026-08-27

Green:

- `cargo test --lib optimizers::kernel -- --test-threads=1`: 28 passed
- `cargo test --lib storage::migrations::tests -- --test-threads=1`: 22 passed
- `cargo test --lib experiments::tests -- --test-threads=1`: 10 passed, 7 retired legacy tests ignored
- `cargo test --lib platform::failure::runtime_tests::admission_failure_links_the_canonical_optimizer_run -- --test-threads=1`: passed
- `cargo test --bin synth-containers-mcp`: 13 passed
- `cargo test --lib contract::specta::tests::export_specta_protocol_bindings -- --test-threads=1`: passed; generated protocol is current
- `npm run typecheck --workspace @synth/synth-desktop`: passed
- `node --test apps/synth_desktop/tests/run_progress_adapters.test.mjs`: 32 passed
- `git diff --check`: passed

Broad optimizer suite: 428 passed, 2 ignored, 4 failed. The four failures are pre-existing or belong to concurrent runtime/capability edits outside the run-kernel cutover:

1. `optimizers::eval_recipes::immutable_target_tests::a_mutable_tag_is_refused_before_the_run_is_created`: expected error spelling differs from `target_digest_missing`.
2. `optimizers::manager::tests::installed_service_has_offline_runtime`: advertised algorithm is null rather than `gepa`.
3. `optimizers::service::tests::absent_capabilities_refuse_paid_start_instead_of_skipping_the_pin`: assertion expects a different no-capability message.
4. `optimizers::service::tests::exceeding_an_approved_cap_is_a_durable_receipt_violation`: concurrent capability receipt behavior reports `violation = false`.

`cargo fmt --all -- --check` is not globally green because unrelated files in the shared dirty worktree already contain formatting drift. The new kernel directory was formatted directly and `git diff --check` is clean.

No hosted credential-backed end-to-end run was attempted. No authorized project-local provider credential was needed for the validation above, and no Keychain operation occurred.

## Review notes

- Do not merge a subset that includes migration 53 without the V2 UI and experiment cutover.
- Keep raw historical event inspection diagnostic-only; do not reintroduce a live fallback when V2 is missing or invalid.
- Do not restore campaign/direct-evaluation runtime vocabulary or any legacy table writer.
- Split the run-kernel changes from the concurrent credential-locator work before committing.
