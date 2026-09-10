#!/usr/bin/env bash
# Packaged managed-browser runtime gate.
#
# Builds real packaged layouts around an assembled runtime and proves that the
# readiness probe and the backend resolve the same pinned Node, the same
# Playwright package and the same Chromium — including when PATH offers a
# different interpreter, and including the ways a runtime can be incomplete.
#
# Read-only with respect to the assembled runtime: every layout links to it and
# all writable state (profiles, screenshots, pages) lives in a scratch tree.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BROWSER="$ROOT/apps/synth_desktop/browser"
LOCK="$BROWSER/runtime.lock.json"
RUNTIME="${SYNTH_BROWSER_RUNTIME_ROOT:-$BROWSER/runtime}"

pass=0
note() { echo "[packaged-runtime] $*"; }
die() { echo "[packaged-runtime] FAIL: $*" >&2; exit 1; }
ok() { pass=$((pass + 1)); echo "[packaged-runtime] ok: $*"; }

if [[ ! -f "$RUNTIME/manifest.json" ]]; then
  echo "[packaged-runtime] SKIP: no assembled runtime at $RUNTIME" >&2
  echo "[packaged-runtime] assemble one with scripts/build-browser-runtime.sh assemble," >&2
  echo "[packaged-runtime] or point SYNTH_BROWSER_RUNTIME_ROOT at an assembled runtime." >&2
  exit 2
fi

SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/workshop-packaged-runtime.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

RUNTIME_NODE="$RUNTIME/node/bin/node"
LOCK_NODE="v$("$RUNTIME_NODE" -e "process.stdout.write(require('$LOCK').node.version)")"
LOCK_PLAYWRIGHT="$("$RUNTIME_NODE" -e "process.stdout.write(require('$LOCK').playwright.version)")"

# A packaged application layout: the backend and probe live in
# Contents/Resources/browser, the runtime nested one level below them. No
# static specifier reaches the nested package, which is the failure this gate
# exists to catch.
layout() {
  local name="$1" runtime_link="$2"
  local resources="$SCRATCH/$name.app/Contents/Resources/browser"
  mkdir -p "$resources"
  cp "$BROWSER/playwright_backend.mjs" "$BROWSER/readiness_probe.mjs" "$resources/"
  [[ -n "$runtime_link" ]] && ln -s "$runtime_link" "$resources/runtime"
  echo "$resources"
}

# A partial runtime that links to the real artifacts it does keep, so an
# incomplete-bundle case never copies half a gigabyte of Chromium.
partial_runtime() {
  local name="$1" keep_node="$2" keep_package="$3" keep_browsers="$4"
  local root
  root="$SCRATCH/$name-runtime"
  mkdir -p "$root"
  [[ "$keep_node" == yes ]] && ln -s "$RUNTIME/node" "$root/node"
  [[ "$keep_package" == yes ]] && ln -s "$RUNTIME/node_modules" "$root/node_modules"
  if [[ "$keep_browsers" == yes ]]; then ln -s "$RUNTIME/browsers" "$root/browsers"; else mkdir -p "$root/browsers"; fi
  echo "$root"
}

probe() {
  local resources="$1" root="$2" node="$3"
  SYNTH_BROWSER_RUNTIME_ROOT="$root" PLAYWRIGHT_BROWSERS_PATH="$root/browsers" \
    "$node" "$resources/readiness_probe.mjs"
}

field() { "$RUNTIME_NODE" -e "process.stdout.write(String(JSON.parse(process.argv[1])[process.argv[2]] ?? ''))" "$1" "$2"; }

# --- 1. Packaged-path selection -------------------------------------------
PACKAGED="$(layout packaged "$RUNTIME")"
report="$(probe "$PACKAGED" "$PACKAGED/runtime" "$PACKAGED/runtime/node/bin/node")"
[[ "$(field "$report" playwright)" == "true" ]] || die "packaged layout could not load Playwright: $report"
[[ "$(field "$report" chromium)" == "true" ]] || die "packaged layout has no Chromium: $report"
[[ "$(field "$report" version)" == "$LOCK_PLAYWRIGHT" ]] || die "packaged Playwright is not the pinned $LOCK_PLAYWRIGHT: $report"
[[ "$(field "$report" specifier)" == "$PACKAGED/runtime/node_modules/playwright" ]] || die "probe resolved outside the packaged runtime: $report"
[[ "$("$PACKAGED/runtime/node/bin/node" --version)" == "$LOCK_NODE" ]] || die "packaged Node is not the pinned $LOCK_NODE"
ok "packaged layout resolves the pinned Node, Playwright $LOCK_PLAYWRIGHT and its Chromium"

# The static specifier the backend used before this gate existed. Kept as an
# explicit regression: if this ever starts resolving, the nested runtime is no
# longer the thing under test.
if (cd "$PACKAGED" && "$PACKAGED/runtime/node/bin/node" --input-type=module -e "import 'playwright'") 2>/dev/null; then
  die "a bare 'playwright' specifier resolved from the packaged layout; the gate is measuring the wrong tree"
fi
ok "a bare Playwright specifier still cannot reach the nested packaged runtime"

# --- 2. Misleading PATH ----------------------------------------------------
mkdir -p "$SCRATCH/fake-bin"
cat > "$SCRATCH/fake-bin/node" <<'FAKE'
#!/bin/sh
# A plausible-looking developer interpreter that must never be selected.
if [ "$1" = "--version" ]; then echo "v26.7.0"; exit 0; fi
echo "misleading PATH interpreter was used" >&2
exit 1
FAKE
chmod +x "$SCRATCH/fake-bin/node"
[[ "$(PATH="$SCRATCH/fake-bin:$PATH" node --version)" == "v26.7.0" ]] || die "misleading PATH fixture is not in effect"
report="$(PATH="$SCRATCH/fake-bin:$PATH" probe "$PACKAGED" "$PACKAGED/runtime" "$PACKAGED/runtime/node/bin/node")"
[[ "$(field "$report" playwright)" == "true" && "$(field "$report" chromium)" == "true" ]] \
  || die "a misleading PATH changed the packaged readiness result: $report"
ok "readiness is unchanged by a misleading PATH interpreter (fixture reports v26.7.0)"

# --- 3. Missing Playwright package ----------------------------------------
NO_PACKAGE="$(partial_runtime no-package yes no yes)"
MISSING_PKG="$(layout missing-package "$NO_PACKAGE")"
report="$(probe "$MISSING_PKG" "$MISSING_PKG/runtime" "$MISSING_PKG/runtime/node/bin/node")"
[[ "$(field "$report" playwright)" == "false" ]] || die "a runtime without Playwright reported ready: $report"
[[ -n "$(field "$report" error)" ]] || die "a missing package produced no reported reason: $report"
set +e
SYNTH_BROWSER_RUNTIME_ROOT="$MISSING_PKG/runtime" "$MISSING_PKG/runtime/node/bin/node" \
  "$MISSING_PKG/playwright_backend.mjs" </dev/null >/dev/null 2>"$SCRATCH/missing-package.err"
status=$?
set -e
[[ $status -eq 78 ]] || die "backend exited $status for a missing package; expected the named 78"
grep -q "browser_runtime_unavailable" "$SCRATCH/missing-package.err" || die "backend did not name the runtime fault: $(cat "$SCRATCH/missing-package.err")"
ok "a missing Playwright package fails closed and names the unreachable path"

# --- 4. Missing Chromium ---------------------------------------------------
NO_BROWSER="$(partial_runtime no-browser yes yes no)"
MISSING_BROWSER="$(layout missing-browser "$NO_BROWSER")"
report="$(probe "$MISSING_BROWSER" "$MISSING_BROWSER/runtime" "$MISSING_BROWSER/runtime/node/bin/node")"
[[ "$(field "$report" playwright)" == "true" ]] || die "the package should still load without a browser: $report"
[[ "$(field "$report" chromium)" == "false" ]] || die "an empty browser cache reported Chromium present: $report"
[[ -n "$(field "$report" chromiumPath)" ]] || die "readiness did not report the Chromium path it checked: $report"
ok "a missing Chromium is reported separately, with the path that was checked"

# --- 5. Clean startup, navigation and screenshot --------------------------
"$PACKAGED/runtime/node/bin/node" - "$PACKAGED" "$SCRATCH" <<'NODE'
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import readline from "node:readline";

const [resources, scratch] = process.argv.slice(2);
const page = `<!doctype html><html><body><h1>Packaged runtime test page</h1>
<p>Served from the loopback interface for the packaged managed-browser gate.</p>
<button id="apply">Apply</button></body></html>`;

const server = http.createServer((_request, response) => {
  response.setHeader("content-type", "text/html");
  response.end(page);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const profiles = path.join(scratch, "profiles");

const runtime = path.join(resources, "runtime");
const child = spawn(path.join(runtime, "node/bin/node"), [path.join(resources, "playwright_backend.mjs")], {
  env: {
    PATH: process.env.PATH,
    HOME: process.env.HOME,
    TMPDIR: process.env.TMPDIR,
    SYNTH_BROWSER_RUNTIME_ROOT: runtime,
    PLAYWRIGHT_BROWSERS_PATH: path.join(runtime, "browsers"),
    SYNTH_BROWSER_HEADLESS: "1",
    SYNTH_BROWSER_ALLOWED_ORIGINS: origin,
    SYNTH_BROWSER_PROFILE_ROOT: profiles,
  },
  stdio: ["pipe", "pipe", "inherit"],
});
const lines = readline.createInterface({ input: child.stdout });
const pending = new Map();
let sequence = 0;
lines.on("line", (line) => {
  const message = JSON.parse(line);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  message.ok ? waiter.resolve(message.response) : waiter.reject(new Error(message.error));
});
const call = (operation, args = {}) => new Promise((resolve, reject) => {
  const id = ++sequence;
  pending.set(id, { resolve, reject });
  child.stdin.write(`${JSON.stringify({ id, operation, arguments: args })}\n`);
});

try {
  const created = await call("browser_create_session", { profile: "packaged-gate" });
  const session_id = created.result.sessionId;
  const tab_id = created.result.tabId;
  await call("browser_navigate", { session_id, tab_id, url: `${origin}/` });
  const snapshot = await call("browser_snapshot", { session_id, tab_id });
  assert.match(snapshot.result.text, /Packaged runtime test page/, "navigated page was not observed");
  const shot = await call("browser_screenshot", { session_id, tab_id });
  const file = shot.result.path;
  assert.ok(file && fs.existsSync(file), `screenshot path was not produced: ${JSON.stringify(shot)}`);
  assert.equal(fs.readFileSync(file).subarray(1, 4).toString("latin1"), "PNG", "screenshot is not a PNG");
  assert.ok(path.resolve(file).startsWith(path.resolve(profiles)), "screenshot escaped the managed profile root");
  // The origin policy is still enforced by the packaged runtime, not bypassed
  // by the new resolution path.
  await assert.rejects(
    call("browser_navigate", { session_id, tab_id, url: "https://example.com/" }),
    /not approved|fail|origin/i,
    "an unapproved origin was navigated from the packaged runtime",
  );
  await call("browser_close_session", { session_id });
  console.log("[packaged-runtime] ok: clean startup, loopback navigation and screenshot from the packaged runtime");
} finally {
  child.stdin.end();
  child.kill("SIGKILL");
  server.close();
}
NODE
pass=$((pass + 1))

# --- 6. A real finalized application bundle -------------------------------
# Optional: `scripts/tests/browser-packaged-runtime.sh <App.app>` re-runs the
# resolution against a bundle produced by finalize-browser-app.sh, which is the
# layout the shipped host resolves through current_exe().
APP="${1:-}"
if [[ -n "$APP" ]]; then
  [[ -d "$APP" && "$APP" == *.app ]] || die "not an application bundle: $APP"
  APP_BROWSER="$APP/Contents/Resources/browser"
  for required in playwright_backend.mjs readiness_probe.mjs runtime/manifest.json; do
    [[ -e "$APP_BROWSER/$required" ]] || die "the bundle does not ship browser/$required"
  done
  report="$(probe "$APP_BROWSER" "$APP_BROWSER/runtime" "$APP_BROWSER/runtime/node/bin/node")"
  [[ "$(field "$report" playwright)" == "true" && "$(field "$report" chromium)" == "true" ]] \
    || die "the finalized bundle cannot resolve its own runtime: $report"
  [[ "$(field "$report" version)" == "$LOCK_PLAYWRIGHT" ]] || die "bundle Playwright drifted from the lock: $report"
  [[ "$("$APP_BROWSER/runtime/node/bin/node" --version)" == "$LOCK_NODE" ]] || die "bundle Node drifted from the lock"
  ok "the finalized bundle ships and resolves its own pinned runtime"
fi

note "$pass checks passed against $RUNTIME"
