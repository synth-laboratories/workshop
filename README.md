# Workshop

Official downloads: [usesynth.ai/download](https://www.usesynth.ai/download).

For an Apple Silicon source checkout, run `./scripts/install.sh`, then
`./scripts/workshop.sh build-and-run`. This produces a separate ad-hoc signed
Workshop Local app. Runtime staging validates the pinned dependencies; local
builds are not official notarized distributions. `./scripts/install.sh --check`
checks prerequisites without modifying local configuration.

> **Visibility note:** This repository is currently **private**. It is intended to become **public**.

Synth Desktop / Local Agent Workbench — a local-first agent research and development workbench where agents can run locally (Laguna XS 2.1) or in Synth Cloud (Intern sync/async), and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

## Use Workshop from Codex, Claude Code, or another agent

Workshop exposes its product operations through one local MCP server: desktop
commands, managed browser sessions, container rollouts, visual authoring,
optimizers, diagnostics, plugins, persisted settings, fullscreen visuals and
captures. The desktop and agent adapters call the same registered handlers.
One native runtime owns the database and managed work, with or without a window.
ACP hosting lets installed agent adapters run inside Workshop and use that same
MCP connection. Human approvals and credential consent remain human actions;
renderer acknowledgements are internal callbacks.

This integration is implemented on this branch. The [coverage ledger](./docs/engineering/capability-migration-ledger.md)
records the exact coverage boundary and native acceptance, including the
provider and installed-release checks that have not been performed.

### Connect your agent

1. Start a Desktop built from this branch. For development, use
   `npm run desktop:dev`. Older installed builds do not expose this integration.
2. Find its **Data root** in Settings → About → Data root. Connecting
   grants access to the whole selected local instance, including its shared
   library and conversation visuals. It is not a grant to one chat.
3. From this repository, run one of:

   ```bash
   npm run workshop -- connect codex --data-root "/absolute/path/to/instance/data"
   npm run workshop -- connect claude --data-root "/absolute/path/to/instance/data"
   ```

   The command verifies the running instance before changing client settings.
   It registers one `workshop` MCP server and preserves unrelated configuration.
   Repeating the same command is safe; conflicting or customized Workshop
   entries are left untouched.
4. Restart the agent client. Check `/mcp` in Codex or Claude Code, then ask:

   > Use Workshop to create a Mermaid diagram showing Input → Analysis → Result.
   > Open it full screen, capture it, and inspect the image.

The `workshop` executable is also built and bundled alongside the Desktop
executable. With an installed build that includes this integration, replace
`npm run workshop --` above with
`"/Applications/Synth Desktop.app/Contents/MacOS/workshop"`; no source checkout,
Node.js, or Rust toolchain is needed to run that packaged executable.
Named development instances have their own executable and data root; use the
matching instance rather than another running Workshop window.

No hosted conversation ID, provider-key import, or pasted system prompt is
required. Shared visuals have a durable workspace owner in Workshop's database.
Closing the desktop window retains the runtime. Explicit runtime shutdown
stops managed work and disconnects all clients.

### Run without a desktop window

Build/install both the native Desktop executable and its matching `workshop`
bridge first. The CLI starts that same runtime with zero WebViews:

```bash
npm run workshop -- runtime start --data-root "/absolute/path/to/instance/data"
npm run workshop -- runtime status --data-root "/absolute/path/to/instance/data"
npm run workshop -- runtime attach --data-root "/absolute/path/to/instance/data"
npm run workshop -- runtime detach --data-root "/absolute/path/to/instance/data"
npm run workshop -- runtime stop --data-root "/absolute/path/to/instance/data"
```

`attach` and `detach` affect the window in the existing process. Database,
sessions, tools, and event history keep the same owner. `visual_present`, `app_present`, and
`app_capture` attach a desktop when needed; a native screenshot still requires
a desktop-capable OS session. This mode uses the native event loop, and does
not claim support for a Linux server without a display server. Development
builds need their matching frontend dev server when attaching a window.
`stop` explicitly shuts down the runtime and its managed ACP processes.

### Use the full product

Ask your agent to discover Workshop's tools and use the named operation for the
task. Browser and Computer Use retain their opt-in switches in **Settings →
Context**. An agent cannot enable its own access or answer its own permission
requests. For a source build, install dependencies with `npm install` and the
managed browser with `npx playwright install chromium` from the repository root.
`browser_status` reports missing Node, Playwright or Chromium dependencies. The
CLI/MCP bridge itself does not require those browser dependencies.
Browser sessions belong to the runtime and survive an MCP client's
disconnection; explicitly close sessions when finished.

`desktop_state_get` and revision-checked `desktop_state_update` expose persisted
preferences and layout choices. The desktop follows runtime changes and rejects
stale writes. Agents cannot change their own approval or sandbox policy.
`app_present` opens a page, settings section or existing task; `visual_present`
opens a shared visual, including fullscreen. Capture and inspect the actual
pixels to verify what appeared.

Shared visuals do not require a chat. Operations which attach artifacts or work
to a task accept an explicit existing session ID; use a real task when that
correlation is required. The optional skill below explains these workflows.

### Host an ACP agent inside Workshop

An external Codex/Claude client only needs MCP configuration above. To run an
agent **inside Workshop**, install an ACP adapter separately, then register its
absolute executable and allowed working directory. Plain `codex` or `claude`
commands are not ACP servers. See the maintained
[ACP agent registry](https://agentclientprotocol.com/get-started/agents) and
[Codex ACP adapter](https://github.com/agentclientprotocol/codex-acp).
Pin and test the adapter version you install.

Create `agent-backends.json` in the selected instance's data directory:

```json
[
  {
    "id": "my-agent",
    "command": "/absolute/path/to/installed/acp-agent",
    "args": [],
    "workspace": "/absolute/path/to/project",
    "envFile": "/absolute/path/to/project/.env",
    "maxSessions": 2,
    "maxTurnSeconds": 600
  }
]
```

On macOS/Linux, make that file private with `chmod 600`. Use `null` for `envFile`
when no project environment file is needed. Workshop loads that file directly;
it does not import keys into the Keychain-backed registry or copy them into
MCP configuration. The launched backend is a local program with your OS user's
permissions: its configured working directory is **not an OS sandbox**.
Register only an executable you intend Workshop to run. MCP cannot edit the
backend registry or choose an arbitrary executable.

Validate the registry:

```bash
npm run workshop -- backends --data-root "/absolute/path/to/instance/data"
```

Open **Settings → Context → Hosted agents** to select a backend, start a task,
send prompts, inspect its journal, answer permission requests, cancel, close,
or explicitly resume a retained task. External MCP clients use
`agent_backends_list`, `agent_session_start`, `agent_sessions_list`,
`agent_session_send`, `agent_session_cancel`, `agent_session_close`, and
`agent_session_resume`. Each hosted agent receives the same Workshop MCP
connection automatically, including the visual panel. No second database or
synthetic Codex chat is created.

The host negotiates [ACP v1](https://agentclientprotocol.com/protocol/v1/initialization)
over stdio. It supports text prompts, streamed session updates, one-time human
permission decisions, cancellation, and `session/load` when advertised by the
backend. It does not advertise ACP client filesystem or terminal capabilities;
choose an adapter which supplies those facilities itself. Authentication must
already be configured using an authorized mechanism; Workshop does not automate
an adapter's login or read Keychain credentials. Connection loss fails pending
work rather than replaying uncertain prompts. Cancellation gets a five-second
grace period before terminating an unresponsive backend. Retained process
identities support cleanup on the next runtime start. There is a 16-agent
instance limit and a three-level delegation limit; closing a parent closes its
attached descendants.

### Optional Workshop skill and plugins

MCP alone exposes the operations and their instructions. For guided workflows,
export a local plugin containing the same MCP connection and the shared
[Workshop skill](./integrations/workshop/skills/workshop/SKILL.md):

```bash
npm run workshop -- plugin --data-root "/absolute/path/to/instance/data" \
  --output "/absolute/path/to/agent-plugins/workshop"
```

The output directory must be new and named `workshop`. It contains Codex and
Claude Code manifests, one MCP configuration, and one skill. Install the export
through the client's local plugin workflow. It contains local executable and
instance paths, so export it on each machine; no credentials are copied. This
is a local export, not a published marketplace plugin.

Claude Code can load the export for one session with
`claude --plugin-dir "/absolute/path/to/agent-plugins/workshop"`.
Codex plugin installation requires a configured local marketplace containing
the export; direct MCP registration above is the simpler default and already
includes tool descriptions and startup instructions.

Use **either** direct registration **or** the plugin. Disconnect an existing
direct registration before enabling its plugin equivalent. No changes to
`AGENTS.md` or `CLAUDE.md` are required.

### Diagnostics, removal, and upgrades

```bash
npm run workshop -- doctor --data-root "/absolute/path/to/instance/data"
npm run workshop -- disconnect codex --data-root "/absolute/path/to/instance/data"
npm run workshop -- disconnect claude --data-root "/absolute/path/to/instance/data"
```

`doctor` checks authenticated connectivity, contract compatibility, instance
identity, and the available tools. It reports `renderingVerified: false`:
verify display separately with the visual request above. A presentation
acknowledgement means display was requested; `visual_capture` returns actual
pixels. Some templates do not publish semantic observations, so a null
`visual_observe` result is not rendering proof.

| Problem | What to check |
| --- | --- |
| Runtime unavailable | Start the selected Desktop instance and verify its Data root. The bridge never attaches to another instance automatically. |
| Runtime rejects the Workshop route or contract | Rebuild/update Desktop and the CLI from the same revision. |
| Server missing in the client | Restart it and inspect its MCP configuration. Codex uses `CODEX_HOME` or `~/.codex`; Claude uses `CLAUDE_CONFIG_DIR` when set, otherwise `~/.claude.json`. `--config-root` selects an explicit configuration directory for setup/removal. |
| Existing Workshop entry differs | Preserve its custom settings. Resolve the entry before reconnecting; setup will not overwrite it. |
| No rendered observation | Use native capture on macOS and inspect the result. Do not treat a pending render as success. |

Disconnect removes only an unchanged matching registration; restart the client
to close existing connections. Remove a plugin using its host's plugin manager.
For upgrades at the same executable path, update Desktop/CLI, restart them, and
rerun `doctor`. If moving the executable or instance, disconnect using the old
path before registering the new one. Export plugins again when their paths or
skill content change, and update them through the client.

Other local MCP clients can launch the same executable with arguments
`mcp --data-root /absolute/path/to/instance/data` using **stdio** transport.
Workshop's own visual window is the baseline; embedded panels require host
support for [MCP Apps](https://modelcontextprotocol.io/extensions/apps/overview).
ACP and Codex App Server are separate agent-hosting integrations and are not
needed for this connection.

See [Codex MCP setup](https://developers.openai.com/codex/mcp),
[Claude Code MCP setup](https://code.claude.com/docs/en/mcp), and the
[implementation plan](./docs/engineering/authoritative-capabilities.md) for
client reference and remaining release gates.

## Branching (single release trunk)

| Branch | Role |
| --- | --- |
| **`main`** | **Release source of truth.** Friends ZIPs, notarized builds, and published Desktop artifacts cut only from a clean `origin/main` tip (or an annotated tag on that tip). |
| **`dev`** | Day-to-day integration branch. Open feature PRs against `dev`. After every release merge into `main`, **fast-forward `dev` to `main`** so the two tips match again. |

Do not land feature work directly on `main`. Do not leave `dev` lagging behind a released `main` tip — that recreates parallel histories. If `dev` has unique commits that are not on `main`, land them via PR into the synced tip or retire them; do not treat a divergent `dev` as a second release trunk.

**No artifacts from dirty trees.** `desktop:build`, `desktop:install`, `desktop:install:release`, and `desktop:verify` refuse a dirty worktree. Named `desktop:dev` instances may run dirty (revision is tagged `-dirty`); those are not release artifacts. Alignment checklist: [`HANDOFF_DEV_MAIN.md`](./HANDOFF_DEV_MAIN.md).

## Status — v0 ready for review

| Surface | Path | Role |
| --- | --- | --- |
| **Real app** | [`apps/synth_desktop`](./apps/synth_desktop) | Tauri 2 + Rust CoreRuntime |
| **Visuals infra** | [`visuals/`](./visuals) | 9 genre templates, registry, MCP tools, TSX save |
| **Mock (UX pin-down)** | [`apps/mock`](./apps/mock) | Fixture-only; do not confuse with product |
| Runtime | `apps/synth_desktop/src-tauri` | Rust-owned sessions / runs / events / inventory / visuals |
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

### Desktop development and acceptance

Use the repository-owned lifecycle commands instead of opening a build-tree
`.app` manually:

```bash
npm run desktop:dev      # primary hot-reload loop; isolated instance "codex"
npm run desktop:codex:status
npm run desktop:codex:stop
npm run desktop:check   # parallel typecheck + cargo check; normal checkpoint
npm run desktop:build   # parallel typecheck + Tauri release build; no tests
npm run cache:rust:stats # inspect Rust compiler-cache effectiveness
npm run desktop:verify   # full Rust + renderer acceptance battery; release/CI gate
npm run desktop:install  # standard build → atomic local /Applications install → launch
npm run desktop:install:release # full release gate → install → launch
npm run desktop:restart  # restart the installed canonical app
npm run desktop:status   # verify the one allowed process and install path
npm run desktop:stop
```

Named instances are the normal edit/test loop and never stop another instance.
Their exact source revision, executable, PID, data root, and manifest are shown
under Settings → About → Data root. The canonical lifecycle is
reserved for release acceptance.

Use the test batteries according to the scope of the change:

| Battery | Command | Run it when |
| --- | --- | --- |
| Focused | The relevant `npm`, Playwright, or Cargo test directly | During iteration and after a localized UI/runtime change. |
| Check | `npm run desktop:check` | Before handoff or when renderer/native contracts changed; parallel TypeScript and Rust compile checks. |
| Build | `npm run desktop:build` | Produce a local release bundle. It overlaps typechecking with the real Tauri build and runs no tests. |
| Full release | `npm run desktop:verify` | Before merging a release PR, cutting a release, or after broad runtime/integration changes. |

`desktop:install` runs the standard build (with no separate `cargo check` or test
battery), then signs and verifies the staged bundle, backs up the previous
install under `~/.synth-desktop/backups/app-builds`, and launches only
`/Applications/Synth Desktop.app`. Use `desktop:install:release` when the full
release battery must pass before installation. Acceptance testing and Computer Use must
target that full path. `desktop:stop` targets only that exact installed path (or
the canonical Cargo debug executable); it does not stop named instances or
arbitrary copied apps. Do not launch
`apps/synth_desktop/src-tauri/target/*/bundle/macos/Synth Desktop.app`; the
lifecycle commands never use a generic process-name match. Use the Runtime
identity receipt or the named instance manifest for CUA rather than relying on
whichever generic Synth window is focused.

Build acceleration is layered: Turborepo owns the npm-workspace task graph and
caches deterministic renderer tasks; Cargo remains authoritative for Rust;
`sccache` is detected automatically and caches eligible `rustc` invocations
under `~/.cache/synth-workshop/sccache`. The final macOS bundle, signing,
backup, and installation remain uncached and explicit.


### Dogfood gates (verified)

- Local Laguna XS 2.1 agent path through Codex app-server and the Responses-compatible MLX sidecar
- Configured Responses-compatible model APIs through Codex app-server
- Inventory: local + cloud containers, Trace V5 ingest, 9 visual templates, save-as-TSX
- Live Harbor/eval visual simulation
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

- [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](./WORKSHOP_QUALITY_STYLE_GUIDE.md) — unified visual, interaction, runtime-honesty, accessibility, and test quality bar
- [`workshop_style.md`](./workshop_style.md) — provisional categorical triage: unacceptable, fix-before-review, and expected-fail debt
- [`HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md`](./HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md) — current Rust core / visuals / Intern SDK handoff
- [`testing.md`](./testing.md) — Playwright, Bombadil, Rust, and runtime coverage map
- [`docs/top_containers.md`](./docs/top_containers.md) — filepaths for Banking77, HealthBench, Craftax, Harbor, and dig.bench test containers
- [`HANDOFF.md`](./HANDOFF.md) — full product + architecture
- [`synth_desktop_research_eng.md`](./synth_desktop_research_eng.md) — Trace V5 / visuals / containers
- [`apps/synth_desktop/README.md`](./apps/synth_desktop/README.md) — runbook
- [`visuals/README.md`](./visuals/README.md) — template + MCP agent flow
- [`handoff-package/`](./handoff-package/) — eng reuse bundle

## License / ownership

Owned by [synth-laboratories](https://github.com/synth-laboratories). Public release planned; treat contents as pre-release until then.
