# v0.3 known issues and deferred items

## Release blockers

| Item | Why |
| --- | --- |
| Dirty tree | Changelog, identity, launch docs, and focused-test updates are uncommitted. Friends packaging requires a clean source tree. |
| No tested package | `scripts/release-artifact.sh` has not produced a v0.3.0 ZIP from a committed SHA. |
| CUA not run | Installed-app acceptance across 1440–480 widths was not executed this pass. |

## Must-fix before GO (product)

| Item | Ticket | Notes |
| --- | --- | --- |
| Reports not on this branch | — | `agent/v03-reports-complete` is stacked on optimizer plugin MCP. Port or drop the claim. |
| Subagents rail incomplete | SYN-3222 | Grouped visual exists. No child workspace, no wall-clock overlap proof. |
| E4 matrix evidence missing | SYN-3224 | No canonical run IDs / configs / raw evidence package found. Do not imply complete. |
| Linear closure | SYN-3220, SYN-3227, SYN-3222, SYN-3217, SYN-3224 | MCP auth unavailable in this agent environment. |

## Explicitly deferred (do not claim)

- E2 Craftax alignment ladder (SYN-3221)
- E3 DungeonGrid concurrent multi-agent (SYN-3223)
- E5 ResearchAssistantBench / Nemotron (themes freeze)
- GELO iteration + OHCO (SYN-3225)
- Intern (v0.4)
- Optimizer plugin MCP as a friends product
- Publishing, notarizing, or external distribution

## Can defer (polish)

- `ProviderTransport::resolve_approval` still exists; the first-pass broker handoff left the command rename for last.
- Unused-import / dead-code warnings in the Desktop lib.
- Root workspace `package.json` still says `0.1.0` (not product-visible).
