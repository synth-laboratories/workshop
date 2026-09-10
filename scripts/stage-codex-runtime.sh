#!/usr/bin/env bash
# npm ci verifies the lockfile integrity; stage its pinned native runtime.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/node_modules/@openai/codex-darwin-arm64"
expected=0.153.0
actual="$(node -p 'require(process.argv[1]).version' "$root/node_modules/@openai/codex/package.json")"
[[ "$actual" == "$expected" ]] || { echo "Expected Codex $expected; run npm ci." >&2; exit 1; }
vendor="$package/vendor/aarch64-apple-darwin"
[[ -x "$vendor/bin/codex" && -x "$vendor/codex-path/rg" ]] || {
  echo 'Pinned native Codex package is missing. Run npm ci with optional dependencies enabled.' >&2; exit 1;
}
destination="$root/runtime-distributions/codex"
mkdir -p "$destination"
# Preserve the complete upstream layout, including code-mode host and zsh.
/usr/bin/ditto "$vendor" "$destination"
cp "$root/resources/codex/LICENSE" "$destination/LICENSE"
cp "$root/resources/codex/NOTICE" "$destination/NOTICE"
echo "[codex] staged native Codex $expected (no Node required in the installed app)"
