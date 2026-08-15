# v0.3 commit / merge map

**Branch:** `josh/v03-gemini-flash-openrouter`  
**HEAD:** `b2651fba07cef95cd888a8e49adccba24b000229`  
**Message:** feat(desktop): complete typed approval broker

## Landed on this branch (since v0.2 merge `8ed2613`)

| SHA | Subject | Workstream |
| --- | --- | --- |
| `7f260b4` | feat(workshop): add Gemini 3.7 Flash via OpenRouter | SYN-3216 |
| `8fc3fae` | feat(workshop): complete v0.3 Gemini and context surfaces | SYN-3216 / SYN-3220 |
| `7dd0817` | fix(desktop): complete Context settings acceptance | SYN-3220 |
| `1da5b2d` | fix(desktop): present Context command errors cleanly | SYN-3220 |
| `38b2ea3` | fix(desktop): keep cookbook progress current | SYN-3220 |
| `003a36f` | feat(visuals): organize families and finish trace inspection | SYN-3217 |
| `4b4e92a` | Reapply typed approval broker | SYN-3227 |
| `554d7c0` | fix(visuals): reach compact stack at app minimum | SYN-3217 |
| `b2651fb` | feat(desktop): complete typed approval broker | SYN-3227 |

Earlier SFT recipe-field commits (`10a8eb6`, `54bfcb7`) are on the line but are not v0.3 collaboration claims.

## Not merged (reviewed)

| Branch | Tip | Reason |
| --- | --- | --- |
| `agent/v03-reports-complete` | `2157f26` | Reports is in scope, but the branch is stacked on optimizer plugin MCP (`10ec866`). Do not merge the plugin. Port Reports later. |
| `agent/v03-proofs-e2-e4` | `efc92d5` | Adds deferred E2/E3/GELO/OHCO plus E4 surfaces. No result package. |
| `josh/v03-optimizer-plugin-mcp-e2e` | `460187b` | Out of scope. |
| `josh/v03-approval-broker` | `480e1fe` | Planning docs + original broker; the broker was reapplied onto this branch instead. |

## Working tree (uncommitted integration)

Host authorization and sidecar gating landed in `b2651fb`. Remaining dirty files are About 0.3.0 changelog, packaging/instance identity, launch docs, the release folder, and focused-test updates. See [TEST_REPORT.md](./TEST_REPORT.md).
