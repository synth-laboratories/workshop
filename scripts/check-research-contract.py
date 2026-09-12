"""Verify committed research schema provenance; source proof, not live conformance."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def verify(root: Path, backend: Path | None = None) -> None:
    pin = json.loads((root / "contracts/research-v1.source.json").read_text())
    schema_bytes = (root / "contracts/research-v1.json").read_bytes()
    digest = hashlib.sha256(schema_bytes).hexdigest()
    if pin.get("sha256") != digest:
        raise ValueError("research schema digest differs from its source pin")
    revision = pin.get("backend_revision", "")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("backend revision must be an immutable full Git hash")
    if pin.get("backend_path") != "research_openapi.json":
        raise ValueError("unexpected canonical backend schema path")
    schema = json.loads(schema_bytes)
    ids = []
    for path in schema["paths"].values():
        for method, operation in path.items():
            if method.lower() in {"get", "put", "post", "delete", "patch", "head", "options", "trace"}:
                operation_id = operation.get("operationId")
                if not isinstance(operation_id, str) or not operation_id:
                    raise ValueError("operation without a canonical ID")
                ids.append(operation_id)
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate operation IDs")
    if backend is not None:
        committed = subprocess.run(
            ["git", "-C", str(backend), "show", f"{revision}:research_openapi.json"],
            check=True, capture_output=True,
        ).stdout
        if committed != schema_bytes:
            raise ValueError("schema differs from the pinned backend Git object")
    print(f"Research contract verified: {len(ids)} operations, sha256={digest}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", type=Path)
    args = parser.parse_args()
    verify(Path(__file__).resolve().parents[1], args.backend)
