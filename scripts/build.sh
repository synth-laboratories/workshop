#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/doctor.sh"
[[ -d node_modules ]] || npm ci
npm run typecheck
npm run build:graph
cargo build --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --release -j "${WORKSHOP_BUILD_JOBS:-4}"

echo "Unsigned Workshop build complete."
echo "Native binaries: apps/synth_desktop/src-tauri/target/release/"
