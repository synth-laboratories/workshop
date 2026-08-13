# Luna vs Terra: authenticated Codex discovery canary

This is a real `codex exec` comparison using ChatGPT authentication and medium reasoning. It is a local DiG-style canary, not an official DiG-bench score; the official endpoint returned HTTP 401 without `DIGBENCH_API_TOKEN`.

| Model | Runs | Game wins | Mean actions | Mean elapsed | Memory rule | Protocol |
|---|---:|---:|---:|---:|---:|---:|
| Luna | 3 | 15/15 | 25.667 | 100.958 s | 24/24 | 2/3 |
| Terra | 3 | 15/15 | 25 | 87.198 s | 22/24 | 3/3 |

Terra was 13.6% faster by mean wall time and used 2.6% fewer game actions. Both won every game. Luna achieved 24/24 on the memory-rule subgame; Terra achieved 22/24. Luna replicate 3 made one malformed-path tool call, recovered, and still finished 5/5; neither model inspected source or state.

The raw JSONL, stderr, final messages, state, and per-run receipts are retained beside this report. `workshop_events.json` is a six-lane trace-stream fixture for Workshop.
