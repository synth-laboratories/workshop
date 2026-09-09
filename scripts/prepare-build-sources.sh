#!/usr/bin/env bash
# Source this file to resolve immutable public build inputs without sibling
# checkouts, credentials, or changes to a developer's existing repositories.
set -euo pipefail
BUILD_SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/work/build-sources"

fetch_build_source() {
  local repository="$1" revision="$2" destination="$BUILD_SOURCE_ROOT/$1-$2"
  mkdir -p "$BUILD_SOURCE_ROOT"
  if [[ ! -d "$destination" ]]; then
    mkdir "$destination"
    git -C "$destination" init --quiet
    git -C "$destination" remote add origin "https://github.com/synth-laboratories/$repository.git"
  fi
  [[ -d "$destination/.git" ]] || { echo "Invalid managed build source: $destination" >&2; return 1; }
  [[ -z "$(git -C "$destination" status --porcelain)" ]] || { echo "Refusing dirty build source: $destination" >&2; return 1; }
  if [[ "$(git -C "$destination" rev-parse HEAD 2>/dev/null || true)" != "$revision" ]]; then
    # Empty interrupted fetches can resume; existing checkouts are never reset.
    if git -C "$destination" rev-parse --verify HEAD >/dev/null 2>&1; then
      echo "Unexpected revision in managed build source: $destination" >&2; return 1
    fi
    GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND='ssh -oUseKeychain=no -oBatchMode=yes' \
      git -c credential.helper= -C "$destination" fetch --depth 1 origin "$revision"
    git -C "$destination" checkout --quiet --detach FETCH_HEAD
  fi
  [[ "$(git -C "$destination" rev-parse HEAD)" == "$revision" ]] || return 1
}

fetch_build_source containers be173da210c116c0013b7e9d6baf2d1531e07d5f
fetch_build_source optimizers 137fe713fc485687d9176ea4d50cddeba2f5fb44
fetch_build_source synth-mlx-rl 5d6db14330babcff170d2afbb8535de2138385a9
export SYNTH_CONTAINERS_PROJECT_ROOT="$BUILD_SOURCE_ROOT/containers-be173da210c116c0013b7e9d6baf2d1531e07d5f"
export SYNTH_MLX_RL_PROJECT_ROOT="$BUILD_SOURCE_ROOT/synth-mlx-rl-5d6db14330babcff170d2afbb8535de2138385a9"
export SYNTH_OPTIMIZER_DISTRIBUTION_SOURCE="$BUILD_SOURCE_ROOT/optimizers-137fe713fc485687d9176ea4d50cddeba2f5fb44"
