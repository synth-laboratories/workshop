#!/usr/bin/env bash
# Stage the bundled VictoriaLogs executable for the diagnostics index.
#
#   ./scripts/diagnostics/fetch-victorialogs.sh               # pinned version
#   ./scripts/diagnostics/fetch-victorialogs.sh --if-missing  # packaging hook
#   VICTORIALOGS_SHA256=<sha256> ./scripts/diagnostics/fetch-victorialogs.sh v1.53.0
#
# The binary is not committed: it is a multi-megabyte third-party executable
# that changes on its own release cadence. This script puts it where
# `tauri.package.json` bundles it, so a packaged build carries it at
#   Synth Workshop.app/Contents/Resources/services/victoria-logs/victoria-logs
# and a development build finds it in the checkout.
#
# Every packaged `tauri build` runs this through `package:stage-diagnostics`
# (tauri.package.json beforeBuildCommand). Without it the bundle silently
# shipped an empty services/victoria-logs directory and the instance log
# store reported `binary_missing`. The archive checksum is pinned per
# platform; an unpinned version or platform must supply VICTORIALOGS_SHA256.
#
# Workshop itself still runs without the binary: diagnostics report
# `degraded` and every query answers from the authoritative journal.
set -euo pipefail

PINNED_VERSION="v1.52.0"
IF_MISSING=0
POSITIONAL=()
for arg in "$@"; do
  case "$arg" in
    --if-missing) IF_MISSING=1 ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
VERSION="${POSITIONAL[0]:-${VICTORIALOGS_VERSION:-$PINNED_VERSION}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST_DIR="$ROOT/services/victoria-logs"
DEST="$DEST_DIR/victoria-logs"
STAMP="$DEST_DIR/.staged-version"

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

if [[ "$IF_MISSING" == "1" && -x "$DEST" && -f "$STAMP" && "$(cat "$STAMP")" == "$VERSION/$OS/$ARCH" ]]; then
  echo "[victoria-logs] $VERSION ($OS/$ARCH) already staged at $DEST"
  exit 0
fi

pinned_sha256() {
  case "$1" in
    v1.52.0/darwin/arm64) echo "3157d4b6181d8a7e3e30918e2cbfcd4cc4cb66263e3ef21ea91e4f20f8980883" ;;
    v1.52.0/darwin/amd64) echo "5ac429b81dfa007c258c537eeb63eb59bd6a8f10e8686507970c18a1b3d2dd5a" ;;
    *) echo "" ;;
  esac
}
EXPECTED="${VICTORIALOGS_SHA256:-$(pinned_sha256 "$VERSION/$OS/$ARCH")}"
if [[ -z "$EXPECTED" ]]; then
  echo "[victoria-logs] no pinned checksum for $VERSION ($OS/$ARCH); set VICTORIALOGS_SHA256" >&2
  exit 1
fi

ASSET="victoria-logs-${OS}-${ARCH}-${VERSION}.tar.gz"
URL="https://github.com/VictoriaMetrics/VictoriaLogs/releases/download/${VERSION}/${ASSET}"

mkdir -p "$DEST_DIR"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "[victoria-logs] fetching ${VERSION} (${OS}/${ARCH})"
curl --fail --location --silent --show-error --output "$WORK/$ASSET" "$URL"
if command -v shasum >/dev/null 2>&1; then
  ACTUAL="$(shasum -a 256 "$WORK/$ASSET" | awk '{print $1}')"
else
  ACTUAL="$(sha256sum "$WORK/$ASSET" | awk '{print $1}')"
fi
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "[victoria-logs] checksum mismatch for $ASSET (expected $EXPECTED, got $ACTUAL)" >&2
  exit 1
fi
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
printf '%s\n' "$VERSION/$OS/$ARCH" >"$STAMP"
echo "[victoria-logs] staged at $DEST"
