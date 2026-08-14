---
name: use-synth-plugins
description: Enable, disable, install, start, stop, update, and remove built-in Workshop product plugins through mcp__synth_plugins__plugin_manage. Use for the Optimizers plugin lifecycle, verified downloads, sidecar start/stop, and plugin status. Do not use for optimizer runs — those stay on use-synth-optimizers.
---

# Use Synth Plugins

Use `mcp__synth_plugins__plugin_manage`. Supply only `plugin_id` (`optimizers`) and an optional catalog `version`. Never pass a URL, repository, image, command, environment variable, filesystem path, token, or credential.

`synth_plugins` stays available even when Optimizers is disabled. Disabling Optimizers hides its navigation and stops advertising optimizer MCP tools to new sessions; it does not stop a running service or delete retained runs.

## Lifecycle

1. `status` or `list` — observe `not_installed` / `installed` / `ready` / `stopped`. Reads never prompt.
2. `install` — native approval, then Downloading → Verifying → Installed. Receipt includes version and digest. Download finishes before `start`.
3. `start` — required policy approval, then capabilities handshake. Confirm `gepa`, bounded Banking77 recipes, replay, cancellation, and `optimizer.gepa.live.v1`.
4. Run compute through `mcp__synth_optimizers__optimizer_manage` (`prepare` → `open_visual` → `await_ready` → `start`).
5. `stop` after the run is terminal. Stop retains the distribution, mirrored run, artifacts, and visual replay. Do not `remove` on the happy path.

## Approvals

The host owns approvals. Do not send an approval decision in MCP arguments. Reject leaves state unchanged.
