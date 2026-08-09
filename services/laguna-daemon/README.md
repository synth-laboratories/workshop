# Synth Laguna Sidecar

Independent OpenAI-compatible MLX sidecar for Synth Desktop. **Matches Poolside’s API shape** (`/health`, `/v1/models`, `/v1/chat/completions`, bearer 401/404) without depending on `Poolside.app` / `poolside-mlx-sidecar`.

## Language boundary

| Layer | Language |
| --- | --- |
| Desktop / visuals | TypeScript |
| Orchestration | TS now → Rust preferred |
| **This sidecar (MLX only)** | Python |

## Quick start (native Mac)

```bash
# optional: reuse weights already at ~/.config/poolside/models (~20GB)
./scripts/laguna/serve.sh

export SYNTH_LAGUNA_BASE_URL=http://127.0.0.1:7333
export SYNTH_LAGUNA_API_KEY=$(cat ~/.synth-desktop/laguna/api_key)
```

CLI mirrors Poolside’s flags:

```text
--host --port --models-dir --default-model --api-key
```

## Docker Compose

Metal MLX **cannot** run in Linux containers. Compose runs this sidecar in **mock** mode for API parity; use the host sidecar for real tokens.

```bash
cp .env.compose.example .env
docker compose up --build
```

## Tests

```bash
./scripts/laguna/test.sh
```
