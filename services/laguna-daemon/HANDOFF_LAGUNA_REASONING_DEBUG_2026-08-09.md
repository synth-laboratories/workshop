# Laguna MLX reasoning/debug handoff — 2026-08-09

## Stop state

Work stopped at the user's request. Do not continue from assumptions: reproduce
the live matrix first.

- Repo: `/Users/joshuapurtell/Documents/GitHub/workshop`
- The worktree is heavily dirty with unrelated concurrent workstreams. Do not
  reset, stash, clean, checkout, or bulk-stage it.
- No commit was created.
- At the final process check, the installed desktop and managed daemon had been
  restarted by the environment/another workstream: desktop PID `77622`, daemon
  PID `77669`. Re-check; PIDs are ephemeral.
- The stock `mlx_lm.server` comparison process was stopped.
- The bad provisional "minimum 16 reasoning tokens" logits-processor idea was
  fully removed from source. Do not restore it. It was an unjustified decoding
  hack, not how Poolside, vLLM, SGLang, TRT-LLM, or mlx-lm implement reasoning.

## User-visible defect

With local Laguna selected and Thinking On, Codex-shaped turns (which advertise
tools on almost every request) show no live reasoning. Text-only requests do
stream reasoning. The renderer cannot display tokens the daemon never receives.

The real-weight matrix now lives in:

`services/laguna-daemon/tests/integration/test_live_mlx.py` →
`LiveReasoningMatrixTests.test_reasoning_effort_matrix_streams_what_the_model_was_asked_for`

Measured against the installed sidecar before the final handoff:

| Case | Reasoning frames | Reasoning chars | Terminal |
|---|---:|---:|---|
| none, no tools | 0 | 0 | incomplete |
| high, no tools | 267 | 1,585 | incomplete |
| legacy max alias, no tools | 285 | 1,587 | incomplete |
| high, tools advertised | 0 | 0 | completed |

The test intentionally fails on `high_with_tools`. It is a release-blocking,
real-weight regression test, not a hermetic unit test.

## What was proven

1. The screenshot turn did request reasoning. The Synth event journal recorded
   `reasoning_effort: "max"` / `effort: "max"` for session
   `05ab1d4a-724a-465a-ba0a-481200484ad4`.
2. The local UI's legacy `max` value did not match the generated Responses
   schema, which accepts `none/low/medium/high/xhigh`. The source now normalizes
   local legacy `max` to `high`, and the local model advertises only Off/High.
3. The prompt compiler is not flipping Thinking off. Rendering the exact local
   tokenizer template with tools produced:
   `...<assistant><think>` when enabled and `...<assistant></think>` when off.
4. The custom-tool grammar logits processor is not responsible. It activates
   only for `custom` tools with a Lark grammar and only after the model has
   already emitted that custom tool's raw-input marker. Ordinary Codex function
   and MCP declarations do not activate it.
5. The checkpoint/runtime emits `</think>` immediately whenever tools are
   present in the tested requests. Even a multiplication prompt with an
   irrelevant optional tool emitted zero reasoning, while the same prompt
   without tools emitted hundreds of reasoning SSE deltas.
6. Tool-bearing reasoning was also buffered by our backend. That source bug was
   fixed: reasoning now passes through `_IncrementalReasoningSplitter` live,
   while only the tool envelope remains buffered for validation/rehydration.
   This is necessary but cannot create reasoning the model did not emit.
7. A second real compiler bug was found: Responses history discarded every
   prior `reasoning` item. Source now attaches summary text as
   `reasoning_content` to the following assistant message/tool call. Poolside
   explicitly recommends preserving reasoning between tool steps.

## Scoped source changes from this debugging pass

- `services/laguna-daemon/laguna_daemon/responses_api/validation.py`
  - Normalize legacy local `reasoning.effort=max` to `high` before generated
    schema validation.
- `services/laguna-daemon/laguna_daemon/app.py`
  - Advertise local reasoning levels `none/high`, not `none/max`.
- `apps/synth_desktop/src/renderer/src/runtime/modelCapabilities.ts`
  - Local Laguna Thinking On now sends `high`; remote binary model settings are
    unchanged.
- `services/laguna-daemon/laguna_daemon/responses_api/backends/mlx.py`
  - Stream reasoning deltas on tool-bearing turns; buffer only answer/tool
    envelope fragments.
- `services/laguna-daemon/laguna_daemon/responses_api/compiler.py`
  - Preserve Responses reasoning summaries as assistant
    `reasoning_content` across tool boundaries.
- `services/laguna-daemon/tests/test_neutral_core.py`
  - Regression tests for max→high normalization and reasoning-history
    preservation.
- `services/laguna-daemon/tests/test_incremental_streaming.py`
  - Regression test proving tool-envelope buffering need not buffer reasoning.
- `services/laguna-daemon/tests/integration/test_live_mlx.py`
  - Real sidecar reasoning/tool/effort SSE matrix described above.

The provisional minimum-token processor and its tests were removed. The
focused suite was green before that removal; because the user ordered an
immediate stop, rerun the focused suite after taking over.

## Verification completed before stop

- Full daemon discovery before the later history patch: `159 passed, 25 skipped`.
- Focused neutral-core/streaming run before the rejected minimum-token idea:
  `28 passed`.
- Desktop TypeScript typecheck: passed.
- Desktop inference-panel tests: `20/20` passed.
- Canonical desktop install gate: Rust unit/protocol gates passed and Playwright
  `87/87` passed, including cold-turn `Warming up` UI coverage.
- Installed daemon `/v1/models` advertised `none/high` and default `high`.
- Installed daemon accepted legacy `max`; a text-only stream produced 172
  `response.reasoning_summary_text.delta` events.
- Live matrix above reproduced the remaining tools-present failure.

## Research and official implementation guidance

Read these before changing generation behavior:

1. [Poolside Laguna XS 2.1 model card](https://huggingface.co/poolside/Laguna-XS-2.1)
   - Canonical guidance: set chat-template `enable_thinking=true`, parse
     reasoning separately, preserve `reasoning_content` in assistant history,
     and return it on subsequent tool steps.
   - Says the model will *generally* reason before and between tool calls.
   - Reference sampler examples use `temperature=1.0`, `top_k=20`.
   - Gives official vLLM, SGLang, Transformers, and TRT-LLM recipes. vLLM uses
     `--tool-call-parser poolside_v1`, `--reasoning-parser poolside_v1`,
     `--enable-auto-tool-choice`, and default template kwargs enabling thinking.
     TRT-LLM uses `--tool_parser poolside_v1 --reasoning_parser laguna`.
2. [Poolside Laguna XS 2.1 NVFP4 card](https://huggingface.co/poolside/Laguna-XS-2.1-NVFP4)
   - States that NVFP4 uses the same interleaved-thinking, preserved-reasoning,
     and `enable_thinking` controls as the base checkpoint.
3. [Official Laguna XS 2.1 chat template](https://huggingface.co/poolside/Laguna-XS-2.1/blob/main/chat_template.jinja)
   - Generation starts with `<assistant><think>` when enabled and
     `<assistant></think>` when disabled. Local exact-weight template matched
     upstream byte-for-byte during this investigation.
4. [mlx-lm server implementation](https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/server.py)
   - Passes tools and request/server `chat_template_kwargs` directly into
     `tokenizer.apply_chat_template`; determines an initial `reasoning` state
     by inspecting the rendered prompt's last think-open/think-close markers;
     parses reasoning/tool output as a state machine. It does not force minimum
     reasoning tokens.
5. [mlx-lm HTTP server documentation](https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/SERVER.md)
   - Documents the stock OpenAI-like Chat server and warns it is not intended
     as a production server.
6. [mlx-lm releases](https://github.com/ml-explore/mlx-lm/releases)
   - Includes prior fixes for passing `tools` correctly into
     `apply_chat_template` (PR #747) and parallel tool-call handling. Useful
     history when comparing our lowering.
7. [mlx-lm Responses PR #1207](https://github.com/ml-explore/mlx-lm/pull/1207)
   - Implements Responses by translating it into the Chat pipeline. This is a
     useful compatibility reference but is intentionally not Workshop's
     architecture because lowering custom Responses tools to Chat is lossy.
8. [mlx-serve](https://github.com/ddalcu/mlx-serve)
   - Another Responses-on-Chat MLX implementation; useful for comparison, not
     a safe direct replacement for Workshop's neutral core.

Important A/B result: stock `mlx_lm.server` 0.31.3 could not load these exact
weights and failed with `ValueError: Model type laguna not supported`. The
Workshop daemon uses `mlx-vlm` 0.6.6, which does load them. Therefore no valid
same-machine stock-server generation comparison was completed; do not claim
that stock mlx-lm reproduces or fixes the zero-reasoning behavior.

## Recommended next investigation (not executed)

Do not add logits masks or forced token minima. The next owner should:

1. Re-run the focused suite after the final revert.
2. Add `top_k` to the neutral sampling contract and MLX sampler, then run the
   matrix with Poolside's exact recommended `temperature=1.0/top_k=20` versus
   current defaults. Do not assume it fixes the defect; measure it.
3. Compare raw token IDs/logits for the first generated token with and without
   tools using the same `mlx-vlm` loader. Confirm whether `</think>` is actually
   the sampled token and capture its probability/rank.
4. Compare our fully rendered prompt—not just its suffix—to Poolside's exact
   model-card Chat example. In particular audit the extra compiler-inserted
   system directives (`only callable tool names`, `tool_choice required`) and
   run an A/B without them in an isolated test harness.
5. Add a multi-step real-weight test proving preserved reasoning survives a
   tool call/output continuation.
6. Only after those A/Bs choose between a prompt construction fix, sampler fix,
   parser/state-machine fix, or reporting zero emitted reasoning honestly as an
   adaptive model decision.

## Operational traps

- The exact model is at
  `~/.config/poolside/models/poolside/Laguna-XS-2.1-NVFP4-mlx`.
- Managed API is `http://127.0.0.1:7333`; auth key is read from
  `~/.synth-desktop/laguna/api_key`. Never print the key.
- The installed daemon imports current repo source through `PYTHONPATH`, but a
  live process retains old Python code until restarted.
- Manage the canonical app through `./scripts/desktop.sh`; concurrent
  workstreams may restart it.
- Do not trust process RSS for MLX/Metal residency; use
  `mx.get_active_memory()`.

