#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

command -v npm >/dev/null || { echo "npm is required (Node.js 22+)." >&2; exit 1; }
command -v cargo >/dev/null || { echo "Cargo is required (Rust 1.85+)." >&2; exit 1; }
npm ci
echo "Workshop dependencies installed. Run ./scripts/doctor.sh, then ./scripts/build.sh."
