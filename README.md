# Workshop

> **Visibility note:** This repository is currently **private**. It is intended to become **public**.

Synth Desktop / Local Agent Workbench — a local-first agent research and development workbench where agents can run locally (Laguna XS 2.1) or in Synth Cloud (Intern sync/async), and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

## Status — v0 ready for review

| Surface | Path | Role |
| --- | --- | --- |
| **Real app** | [`apps/synth_desktop`](./apps/synth_desktop) | Tauri 2 + local-runtime daemon |
| **Visuals infra** | [`visuals/`](./visuals) | 9 genre templates, registry, MCP tools, TSX save |
| **Mock (UX pin-down)** | [`apps/mock`](./apps/mock) | Fixture-only; do not confuse with product |
| Runtime | `services/local-runtime` | Sessions / runs / events / inventory |
| Agent runtime | `codex app-server` | Local/configured-provider coding-agent sessions |
| Inference | `services/laguna-daemon` | Responses-compatible Laguna → MLX boundary |

### Local Laguna XS 2.1

```bash
npm run laguna:setup    # once: mlx venv + NVFP4 weights (~21.6 GB)
npm run laguna:serve    # :7333 OpenAI-compatible daemon
source ~/.synth-desktop/laguna/env.sh
npm run dev --workspace @synth/synth-desktop
```

Desktop probes `http://127.0.0.1:7333` automatically. Details: [`services/laguna-daemon/README.md`](./services/laguna-daemon/README.md).


### Dogfood gates (verified)

- Local Laguna XS 2.1 agent path through Codex app-server and the Responses-compatible MLX sidecar
- Configured Responses-compatible model APIs through Codex app-server
- Inventory: local + cloud containers, Trace V5 ingest, 9 visual templates, save-as-TSX
- Live dock/eval visual simulation
- Intern sync demo mailbox
- Accessibility surface testids + semantic eval hook
- Intern endpoint profiles (`prod`, `staging`, `local`) via `~/.synth-desktop/config.toml`

The runtime selects the production Intern endpoint by default. For local
dogfood, set `SYNTH_INTERN_DEMO=1`; the checked-in [`config.toml.example`](./config.toml.example)
shows the profile and endpoint shape.

## Product framing

> Synth Desktop is a local-first agent research and development workbench where agents can run locally or in Synth Cloud, and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

Core loop: **observe → understand → modify → evaluate → fine-tune → deploy**

## Docs

- [`HANDOFF.md`](./HANDOFF.md) — full product + architecture
- [`synth_desktop_research_eng.md`](./synth_desktop_research_eng.md) — Trace V5 / visuals / containers
- [`apps/synth_desktop/README.md`](./apps/synth_desktop/README.md) — runbook
- [`visuals/README.md`](./visuals/README.md) — template + MCP agent flow
- [`handoff-package/`](./handoff-package/) — eng reuse bundle

## License / ownership

Owned by [synth-laboratories](https://github.com/synth-laboratories). Public release planned; treat contents as pre-release until then.
