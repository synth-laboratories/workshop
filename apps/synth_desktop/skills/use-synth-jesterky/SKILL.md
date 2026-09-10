---
name: use-synth-jesterky
description: Prepare optional Jesterky analysis of long or multi-agent traces, inspect every shard, and review findings with exact evidence and visuals.
---

Use the optional Jesterky plugin for bulk trace analysis. `plugin_manage` with
`plugin_id: jesterky` reports status, installs the catalog runtime, and exposes
capabilities. Never invent download URLs or substitute a PATH executable.

1. Use `trace_manage` with a `synth.trace-query.v2` query and existing `evalJobIds`.
   Query rewards/configuration without requiring annotations or Jesterky.
2. Page the saved snapshot. `jesterky_prepare` selects exact result IDs from the
   immutable snapshot and returns deduplicated trace/container targets, default
   model `gpt-5.6-luna`, effort `low`, and references to the original results.
   Preparation is free and does not start model work.
   This works on existing rollouts from multiple eval jobs; an annotated eval
   job is never required and the evaluated policies are not rerun. Read the
   saved scope with `jesterky_settings`, or override `annotation_scope` for one
   preparation: `selected_rollouts` (default) analyzes each selected trace in
   full; `selected_evidence` requires event results and limits analysis to them.
   Copy each event target's returned `metadata.jesterky_event_ids` into its
   annotation request. Never silently expand selected-event scope.
   For whole rollouts, `campaignSelections` contains explicit sealed trace refs
   grouped by container. Add compatible Jesterky annotators and bounded limits,
   then use `annotation_campaign` to estimate and start each batch. Do not add
   or inherit the active eval's `run_id` or annotation protocol.
3. For each target use `annotation_manage` to list compatible definitions, estimate,
   then start the selected Jesterky-backed annotator. The target container must
   advertise that runner; installing the host plugin does not install software
   in remote containers. Never silently fall back to a different paid runner.
   Keep the evaluated model separate from the analysis model.
4. Use the existing annotation reservation/approval flow. State the aggregate
   cap once; respect the provider proxy's enforced limit. A CLI token budget is
   telemetry, not a substitute for pre-request paid-compute enforcement.
5. Poll jobs/events. Preserve failure, abstention, uninspected, and unavailable
   states. Retrieve all shard outputs, not only the first successful worker.
   Retries retain completed shard receipts. Overlapping claims may deduplicate;
   differing claims remain reviewable. Findings never overwrite rewards.
6. Refresh the query to include new annotation/review revisions. Keep old
   snapshots reproducible. Optional trace catalog and rollout-inspector visuals
   consume the same trace digests and selectors; use the visuals tools to open
   them. No visualization is required to finish an analysis.

For local workflow development, the installed executable is returned by plugin
capabilities. Run only explicitly scoped workflow inputs. Credentials come from
an authorized project environment or Workshop secrets proxy, never Keychain.
