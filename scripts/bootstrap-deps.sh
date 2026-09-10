#!/usr/bin/env bash
# Invoked explicitly by install.sh --bootstrap, never by --check/--dry-run.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || {
  echo '[bootstrap] Requires an Apple Silicon Mac.' >&2; exit 1;
}
macos_major="$(sw_vers -productVersion | cut -d. -f1)"
[[ "$macos_major" =~ ^[0-9]+$ ]] && (( macos_major >= 14 )) || {
  echo '[bootstrap] Requires macOS 14 or newer.' >&2; exit 1;
}
if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  echo '[bootstrap] Finish the Apple command-line tools installer, then rerun the same command.' >&2
  exit 1
fi
swift_major="$(swift --version 2>/dev/null | sed -nE 's/.*Swift version ([0-9]+).*/\1/p' | head -n 1)" || swift_major=""
[[ "$swift_major" =~ ^[0-9]+$ ]] && (( swift_major >= 6 )) || {
  echo '[bootstrap] Update Apple command-line tools or select Xcode 16+ (Swift 6 required), then rerun. Apple installation/license prompts must be completed by you.' >&2
  exit 1
}
source "$ROOT/scripts/local-toolchain-env.sh"
if ! command -v brew >/dev/null 2>&1; then
  echo '[bootstrap] Installing Homebrew using its official interactive installer (may ask for administrator approval).'
  installer="$(mktemp "${TMPDIR:-/tmp}/workshop-homebrew.XXXXXX")"
  trap 'rm -f "$installer"' EXIT
  curl --fail --show-error --silent --location https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh -o "$installer"
  /bin/bash "$installer"
  source "$ROOT/scripts/local-toolchain-env.sh"
fi
echo '[bootstrap] Installing build tools via Homebrew; existing formula installations are reused.'
brew install node@20 python@3.12 rustup uv jq ripgrep git
source "$ROOT/scripts/local-toolchain-env.sh"
# Install the repository's stable toolchain without changing the global default.
(cd "$ROOT" && rustup toolchain install stable --profile minimal)
echo '[bootstrap] Build tools ready. Continuing with locked npm dependencies.'
