#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).parents[1]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    pins = json.loads((ROOT / "schemas" / "PIN.json").read_text(encoding="utf-8"))
    for entry in pins["files"]:
        path = ROOT / entry["path"]
        actual = digest(path)
        if actual != entry["sha256"]:
            raise SystemExit(f"schema pin mismatch for {path}: {actual}")
    from openresponses_types import __spec_hash__, __spec_version__, __version__

    generated = pins["generatedTypes"]
    observed = {
        "package": "openresponses-types",
        "version": __version__,
        "specVersion": __spec_version__,
        "specHash": __spec_hash__,
    }
    if observed != generated:
        raise SystemExit(f"generated type pin mismatch: {observed!r}")
    print(f"validated {len(pins['files'])} schema/license pins and generated types")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
