#!/usr/bin/env bash
# Fail-closed preflight for the real Workshop CEF/cef-rs embedded-browser POC.
# This does not turn a standalone cefsimple window into an embedding claim.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMAND="${1:-preflight}"
RECEIPT="${SYNTH_CEF_POC_RECEIPT:-${TMPDIR:-/tmp}/workshop-cef-poc-preflight.json}"
CEF_RS_VERSION="${SYNTH_CEF_RS_VERSION:-151.6.0+151.3.18}"

die() { echo "[cef-poc] ERROR: $*" >&2; exit 1; }

preflight() {
  local architecture macos_version xcode_version rust_version identity_count build_ready=true production_ready=true
  local -a build_blockers=() production_blockers=()
  architecture="$(uname -m)"
  macos_version="$(sw_vers -productVersion)"
  rust_version="$(rustc --version 2>/dev/null || true)"
  if xcode_version="$(xcodebuild -version 2>/dev/null)"; then
    :
  else
    xcode_version="unavailable (active developer directory: $(xcode-select -p 2>/dev/null || echo unknown))"
    build_blockers+=("full_xcode_required")
    build_ready=false
    production_ready=false
  fi
  identity_count="$(security find-identity -v -p codesigning 2>/dev/null | awk '/valid identities found/{print $1; exit}')"
  identity_count="${identity_count:-0}"
  if [[ "$identity_count" -lt 1 ]]; then production_blockers+=("codesigning_identity_required"); production_ready=false; fi
  if [[ -z "${SYNTH_NOTARY_PROFILE:-}" ]]; then production_blockers+=("notary_profile_required"); production_ready=false; fi
  jq -n \
    --arg schema "workshop.cef-poc-preflight.v1" --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    --arg architecture "$architecture" --arg macOS "$macos_version" --arg xcode "$xcode_version" \
    --arg rust "$rust_version" --arg cefRsVersion "$CEF_RS_VERSION" --argjson signingIdentities "$identity_count" \
    --argjson buildReady "$build_ready" --argjson productionEvidenceReady "$production_ready" \
    --argjson buildBlockers "$(printf '%s\n' "${build_blockers[@]}" | jq -Rsc 'split("\n") | map(select(length > 0))')" \
    --argjson productionBlockers "$(printf '%s\n' "${production_blockers[@]}" | jq -Rsc 'split("\n") | map(select(length > 0))')" \
    '{schema:$schema,checkedAt:$checkedAt,buildReady:$buildReady,productionEvidenceReady:$productionEvidenceReady,host:{architecture:$architecture,macOS:$macOS,xcode:$xcode,rust:$rust},cefRsVersion:$cefRsVersion,signingIdentities:$signingIdentities,buildBlockers:$buildBlockers,productionBlockers:$productionBlockers,productionClaim:false}' > "$RECEIPT"
  echo "[cef-poc] preflight receipt: $RECEIPT"
  [[ "$build_ready" == true ]] || return 2
}

case "$COMMAND" in
  preflight) preflight ;;
  help|-h|--help) echo "Usage: $0 preflight" ;;
  *) die "unknown command: $COMMAND" ;;
esac
