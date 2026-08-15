# Gemini CUA — Optimizers plugin MCP + Banking77

**Date:** 2026-08-14  
**Driver:** Gemini in a fresh Workshop v0.3 instance. Do not substitute a local model.  
**Instruction (single user turn):**

> Download and start the Optimizers plugin, run the bounded Banking77 GEPA recipe, keep its native visual open, give me the optimized prompt and evidence when it finishes, then stop the optimizer service without deleting the run.

## Preconditions

- Fresh named Workshop instance with no installed optimizer distribution.
- Approval policy: Ask for risky actions (preferred) or Always ask.
- Trusted Banking77/OpenAI credential is already in the Desktop process. Never paste it into chat.
- No shell, hidden Tauri invoke, or operator repair.

## Required sequence

1. `plugin_manage(status)` → `not_installed`
2. `plugin_manage(install)` → native install approval → Downloading → Verifying → Installed
3. `plugin_manage(start)` → start approval → `ready` with `gepa` and `optimizer.gepa.live.v1`
4. optimizer `list_algorithms` / `list_recipes` → report Banking77 limits
5. `prepare` (`gepa.banking77.smoke.v1`) → `waiting_for_viewer`
6. `open_visual` → same run ID in the right panel
7. `await_ready` → `synth.visual-subscription-receipt.v1`
8. `start` → native compute approval for the exact recipe/ceilings
9. Visual streams progress; `watch_run` follows the durable cursor
10. `completed` → `get_result` returns a non-empty prompt + digest, no filesystem path
11. `plugin_manage(stop)` → service stopped, run/visual retained
12. Reopen the same visual; terminal result still renders from the local mirror

## Evidence bundle

Write one JSON receipt under `docs/receipts/2026-08-14/` containing Workshop/app version, instance ID, Gemini model ID, plugin action receipt IDs, approval receipt IDs (no secrets), sidecar version/digest, capabilities digest, recipe/preparation digest, optimizer run ID, visual ID/template digest, readiness receipt, first/final cursor, selected candidate ID, prompt digest, declared limits vs usage, and proof that status is `stopped` with historical replay.

Live Gemini CUA is the definition of done. Renderer/unit/MCP tests in this branch prove the contract; they do not replace that run.
