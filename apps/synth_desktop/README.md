# Synth Desktop (`apps/synth_desktop`)

Real local-first research engineering workbench — **not** the fixture mock.

Electron is the viewer. A Python **local-runtime daemon** owns sessions, runs,
events, containers, Trace V5, visuals, OpenRouter usage, and Intern adapters.

## Local Laguna XS 2.1

Desktop already speaks OpenAI-compatible SSE via `SYNTH_LAGUNA_BASE_URL`.

```bash
# one-time: venv + mlx + download NVFP4 (~21.6 GB)
npm run laguna:setup

# terminal A — daemon on :7333
npm run laguna:serve

# terminal B — desktop (auto-probes :7333)
source ~/.synth-desktop/laguna/env.sh   # or: source scripts/laguna/env.sh
npm run dev --workspace @synth/synth-desktop
```

Architecture and Arena (~200 tok/s) notes: [`services/laguna-daemon/README.md`](../../services/laguna-daemon/README.md).

Vanilla `mlx_lm` is the integration path. Optimized mere-run / mlxfast kernels plug in as:

```bash
export SYNTH_LAGUNA_BACKEND=external
export SYNTH_LAGUNA_EXTERNAL_URL=http://127.0.0.1:8090
npm run laguna:serve
```


## Targets

| Target | How |
| --- | --- |
| **Laguna XS 2.1** (local) | Stub stream by default; set `SYNTH_LAGUNA_BASE_URL` to an OpenAI-compatible MLX server. Weights detected at `~/.cache/huggingface/hub/models--poolside--Laguna-XS-2.1`. |
| **Luna** | OpenRouter `moonshotai/kimi-k2.5` + local usage ledger |
| **Laguna S 2.1** | OpenRouter `poolside/laguna-s-2.1` + local usage ledger |
| **Intern · Live / Background** | Demo mailbox, or `SYNTH_API_KEY` + `SYNTH_INTERN_DEMO=0` against a slot / hosted backend |

## First-class inventory

Sidebar → **Inventory**:

- **Containers** — unified local + cloud deployments / probe
- **Traces** — sealed Trace V5 digests (CAS on disk)
- **Visuals** — instances from `workshop/visuals` templates; open in pane; save TSX

## Visuals

Deep templates live in [`../../visuals`](../../visuals). Agents use MCP tools in
`visuals/mcp/tools.json` (list / create / bind / save-tsx / open / live stream).

## Accessibility / eval

Stable `data-testid`s on sidebar, composer, visual pane, inventory, cloud desk.
`window.__synthEval` exposes semantic getState/invoke for scoring.

## Tests

```bash
npm test                 # runtime + visuals + a11y surface
npm run test:ui          # Playwright examples + Bombadil properties
npm run test:playwright  # deterministic Electron viewport invariants
npm run test:bombadil    # property exploration against Electron over CDP
npm run dogfood:models   # Laguna stub + OpenRouter + inventory + visuals
```
