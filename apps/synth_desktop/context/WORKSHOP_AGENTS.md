# Workshop collaboration context

Workshop is an in-the-loop collaboration workbench. Keep the practitioner in
the flow: make the current visual, trace, diagram, or labelled evidence the
shared workspace rather than narrating a hidden process in the transcript.

- Bind a visual before mutating it and revise that same durable instance.
- Preserve missing observations as missing. Never turn an absent reward,
  score, cost, or attack count into numeric zero.
- Treat sealed Trace V5 data as read-only. Store user labels in the visual's
  `synth.visual-annotations.v1` metadata overlay and read those labels before
  the next revision.
- Use only the skills and MCP servers installed in this session's Codex home.
  Disabled context is intentionally absent; do not infer a cookbook checkout
  or tool from a prior session.
- When the task is the Banking77 GEPA proof under
  `/Users/joshuapurtell/GitHub/evals/temp/banking77-gepa/`, load and follow
  `$run-banking77-gepa` before taking task actions.
- Do not read `.env`, `.env.*`, `secrets.toml`, or other plaintext credential
  files (`cat`, `head`, `rg`, editor tools). Codex `sandbox_workspace_write`
  has no read-denylist field — this file, the installed task skill, and the
  secrets skill are the policy. Never use the Keychain-backed Secrets registry,
  its credential-import flow, or macOS Keychain.
  Use `mcp__synth_secrets__secrets_manage`: call `workspace_roots_list`,
  `bindings_list`, and `locators_list`, then call `source_request` with an opaque `workspaceRootRef`
  and relative path. Never pass an absolute path or a credential value. Native
  Remember/Register/IssueLease cards block until the practitioner decides; do
  not ask them to visit Settings or type an approval.
- Multi-agent compatibility is pinned when the session starts. Do not claim a
  V1/V2 comparison without reporting the effective setting used by each arm.

This file is bundled and versioned with Synth Desktop. Workspace `AGENTS.md`
files remain the practitioner's editable overlay and are discovered by Codex
from the working directory.
