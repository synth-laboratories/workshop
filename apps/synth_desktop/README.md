# Synth Desktop (`apps/synth_desktop`)

Real local-first research engineering workbench — **not** the fixture mock.

Tauri is the secure desktop host. A Python **local-runtime daemon** owns
sessions, runs, events, containers, Trace V5, visuals, and agent routing.

Every session is either Synth Intern/cloud or Codex app-server. There is no
direct model-chat path. See [`../../architecture.md`](../../architecture.md).

Local and configured-provider sessions run natively through the Rust Codex
manager. The embedded terminal is also Rust-owned (`portable-pty`) with an
xterm view; use `⌘J` to toggle it and `⌘⇧T` to create another terminal.

## Local Laguna XS 2.1

Codex app-server speaks the Responses protocol to the Laguna daemon at
`SYNTH_LAGUNA_BASE_URL`. The daemon translates Responses requests to the MLX
sidecar and returns Responses SSE events.

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
| **Laguna XS 2.1** (local) | Codex app-server → `/v1/responses` → Laguna daemon → MLX sidecar. |
| **Configured model API** | Codex app-server → configured Responses-compatible provider. |
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
npm run test:playwright  # deterministic renderer viewport invariants
npm run test:bombadil    # property exploration against the renderer harness
npm run dogfood:models   # local Codex/Laguna + inventory + visuals
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml
```
