# v0.3 ready?

**Friends ZIP: yes.** Workshop `0.3.0` is packaged from `origin/dev` at `c146e83` (merge of PR #24) and published as GitHub `v0.3.0`. It is not Apple-notarized.

## What shipped in the mint

- Gemini 3.7 Flash (SYN-3216)
- Settings → Context (SYN-3220)
- Visual families, Trace V5 inspector, native diagrams, splitters (SYN-3217)
- Typed approval broker (SYN-3227)
- Reports (seal / compare / private share / committed publish)
- Optimizers plugin MCP lifecycle
- Bounded Craftax GEPA product recipe
- SYN-3222 Subagents rail, child-thread reads, overlap Playwright
- SYN-3224 Harbor live SSE backfill, harbor-lite ingest, adapter protocol env (evals #278, containers #11, gamebench #8, cardbench #1)

## Still not this cut

1. Live Gemini CUA on the packaged ZIP is still outstanding (Playwright covers reject + expiry).
2. The Harbor 3×2×5 matrix has not been executed. Do not close SYN-3224 on the Craftax Harbor demo.
3. Optimizer DAG live visual (`optimizer.dag.live.v1`) — v0.4.
4. Intern — v0.4.
5. Installed-app CUA, Bombadil, and Playwright UI gates were not run on this ZIP.
6. Linear could not be closed from this environment (Linear MCP needs Cursor desktop auth).

## Package

See [PACKAGE.md](./PACKAGE.md). Cut with `./scripts/release-artifact.sh`; ZIP is `Synth-Desktop-v0.3.0-macOS-arm64-UNNOTARIZED.zip`.
