# 2026-08-12-clean live / headless receipts

Authority for this folder: [`HANDOFF_CLEAN_TIP_LIVE_JOBS_2026-08-12.md`](../../HANDOFF_CLEAN_TIP_LIVE_JOBS_2026-08-12.md).
Dirty-tip [`../2026-08-12/`](../2026-08-12/) does not count. Nothing pushed.

| Receipt | What ran | Claim |
| --- | --- | --- |
| [`a1.json`](./a1.json) | eval-driver Craftax Luna, isolated instance `a1final`, façade `:8297` | **A1 PASS** — 10/10 paid seeds; 113 calls; no recovery |
| [`a5.json`](./a5.json) | stream contract on A1 seed 0 | **A5 PASS** — 8/8 |
| [`a3.json`](./a3.json) | patched dual Banking77 GEPA on `a3retry` | **A3 PASS** — both completed; IPC deltas + typed child refs; unfocused lane advanced |
| [`a4.json`](./a4.json) | rebuilt one-process hosted Tinker occupancy proof on `:8881` | **A4 PASS** — typed `accelerator_busy`, then started after release |
| [`a6.json`](./a6.json) | fresh bounded Tinker checkpoint + Banking77 eval | **A6 PASS** — numeric reward 0.0, scored 1/1, checkpoint promoted |
| [`harbor_docker.json`](./harbor_docker.json) | `tests/test_harbor_docker.py` | **NOT A2** |
| [`digbench_headless.json`](./digbench_headless.json) | dig.bench mock + bind-surface | **NOT A8** |
