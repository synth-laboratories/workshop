#!/usr/bin/env bash
# Validate an embedded-engine POC receipt against the same evidence contract.
# This intentionally cannot manufacture a pass from a compile-only demo.
set -euo pipefail

RECEIPT="${1:-}"
[[ -f "$RECEIPT" ]] || { echo "usage: $0 RECEIPT.json" >&2; exit 2; }

jq -e '
  .schema == "workshop.embedded-browser-poc.v1" and
  (.backend | IN("cef-rs", "wry-wkwebview", "servo-webview")) and
  .workshopBuild.signed == true and
  .workshopBuild.notarized == true and
  .embedding.childSurface == true and
  .embedding.eventLoopCoexistence == true and
  .embedding.focus == true and
  .embedding.keyboard == true and
  .embedding.mouse == true and
  .embedding.ime == true and
  .embedding.resize == true and
  .profile.persistence == true and
  .packaging.hardenedRuntime == true and
  .packaging.updaterRoundTrip == true and
  .stability.gpuMinutes >= 30 and
  .stability.rendererCrashIsolated == true and
  .stability.browserCrashRecovered == true and
  .protocol.exampleDotCom == true and
  .protocol.craftaxBounded == true and
  .protocol.spaMutation == true and
  .protocol.staleRefsFailClosed == true and
  .protocol.userTabsPreserved == true and
  (.measurements.coldStartupMs | numbers) and
  (.measurements.warmStartupMs | numbers) and
  (.measurements.snapshotP95Ms | numbers) and
  (.measurements.idleRssBytes | numbers) and
  (.measurements.activeRssBytes | numbers) and
  (.measurements.bundleBytes | numbers)
' "$RECEIPT" >/dev/null || {
  echo "[embedded-bakeoff] FAIL: missing or false production gate in $RECEIPT" >&2
  exit 1
}

echo "[embedded-bakeoff] PASS: $(jq -r .backend "$RECEIPT") satisfies the Workshop POC evidence contract"
