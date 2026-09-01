# Single authority for the embedded-agent MCP adapter binaries that ship
# beside the desktop executable. Must stay in sync with the [[bin]] targets
# in apps/synth_desktop/src-tauri/Cargo.toml; scripts source this file so the
# build, bundle-copy, and signing loops cannot drift apart.
# shellcheck shell=bash
SYNTH_MCP_ADAPTERS=(
  synth-containers-mcp
  synth-visuals-mcp
  synth-optimizers-mcp
  synth-plugins-mcp
  synth-display-mcp
  synth-computer-use-mcp
  synth-browser-mcp
  synth-session-mcp
  synth-traces-mcp
  synth-annotations-mcp
  synth-diagnostics-mcp
)
