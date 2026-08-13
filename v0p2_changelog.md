# Workshop v0.2

## New

- Added ChatGPT subscription (Codex OAuth) support in Models, including Codex model selection after connecting.
- Separated ChatGPT plan allowance from Synth Cloud and API-key provider usage.
- Added local Mermaid rendering through the pinned Grok renderer, with support for flowcharts, sequence, state, class, ER, C4, and additional Mermaid families.
- Added deliberately composed 2D systems maps for architecture, ownership, before/after, and missing-edge views.
- Added Benjamin Dicken Style dynamic explainers with storyboard beats, interpolated motion, scrubbing, reduced motion, and deterministic SVG stills.

## Improved

- Permission choices now persist as machine-level Synth defaults and restored chats no longer silently revert them.
- Added per-chat sent-message history: Up and Down traverse sent prompts and restore the current draft.
- Mermaid diagrams now fit the active pane by default and include compact zoom, fit, source, copy, retry, and SVG export controls.
- Improved diagram typography, node spacing, edge labels, colors, and lifecycle layouts for compact desktop panes.
- Updated the diagram-authoring skill to use the visual-management tools directly and create real Mermaid content before showing it.
- Updated diagram authoring to choose among UML, static 2D systems maps, and Benjamin Dicken Style explainers; dynamic scenes are delegated to a bounded subagent when available.

## Fixed

- Sequence diagrams now render multiline labels instead of displaying literal break markup.
- Wide diagrams no longer initially render clipped outside the visible pane.
- Named development instances can use an explicit read-only Codex auth file without creating or opening a Keychain credential prompt.
- Mermaid SVG registry validation now accepts XML declarations emitted by the renderer.
- C4 diagrams now emit safe SVG links.
