# Muse Glimmer engine scripts

## Start the engine (one service, by hand)

```bash
scripts/muse/serve.sh
```

This is the by-hand equivalent of what Synth Desktop's supervisor spawns
(`spawn_muse_engine` in `apps/synth_desktop/src-tauri/src/laguna.rs`); the two
argument lists must stay in agreement. The engine is a backend for the Laguna
daemon on `:7333` — point clients at the daemon, not at the engine.

## Run the long-context benchmark

```bash
python3 scripts/muse/bench.py --label my-run --out scripts/muse/results/my-run.json
```

One invocation starts exactly one engine (on `:7434`, never the production
`:7334`), runs the workload, tears the engine down, and writes one JSON
artifact. Every reported number comes from the engine's own `timings` counters.

The benchmark refuses to start while any other model-serving process is alive,
and will not load the model unless the expected footprint plus system headroom
is genuinely free. While the engine lives, a watchdog kills it the moment
system free memory crosses the floor, the engine outgrows its declared
footprint, or another model process appears. The engine pid is written to
`$TMPDIR/muse-bench-engine.pid` and reaped on every exit path, including a
crashed previous run.

Phases, in order, all against a deterministic ≥17K-token document sized by the
engine's own `/tokenize`:

1. **uncached prefill** — cold KV, `cache_n` must be ~0 for the number to count
2. **identical prompt** — proves prefix reuse (`cache_n` ≈ `prompt_n`)
3. **decode continuation** — decode tok/s and DFlash `draft_n`/`draft_n_accepted`
4. **multi-turn follow-up** — history now carries a reasoning trace; `cache_n`
   here measures whether the KV prefix survives the agentic tool-turn shape

Useful knobs (each maps to one `llama-server` flag): `--n-batch`, `--n-ubatch`,
`--flash-attn`, `--cache-type-k/v`, `--threads`, `--draft-max`, `--no-draft`,
`--runtime`, `--extra-arg '<raw flag>'`. `--cooldown N` sleeps before starting
so back-to-back runs don't measure the previous run's heat: a 20K-token prefill
throttles the SoC by roughly a quarter for minutes afterward, which is larger
than most configuration effects being measured.

Results live in `scripts/muse/results/*.json`; the interpreted history is in
`apps/synth_desktop/MUSE_V0P2_THROUGHPUT.md`.
