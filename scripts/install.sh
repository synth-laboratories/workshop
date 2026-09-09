#!/usr/bin/env bash
# Install the dependencies needed for local Synth Workshop development.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="install"

usage() {
  cat <<'EOF'
Usage: ./scripts/install.sh [--check | --dry-run]

  (no option)  Check prerequisites, create .env if absent, and run npm ci
  --check      Check prerequisites only; do not change the checkout
  --dry-run    Check prerequisites and print planned changes without making them
EOF
}

fail() {
  printf '[install] ERROR: %s\n' "$*" >&2
  exit 1
}

note() {
  printf '[install] %s\n' "$*"
}

version_major() {
  printf '%s' "$1" | sed -E 's/^[^0-9]*([0-9]+).*/\1/'
}

require_command() {
  local command_name="$1" help_text="$2"
  command -v "$command_name" >/dev/null 2>&1 || fail "Missing $command_name. $help_text"
}

require_major() {
  local command_name="$1" minimum="$2" raw major
  raw="$($command_name --version 2>/dev/null | head -n 1)" || fail "Could not read the $command_name version."
  major="$(version_major "$raw")"
  [[ "$major" =~ ^[0-9]+$ ]] || fail "Could not parse the $command_name version from: $raw"
  (( major >= minimum )) || fail "$command_name $minimum+ is required; found $raw."
  note "$command_name: $raw"
}

case "${1:-}" in
  "") ;;
  --check) MODE="check" ;;
  --dry-run) MODE="dry-run" ;;
  --help|-h) usage; exit 0 ;;
  *) usage >&2; fail "Unknown option: $1" ;;
esac
[[ $# -le 1 ]] || { usage >&2; fail "Expected at most one option."; }

[[ "$(uname -s)" == "Darwin" ]] || fail "Workshop development currently supports macOS only."
[[ "$(uname -m)" == "arm64" ]] || fail "Workshop development currently supports Apple Silicon (arm64) only."

require_command sw_vers "Install or update macOS."
macos_major="$(sw_vers -productVersion | cut -d. -f1)"
[[ "$macos_major" =~ ^[0-9]+$ ]] || fail "Could not determine the macOS version."
(( macos_major >= 14 )) || fail "macOS 14 (Sonoma) or newer is required."
note "platform: macOS $(sw_vers -productVersion) arm64"

require_command xcode-select "Install the Xcode command-line tools with: xcode-select --install"
xcode_path="$(xcode-select -p 2>/dev/null)" || fail "Xcode command-line tools are not configured. Run: xcode-select --install"
[[ -d "$xcode_path" ]] || fail "Xcode developer directory does not exist: $xcode_path"
note "Xcode command-line tools: $xcode_path"
require_command swift "Install Xcode 16 or newer (Swift 6 is required by the terminal host)."
swift_major="$(swift --version | sed -nE 's/.*Swift version ([0-9]+).*/\1/p' | head -n 1)"
[[ "$swift_major" =~ ^[0-9]+$ ]] && (( swift_major >= 6 )) || \
  fail "Swift 6+ is required. Select Xcode 16+ with DEVELOPER_DIR before building."

require_command git "Install Git (included with the Xcode command-line tools)."
require_command node "Install Node.js 20 or newer."
require_command npm "Install npm 10 or newer."
require_command rustc "Install Rust from https://rustup.rs/."
require_command cargo "Install Rust from https://rustup.rs/."
require_command python3 "Install Python 3.11 or newer (required by runtime staging)."
require_command jq "Install jq (for example: brew install jq)."
require_command rg "Install ripgrep (for example: brew install ripgrep)."
require_command curl "Install the macOS command-line tools."
require_command lsof "Install the macOS command-line tools."
require_major node 20
require_major npm 10
require_major rustc 1
require_major python3 3
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)' || \
  fail "Python 3.11+ must be available as python3 on PATH (runtime staging uses tomllib)."

[[ -f "$ROOT/package.json" && -f "$ROOT/package-lock.json" ]] || \
  fail "Run this script from a complete Workshop checkout containing package.json and package-lock.json."

if [[ "$MODE" == "check" ]]; then
  note "prerequisite check passed"
  exit 0
fi

if [[ -e "$ROOT/.env" ]]; then
  note "preserving existing .env"
elif [[ "$MODE" == "dry-run" ]]; then
  note "would copy .env.example to .env with mode 0600"
else
  [[ -f "$ROOT/.env.example" ]] || fail "Missing .env.example; cannot create local configuration."
  cp "$ROOT/.env.example" "$ROOT/.env"
  chmod 600 "$ROOT/.env"
  note "created .env from .env.example (mode 0600)"
fi

if [[ "$MODE" == "dry-run" ]]; then
  note "would run: npm ci"
  note "dry run passed"
  exit 0
fi

note "installing locked npm workspace dependencies"
(cd "$ROOT" && npm ci)
note "installation complete"
note "configure .env if needed, then launch with: npm run desktop:dev"
