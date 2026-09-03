# Hosted SFT through public `synth-optimizers`

**Status:** Workshop's hosted SFT control plane is the public `synth-optimizers`
service. Optimizers-beta may be used behind that service as a private training
executor, but Workshop must never contact it directly.

The filename is retained so existing links keep working. New documentation should
refer to this as **hosted SFT**, not “beta SFT.”

## Ownership boundary

```text
Workshop
  → public synth-optimizers SFT service
    → private training executor (currently optimizers-beta)
```

Workshop owns the local run mirror, event ingestion, and visuals. The public
service owns the canonical SFT run, request validation, lifecycle, and cancellation.
The private executor is not a Workshop API.

The separate local recipe `sft.craftax.gpt-oss.smoke.v1` runs a product-owned
Groq + Tinker Python smoke. It is not hosted SFT and is not evidence for the public
service integration.

## Workshop service contract

Workshop uses only these public endpoints:

```text
POST /v1/runs
GET  /v1/runs/:id
GET  /v1/runs/:id/optimizer-events?after_sequence=&limit=
POST /v1/runs/:id/cancel
```

Authentication and origin:

- `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN`
- `SYNTH_OPTIMIZERS_SFT_SERVICE_URL` (defaults to `http://127.0.0.1:8878`)

Do not configure `OPTIMIZERS_BETA_SERVICE_TOKEN`,
`SYNTH_OPTIMIZERS_BETA_URL`, or `OPTIMIZERS_BETA_URL` for hosted SFT in
Workshop. Those settings remain valid only for the separate hosted GELO path.

Workshop submits:

```json
{
  "algorithm": "sft",
  "idempotency_key": "sft_hosted_<suffix>",
  "config_toml": "<product-owned recipe>"
}
```

The public service validates the configuration and owns any translation to its
executor. Workshop does not fetch an executor-specific config contract.

## Recipes

| Recipe | Path | Cost behavior |
| --- | --- | --- |
| `sft.banking77.nemotron-lightning.tinker.v1` | Public service, Tinker backend | Provider charges apply |
| `sft.craftax.nemotron-nano.tinker.v1` | Public service, Tinker + local Craftax evaluation | Provider charges apply |
| `sft.craftax.gpt-oss.smoke.v1` | Local product-owned Python runner | Separate legacy smoke |

Paid recipes must wait for explicit capped-compute approval. Missing reward, cost,
or validation loss remains missing (`null`/omitted) and renders as `—`; Workshop
must never invent zeroes.

## Event projection

The public event page has a producer sequence. Workshop fails closed on gaps or a
wrong algorithm, remaps accepted events onto its SQLite cursor, and preserves the
producer sequence as `sourceSequenceNumber`.

| Event | Workshop projection |
| --- | --- |
| `optimizer.visual.ready` | Open `optimizer.sft.live.v1` on slot `optimizer_run` |
| `sft.training.queued` / `started` | Queued / running lifecycle |
| `sft.training.metrics` | One aligned train/validation metric point |
| `sft.checkpoint.created` / `ready` | Immutable checkpoint rail; ready is not promoted |
| `sft.checkpoint_evaluation.allocated` | Evaluation campaign and rollout children |
| `sft.checkpoint_rollout.completed` | Patch reward/cost only when present |
| `sft.checkpoint.promotion_evaluated` / `promoted` | Promotion decision and selected checkpoint |
| `optimizer.run.completed` / `failed` / `cancelled` | Terminal lifecycle |

## Code map

| File | Responsibility |
| --- | --- |
| `apps/synth_desktop/src-tauri/src/optimizers/sft_client.rs` | The only hosted SFT transport |
| `apps/synth_desktop/src-tauri/src/optimizers/hosted_sft.rs` | Product-owned recipes and public event mirroring |
| `apps/synth_desktop/src-tauri/src/optimizers/hosted_client.rs` | Private beta client for GELO only |
| `apps/synth_desktop/src-tauri/src/optimizers/ingest.rs` | Fail-closed event-page normalization |
| `apps/synth_desktop/src-tauri/src/optimizers/service.rs` | Recipe routing and restarted-run cancellation |
| `apps/synth_desktop/src/renderer/src/components/OptimizersPage.tsx` | Agent planning and training workspace entry |

## Acceptance and verification

- Hosted SFT submit, poll, and cancel contain no `HostedOptimizerClient` or beta
  environment-variable references.
- Restarted hosted SFT cancellation reaches the public service.
- No product UI or recipe path emits deterministic training results.
- The public service's SFT tests pass independently.
- Workshop Rust tests, source invariants, typecheck, and targeted Playwright tests
  pass.

```bash
# Workshop
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml optimizers::hosted_sft
node --test apps/synth_desktop/tests/v02_surface_invariants.test.mjs
npm --prefix apps/synth_desktop run typecheck
npm --prefix apps/synth_desktop exec playwright test -- \
  --config apps/synth_desktop/playwright.config.ts \
  apps/synth_desktop/tests/playwright/optimizer-banking77.spec.ts

# Public synth-optimizers service
uv run pytest -q tests/test_sft_service.py
```

Paid production readiness still requires a bounded live Tinker receipt, two
distinct dataset digests, multi-checkpoint evaluation evidence, honest metering,
and reopen/cancel verification after local evaluation slots are gone.
