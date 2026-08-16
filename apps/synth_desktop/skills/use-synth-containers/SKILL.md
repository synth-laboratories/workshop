---
name: use-synth-containers
description: Use for Synth container discovery, real workspace-owned rollout harnesses, Trace V5 evidence, or container-backed eval visuals.
---

# Use Synth containers

Use `synth-containers-mcp` as the registry authority. Never scan ports or invent
container records, endpoints, results, model metadata, token usage, or rewards.

Codex exposes this MCP server as the `mcp__synth_containers` namespace. Call its
named operations directly; do not use shell or scan ports as a fallback.

## Discover the engine

1. Call `container_list` in the `mcp__synth_containers` namespace.
2. Select a registered container by task family and by its typed
   `metadata.capabilities`, not by a guessed name or port.
3. Refresh it with `container_probe` and read it with `container_get`.
4. Treat `/health`, `/info`, or `/metadata` as engine discovery only. A ready
   engine is not a policy and proves no model was in the loop.
5. Register a container only when the user or workspace gives an explicit URL.
   Use `container_register`; never infer a localhost port.

## Select by capability, not by liveness

Select only a currently healthy container advertising the normalized
prepared-rollout protocol and the exact requested policy ref. If none exists,
stop and report `compatible_runtime_unavailable`. Do not try raw engines,
alternate ports, archived rollouts, or prior traces. Evidence must match the
current invocation's rollout IDs and requested seeds. Missing sealed Trace V5
means the requested task is incomplete.

`metadata.capabilities` is the authority. It carries `protocol`, tri-state
`operations` (`supported` / `unsupported` / `unknown`), advertised
`policy_refs`, the capability `source`, and `observed_at`. Unknown is not
supported, and it is never inferred:

- `health` proves liveness, not workflow compatibility.
- SSE support does not imply prepared-rollout support. A raw Gold `/info`
  advertising `rollout_stream_sse` supports no normalized operation.
- Never fall back from a selected policy pool to raw Gold, to another port, or
  to a different policy config.
- After a preflight failure, do not perform shell or repository archaeology as
  a substitute for execution. Report the failure and its remediation.
- Prior evidence may be reported as prior evidence only. It cannot satisfy a
  new live request, whatever its seeds or rollout IDs.

`container_prepare_rollout` refuses locally, before any request reaches the
container, and returns a failed tool call with a stable code:

- `container_unhealthy` — start or repair that registered pool, then
  `container_probe`. Retryable.
- `container_capabilities_stale` — call `container_probe` first. Retryable.
- `container_capability_mismatch` — this record does not advertise the
  operations or the requested `policy_ref`. Not retryable: read `missing` and
  `available_policy_refs`, then select a compatible registered target or report
  `compatible_runtime_unavailable`.

Pass `require_trace_v5: true` when the request promises sealed Trace V5
evidence; preflight then also requires an explicitly advertised
`trace_v5.capture` rather than assuming SSE implies capture.

For a bounded **engine acceptance** run (fixed actions, not a model eval), call
`container_run_rollouts` once with the registered `container_id`, an exact
`count`, optional integer `seeds`, and 1-64 **explicit** action names. The host
does not invent an action list. Do not call it again when the requested count
has already completed. Do not report this as ReAct or LLM policy evidence.

For a live policy eval, follow `run-live-container-evals`: prepare, bind the
declared stream on a task-family visual, then `container_start_prepared_rollout`
with an explicit `policy_ref` (`harness` + `config`). The coding agent names
the pin; the host does not default `luna_med`.

Use only the normalized Containers contract: plural rollout routes,
`snake_case` wire fields, and descriptor-nested transport URLs. Never consume
flat `poll_url`/`sse_url`, singular `/rollout`, native `event_log`, or a guessed
benchmark route. Native and Harbor-specific APIs are translated inside the
registered Containers compatibility fold.

Treat a timeout as an unknown transport outcome. Reuse the original
`rollout_id`; never allocate a replacement. Call `container_get_rollout` to
restore authoritative lifecycle state and `container_poll_rollout` with the
last durable `after` cursor to backfill. Repeating prepare or start is permitted
only with the exact same immutable identity. A `409` means the retry changed
identity and must not be bypassed.

## Locate the policy harness

Search the allowed workspace and nearby checked-out benchmark repositories with
`rg`/`rg --files` for the discovered task family, engine route names, rollout
client, run configs, event-stream schema, and Trace V5 writer. Read repository
instructions and the runner's `--help` before executing it.

Prefer an existing benchmark-owned harness and checked-in run configuration.
Pass the registered container URL through the harness's documented option or
configuration. Do not copy policy logic into Workshop, its MCP adapters, or the
skill. If no real policy harness exists, report that boundary. Implement one in
the benchmark/evals project only when the user asks to build it.

For GameBench Craftax, a nearby `evals` checkout may provide the Python module
`suites.nonproduct.craftax`. Discover its available TOML configurations rather
than assuming a particular checkout path or config name. Use its ReAct policy
mode for LLM rollouts; its uniform policy is a transport baseline, not an LLM
evaluation.

## Verify that a rollout is real

Before presenting a result as an LLM policy rollout, verify the emitted evidence
identifies the model, provider/route, policy kind, seed, limits, and actual model
calls. Preserve reported token usage and cost provenance. `unavailable` is not
zero.

Reject fixed action lists, random/uniform baselines, fixtures, or direct engine
stepping as evidence of model capability. The model must choose the actions or
committed action plans from observations during the measured rollout.

Use a bounded smoke configuration first. State the maximum calls, time, and
spend before starting a paid run unless the user already authorized that exact
run. Never silently substitute a cheaper policy or a shorter scripted loop.

## Preserve and visualize evidence

Every authoritative rollout must finish as a sealed Trace V5 artifact. Keep the
harness's native event stream, result summary, and durable trace store as source
and debugging evidence, but do not treat any of them as the visual's stable
identity. Validate the sealed artifact with the trace CLI provided by the
harness when available and cite its trace ID and semantic digest.

Use the registered Synth visual tools only after inspecting their available
templates. For completed runs, bind the trusted Trace V5 identity. A viewer may
derive a versioned presentation packet from the sealed V5 on demand; that is a
consumer concern and must not become a producer requirement or benchmark policy
code in Workshop. For a still-running view, bind the real emitted SSE and replace
it with the sealed V5 identity at completion. Do not reconstruct rewards or
metadata from labels. Useful existing templates may include live eval streams,
model comparison, reward breakdown, Craftax rollout scrubbing, and Trace V5
inspection.

Report the exact command/config used, container ID and endpoint, model/policy,
seeds, limits, result location, trace identity, and any unavailable fields. A
cached fixture, opened visual, or healthy engine is not proof of execution.
