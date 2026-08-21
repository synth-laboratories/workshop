#!/usr/bin/env bash
# Build, bundle, sign, notarize, and staple the Computer Use helper.
#
# This is Phase 0 of docs/COMPUTER_USE.md expressed as a script. Everything here
# exists because macOS binds TCC grants to code identity: a helper signed
# ad-hoc gets a new cdhash on every build, so every grant the operator gave
# yesterday is gone today. That is gate G1, and it is release-blocking.
#
# Development instances may still sign ad-hoc, while the official Workshop
# release pipeline requires Developer ID signing and notarization. The helper
# keeps a separate pipeline because its TCC identity is verified independently.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/helpers/synth-computer-use"
COMMAND="${1:-help}"
OUTPUT="${SYNTH_HELPER_OUTPUT:-$CRATE/target/bundle}"
BUNDLE="$OUTPUT/Synth Computer Use.app"
BUNDLE_ID="ai.usesynth.workshop.ComputerUseHelper"

# Set by the operator, from the Apple Developer account.
#   SYNTH_TEAM_ID           10-character team identifier
#   SYNTH_SIGN_IDENTITY     e.g. "Developer ID Application: … (TEAMID)"
#   SYNTH_NOTARY_PROFILE    a `notarytool store-credentials` profile name
TEAM_ID="${SYNTH_TEAM_ID:-}"
SIGN_IDENTITY="${SYNTH_SIGN_IDENTITY:-}"
NOTARY_PROFILE="${SYNTH_NOTARY_PROFILE:-}"

note() { echo "[helper] $*"; }
die() { echo "[helper] ERROR: $*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: ./scripts/build-computer-use-helper.sh <command>

  build      Compile release and assemble the .app bundle (no signing)
  sign       Sign with Developer ID and the hardened runtime
  notarize   Submit to Apple, wait, then staple the ticket
  verify     Re-run every check Desktop runs before it will launch the helper
  all        build -> sign -> notarize -> verify
  dev        build, then ad-hoc sign for local development or an explicitly
             unnotarized friends preview (grants will NOT survive a rebuild)
  ensure-dev keep an existing valid helper bundle, otherwise run dev. A
             prebuilt notarized helper is never overwritten by this command.

Environment:
  SYNTH_TEAM_ID          required for sign/notarize
  SYNTH_SIGN_IDENTITY    required for sign
  SYNTH_NOTARY_PROFILE   required for notarize (see: xcrun notarytool store-credentials)
  SYNTH_HELPER_OUTPUT    bundle output directory (default: $CRATE/target/bundle)
EOF
}

build() {
  note "compiling release binary"
  # SYNTH_TEAM_ID is compiled in so the requirement the helper enforces on its
  # caller and the identity it is signed with cannot drift apart.
  # Honor CARGO_TARGET_DIR when a named instance exported one; copying from the
  # crate-local target after a redirected compile is how first cua-build failed.
  local cargo_target="${CARGO_TARGET_DIR:-$CRATE/target}"
  ( cd "$CRATE" && SYNTH_TEAM_ID="${TEAM_ID:-UNSET}" cargo build --release )

  note "assembling bundle at $BUNDLE"
  rm -rf "$BUNDLE"
  mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
  cp "$cargo_target/release/synth-computer-use" "$BUNDLE/Contents/MacOS/synth-computer-use"
  cp "$CRATE/Info.plist" "$BUNDLE/Contents/Info.plist"
  chmod +x "$BUNDLE/Contents/MacOS/synth-computer-use"
  note "bundle assembled"
}

sign() {
  [ -n "$SIGN_IDENTITY" ] || die "SYNTH_SIGN_IDENTITY is required to sign"
  [ -d "$BUNDLE" ] || die "no bundle at $BUNDLE; run build first"
  note "signing with $SIGN_IDENTITY"
  # --options runtime is the hardened runtime. Notarization is refused without
  # it, and it is what stops another process injecting code into a binary that
  # holds Accessibility and Screen Recording.
  # --timestamp is required for notarization and makes the signature outlive
  # the certificate's expiry.
  /usr/bin/codesign --force --sign "$SIGN_IDENTITY" \
    --options runtime \
    --timestamp \
    --entitlements "$CRATE/entitlements.plist" \
    "$BUNDLE"
  /usr/bin/codesign --verify --strict --deep "$BUNDLE"
  note "signed"
}

notarize() {
  [ -n "$NOTARY_PROFILE" ] || die "SYNTH_NOTARY_PROFILE is required to notarize"
  [ -d "$BUNDLE" ] || die "no bundle at $BUNDLE; run build and sign first"
  local zip="$OUTPUT/helper-for-notarization.zip"
  note "submitting to Apple (this waits for the result)"
  rm -f "$zip"
  /usr/bin/ditto -c -k --keepParent "$BUNDLE" "$zip"
  xcrun notarytool submit "$zip" --keychain-profile "$NOTARY_PROFILE" --wait
  rm -f "$zip"

  # We staple; the reference implementation does not. Without a stapled ticket
  # Gatekeeper falls back to an online check, and a first launch on a machine
  # that happens to be offline fails in front of the user.
  note "stapling the ticket"
  xcrun stapler staple "$BUNDLE"
  xcrun stapler validate "$BUNDLE"
  note "notarized and stapled"
}

# Exactly the checks computer_use::helper::verify runs. Kept in lockstep on
# purpose: a helper that passes here and is refused at launch, or the reverse,
# is a bug in one of the two and this is where it shows up.
verify() {
  [ -d "$BUNDLE" ] || die "no bundle at $BUNDLE"
  local report identifier team
  report="$(/usr/bin/codesign --display --verbose=4 "$BUNDLE" 2>&1)"
  identifier="$(awk -F= '/^Identifier=/{print $2; exit}' <<<"$report")"
  team="$(awk -F= '/^TeamIdentifier=/{print $2; exit}' <<<"$report")"

  [ "$identifier" = "$BUNDLE_ID" ] \
    || die "bundle identifier is '$identifier', expected '$BUNDLE_ID' (a different identifier is a different program to TCC)"
  grep -q 'flags=0x10000(runtime)' <<<"$report" \
    || die "hardened runtime is not enabled"
  if [ -n "$TEAM_ID" ]; then
    [ "$team" = "$TEAM_ID" ] || die "signed by team '$team', expected '$TEAM_ID'"
    /usr/bin/codesign --verify -R "anchor apple generic and certificate leaf[subject.OU] = \"$TEAM_ID\"" "$BUNDLE" \
      || die "does not satisfy the pinned code requirement for team $TEAM_ID"
  fi
  /usr/bin/codesign --verify --strict --deep "$BUNDLE" || die "signature does not verify"
  /usr/sbin/spctl --assess --type execute -vv "$BUNDLE" 2>&1 | grep -q 'source=Notarized Developer ID' \
    || die "Gatekeeper does not see this as notarized"
  xcrun stapler validate "$BUNDLE" >/dev/null 2>&1 \
    || die "no stapled ticket; a first launch offline would fail"

  note "cdhash: $(awk -F= '/^CDHash=/{print $2; exit}' <<<"$report")"
  note "all checks passed — Desktop will accept this helper"
}

dev() {
  build
  note "ad-hoc signing for development"
  /usr/bin/codesign --force --sign - \
    --entitlements "$CRATE/entitlements.plist" \
    "$BUNDLE"
  cat >&2 <<'EOF'
[helper] WARNING: ad-hoc signed.
[helper] The cdhash changes on every build, so macOS treats each build as a new
[helper] program and every TCC grant has to be given again. This is exactly the
[helper] condition gate G1 exists to catch. Do not ship it and do not use it to
[helper] argue G1 passes.
EOF
}

ensure_dev() {
  if [ -d "$BUNDLE" ] && /usr/bin/codesign --verify --strict --deep "$BUNDLE" >/dev/null 2>&1; then
    note "using existing signed helper bundle at $BUNDLE"
    return
  fi
  dev
}

case "$COMMAND" in
  build) build ;;
  sign) sign ;;
  notarize) notarize ;;
  verify) verify ;;
  dev) dev ;;
  ensure-dev) ensure_dev ;;
  all) build; sign; notarize; verify ;;
  help|--help|-h) usage ;;
  *) usage; exit 1 ;;
esac
