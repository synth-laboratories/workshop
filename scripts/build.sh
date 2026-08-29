#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/doctor.sh"
[[ -d node_modules ]] || npm ci
npm run typecheck
npm run build:graph
WORKSHOP_BUILD_JOBS="${WORKSHOP_BUILD_JOBS:-4}" \
  "$repo_root/scripts/build-tier.sh" stable

echo "Unsigned Workshop build complete."
echo "App bundle: work/tier-builds/stable/Synth Workshop.app"
