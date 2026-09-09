#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/install.sh" --check
[[ -d node_modules ]] || npm ci
npm run typecheck
npm run build:graph
WORKSHOP_BUILD_JOBS="${WORKSHOP_BUILD_JOBS:-4}" \
  "$repo_root/scripts/build-tier.sh" stable

app="$repo_root/work/tier-builds/stable/Synth Workshop.app"
version="$(python3 -c 'import json; print(json.load(open("apps/synth_desktop/src-tauri/tauri.conf.json"))["version"])')"
arch="$(uname -m)"
dist="$repo_root/dist"
stage="$(mktemp -d -t workshop-package.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
archive="$dist/Synth-Workshop-v${version}-stable-macOS-${arch}-UNNOTARIZED.zip"
manifest="$dist/Synth-Workshop-v${version}-stable-macOS-${arch}-UNNOTARIZED.json"

mkdir -p "$dist"
ditto "$app" "$stage/Synth Workshop.app"
# Ad-hoc signing keeps the local package internally consistent without using
# a Developer ID identity or macOS Keychain. It is explicitly not notarization.
# Nested browser/helper code keeps its own signing and JIT entitlements.
# Ad-hoc binaries have no shared Developer ID TeamIdentifier. Enabling library
# validation through hardened runtime here rejects the bundled Ghostty dylib at
# load time even though --verify --deep --strict succeeds. Developer-ID builds
# can opt into hardened runtime in their separately authorized signing pipeline.
codesign --force --sign - "$stage/Synth Workshop.app"
codesign --verify --deep --strict "$stage/Synth Workshop.app"
rm -f "$archive" "$archive.sha256" "$manifest"
ditto -c -k --sequesterRsrc --keepParent "$stage/Synth Workshop.app" "$archive"
sha256="$(shasum -a 256 "$archive" | awk '{print $1}')"
printf '%s  %s\n' "$sha256" "$(basename "$archive")" >"$archive.sha256"
python3 - "$manifest" "$archive" "$sha256" "$version" "$arch" <<'PY'
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

target, archive, sha256, version, arch = sys.argv[1:]
root = Path.cwd()
commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
receipt = json.loads((root / "work/tier-builds/stable/manifest.json").read_text())
dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip())
if dirty or receipt.get("treeDirty") or receipt.get("commit") != commit or receipt.get("profile") != "release":
    raise SystemExit("Refusing publication metadata for a dirty, debug, or source-mismatched package")
document = {
    "schema": "workshop.distribution.v1",
    "product": "Synth Workshop",
    "version": version,
    "channel": "stable",
    "platform": "macOS",
    "architecture": arch,
    "bundleIdentifier": "com.synth.desktop",
    "sourceCommit": commit,
    "sourceTreeDirty": dirty,
    "archive": Path(archive).name,
    "archiveBytes": os.path.getsize(archive),
    "sha256": sha256,
    "signature": "ad-hoc",
    "notarization": "none",
    "builtAt": datetime.now(timezone.utc).isoformat(),
}
Path(target).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
PY

echo "Local Workshop package complete."
echo "App bundle: $app"
echo "Download ZIP: $archive"
echo "Checksum: $archive.sha256"
echo "Manifest: $manifest"
echo "WARNING: ad-hoc signed and not Apple-notarized. Publication still requires release acceptance."
