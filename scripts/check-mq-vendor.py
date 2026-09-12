#!/usr/bin/env python3
"""Verify the immutable MQ source snapshot used by the native desktop build."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "apps/synth_desktop/src-tauri/third_party/manderqueue"
manifest = json.loads((root / "VENDOR_PROVENANCE.json").read_text())
for name, expected in manifest["files"].items():
    path = root / name
    if path.is_symlink() or not path.is_file():
        raise SystemExit(f"missing or unsafe MQ source: {name}")
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise SystemExit(f"modified MQ source: {name}")
print(f"MQ snapshot verified: {manifest['commit']} ({len(manifest['files'])} files)")
