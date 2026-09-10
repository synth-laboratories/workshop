"""Fail closed unless a distribution matches the operator's accepted byte identity."""

import hashlib
import json
from pathlib import Path
import sys


def verify(directory: Path, source_commit: str, archive_sha256: str) -> None:
    version = json.loads(Path("apps/synth_desktop/package.json").read_text())["version"]
    base = f"Synth-Workshop-v{version}-stable-macOS-arm64-UNNOTARIZED"
    archive = directory / f"{base}.zip"
    manifest = json.loads((directory / f"{base}.json").read_text())
    expected = {
        "schema": "workshop.distribution.v1",
        "version": version,
        "channel": "stable",
        "platform": "macOS",
        "architecture": "arm64",
        "sourceCommit": source_commit,
        "sourceTreeDirty": False,
        "archive": archive.name,
        "sha256": archive_sha256,
        "signature": "ad-hoc",
        "notarization": "none",
    }
    if any(manifest.get(key) != value for key, value in expected.items()):
        raise ValueError("Distribution manifest differs from accepted release identity")
    if set(path.name for path in directory.iterdir()) != {
        archive.name, f"{base}.json", f"{base}.zip.sha256"
    }:
        raise ValueError("Unexpected or missing distribution files")
    with archive.open("rb") as stream:
        actual_sha256 = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual_sha256 != archive_sha256 or archive.stat().st_size != manifest["archiveBytes"]:
        raise ValueError("Accepted archive hash or size mismatch")
    checksum = (directory / f"{base}.zip.sha256").read_text().split()
    if checksum != [archive_sha256, archive.name]:
        raise ValueError("Checksum sidecar differs from accepted archive")
    print(f"Accepted distribution verified: {archive.name} sha256:{actual_sha256}")


if __name__ == "__main__":
    verify(Path(sys.argv[1]), sys.argv[2], sys.argv[3])
