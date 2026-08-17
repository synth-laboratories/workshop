#!/usr/bin/env bash
# Assemble, verify, and optionally Developer-ID-sign Workshop's hermetic browser runtime.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOCK="$ROOT/apps/synth_desktop/browser/runtime.lock.json"
OUTPUT="${SYNTH_BROWSER_RUNTIME_OUTPUT:-$ROOT/apps/synth_desktop/browser/runtime}"
CACHE="${SYNTH_BROWSER_RUNTIME_CACHE:-$ROOT/apps/synth_desktop/src-tauri/target/browser-runtime-cache}"
COMMAND="${1:-help}"
SIGN_IDENTITY="${SYNTH_SIGN_IDENTITY:-}"
TEAM_ID="${SYNTH_TEAM_ID:-}"

note() { echo "[browser-runtime] $*"; }
die() { echo "[browser-runtime] ERROR: $*" >&2; exit 1; }

json_value() {
  "$NODE_FOR_LOCK" -e "const x=require(process.argv[1]); let v=x; for (const key of process.argv[2].split('.')) v=v[key]; process.stdout.write(String(v))" "$LOCK" "$1"
}

host_target() {
  [[ "$(uname -s)" == "Darwin" ]] || die "the visible v1 runtime currently supports macOS only"
  case "$(uname -m)" in
    arm64) echo darwin-arm64 ;;
    x86_64) echo darwin-x64 ;;
    *) die "unsupported macOS architecture $(uname -m)" ;;
  esac
}

sha256() { /usr/bin/shasum -a 256 "$1" | awk '{print $1}'; }

runtime_node() { echo "$OUTPUT/node/bin/node"; }

chromium_executable() {
  find "$OUTPUT/browsers" -type f -path '*/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' -print -quit
}

assemble() {
  mkdir -p "$CACHE"
  local target node_version archive expected actual url stage chromium_revision chromium_version playwright_version
  target="$(host_target)"
  node_version="$(json_value node.version)"
  playwright_version="$(json_value playwright.version)"
  chromium_revision="$(json_value playwright.chromiumRevision)"
  chromium_version="$(json_value playwright.chromiumVersion)"
  archive="node-v${node_version}-${target}.tar.gz"
  expected="$(json_value "node.${target}.sha256")"
  url="https://nodejs.org/dist/v${node_version}/${archive}"
  if [[ ! -f "$CACHE/$archive" || "$(sha256 "$CACHE/$archive")" != "$expected" ]]; then
    note "downloading pinned Node.js $node_version for $target"
    /usr/bin/curl --fail --location --proto '=https' --tlsv1.2 "$url" -o "$CACHE/$archive.part"
    actual="$(sha256 "$CACHE/$archive.part")"
    [[ "$actual" == "$expected" ]] || die "Node archive digest $actual does not match lock $expected"
    mv "$CACHE/$archive.part" "$CACHE/$archive"
  fi

  stage="$(mktemp -d "${TMPDIR:-/tmp}/workshop-browser-runtime.XXXXXX")"
  trap 'rm -rf "$stage"' RETURN
  mkdir -p "$stage/node" "$stage/node_modules" "$stage/browsers" "$stage/licenses"
  /usr/bin/tar -xzf "$CACHE/$archive" -C "$stage/node" --strip-components=1
  [[ "$("$stage/node/bin/node" --version)" == "v${node_version}" ]] || die "staged Node version drift"
  [[ -f "$ROOT/node_modules/playwright/package.json" ]] || die "run npm ci at the repository root first"
  [[ "$("$stage/node/bin/node" -p "require('$ROOT/node_modules/playwright/package.json').version")" == "$playwright_version" ]] || die "installed Playwright does not match runtime lock"
  /usr/bin/ditto "$ROOT/node_modules/playwright" "$stage/node_modules/playwright"
  /usr/bin/ditto "$ROOT/node_modules/playwright-core" "$stage/node_modules/playwright-core"
  [[ -d "$ROOT/node_modules/fsevents" ]] && /usr/bin/ditto "$ROOT/node_modules/fsevents" "$stage/node_modules/fsevents"
  cp "$stage/node/LICENSE" "$stage/licenses/Node-LICENSE"
  cp "$stage/node_modules/playwright/LICENSE" "$stage/licenses/Playwright-LICENSE"
  # The browser host needs the Node executable, not npm/corepack, headers, docs,
  # or the package manager dependency tree. Keeping those would add roughly
  # forty megabytes of unsigned, unreachable release surface.
  find "$stage/node/bin" -mindepth 1 ! -name node -delete
  rm -rf "$stage/node/include" "$stage/node/lib" "$stage/node/share"

  note "installing Playwright Chromium $chromium_version (revision $chromium_revision)"
  local browser_cache="$CACHE/playwright-browsers"
  mkdir -p "$browser_cache"
  PLAYWRIGHT_BROWSERS_PATH="$browser_cache" PLAYWRIGHT_SKIP_BROWSER_GC=1 \
    "$stage/node/bin/node" "$stage/node_modules/playwright/cli.js" install --no-shell chromium
  [[ -d "$browser_cache/chromium-$chromium_revision" ]] || die "Playwright browser revision drift"
  /usr/bin/ditto "$browser_cache/chromium-$chromium_revision" "$stage/browsers/chromium-$chromium_revision"
  # FFmpeg is used for Playwright video recording. Workshop does not expose
  # video capture, so it remains only in the build cache and is never bundled.
  local chromium
  chromium="$(find "$stage/browsers" -type f -path '*/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' -print -quit)"
  [[ -n "$chromium" && -x "$chromium" ]] || die "Playwright did not install full headed Chromium"

  "$stage/node/bin/node" - "$stage" "$LOCK" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const [root, lockPath] = process.argv.slice(2);
const digest = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
  const item = path.join(dir, entry.name);
  return entry.isDirectory() ? walk(item) : [item];
});
const important = [path.join(root, 'node/bin/node'), path.join(root, 'node_modules/playwright/package.json'), path.join(root, 'node_modules/playwright-core/browsers.json')];
const chrome = walk(path.join(root, 'browsers')).find((file) => file.endsWith('/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));
important.push(chrome);
const manifest = {
  schemaVersion: 'workshop.browser-runtime.v1',
  lock: JSON.parse(fs.readFileSync(lockPath, 'utf8')),
  files: important.map((file) => ({ path: path.relative(root, file), sha256: digest(file), bytes: fs.statSync(file).size })),
};
fs.writeFileSync(path.join(root, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', { mode: 0o600 });
NODE
  rm -rf "$OUTPUT"
  mkdir -p "$(dirname "$OUTPUT")"
  mv "$stage" "$OUTPUT"
  trap - RETURN
  note "assembled $(du -sh "$OUTPUT" | awk '{print $1}') at $OUTPUT"
  verify
}

verify() {
  [[ -x "$(runtime_node)" ]] || die "missing bundled Node at $(runtime_node)"
  [[ -f "$OUTPUT/manifest.json" ]] || die "missing runtime manifest"
  local chrome
  chrome="$(chromium_executable)"
  [[ -n "$chrome" && -x "$chrome" ]] || die "full headed Chromium is missing"
  "$(runtime_node)" - "$OUTPUT" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const root = process.argv[2];
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'manifest.json'), 'utf8'));
for (const item of manifest.files) {
  const file = path.join(root, item.path);
  const digest = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  if (digest !== item.sha256) throw new Error(`runtime digest mismatch: ${item.path}`);
}
NODE
  SYNTH_BROWSER_RUNTIME_ROOT="$OUTPUT" PLAYWRIGHT_BROWSERS_PATH="$OUTPUT/browsers" \
    "$(runtime_node)" --input-type=module -e "import fs from 'node:fs'; import path from 'node:path'; import { createRequire } from 'node:module'; const require=createRequire(import.meta.url); const { chromium }=require(path.join(process.env.SYNTH_BROWSER_RUNTIME_ROOT,'node_modules/playwright')); if (!fs.existsSync(chromium.executablePath())) process.exit(9)"
  if [[ -n "$TEAM_ID" ]]; then
    /usr/bin/codesign --verify --strict "$OUTPUT/node/bin/node"
    /usr/bin/codesign --verify --strict "$chrome"
    /usr/bin/codesign --verify -R "anchor apple generic and certificate leaf[subject.OU] = \"$TEAM_ID\"" "$OUTPUT/node/bin/node"
  fi
  note "runtime verified"
}

sign_runtime() {
  [[ -n "$SIGN_IDENTITY" ]] || die "SYNTH_SIGN_IDENTITY is required"
  verify
  local chrome_app
  chrome_app="$(dirname "$(dirname "$(dirname "$(chromium_executable)")")")"
  note "signing Chromium nested code with hardened runtime"
  while IFS= read -r executable; do
    /usr/bin/codesign --force --sign "$SIGN_IDENTITY" --options runtime --timestamp "$executable"
  done < <(find "$chrome_app/Contents" -type f -perm +111 | sort -r)
  /usr/bin/codesign --force --deep --sign "$SIGN_IDENTITY" --options runtime --timestamp "$chrome_app"
  /usr/bin/codesign --force --sign "$SIGN_IDENTITY" --options runtime --timestamp "$OUTPUT/node/bin/node"
  verify
  note "runtime signed"
}

case "$COMMAND" in
  assemble) NODE_FOR_LOCK="${SYNTH_BROWSER_NODE:-$(command -v node)}"; assemble ;;
  verify) NODE_FOR_LOCK="${SYNTH_BROWSER_NODE:-$(command -v node)}"; verify ;;
  sign) NODE_FOR_LOCK="${SYNTH_BROWSER_NODE:-$(command -v node)}"; sign_runtime ;;
  help|--help|-h)
    echo "Usage: $0 assemble|verify|sign"
    ;;
  *) die "unknown command $COMMAND" ;;
esac
