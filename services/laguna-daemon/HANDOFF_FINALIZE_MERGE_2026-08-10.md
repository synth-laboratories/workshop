# Handoff: finalize, merge, and configure the Laguna MLX lane

Date: 2026-08-10 (~00:30). Everything below is UNCOMMITTED, spread across two
dirty shared worktrees (`workshop`, `evals`) with other live workstreams.
Read `HANDOFF_LAGUNA_FIRST_CLASS.md` for architecture,
`HANDOFF_LAGUNA_REASONING_DEBUG_2026-08-09.md` for the prior debugging pass.
This session's durable findings are also in the assistant memory file
`laguna-daemon-refactor-and-throughput`.

## What this session changed (= what you are merging)

### workshop/services/laguna-daemon — refactor + control plane
- **Reasoning-with-tools fix (the release blocker).** Root cause measured on
  raw weights: P(`</think>` as first sampled token) is 0.80 when the terse
  tool-allowlist directive displaces the caller's system prompt in the
  template header (old `messages.insert(0, …)` behavior), 0.07–0.12 with a
  substantive system prompt + directive appended. Fix: directives merge into
  the caller's system message (`responses_api/compiler.py`). With a real
  system prompt, tool turns reason MORE than text-only (0.07 vs 0.27).
- **`_TurnStateMachine`** (`backends/mlx.py`) replaced the splitter +
  end-of-turn regex: reasoning/answer/tool states, marker holdback, live
  answer streaming on tool turns, `<think>` re-entry, truncated envelopes
  discarded (never leak markup), stop-grace computed on non-reasoning text,
  sibling calls survive via holdback. `finish_reason` rewrites to `tool_call`
  whenever calls were dispatched.
- **Contract honesty:** `reasoning_tokens` counted (re-encoded, clamped ≤
  output_tokens), never chars/4; `low/medium/xhigh/minimal` → typed
  `unsupported_reasoning_effort`; bare Responses request = thinking ON
  (matches Chat + advertised default); structured turns render thinking-closed
  (grammar can't close a think span); `top_k` plumbed end-to-end both
  surfaces; schema-driven tool-arg typing (declared strings verbatim — vLLM
  #47311 lesson); Chat round-trips inbound `reasoning_content`/`reasoning`
  (Poolside preserved-thinking contract); alias resolver returns plain `cmd`
  strings (no double JSON).
- **/v1/synth control plane** (`synth_control.py`, `settings.py`,
  `openapi/synth-sidecar.yaml`, served at `/v1/synth/openapi.json`): status
  (14-state canonical machine), capabilities, models, download (real HF
  downloader + 24GiB preflight, injectable for tests), load/unload, metrics
  (JSON + Prometheus additions incl. prefill-length histogram: buckets
  1k/5k/10k/25k/50k/150k/>150k with cache share + prefill tps), SSE events
  with operation_ids, typed error envelope with request_id. Legacy
  `/v1/synth/inference*`, `/v1/synth/model/unload`, `/metrics` unchanged.
- **Settings:** `~/.synth-desktop/laguna/settings.toml` (unknown keys fail
  startup, naming the key) + `GET/PUT /v1/synth/settings`. Precedence:
  explicit request > settings > built-in. Runtime-mutable: sampling defaults
  (temperature/top_p/top_k), default_reasoning_effort, default/max output
  tokens, idle_unload_after_seconds (TOML/PUT wins over the legacy env var),
  prompt_cache_slots, queue_capacity.
- Tests: `tests/integration/test_live_mlx.py` gained the refactor driver
  contracts (answer streaming with tools, chat reasoning round trip, usage
  invariant, typed efforts, thinking-on default, model-card sampling) and the
  reasoning matrix now sends `instructions` + bounded retries (a bare-header
  single sample is a coin flip BY DESIGN — see the comment in the test).
  `tests/contract/test_sidecar_api.py` (37), `tests/integration/
  test_sidecar_lifecycle.py` (read-only live), `tests/performance/
  benchmark_mlx.py` (standalone). One intent-preserving edit:
  `test_no_second_runtime.py` now asserts `/v1/synth/status` is 200 with no
  process-manager vocabulary (it used to assert 404 for the legacy route).

### workshop/apps/synth_desktop — Settings → Inference
- `components/InferenceSettings.tsx` (new), `SettingsPage.tsx` (Inference
  section after Models), `InferencePanel.tsx` (gear → deep-link
  `{kind:"settings", section:"inference"}`), `App.tsx` (view union + prop),
  `app.css`; Rust: `laguna.rs` settings snapshot/update commands + `lib.rs`
  registration. Feature-detects 404 → quiet "not supported" card.
  `tests/inference_settings.test.mjs` 14/14; cargo lib 163 green.

### evals — Craftax throughput lane
- `core/models.py`: `laguna` provider (127.0.0.1:7333, SYNTH_LAGUNA_API_KEY)
  + `_normalise_laguna`; `core/model_cards/laguna.json` snapshot;
  `suites/nonproduct/craftax/harness.py`: laguna request branch (sends ONLY
  reasoning_effort/temperature/top_p/top_k/max_completion_tokens — the daemon
  fail-closes on penalties/chat_template_kwargs), shared local GPU gate,
  `laguna_local_committed_plan` metadata id;
  `runs/laguna_xs_thinking_10lane_bench.toml`.

## Verification state

Green as of handoff: deterministic 223 (39 env-gated skips); live suite incl.
new driver contracts; codex e2e 5/5; `check_schemas.py` 4 pins; renderer
14/14 + inference_panel; cargo 163. Bench artifacts + prefill sweep results
in the session scratchpad (`tput_summary.json`, `prefill_sweep_results.json`)
— copy numbers into docs before the tmp dir is reaped (macOS wipes /private/tmp
at reboot).

NOT yet done (your finalize sequence, in order):
1. **Sweep complete** — full curve in `prefill_sweep_results.json` (numbers
   below). Vendor `prefill_sweep.py` + results from the scratchpad into
   tests/performance/ before /private/tmp is reaped.
2. **Restart the daemon** on current source (recipe in
   HANDOFF_LAGUNA_FIRST_CLASS.md §Verification; kill only the pid on :7333,
   never Poolside's :63300). Then live-smoke the control plane:
   `GET /v1/synth/status`, `GET/PUT /v1/synth/settings`, `GET /v1/synth/
   metrics` (prefill_histogram present), `GET /v1/synth/openapi.json`, and
   run `tests/integration/test_sidecar_lifecycle.py` +
   `tests/integration/test_live_mlx.py` against it.
3. **OpenResponses compliance (17 scenarios)** — NOT rerun since the
   streaming refactor. Clone pinned at /tmp/openresponses cd31bc2 (recreate
   if reaped); needs its own daemon on :7340.
4. **Desktop gates**: `npm run typecheck` currently red on ONE error owned by
   the concurrent App.tsx rewrite (`asyncSession` unused, App.tsx:888, their
   23:15 edit) — coordinate, don't fix over them. Then node --test full,
   Playwright, and a CUA pass on the gear → Inference section flow.
5. **Landing/merge**: this workstream's files are exactly those listed above —
   do not sweep in the concurrent renderer/optimizers/visuals/containers work.
   Suggested commits: (a) daemon refactor, (b) control plane + settings +
   openapi, (c) desktop Inference settings, (d) evals laguna lane. `work/`
   stays untracked. evals repo also has ~20 pre-existing dirty files that are
   NOT ours (see suites/nonproduct/craftax/HANDOFF.md §0).

## Recommended initial settings.toml (calibration-backed)

```toml
default_temperature = 1.0   # Poolside generation_config
default_top_p = 1.0
default_top_k = 20
default_reasoning_effort = "high"
idle_unload_after_seconds = 900
prompt_cache_slots = 4      # >= expected concurrent Codex threads; 2 thrashed
queue_capacity = 9
```

Rationale: workload mining of 733 real Codex session logs (1.63M calls) —
92% of calls are ≥50k prompt tokens (46% >150k), aggregate cache hit 98.4%.
Measured sweep (complete): cold prefill 1,932→739→386 tok/s at 5k→57k→173k;
cold TTFT at 173k tokens = 449 s (7.5 min); warm TTFT 0.6 s at 57k and 2.8 s
at 173k (135–162× speedup); no memory admission failure at 173k context
(weights + KV fit on this 64 GB machine). Local coding is a cache
product: one cold prefill per thread is fine; a cache miss on a warm turn is
fatal. Sustained saturated output is 26-27 tok/s (~1.6k output TPM) at ANY
concurrency — one GPU slot serializes; extra lanes only buy queue-tail pain
(TTFT p95 minutes). Client timeouts must be ≥ queue_depth × call_time.

## Traps
- Editable install: a live daemon keeps old code; every daemon change needs a
  restart. The Desktop supervisor did NOT auto-respawn when we killed :7333 —
  start manually per the first-class handoff recipe.
- Poolside's sidecar on :63300 is not ours: no reasoning on its wire, accepts
  every field silently, returns prose beside tool calls. We are a deliberate
  superset + fail-closed. Never kill/reuse it.
- Craftax effort-high on a 3.2k obs often reasons to ANY cap without emitting
  ACTIONS — model/policy limitation, not transport. Don't "fix" it in the
  daemon.
- The live reasoning matrix needs instructions + retries; a bare-header
  sample is a coin flip (P measured above). Don't revert that test shape.
