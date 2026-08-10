# Synth Desktop (`apps/synth_desktop`)

Real local-first research engineering workbench — **not** the fixture mock.

Tauri is the secure desktop host. A Rust **CoreRuntime** owns durable product
state (SQLite journal, sessions, content store). Local and configured-provider
sessions already run through the Rust Codex manager and append to the unified
event journal (`runtime:event`). The Rust **Visual Registry** owns visual
CRUD/revisions/CAS (`window.synthVisuals`, Visuals page, `synth-visuals-mcp`).
Intern, projects, inventory, and visuals are Rust-owned. Python is used only by
the optional Laguna/MLX Responses sidecar; it is not a desktop product runtime.

Every session is either Synth Intern/cloud or Codex app-server. There is no
direct model-chat path. See [`../../architecture.md`](../../architecture.md).

### Session titles

Session naming is a Rust CoreRuntime concern, not an MCP tool. On the first
accepted Codex turn, the host derives a short title from the user's prompt and
calls Codex app-server `thread/name/set`. The resulting
`thread/name/updated` notification updates the live sidebar, while the same
title is committed to the CoreRuntime session row and the durable Codex thread
record used during restart.

Title provenance is stored as `metadata.titleOrigin` (`default`, `automatic`,
or `manual`). An automatic title can replace only a default title. A rename
received later through Codex app-server is recorded as manual and cannot be
overwritten by a delayed automatic naming attempt. MCP servers can therefore
discover and operate tools for a task, but they do not own task identity or
sidebar presentation.

The embedded terminal is Rust-owned (`portable-pty`) with an xterm view; use
`⌘J` to toggle it and `⌘⇧T` to create another terminal.

## Development lifecycle

Run these from the repository root:

```bash
npm run desktop:dev      # primary edit/refresh loop; isolated instance "codex"
npm run desktop:dev -- alice  # another isolated app that can run concurrently
npm run desktop:verify   # typecheck + Rust tests + renderer acceptance suite
npm run desktop:install  # acceptance build and canonical install
npm run desktop:restart  # restart /Applications/Synth Desktop.app
npm run desktop:status   # must show at most one canonical process
npm run desktop:stop
```

### Multiple isolated development instances

Named instances are the standard local development workflow. `desktop:dev`
uses the name `codex` when none is supplied; pass a short name to run another app
without stopping or mutating an existing instance:

```bash
# terminal A
npm run desktop:dev -- alice

# terminal B — runs at the same time as alice
npm run desktop:dev -- beta

npm run desktop:instance:status -- alice
npm run desktop:instance:stop -- alice
npm run desktop:instance:clean -- alice  # moves its directory to macOS Trash
```

The standard test names `alpha`, `beta`, `gamma`, `delta`, and `epsilon` use
Dock badges **1–5** respectively. `test-1` through `test-5` are equivalent
numbered aliases; other names receive an uppercase first-letter badge. Icons
are generated from the canonical Synth icon inside the private instance
directory and compiled into that instance's Tauri executable.

Names must match `[a-z][a-z0-9-]{0,31}`. Each name deterministically receives
its own product title, bundle identifier, Vite port, Cargo target, executable,
SQLite database, content store, Codex homes, workspace, secrets/config copy,
and authenticated IPC descriptor beneath:

```text
~/.synth-desktop/instances/<name>/
├── instance.json
├── data/                 # SQLite, CAS, config, .env, Codex homes, IPC
├── workspace/
├── generated/            # merged Tauri development config
└── build/target/         # distinct executable and bundle identity
```

The first launch of a new name performs a full private Rust build; subsequent
launches are incremental. On first creation, the launcher copies the canonical
`~/.synth-desktop/config.toml` and `.env` when present. Later settings changes
remain private to that instance.

Large read-only model assets and the Laguna Responses daemon on `:7333` are
intentionally shared. Product state is not. External containers are also
separate only when they use separate endpoints: the container registry is
instance-private, but two apps attached to the same service URL can both send
rollouts to that service. For interference-free Craftax work, launch one server
per instance and attach only its URL:

```bash
cd ~/Documents/GitHub/gamebench/tasks/craftax-singleplayer
python3 scripts/run_service.py --lane rust --port 18098  # alice
python3 scripts/run_service.py --lane rust --port 18099  # beta, another terminal
```

Use Inventory → Containers in each app to attach its corresponding URL. The
instance manifest printed by `desktop:instance:status` and the matching
Settings → Runtime → Desktop identity receipt are the authority for local
automation and CUA; do not identify a window only by the generic process name.

When assigning an agent, include the instance name explicitly—“use Desktop
instance `alpha`.” The app and every Codex/MCP child inherit
`SYNTH_DESKTOP_INSTANCE=alpha`; automation can then read
`~/.synth-desktop/instances/alpha/instance.json` and target the exact title,
executable, workspace, data root, and IPC endpoint. This prevents two agents
from selecting whichever generic Synth window happened to be focused last.

The canonical installed application is deliberately different from named
development instances. It retains its production bundle identifier and legacy
data locations and remains the only supported release acceptance target.

The supported acceptance target is always
`/Applications/Synth Desktop.app`. `desktop:install` runs the acceptance gates, builds the release bundle,
stages and verifies it before replacement, preserves the prior app as a dated
backup, launches the installed app, and verifies its exact executable path.
Never open the release bundle inside `src-tauri/target` directly; that creates a
second process with different environment/state and invalidates UI testing.
Canonical stop/restart targets only the exact installed or canonical debug
executable and never stops named development instances or arbitrary copied apps.

## Local Laguna XS 2.1

Codex app-server speaks the Responses protocol to the Laguna daemon at
`SYNTH_LAGUNA_BASE_URL`. The self-contained daemon validates Responses items,
compiles them directly for Laguna, loads the open MLX/NVFP4 implementation in
process, and emits semantic Responses SSE events. It does not require
Poolside.app, a closed-source sidecar, Chat objects, or an internal
`/v1/chat/completions` hop.

```bash
# one-time: venv + mlx + download NVFP4 (~21.6 GB)
npm run laguna:setup

# terminal A — daemon on :7333
npm run laguna:serve

# terminal B — desktop (auto-probes :7333)
source ~/.synth-desktop/laguna/env.sh   # or: source scripts/laguna/env.sh
npm run desktop:dev
```

Architecture, operations, pinned schemas, capability errors, compliance, and
the temporary rollback flag are documented in
[`services/laguna-daemon/README.md`](../../services/laguna-daemon/README.md).

Native Responses is the default. During the planned cutover window only, the
reviewed legacy adapter can be selected explicitly:

```bash
export SYNTH_LAGUNA_RESPONSES_ENGINE=legacy
npm run laguna:serve
```


## Targets

| Target | How |
| --- | --- |
| **Laguna XS 2.1** (local) | Codex app-server → `/v1/responses` → native Laguna coordinator → in-process MLX. |
| **Configured model API** | Codex app-server → configured Responses-compatible provider. |
| **Intern · Live / Background** | Demo mailbox, or `SYNTH_API_KEY` + `SYNTH_INTERN_DEMO=0` against a slot / hosted backend |

### Registering model controls

Model-specific composer knobs are declared in
`src/renderer/src/runtime/modelCapabilities.ts`. Each registry entry owns its
target/model match, controls and options, defaults, persistence keys, legacy
migration keys, and `turn/start` transport mapping. `Composer` renders the
registered knobs and `App` forwards them without model-specific branches.

To add a model knob:

1. Add the execution target in `types/landing.ts` if the model itself is new.
2. Add one `MODEL_CAPABILITY_REGISTRY` entry with its exact advertised options.
3. Add provider translation only when its wire format differs from Codex's
   `turn/start` field. Laguna reasoning remains a typed Responses capability;
   it is not lowered through Chat.
4. Extend the provider-switch Playwright case with the expected wire value.

For internal dogfood, set `SMR_WORKER_API_KEY` (or the slot-projected
`SYNTH_EVAL_EXEC_WORKER_API_KEY`) in the environment that launches Desktop.
The Rust host then joins the worker-only execution Codex SSE to the public
Intern mailbox: mailbox events remain authoritative, while normalized and
redacted Codex events appear in the Activity pane. The worker key never crosses
the Tauri boundary into the renderer.

## Multi-agent model compatibility

Settings → **Models** exposes a provider-independent `None` / `V1` / `V2`
capability for each registered model family. Built-in presets are Sol and Terra
on V2, Luna on V1, and Laguna S/XS 2.1 disabled. An explicit override—including
forcing Laguna onto V1 or V2—is stored under `[models.multi_agent]` in
`~/.synth-desktop/config.toml` and written into the per-session Codex app-server
configuration when a new session starts. Named development instances store the
same file under `~/.synth-desktop/instances/<name>/data/config.toml`. Reset removes the override and returns
the model family to its preset.

## First-class inventory

Sidebar → **Inventory**:

- **Containers** — durable register/list/get/probe records hydrated from `/health`
  and `/info|/metadata`, including capabilities, actions, task-family hints, and
  the last rollout ID
- **Traces** — sealed Trace V5 digests (CAS on disk)
- **Visuals** — instances from `workshop/visuals` templates; open in pane; save TSX

## Visuals

Deep templates live in [`../../visuals`](../../visuals). Agents use MCP tools in
`visuals/mcp/tools.json` (list / create / bind / save-tsx / open / live stream).

Each embedded coding-agent home is provisioned with `synth-containers-mcp`,
`synth-visuals-mcp`, and concise `use-synth-containers` / `use-synth-visuals`
skill catalog entries. Skill bodies and references load only when their scoped
workflow is selected. Codex sees one compact `visual_manage` dispatcher rather
than 13 visual schemas on every turn; its operation payloads live in the lazy
visual skill, while legacy visual tool names remain available to other MCP
clients. The container MCP provides list, explicit register, get, probe, and a
bounded `container_run_rollouts` transport-acceptance operation. That last
operation accepts only a previously registered loopback HTTP URL, disables
redirects, limits each request to 1–8 rollouts and 1–64 explicit actions, and
returns the engine's exact rollout IDs, states, and event logs. It proves live
agent-to-MCP-to-container dispatch; it is not evidence of LLM policy quality.
Policy evaluation belongs to the workspace-owned benchmark harness:
coding agents discover the container contract, run real LLM policy rollouts,
seal the result as Trace V5, and let Desktop derive its read-only inspector
projection. Workshop embeds no Craftax policy.

The two bundled loopback MCP servers are provisioned with
`default_tools_approval_mode = "approve"`. This trust is deliberately limited to Desktop's
packaged container and visual adapters; it lets an agent complete the local
workflow under “Always ask” without weakening shell or third-party MCP
approvals.

Codex `collabAgentToolCall` and child-thread lifecycle events produce a
first-class **Subagents** visual for local, configured-provider, and Intern
sessions. It opens on the first spawn, maintains Active and Done groups from
the live app-server stream, and keeps child-agent output in that visual rather
than duplicating it into the parent transcript.

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
