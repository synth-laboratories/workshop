#!/usr/bin/env bash
# Stage the bundled VictoriaLogs executable for the diagnostics index.
#
#   ./scripts/diagnostics/fetch-victorialogs.sh            # pinned version
#   ./scripts/diagnostics/fetch-victorialogs.sh v1.52.0    # override
#
# The binary is not committed: it is a multi-megabyte third-party executable
# that changes on its own release cadence. This script puts it where
# `tauri.conf.json` expects it, so a packaged build carries it at
#   Synth Workshop.app/Contents/Resources/services/victoria-logs/victoria-logs
# and a development build finds it in the checkout.
#
# Nothing here is required for Workshop to run. Without the binary, diagnostics
# report `degraded` and every query answers from the authoritative journal.
set -euo pipefail

VERSION="${1:-${VICTORIALOGS_VERSION:-v1.52.0}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST_DIR="$ROOT/services/victoria-logs"
DEST="$DEST_DIR/victoria-logs"

case "$(uname -s)" in
  Darwin) OS="darwin" ;;
  Linux) OS="linux" ;;
  *) echo "[victoria-logs] unsupported platform $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) ARCH="arm64" ;;
  x86_64|amd64) ARCH="amd64" ;;
  *) echo "[victoria-logs] unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

ASSET="victoria-logs-${OS}-${ARCH}-${VERSION}.tar.gz"
URL="https://github.com/VictoriaMetrics/VictoriaLogs/releases/download/${VERSION}/${ASSET}"

mkdir -p "$DEST_DIR"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "[victoria-logs] fetching ${VERSION} (${OS}/${ARCH})"
curl --fail --location --silent --show-error --output "$WORK/$ASSET" "$URL"
tar -xzf "$WORK/$ASSET" -C "$WORK"

# The archive ships `victoria-logs-prod`; the app looks for `victoria-logs`.
BINARY="$(find "$WORK" -type f -name 'victoria-logs*' -perm -u+x | head -1)"
if [[ -z "$BINARY" ]]; then
  echo "[victoria-logs] archive contained no executable" >&2
  exit 1
fi
install -m 0755 "$BINARY" "$DEST"

# A bundled executable is signed with the app on macOS; sign it here too so
# `cargo tauri dev` can launch it without Gatekeeper killing the child.
if [[ "$OS" == "darwin" ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - --timestamp=none "$DEST" >/dev/null 2>&1 || \
    echo "[victoria-logs] ad-hoc signing failed; run scripts/setup-desktop-dev-signing.sh" >&2
fi

"$DEST" -version 2>/dev/null | head -1 || true
echo "[victoria-logs] staged at $DEST"
