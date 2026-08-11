# Synth visual style guide

Use this guide for every user-facing Synth visual. Optimize for the Desktop side pane first, then allow the same composition to expand gracefully.

## Design intent

Make the answer obvious before making the evidence exhaustive. A good Synth visual should feel like an edited analytical story, not a database dump or a generic dashboard.

Borrow the information hierarchy of the public Craftax eval page:

1. establish the environment and objective;
2. name one analytical question;
3. show one dominant visual answer;
4. provide controls and supporting evidence quietly;
5. offer dense detail only when the data genuinely has matrix or trace structure.

Do not copy the page's dark theme mechanically. Preserve Desktop colors, contrast, typography, and narrow-pane behavior.

## First pane-height contract

The first visible pane-height should contain:

- a short kicker naming task, cohort, and sample scope;
- a title that states the comparison;
- a one- or two-sentence conclusion;
- one dominant chart, diagram, frame, or trajectory surface;
- at most three supporting metrics needed to read that visual;
- a compact caveat or provenance line when necessary.

Move methodology, exhaustive IDs, filenames, secondary tables, and long caveats below the fold. Never make users scroll past setup prose to reach the answer.

## Hierarchy and layout

- Use one dominant question per screenful. Start a new section when the question changes.
- Give the primary visual at least twice the area or visual weight of any supporting card group.
- Use metric cards as annotations around a conclusion, not as the whole composition.
- Prefer a 1-column flow below roughly 520 px. Use two columns only when each remains readable without horizontal scrolling.
- Keep outer padding consistent. Use larger gaps between sections than between items within a section.
- Align numbers by baseline or decimal where possible. Keep labels close to their marks.
- Let quiet whitespace separate ideas, but remove empty panels and ornamental voids.

## Typography

- Use sentence case for section titles and labels.
- Keep the title short enough to wrap to no more than two lines in the side pane.
- Use a clear size ladder: title, section title, body, label/metadata. Do not create many nearly identical text sizes.
- Use uppercase sparingly for kickers and tiny categorical labels; add letter spacing only at those sizes.
- Use tabular numerals for aligned metrics and tables when available.
- Keep body copy compact: normally 45–80 characters per line and no more than three lines above the primary visual.
- Avoid raw field names when a human label exists.

## Color and emphasis

- Begin with one accent color and one neutral comparison color.
- Assign a stable color to each arm, model, or capability family and reuse it everywhere.
- Reserve semantic colors for meaning: success, caution, failure, selection, or signed delta.
- Use saturation and brightness to create hierarchy; do not give every series equal intensity.
- Keep gridlines, borders, and inactive controls quiet.
- Verify that text and marks remain distinguishable without relying on color alone.
- Use a heatmap palette only for a real matrix. Include values or a readable scale and preserve meaningful zeros.

## Chart selection for beauty and speed

Prefer the simplest encoding that makes the important relationship visible:

- paired seeds: dumbbell, slope, or signed-delta plot; use a table as secondary audit detail;
- repeated independent outcomes: dot plot, interval, or compact distribution;
- cost versus performance: scatter with direct labels and a computed frontier only when warranted;
- achievement ladder: ordered frequency bars for a few rows, heatmap for many arms × many achievements;
- ordered reward: step plot, not a smoothed line;
- trace or rollout: anchored environment frame + event/decision rail + selected detail;
- one exact record: annotated card or timeline, not a chart invented from one value.

Avoid chart decoration that carries no data. Do not use faux 3D, gradients behind text, thick borders, shadows on every card, or large legends that overpower the plot.

## Canonical compositions

### Repeated-seed comparison

Use this order:

1. conclusion and sample scope;
2. one paired visual by seed;
3. two or three supporting metrics such as mean reward, steps, and invalid actions;
4. achievement deltas sorted by magnitude or progression order;
5. short causal or diagnostic note;
6. compact methodology and exact provenance.

Example first pane:

```text
CRAFTAX · SEEDS 2001–2005 · N=5/ARM
Low won all five matched seeds
Mean reward 1.60 vs 0.40; xhigh stalled after token-budget exhaustion.

[paired reward dumbbell plot — dominant]

Δ reward +1.20     steps 36.0 vs 4.8     invalid 1 vs 19
```

Do not lead with four same-sized cards followed by two tables. The paired result is the story; cards and tables support it.

### Cost versus performance

Follow the Craftax reference pattern:

1. state both axes and the aggregation in plain language;
2. provide one compact control row for metric or capability family;
3. keep the scatter dominant;
4. use direct labels for selected or important points;
5. distinguish effort or cohort with size, stroke, or a secondary channel;
6. compute and label a frontier only from the plotted observations.

### Achievement coverage

For fewer than roughly twelve important achievements, use sorted horizontal bars or a signed-delta view. For a genuine many-arm × many-achievement matrix, use a heatmap ordered by Craftax progression and grouped by capability family. Keep the model label, sample count, reasoning effort, and aggregate score anchored at the row edge.

### Rollout or trace inspector

Use a coordinated three-part composition inspired by the Craftax trajectory view:

- environment: current frame, vitals, inventory, and step position;
- trajectory: calls or events as a compact navigable rail;
- decision: selected reasoning, actions, tool activity, outcome, and evidence.

Keep the selected state synchronized. Make failures, truncation, and reward changes visible in the rail. Do not flatten a trace into prose or a generic event table when the hierarchy matters.

### Live eval

Lead with run state and trustworthy progress. Follow with lane status, current outcome signal, and recent anomalies. Keep the final state frozen after completion. Avoid animations or constantly rescaled axes that make comparison difficult.

## Tables and provenance

- Use tables for exact lookup, auditability, and fields with mixed units.
- Keep the primary table to the columns needed for the question; move secondary fields to details.
- Right-align numeric columns and use consistent precision.
- Make row ordering intentional and state it when not obvious.
- Show a short provenance line near the result: source, `n`, seeds, and aggregation.
- Put full paths, rollout IDs, trial IDs, and field-level sourcing in the final block or expandable detail.

## Controls and interaction

- Keep controls visually subordinate to the result.
- Group related filters into one compact row and show the active state clearly.
- Default to the most decision-relevant view, not an empty or aggregate-only state.
- Preserve selection when switching metrics where possible.
- Never require hover to understand the key result. Hover may add detail, not identity or conclusion.

## Visual review checklist

Inspect the rendered Desktop pane and ask:

- Can someone state the conclusion after three seconds?
- Is there one dominant visual, or do equal cards compete for attention?
- Does the primary visual appear without scrolling?
- Are labels readable at the actual pane width?
- Did any title, metric, or table cell wrap awkwardly?
- Are arm colors consistent across every block?
- Is dense encoding justified by dense data?
- Are caveats close to the claim and provenance out of the way?
- Would removing any card, border, legend, or paragraph make the answer clearer?

Revise until each answer is satisfactory, then call `show` again and verify the pane state.
