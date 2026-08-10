# Muse Glimmer local inference — indefinitely paused (2026-08-10)

Decision: local Muse Glimmer 30B is **paused indefinitely** as a Synth Desktop
model. The throughput investigation on this branch (`muse-investigation`)
concluded that dense-28B economics on Apple Silicon make it not worth carrying,
and no configuration change closes the gap. The local model weights and the
managed llama.cpp runtime have been removed from this machine; hosted Muse (if
any) is unaffected.

## Why

All numbers below are engine-measured (`timings` counters, llama.cpp
`dd1ea524`, M5 Pro 64 GB, production sampling temp 1.0 / top-p 0.95 / top-k 64,
Meta's own recommendation). Full artifacts in `scripts/muse/results/*.json`;
harness in `scripts/muse/bench.py`.

- **Prefill ~270–303 tok/s uncached** over a 21K-token prompt (~70 s wall to
  first token) — and this is roughly what dense-28B-Q4 arithmetic predicts for
  this GPU. Config-insensitive across every lever swept (draft on/off, ubatch,
  batch). The historical "166 tok/s" report was wall-clock inflated by a
  daemon-side `/slots` polling pathology (fixed on this branch) plus thermal
  throttling (~25% for minutes after any long prefill).
- **Decode 13.5 tok/s** plain — the bandwidth bound of reading ~17 GB of
  weights per token. A ~30B MoE (Laguna XS) decodes ~2× faster and prefills
  several × faster on the same silicon at a fraction of the residency.
- **DFlash speculation makes it worse here, at every setting tried**:
  draft-max 3 → 9.5–13.0 tok/s (35–50% acceptance), draft-max 7 → 8.8 (23%),
  draft-max 15 → 4.7 (10.5%). Two compounding causes, both verified in source
  and counters:
  1. Muse's sliding-window layers make llama.cpp's target cache
     non-decomposable, so every speculative round pays a partial-KV checkpoint
     save (~40 MB at 21K ctx) and every partial acceptance pays restore +
     replay. Upstream's headline DFlash numbers come from paths without this
     tax.
  2. Production sampling is temp 1.0; llama.cpp verifies drafts by
     sample-and-match, so acceptance is capped by the model's own per-token
     entropy. Published DFlash speedups (up to 8×) are measured greedy
     (`--temp 0 --top-k 1`).
  `--swa-full` (untested — investigation stopped here) would remove tax (1)
  but cannot touch cap (2).
- **What actually worked**: prompt-prefix caching is genuine and excellent —
  identical prompt reuses 21,057/21,058 tokens (TTFT 0.28 s) and a multi-turn
  follow-up with a reasoning trace reuses 21,309 tokens with 24 new (TTFT
  1.5 s). Peak engine RSS 22.7–23.3 GB.

## What this branch keeps

- `scripts/muse/bench.py` + `scripts/muse/README.md` — the guarded long-context
  benchmark (memory admission, watchdog, thermal cool-downs, engine-measured
  JSON artifacts). Reusable for any future local model.
- `scripts/muse/results/` — all measured runs.
- Laguna daemon telemetry fixes in `services/laguna-daemon/` — the llama.cpp
  backend now reads engine-measured prefill/decode rates, cache reuse, and
  draft counters from the generation stream (`return_progress` + `timings`)
  instead of the starving `/slots` poll. These are backend-generic and worth
  landing regardless of Muse.

## If this is ever revisited

- Re-download from `meta-models/Muse-Glimmer-30B-GGUF`; runtime pin and spawn
  args live in `spawn_muse_engine` (`apps/synth_desktop/src-tauri/src/laguna.rs`)
  and `scripts/muse/serve.sh`.
- Try `--swa-full` before anything else; add `--fit off` (the default memory-fit
  probe cannot construct a DFlash context — always warns, and once tripped a
  fatal Metal residency-set assert during teardown).
- The economics only change materially with an MoE variant, a much
  faster-matmul GPU path, or acceptance-friendly (low-temp) sampling being
  acceptable for quality.
