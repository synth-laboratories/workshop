# Workshop

Workshop is a local-first macOS workbench for coding agents, evaluations,
optimization runs, containers, and inspectable research artifacts. The v0.8
release is built with Tauri 2, Rust, React, and TypeScript.

## Download or build

- Download the prebuilt v0.8 release from
  [synthlabs.ai/download](https://synthlabs.ai/download).
- Build the unsigned app from source with the commands below. The build does
  not require private Synth repositories or macOS Keychain credentials.

```bash
git clone https://github.com/synth-laboratories/workshop.git
cd workshop
./scripts/bootstrap.sh
./scripts/doctor.sh
./scripts/build.sh
```

The app bundle is written under
`apps/synth_desktop/src-tauri/target/release/bundle/macos/`. macOS may require
you to approve or ad-hoc sign a locally built app before opening it. Official
downloads are signed and notarized separately from this source build.

## Requirements

- macOS 14 or newer on Apple Silicon
- Xcode Command Line Tools
- Node.js 22 or newer with npm
- Rust 1.85 or newer with Cargo
- Python 3.11 or newer
- `jq`, `git`, and `curl`

`bootstrap.sh` installs repository dependencies. It does not install system
packages, alter shell profiles, or read credentials. `doctor.sh` reports every
missing prerequisite and checks that generated protocol bindings are present.

## Development

```bash
npm run dev:desktop
npm run typecheck
npm run build:graph
cargo check --manifest-path apps/synth_desktop/src-tauri/Cargo.toml
```

Workshop builds one cumulative feature envelope:
`core ⊂ stable ⊂ beta ⊂ alpha ⊂ dev`. Stable is the default.

```bash
scripts/build-tier.sh stable
scripts/build-tier.sh beta
```

The feature contract is
[`contracts/release-tiers-v1.toml`](contracts/release-tiers-v1.toml), with the
runtime model documented in [`docs/RELEASE_TIERS.md`](docs/RELEASE_TIERS.md).

## Building with a coding agent

Any coding agent that can run shell commands can build Workshop. Give it this
repository and ask it to run `doctor.sh`, `bootstrap.sh` if dependencies are
absent, and then `build.sh`. The same instructions work with Codex, Claude
Code, Cursor, or another agent. See [`AGENTS.md`](AGENTS.md) for repository
boundaries and generated files.

Provider credentials are optional for compiling Workshop. When exercising
provider-backed features, use a project-local `.env` and Workshop's ephemeral
secrets proxy; do not import credentials into Keychain.

## Architecture

- `apps/synth_desktop/` — Tauri desktop application and renderer
- `packages/` — shared TypeScript protocol packages
- `visuals/` — inspectable visualization families and runtime
- `services/laguna-daemon/` — optional local inference boundary
- `contracts/` — versioned runtime and release-tier contracts

Start with [`architecture.md`](architecture.md) for the system boundaries.

## Security and contributions

Read [`SECURITY.md`](SECURITY.md) before reporting a vulnerability and
[`CONTRIBUTING.md`](CONTRIBUTING.md) before proposing a change. This repository
contains product and build source; the private release verification corpus is
maintained separately.

Copyright 2026 Synth Laboratories. Licensed under the
[Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution notices.
