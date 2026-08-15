# Workshop v0.3

## New

- Added Gemini 3.7 Flash via OpenRouter for live collaboration turns.
- Added Settings → Context: agent limits, skills, MCP groups, cookbooks, and subagent compatibility, persisted with the existing preferences store.
- Organized visual templates into families (containers, optimizers, diagrams, analysis) without changing stable template IDs.
- Added a generic Trace V5 inspector so any compatible sealed archive can open from Data → Traces and reopen by digest after restart.
- Added a typed approval broker for paid compute, sidecar lifecycle, and credential access. Permissive `.synth` / Full-system-access policy is honored and audited; it does not silently revert to Always Ask.
- Paid-compute approvals carry an explicit cap. Exceeding that cap is recorded as a receipt violation, not coerced to zero.
- Added local Reports: create, seal, reopen, compare, then privately share or publish a committed revision.
- Added the Optimizers plugin MCP lifecycle: install/start/stop through the typed broker, then prepare → open visual → await ready → start.
- Added Larval Mander presence and `session_present` MCP so a chat can show a title, emotion, and ≤7-word summary.

## Improved

- Native Mermaid and systems diagrams use the packaged Rust renderer, work offline, and fail closed on invalid source.
- Chat/visual and Visuals list/preview splitters drag and persist independently, then stack at compact widths with no dead gap.
- Subagent activity is grouped in the visual pane (working / needs attention / completed) without dumping child chat into the parent transcript.
- Craftax GEPA can run as a bounded Optimizers product recipe.

## Fixed

- Cookbook pin progress stays current, and Context command errors are presented instead of failing silently.
- Pending approvals no longer render as live cards with dead buttons after restart. Unresolved requests expire as durable history.

## Known limitations

- The Codex-like subagent rail, child workspace, and overlapping spawn/wait product surface (SYN-3222) is not in this cut. The grouped visual remains.
- E4 Harbor DEO matrix evidence (SYN-3224) is not a v0.3 claim. E2, E3, and E5 remain deferred.
- The optimizer DAG live visual (`optimizer.dag.live.v1`) is deferred to v0.4.
- Intern remains dormant (v0.4).
