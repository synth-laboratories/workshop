# v0.3 ready?

**Friends ZIP: yes.** Workshop `0.3.0` is packaged from `josh/v03` at `4965ca4` and published as GitHub `v0.3.0`. It is not Apple-notarized.

## What shipped in the mint

- Gemini 3.7 Flash (SYN-3216)
- Settings → Context (SYN-3220)
- Visual families, Trace V5 inspector, native diagrams, splitters (SYN-3217)
- Typed approval broker (SYN-3227)
- Reports (seal / compare / private share / committed publish)
- Optimizers plugin MCP lifecycle
- Bounded Craftax GEPA product recipe

## Still not this cut

1. SYN-3222 rail / child reads / overlap Playwright landed on `2dd1cf0`. Live Gemini CUA on the packaged ZIP still outstanding.
2. SYN-3224 Workshop SSE backfill + harbor-lite ingest landed on `2dd1cf0`. Adapter/protocol PRs are in isolated worktrees; the 3×2×5 matrix has not been executed. Do not close on the Craftax Harbor demo.
3. Optimizer DAG live visual (`optimizer.dag.live.v1`) — v0.4.
4. Intern — v0.4.
5. Installed-app CUA, Bombadil, and Playwright UI gates were not run on this ZIP.
6. Linear could not be closed from this environment (Linear MCP needs Cursor desktop auth).

## Package

See [PACKAGE.md](./PACKAGE.md). Cut with `./scripts/release-artifact.sh`; ZIP is `Synth-Desktop-v0.3.0-macOS-arm64-UNNOTARIZED.zip`.
