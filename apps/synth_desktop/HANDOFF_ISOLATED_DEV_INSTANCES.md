# Handoff: Isolated Synth Desktop development instances

**Status:** Implemented and verified  
**Audience:** Engineers and coding agents running multiple local Desktop builds  
**Canonical docs:** [`README.md`](./README.md#multiple-isolated-development-instances)

## What changed

Named, isolated applications are now the standard Synth Desktop development
workflow. Running `desktop:dev` without a name starts the `codex` instance. Any
number of additional names can run concurrently without sharing product state.

The canonical `/Applications/Synth Desktop.app` remains the release acceptance
target. Named development instances do not replace or modify it.

## Quick start

Run commands from the workshop repository root:

```bash
# Default isolated development instance
npm run desktop:dev
npm run desktop:codex:status

# Two concurrent instances
npm run desktop:dev -- alpha
npm run desktop:dev -- beta

# Inspect or stop one exact instance
npm run desktop:instance:status -- alpha
npm run desktop:instance:stop -- alpha

# Recoverable cleanup: moves the instance directory to macOS Trash
npm run desktop:instance:clean -- alpha
```

Names must match:

```text
[a-z][a-z0-9-]{0,31}
```

## Identity model

> **v0.2 branch note:** v0.2 development instances are release-scoped under
> `instances/v02/`, display as `Synth Workshop v0.2 · <name>`, use the bundle
> namespace `com.synth.desktop.v02.dev.<name>`, and render both `v0.2` and the
> instance badge in the icon. The examples below describe the original v0.1
> contract and remain useful background for the general isolation model.

An instance name is the authority for all local automation. For `alpha`, the
launcher generates:

| Field | Value |
| --- | --- |
| Display name | `Synth Desktop · alpha` |
| Dock badge | `1` |
| Bundle identifier | `com.synth.desktop.dev.alpha` |
| Data root | `~/.synth-desktop/instances/alpha/data` |
| Workspace | `~/.synth-desktop/instances/alpha/workspace` |
| Cargo target | `~/.synth-desktop/instances/alpha/build/target` |
| Executable | `…/alpha/build/target/debug/synth-desktop` |
| Manifest | `~/.synth-desktop/instances/alpha/instance.json` |
| Tauri overlay | `…/alpha/generated/tauri.instance.json` |
| IPC descriptor | `…/alpha/data/visuals-ipc.json` |
| Vite port | Deterministic from the instance name |

The app and its Codex/MCP children inherit:

```text
SYNTH_DESKTOP_INSTANCE=alpha
SYNTH_DESKTOP_DATA_ROOT=…/alpha/data
SYNTH_DESKTOP_CONFIG=…/alpha/data/config.toml
SYNTH_CODEX_HOME=…/alpha/data/codex
SYNTH_DESKTOP_WORKSPACE=…/alpha/workspace
SYNTH_DESKTOP_APP_NAME=Synth Desktop · alpha
SYNTH_DESKTOP_INSTANCE_MANIFEST=…/alpha/instance.json
SYNTH_DESKTOP_SOURCE_REVISION=<git-short-revision>[-dirty]
```

## Codex metadata versus workspace access

Synth keeps Codex metadata isolated from source repositories. A session named
`<session-id>` receives its own generated home at:

```text
~/.synth-desktop/instances/<instance>/data/codex/homes/<session-id>/
├── auth.json
├── config.toml
├── sessions/
└── skills/use-synth-containers/SKILL.md
```

Project instructions such as `AGENTS.md` remain repository files and are
discovered from the conversation working directory in the normal Codex order.
Configure that root under **Settings → Runtime → Agent workspace access**. The
first entry is the default working directory for new conversations, and every
entry is emitted as a `sandbox_workspace_write.writable_roots` value. For
example:

```toml
[workspace]
allowed_roots = ["/Users/joshuapurtell/Documents/GitHub"]
```

Running conversations retain their current process and working directory;
start or restart an agent session after changing workspace access.

The generated `instance.json` is machine-readable and should be preferred over
window-position, focus, or generic process-name heuristics. Settings → Runtime
shows the same running build identity, including its source/build revisions,
PID, executable, and manifest path.

## Numbered Dock icons

The launcher derives a development icon from the canonical Synth icon and adds
a high-contrast badge:

| Instance | Badge |
| --- | --- |
| `alpha` or `test-1` | 1 |
| `beta` or `test-2` | 2 |
| `gamma` or `test-3` | 3 |
| `delta` or `test-4` | 4 |
| `epsilon` or `test-5` | 5 |

Other names receive an uppercase first-letter badge. The generated PNG and
ICNS files live under the instance's `generated/` directory and are compiled
into that instance's Tauri executable.

## Assigning an instance to an agent

Always name the instance in the assignment:

> Use Synth Desktop instance `alpha`. Read its instance manifest first. Do not
> operate another Synth Desktop window.

The agent should:

1. Read `~/.synth-desktop/instances/alpha/instance.json`.
2. Confirm `npm run desktop:instance:status -- alpha` reports it running.
3. Target the exact display name, bundle ID, or executable from the manifest.
4. Use only the workspace, IPC descriptor, and data root recorded there.
5. Never select a window merely because its process is named `synth-desktop`.

This keeps multiple coding agents from taking over whichever generic window
was focused most recently.

## What is isolated

Each named instance owns its own:

- Tauri product name and bundle identifier;
- single-instance lock namespace;
- executable and Cargo target directory;
- Vite development server and port;
- SQLite database and WAL files;
- content-addressed store and visual registry;
- projects, sessions, runs, inventory, and usage records;
- Codex homes, threads, and generated MCP configuration;
- default workspace;
- backend configuration and secrets file;
- authenticated visual/container IPC listener and descriptor.

On first creation, the launcher copies canonical
`~/.synth-desktop/config.toml` and `~/.synth-desktop/.env`, when present. The
copies are private after creation; settings changes do not flow between
instances.

## What is intentionally shared

Large read-only model files and the Laguna Responses daemon at `:7333` are
shared by default. This avoids duplicating the roughly 20 GB Laguna model.

External container services are isolated only by endpoint. The Desktop
container registry is private, but two apps attached to the same Craftax URL
can both mutate that service's session count. For interference-free rollouts,
launch one Craftax service per instance:

```bash
cd ~/Documents/GitHub/gamebench/tasks/craftax-singleplayer

# Separate terminals
python3 scripts/run_service.py --lane rust --port 18098  # alpha
python3 scripts/run_service.py --lane rust --port 18099  # beta
```

Attach `http://127.0.0.1:18098` only in `alpha` and `:18099` only in `beta`.

## Canonical versus development lifecycle

Use named instances for normal local development:

```bash
npm run desktop:dev -- alpha
```

Use the canonical lifecycle only for release acceptance:

```bash
npm run desktop:verify
npm run desktop:install
npm run desktop:restart
npm run desktop:status
```

`desktop:status` describes the canonical installation. Use
`desktop:instance:status -- <name>` for a named development instance.

Do not manually open or copy the generated release bundle from
`src-tauri/target`. The canonical lifecycle deliberately treats arbitrary
release-bundle copies as invalid acceptance targets.

## Implementation map

| Area | File |
| --- | --- |
| Named-instance launcher and lifecycle | [`../../scripts/desktop-instance.sh`](../../scripts/desktop-instance.sh) |
| Icon generator | [`../../scripts/generate-desktop-instance-icon.py`](../../scripts/generate-desktop-instance-icon.py) |
| Launcher contract test | [`../../scripts/test-desktop-instance.sh`](../../scripts/test-desktop-instance.sh) |
| Canonical lifecycle delegation | [`../../scripts/desktop.sh`](../../scripts/desktop.sh) |
| Rust identity and root selection | [`src-tauri/src/instance.rs`](./src-tauri/src/instance.rs) |
| SQLite/application data root | [`src-tauri/src/storage/database.rs`](./src-tauri/src/storage/database.rs) |
| Backend config and secrets root | [`src-tauri/src/synth_config.rs`](./src-tauri/src/synth_config.rs) |
| Codex home and MCP injection | [`src-tauri/src/codex.rs`](./src-tauri/src/codex.rs) |
| Default workspace and startup | [`src-tauri/src/lib.rs`](./src-tauri/src/lib.rs) |
| User-facing runbook | [`README.md`](./README.md#multiple-isolated-development-instances) |

## Verification performed

The implementation was verified with two real concurrent GUI applications:

- `alpha`: badge 1, private executable/data/IPC/Vite port;
- `beta`: badge 2, private executable/data/IPC/Vite port.

Both remained alive concurrently. Stopping `alpha` left `beta` running, proving
that their process and single-instance namespaces are independent.

Automated verification:

```text
TypeScript typecheck: passed
Rust library tests: 73 passed
Playwright renderer tests: 40 passed
Instance launcher contract test: passed
Shell syntax checks: passed
```

Run the focused contract again with:

```bash
npm run desktop:instance:test
```

## Operational notes

- The first launch of a new name performs a full private Rust build. Later
  launches use that instance's incremental cache.
- Two first-time builds may briefly contend on Cargo's global package cache,
  but their target artifacts remain separate.
- `status` regenerates the deterministic manifest, Tauri overlay, and icon; it
  does not launch the app.
- `clean` stops the exact resolved executable and moves only that validated
  instance root to Trash.
- A Vite-port collision fails closed because the generated command uses
  `--strictPort`.

## Suggested next refinements

These are not required for isolation:

1. Add an optional launcher flag that starts and registers a dedicated Craftax
   service automatically.
2. Display the instance name in Settings diagnostics in addition to the window
   title and Dock icon.
3. Add a lightweight instance picker/status panel for developers who prefer not
   to use the CLI.
