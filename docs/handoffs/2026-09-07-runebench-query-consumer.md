# RuneBench query consumer — 2026-09-07

## Coordination boundary

This consumes the shared trace-research implementation documented in `docs/engineering/TRACE_RESEARCH_IMPLEMENTATION_2026-09-07.md`. No changes were made to query evaluation, job membership, campaign execution, Jesterky, Containers schemas, or optimizer scheduling. Concurrent changes are preserved; the regenerated protocol includes their current commands and types.

## Implemented

- Shared `TraceResearchPanel` and client interface in `visuals/components/agent_trace.v1/TraceResearch.tsx`: explicit typed V2 query, saved snapshot restoration, bounded result/source paging, snapshot and source digest checks, exact source reading, and optional annotation preparation. Preparation starts no compute.
- Host adapter in `apps/synth_desktop/src/renderer/src/runtime/traceResearch.ts`, supplied by VisualHost to sourced visuals. One allowlisted native command forwards query/page/snapshot/source/prepare_annotations to existing visuals IPC handlers. Arguments/results cross Specta as JSON strings; raw recursive JSON types caused export stack overflow. No credentials cross the bridge.
- RuneBench comparison uses the shared panel; exact selectors match retained trace digests and entity IDs. Unmatched replays fail explicitly while source remains accessible.
- Retained comparison navigation persists run selection, alignment, actor, event filter, search, replay cursors, and focus. Contribution table is collapsible; pane headings remain visible.
- Local visual `vis_12c0bfef3cc34b1ea0537f1043cc04c0` saved as revision 29. Existing replay/evidence transport is retained.

## Validation

- Workshop TypeScript check passed (`artifacts/runebench-query-typecheck.log`).
- Three consumer/view/scroll tests passed (`artifacts/runebench-query-consumer-tests.log`). Controlled consumer test covers explicit query, source paging, snapshot mismatch rejection, annotation preparation, and restoration after reload. This is adapter validation, not live backend acceptance.
- Native Specta regeneration and export contract check passed (`artifacts/runebench-query-bindings-retry.log`, `artifacts/runebench-query-contract.log`).
- Real retained RuneBench browser check preserved actor mab and cursor 15179 ms across leaving/reopening comparison; no page errors. Standalone mode shows transport unavailable explicitly. Screenshot: Evals `workshop/runebench-ma-demo/ma/query-consumer-offline.png`.
- Native capture receipt confirms revision 29: Evals `workshop/runebench-ma-demo/ma/query-consumer-native-receipt.json`.

## Completed integration

The user authorized completing the remaining integration. Containers inspection now carries existing run/trial/episode identity and recorded effort; older RuneBench evaluated-model metadata is read from its sealed terminal result. Workshop retains validated standalone V5 bytes in CAS without rebinding/resealing them, and resolves imported run IDs from trace metadata. The internal `traces.run_id` FK remains reserved for Workshop runs. Existing optimizer resolution remains unchanged and takes precedence.

The Containers query/source reader accepts these verified standalone documents as well as archives. Missing standalone evidence/reward fields remain missing. This does not manufacture a benchmark score from the displayed team XP total. Native activation and live acceptance are complete; receipts follow.

## Live acceptance completed

Rebuilt and launched hitlqa, version 0.9.7, process 45727 at validation, executable digest `sha256:0312c265c6f7f4a0aacebe0364d4f73d61244e8d4edc449a31c9ebf924b31a69`. Health returned ok. CUA confirmed revision 29 and both comparison panes in the rebuilt app.

Imported Luna low/medium/high and Terra low through the existing development driver ingest route. All four original trace digests were retained. Registered their actual execution container endpoint, localhost:8116 (runebench-ma-demo), which remains stopped; retained reads need no new rollout or inference.

The real Workshop query returned 159 tool results across the four jobs. Pages of 17 contained no missing/duplicate result IDs and retained one snapshot/result digest. Exact source paged to 908 characters with a stable text digest. Annotation preparation returned startsCompute=false and the registered source-container target. Foreign result IDs were rejected. Saved snapshot restoration passed.

The shared React panel passed against the live API (query, source, prepare, next page, reload restoration). The real RuneBench viewer also passed query → exact source → Open in replay: selected trace digest and event ID exactly equal the query citation. The fresh headless Chromium test profile needed its supported local-network-access permission for the localhost evidence service; no app browser-security setting was changed.

Receipts under Workshop artifacts:
- runebench-query-live-acceptance.json
- runebench-query-browser-receipt.json and runebench-query-live-browser.png
- runebench-query-replay-receipt.json and runebench-query-replay.png
- runebench-query-membership-tests.log: 5 passed
- runebench-query-capture-checks.log: 20 passed
- runebench-query-remaining-ui.log: 3 passed
- runebench-query-shared-regressions.log: 9 passed
- runebench-query-remaining-contract.log: native generated contract passed
- runebench-query-remaining-typecheck.log: TypeScript passed

This local build uses the supported browser-runtime-optional mode because the separate embedded browser runtime was absent. It uses ad-hoc signing and no Keychain access. This validates the retained-query workflow, not production signing/browser packaging. Missing reward/evidence in a standalone seal remains missing; existing RuneBench replay annotations remain in the original local evidence service. No annotation campaign or provider run was launched.
