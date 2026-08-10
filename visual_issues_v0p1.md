# Synth Workshop settings: visual issues v0.1

## Scope

Visual review of the Synth Workshop General settings page against the Poolside and Codex/ChatGPT settings experiences. This is primarily a layout, hierarchy, density, control, consistency, and accessibility audit rather than a feature audit.

The shared rules proposed by this audit are captured in [Synth Workshop visual style guide v0.1](./visual_style_guide_v0p1.md).

## Highest-priority fixes

### P0 — Remove the nested app and settings sidebars

Synth leaves the conversation sidebar, model/status card, and account controls visible while adding a second settings-navigation sidebar. This consumes a large part of the window, compresses the form, and keeps irrelevant information competing with the task.

Replace the normal app sidebar with settings navigation, as Poolside does, or open settings in a dedicated shell/window, as Codex does.

### P0 — Fix the responsive layout

At ordinary window widths, the three-column font row becomes cramped and the font-family value is clipped. On a wide window, the same layout stretches controls and choice cards too far.

Use a centered content column with an approximately 880–960px maximum width. Use three columns only when there is sufficient content width, two columns at medium widths, and one column when narrow.

### P0 — Redesign selected choice states

Selected options depend on a pale peach fill and orange outline. This can resemble a warning or validation error, and selection is communicated by color alone.

Add an explicit radio/check indicator and define consistent selected, hover, focus, and disabled states. Use a quieter accent treatment that remains distinct without reading as an error.

### P0 — Replace the raw font-stack field

The Code font family field exposes a raw CSS stack and truncates it. It reads as an implementation detail rather than a user-facing setting.

Use a dropdown with friendly values such as System Mono, SF Mono, Menlo, Monaco, or Consolas. Show the selected family without exposing the fallback stack.

### P0 — Simplify the composer model picker

The current model menu renders each choice as a multi-line metadata block. Model name, source, runtime, framework, usage tracking, modality, and context length compete for attention; values run together; duplicate model names are difficult to distinguish; and the menu becomes tall enough to collide with the composer and viewport.

Follow the [model and capability picker pattern](./visual_style_guide_v0p1.md#model-and-capability-pickers): keep the closed control and default model list minimal, move technical data into Advanced or details for the highlighted/selected model, use one consistent row height, and keep selection in a fixed checkmark column.

## Layout and hierarchy

- Remove redundant chrome. The current experience shows a Settings tab, Back control, Settings heading, General navigation item, and General page heading together. Prefer a single compact `Settings > General` header.
- Hide the normal app model/status card and conversation navigation while settings are open. They are visually prominent but irrelevant to the task.
- Give the content a stable readable measure instead of allowing it to alternate between overcrowded and excessively wide.
- Reduce the size of Prompt submission and Tool activity choice cards. Simple radio choices should not span nearly the entire window.
- Present Tool activity as a compact radio list, compact cards, or a three-column option group when space permits.
- Strengthen the distinction between page headings, section headings, field labels, option titles, and helper text.
- Use consistent section containers or dividers. The current page relies too heavily on whitespace and font weight for grouping.
- Reduce the amount of scrolling created by oversized early controls so Agent context, Layout, Keyboard shortcuts, Archived chats, and Reset are easier to discover.

## Controls

- Size controls according to their contents. Two-digit font-size fields should be approximately 88–110px wide rather than several hundred pixels.
- Give long-value controls, such as font-family selectors, enough width to display their selected value.
- Replace or style native number steppers so they match the rest of the interface.
- Avoid a font grid that leaves Terminal font size alone on a second row with a large empty area.
- Redesign the System/Light/Dark selector. It currently looks like three loose text links, and the selected rectangle does not match the other control shapes.
- Give radio-card selections an explicit indicator so state remains visible without color.
- Style keyboard shortcuts such as `Command + Enter` as recognizable keycaps or otherwise separate them from prose.

## Consistency and polish

- Establish one selected-state language across settings navigation, segmented controls, radio cards, and other choices.
- Normalize corner radii across navigation rows, inputs, choice cards, section containers, and the window shell.
- Normalize border weights and colors. Hairline borders and secondary text are currently very faint against the near-white background.
- Increase helper-text contrast and verify small text against WCAG contrast requirements.
- Avoid making the selected General navigation item look like an unrelated floating white card.
- Keep spacing between headings, descriptions, controls, and sections on a consistent rhythm.

## Reference takeaways

### Poolside patterns worth borrowing

- Replaces the normal application sidebar with settings navigation.
- Divides settings into compact, clearly bordered sections.
- Sizes controls according to their data.
- Presents font families as friendly dropdown values.
- Gives theme options recognizable icons and radio semantics.

Do not copy Poolside verbatim: much of its text is too small and washed out, the page can become over-carded, and the app-icon grid dominates the visible area.

### Codex patterns worth borrowing

- Uses a consistent row pattern with labels and explanations on the left and controls on the right.
- Constrains content to a readable maximum width.
- Uses rows and dividers to make dense settings easy to scan.
- Keeps the settings experience visually separate from the main conversation interface.

Do not copy Codex verbatim: it wastes substantial vertical space, has a dense sidebar, and the reviewed screen showed two sidebar rows appearing highlighted at once.

## Target direction

The v0.1 target should have one settings sidebar, one compact page header, a centered maximum-width content column, responsive field grids, consistent section grouping, appropriately sized controls, and explicit selection indicators.
