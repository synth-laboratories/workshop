# v0.3 known issues and deferred items

## Release blockers for this friends ZIP

| Item | Why |
| --- | --- |
| CUA not run | Installed-app acceptance across 1440–480 widths was not executed on the minted ZIP. |
| Unnotarized | Adhoc signature only. GitHub `v0.3.0` is the friends ZIP, not an Apple-notarized build. |

## Product gaps (do not claim)

| Item | Ticket | Notes |
| --- | --- | --- |
| Subagents rail incomplete | SYN-3222 | Grouped visual exists. No child workspace, no wall-clock overlap proof. WIP is stashed off this mint. |
| E4 matrix evidence missing | SYN-3224 | No canonical run IDs / configs / raw evidence package. Do not imply complete. |
| Linear closure | SYN-3220, SYN-3227, SYN-3222, SYN-3217, SYN-3224 | MCP auth unavailable in this agent environment. |

## Explicitly deferred (do not claim)

- Optimizer DAG live visual (`optimizer.dag.live.v1`) — v0.4
- E2 Craftax alignment ladder (SYN-3221)
- E3 DungeonGrid concurrent multi-agent (SYN-3223)
- E5 ResearchAssistantBench / Nemotron (themes freeze)
- GELO iteration + OHCO (SYN-3225)
- Intern (v0.4)
- Publishing an Apple-notarized build; this friends ZIP is adhoc-signed on GitHub `v0.3.0`

## Can defer (polish)

- `ProviderTransport::resolve_approval` still exists; the first-pass broker handoff left the command rename for last.
- Unused-import / dead-code warnings in the Desktop lib.
- Root workspace `package.json` still says `0.1.0` (not product-visible).
- `scripts/release-artifact.sh` treats Containers as missing when `.git` is a worktree pointer file rather than a directory.
