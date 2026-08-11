#!/usr/bin/env bash
# Canonical Synth Desktop release cut (macOS arm64, ad-hoc signed, UNNOTARIZED).
#
# One script = one build authority. Every release ZIP MUST come out of this
# script so there is exactly one digest per version, recorded once, in one
# receipt. Hand-running the steps produces a different codesign-verified ZIP
# of the same source every time (ad-hoc signing is not reproducible) — that is
# how v0.1.0 grew four "identical" artifacts in one day. Do not do that.
#
# Usage:
#   ./scripts/release_desktop.sh <version> [--source-ref <ref>] [--gh-release] [--allow-off-main]
#
#   <version>         Public release version, e.g. 0.2.0 (leading "v" ok).
#   --source-ref REF  Commit to build (default: HEAD of origin/main).
#   --allow-off-main  Permit a source ref that is not on origin/main.
#   --gh-release      Actually create the GitHub release (default OFF; the
#                     exact command is printed either way).
#
# Process (mirrors apps/synth_desktop/PROVENANCE.md + the 077579a rebuild):
#   1. fetch origin/main; resolve + announce the exact source SHA
#   2. build in a clean detached `git worktree` of that SHA:
#        npm ci && ./scripts/desktop.sh build
#   3. ditto the bundle .app into a staging dir; ditto the synth-*-mcp
#      adapter binaries from target/release into Contents/MacOS
#   4. codesign --force --deep --options runtime --sign -   (IN PLACE in the
#      staging dir — AMFI kills signed binaries that are copied after signing)
#   5. codesign --verify --deep --strict
#   6. ditto -c -k --keepParent  ->  Synth-Desktop-v<ver>-macOS-arm64-UNNOTARIZED.zip
#   7. write RECEIPT.txt (source SHA, ZIP SHA-256 + bytes, CFBundle versions,
#      per-Mach-O digests, CDHash, backend + gateway prod SHAs at build time)
#      plus paste-ready PROVENANCE.md and frontend desktopRelease constants.
#
# Output: dist/release/v<version>/ (gitignored).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Synth Desktop.app"
ADAPTERS=(synth-containers-mcp synth-visuals-mcp synth-optimizers-mcp)
BACKEND_VERSION_URLS=(
  "https://api.usesynth.ai/api/v1/version"
  "https://api.usesynth.ai/version"
)
GATEWAY_VERSION_URL="https://synth-responses-gateway-prod-production.up.railway.app/version"
RELEASE_URL_BASE="https://www.usesynth.ai/releases"

say()  { printf '[release] %s\n' "$*"; }
warn() { printf '[release] WARNING: %s\n' "$*" >&2; }
die()  { printf '[release] ERROR: %s\n' "$*" >&2; exit 1; }

usage() { sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; }

# ---------------------------------------------------------------- arguments
VERSION=""
SOURCE_REF="origin/main"
DO_GH_RELEASE=0
ALLOW_OFF_MAIN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --source-ref)
      [[ $# -ge 2 ]] || die "--source-ref requires an argument"
      SOURCE_REF="$2"; shift 2 ;;
    --gh-release) DO_GH_RELEASE=1; shift ;;
    --allow-off-main) ALLOW_OFF_MAIN=1; shift ;;
    --*) die "unknown flag: $1" ;;
    *)
      [[ -z "$VERSION" ]] || die "unexpected extra argument: $1"
      VERSION="$1"; shift ;;
  esac
done

[[ -n "$VERSION" ]] || { usage; die "version argument is required"; }
VERSION="${VERSION#v}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][A-Za-z0-9.-]+)?$ ]] \
  || die "version '$VERSION' does not look like semver (e.g. 0.2.0 or 0.2.0-rc1)"

# ---------------------------------------------------------------- preflight
[[ "$(uname -s)/$(uname -m)" == "Darwin/arm64" ]] \
  || die "this release is macOS arm64 only; refusing to build on $(uname -s)/$(uname -m)"

for tool in git npm curl shasum stat /usr/bin/ditto /usr/bin/codesign /usr/libexec/PlistBuddy; do
  command -v "$tool" >/dev/null 2>&1 || die "required tool not found: $tool"
done
if [[ "$DO_GH_RELEASE" -eq 1 ]]; then
  command -v gh >/dev/null 2>&1 || die "--gh-release requires the gh CLI"
fi

# Refuse a dirty invoking worktree. The build itself happens in a fresh
# detached worktree, but a dirty invoking tree usually means someone is about
# to release something that is not what they think it is.
dirty="$(git -C "$ROOT" status --porcelain 2>/dev/null || true)"
if [[ -n "$dirty" ]]; then
  printf '%s\n' "$dirty" | sed 's/^/[release]   /' >&2
  die "refusing to run from a dirty worktree ($ROOT). Commit or stash first."
fi

say "fetching origin/main ..."
git -C "$ROOT" fetch --quiet origin main || die "git fetch origin main failed"
MAIN_SHA="$(git -C "$ROOT" rev-parse origin/main)"

SOURCE_SHA="$(git -C "$ROOT" rev-parse --verify "${SOURCE_REF}^{commit}" 2>/dev/null)" \
  || die "cannot resolve --source-ref '$SOURCE_REF' to a commit"
SOURCE_SUBJECT="$(git -C "$ROOT" log -1 --format='%s' "$SOURCE_SHA")"

echo
echo "=============================================================================="
echo "  SYNTH DESKTOP RELEASE  v$VERSION"
echo "  source ref : $SOURCE_REF"
echo "  source SHA : $SOURCE_SHA"
echo "  subject    : $SOURCE_SUBJECT"
echo "  origin/main: $MAIN_SHA"
echo "=============================================================================="
echo

if ! git -C "$ROOT" merge-base --is-ancestor "$SOURCE_SHA" "$MAIN_SHA"; then
  if [[ "$ALLOW_OFF_MAIN" -eq 1 ]]; then
    warn "source $SOURCE_SHA is NOT on origin/main — proceeding because --allow-off-main was given"
  else
    die "source $SOURCE_SHA is not an ancestor of origin/main. Releases are cut from main. Pass --allow-off-main only if you really mean it."
  fi
fi

ZIP_NAME="Synth-Desktop-v${VERSION}-macOS-arm64-UNNOTARIZED.zip"
OUT_DIR="$ROOT/dist/release/v$VERSION"
ZIP_PATH="$OUT_DIR/$ZIP_NAME"
RECEIPT_PATH="$OUT_DIR/RECEIPT.txt"
PROVENANCE_SNIPPET_PATH="$OUT_DIR/PROVENANCE-section.md"
FRONTEND_SNIPPET_PATH="$OUT_DIR/desktopRelease-constants.ts"
ARTIFACT_URL="$RELEASE_URL_BASE/v$VERSION/$ZIP_NAME"

[[ -e "$ZIP_PATH" ]] && die "$ZIP_PATH already exists. One version = one digest; delete it explicitly if you intend to re-cut v$VERSION."
mkdir -p "$OUT_DIR"

# ------------------------------------------------------- clean build worktree
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/synth-desktop-release-v$VERSION.XXXXXX")"
BUILD_WT="$WORK_DIR/src"
SUCCESS=0

cleanup() {
  if [[ "$SUCCESS" -eq 1 ]]; then
    if [[ -d "$BUILD_WT" ]]; then
      git -C "$ROOT" worktree remove --force "$BUILD_WT" >/dev/null 2>&1 \
        || /bin/rm -rf "$BUILD_WT"
      git -C "$ROOT" worktree prune >/dev/null 2>&1 || true
    fi
    /bin/rm -rf "$WORK_DIR"
  else
    warn "build FAILED; leaving worktree for inspection: $BUILD_WT"
    warn "remove it later with: git -C \"$ROOT\" worktree remove --force \"$BUILD_WT\""
  fi
}
trap cleanup EXIT

say "creating clean detached build worktree at $BUILD_WT"
git -C "$ROOT" worktree add --detach "$BUILD_WT" "$SOURCE_SHA" >/dev/null
[[ -z "$(git -C "$BUILD_WT" status --porcelain)" ]] || die "fresh worktree is unexpectedly dirty"

say "npm ci (this installs the exact locked toolchain) ..."
(cd "$BUILD_WT" && npm ci --no-audit --no-fund) || die "npm ci failed"

say "building via the canonical ./scripts/desktop.sh build ..."
(cd "$BUILD_WT" && ./scripts/desktop.sh build) || die "desktop.sh build failed"

BUNDLE_APP="$BUILD_WT/apps/synth_desktop/src-tauri/target/release/bundle/macos/$APP_NAME"
[[ -d "$BUNDLE_APP" && -x "$BUNDLE_APP/Contents/MacOS/synth-desktop" ]] \
  || die "build did not produce $BUNDLE_APP"
for adapter in "${ADAPTERS[@]}"; do
  [[ -f "$BUILD_WT/apps/synth_desktop/src-tauri/target/release/$adapter" ]] \
    || die "build did not produce adapter binary target/release/$adapter"
done

# ------------------------------------------------------------ stage + sign
# Sign IN PLACE in the staging dir and zip from the same path. Copying a
# bundle after signing invalidates it under AMFI (copied signed binaries get
# killed on launch); ditto from the signed stage preserves the signature.
STAGE_DIR="$OUT_DIR/stage"
STAGE_APP="$STAGE_DIR/$APP_NAME"
/bin/rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

say "staging bundle -> $STAGE_APP"
/usr/bin/ditto "$BUNDLE_APP" "$STAGE_APP"
for adapter in "${ADAPTERS[@]}"; do
  /usr/bin/ditto \
    "$BUILD_WT/apps/synth_desktop/src-tauri/target/release/$adapter" \
    "$STAGE_APP/Contents/MacOS/$adapter"
done

say "codesign (ad-hoc, hardened runtime) + strict verify ..."
/usr/bin/codesign --force --deep --options runtime --sign - "$STAGE_APP"
/usr/bin/codesign --verify --deep --strict "$STAGE_APP" \
  || die "strict codesign verification FAILED"
say "codesign --verify --deep --strict: OK"

CODESIGN_DETAILS="$(/usr/bin/codesign -dvvv "$STAGE_APP" 2>&1 || true)"
CD_FLAGS="$(printf '%s\n' "$CODESIGN_DETAILS" | sed -n 's/^CodeDirectory .*flags=\(0x[0-9a-f]*(\([^)]*\))\).*/\2/p' | head -1)"
CDHASH_FULL="$(printf '%s\n' "$CODESIGN_DETAILS" | sed -n 's/^CandidateCDHashFull sha256=\(.*\)$/\1/p' | head -1)"

INFO_PLIST="$STAGE_APP/Contents/Info.plist"
CF_SHORT="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INFO_PLIST")"
CF_VER="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$INFO_PLIST")"
BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$INFO_PLIST")"
if [[ "$CF_SHORT" != "$VERSION" ]]; then
  warn "CFBundleShortVersionString ($CF_SHORT) != release version ($VERSION)."
  warn "Public path/version authority is the ZIP name; CFBundle comes from the source tree (see PROVENANCE.md)."
fi

# Per-Mach-O digest table for every executable in Contents/MacOS.
MACHO_TABLE=""
EXEC_SHA=""
while IFS= read -r bin; do
  name="$(basename "$bin")"
  size="$(stat -f %z "$bin")"
  sha="$(shasum -a 256 "$bin" | awk '{print $1}')"
  MACHO_TABLE+="$name|$size|$sha"$'\n'
  [[ "$name" == "synth-desktop" ]] && EXEC_SHA="$sha"
done < <(find "$STAGE_APP/Contents/MacOS" -maxdepth 1 -type f | sort)
[[ -n "$EXEC_SHA" ]] || die "main executable digest not captured"

# ------------------------------------------------------------------- package
say "packaging $ZIP_NAME via ditto ..."
(cd "$STAGE_DIR" && /usr/bin/ditto -c -k --keepParent "$APP_NAME" "$ZIP_PATH")
ZIP_SHA="$(shasum -a 256 "$ZIP_PATH" | awk '{print $1}')"
ZIP_BYTES="$(stat -f %z "$ZIP_PATH")"
BUILD_TS="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

# ------------------------------------------- live prod SHAs at publish time
extract_git_sha() {  # reads JSON on stdin; prints .build.git_sha or .git_sha
  if command -v jq >/dev/null 2>&1; then
    jq -r '(.build.git_sha // .git_sha) // empty' 2>/dev/null
  else
    /usr/bin/python3 -c 'import json,sys
d = json.load(sys.stdin)
print((d.get("build") or {}).get("git_sha") or d.get("git_sha") or "")' 2>/dev/null
  fi
}

BACKEND_SHA="UNAVAILABLE"
BACKEND_SRC="none"
for url in "${BACKEND_VERSION_URLS[@]}"; do
  body="$(curl -fsS --max-time 20 "$url" 2>/dev/null || true)"
  [[ -n "$body" ]] || continue
  sha="$(printf '%s' "$body" | extract_git_sha)"
  if [[ -n "$sha" ]]; then
    BACKEND_SHA="$sha"; BACKEND_SRC="$url"; break
  fi
done
[[ "$BACKEND_SHA" == "UNAVAILABLE" ]] && warn "could not fetch backend prod git_sha; receipt will say UNAVAILABLE"

GATEWAY_SHA="UNAVAILABLE"
sha="$(curl -fsS --max-time 20 "$GATEWAY_VERSION_URL" 2>/dev/null | extract_git_sha || true)"
[[ -n "$sha" ]] && GATEWAY_SHA="$sha"
[[ "$GATEWAY_SHA" == "UNAVAILABLE" ]] && warn "could not fetch gateway prod git_sha; receipt will say UNAVAILABLE"

# ------------------------------------------------------------------ receipt
{
  echo "Synth Desktop v$VERSION release artifact — built $BUILD_TS by $(whoami)@$(hostname -s) via scripts/release_desktop.sh"
  echo "Source: workshop $SOURCE_SHA ($SOURCE_REF; origin/main tip $MAIN_SHA) — clean detached worktree, npm ci, ./scripts/desktop.sh build"
  echo "Staging: ditto bundle -> stage; ditto ${ADAPTERS[*]} from target/release; codesign --force --deep --options runtime --sign - ; codesign --verify --deep --strict OK"
  echo "ZIP: $ZIP_NAME  $ZIP_BYTES bytes"
  echo "ZIP SHA-256: $ZIP_SHA"
  echo "Intended URL: $ARTIFACT_URL"
  echo "Bundle ID: $BUNDLE_ID"
  echo "CFBundleShortVersionString/CFBundleVersion: $CF_SHORT / $CF_VER"
  echo "CodeDirectory: ${CD_FLAGS:-unknown}; CDHashFull sha256=${CDHASH_FULL:-unknown}"
  echo "Main executable SHA-256: $EXEC_SHA"
  echo "Inner Mach-O sha256 (name|size|sha256):"
  printf '%s' "$MACHO_TABLE"
  echo "Backend prod /version at build time: git_sha $BACKEND_SHA (from $BACKEND_SRC)"
  echo "Gateway prod /version at build time: git_sha $GATEWAY_SHA (from $GATEWAY_VERSION_URL)"
} > "$RECEIPT_PATH"

# ------------------------------------------- paste-ready PROVENANCE section
{
  echo "## v$VERSION artifact (cut by scripts/release_desktop.sh)"
  echo
  echo "| Field | Value |"
  echo "| --- | --- |"
  echo "| Public ZIP | $ARTIFACT_URL |"
  echo "| Asset name | \`$ZIP_NAME\` |"
  echo "| Size (bytes) | \`$ZIP_BYTES\` |"
  echo "| SHA-256 | \`$ZIP_SHA\` |"
  echo "| Signing | ad-hoc (\`flags=${CD_FLAGS:-adhoc,runtime}\`) — **not** Apple-notarized |"
  echo "| Bundle ID | \`$BUNDLE_ID\` |"
  echo "| CFBundleShortVersionString / CFBundleVersion | \`$CF_SHORT\` / \`$CF_VER\` |"
  echo "| Source SHA | \`$SOURCE_SHA\` |"
  echo "| Built | $BUILD_TS |"
  echo "| CandidateCDHashFull (sha256) | \`${CDHASH_FULL:-unknown}\` |"
  echo
  echo "### Inner Mach-O digests (extracted app)"
  echo
  echo "| Path | Size | SHA-256 |"
  echo "| --- | ---: | --- |"
  printf '%s' "$MACHO_TABLE" | while IFS='|' read -r name size sha; do
    [[ -n "$name" ]] && echo "| \`Contents/MacOS/$name\` | $size | \`$sha\` |"
  done
  echo
  echo "### Backend + gateway tips at build time"
  echo
  echo "| Surface | git_sha |"
  echo "| --- | --- |"
  echo "| Prod backend (\`https://api.usesynth.ai\`) | \`$BACKEND_SHA\` |"
  echo "| Prod Responses gateway | \`$GATEWAY_SHA\` |"
} > "$PROVENANCE_SNIPPET_PATH"

# --------------------------------- paste-ready frontend release constants
{
  echo "// Paste into frontend src/lib/desktopRelease.ts wiring / Vercel env."
  echo "// Env suffix convention: ${VERSION} -> $(printf '%s' "$VERSION" | tr '.' '_')"
  echo "export const DESKTOP_STABLE_VERSION = \"$VERSION\";"
  echo "export const DESKTOP_STABLE_ARTIFACT = {"
  echo "	artifactUrl:"
  echo "		\"$ARTIFACT_URL\","
  echo "	artifactSha256:"
  echo "		\"$ZIP_SHA\""
  echo "} as const;"
} > "$FRONTEND_SNIPPET_PATH"

SUCCESS=1

# ------------------------------------------------------------------- output
echo
echo "=============================================================================="
echo "  RELEASE ARTIFACT READY"
echo "=============================================================================="
cat "$RECEIPT_PATH"
echo
echo "---- paste-ready PROVENANCE.md section ($PROVENANCE_SNIPPET_PATH) ----"
cat "$PROVENANCE_SNIPPET_PATH"
echo
echo "---- paste-ready frontend constants ($FRONTEND_SNIPPET_PATH) ----"
cat "$FRONTEND_SNIPPET_PATH"
echo

GH_CMD=(gh release create "v$VERSION" --target "$SOURCE_SHA" --title "Synth Desktop v$VERSION (Unnotarized)" "$ZIP_PATH")
if [[ "$DO_GH_RELEASE" -eq 1 ]]; then
  say "creating GitHub release v$VERSION ..."
  "${GH_CMD[@]}"
else
  say "GitHub release NOT created (--gh-release was not passed)."
  say "to publish, run exactly:"
  printf '  %q' "${GH_CMD[@]}"; echo
fi

echo
say "artifacts:"
say "  ZIP     : $ZIP_PATH"
say "  receipt : $RECEIPT_PATH"
say "  staged  : $STAGE_APP  (signed in place; verify with codesign --verify --deep --strict)"
say "done."
