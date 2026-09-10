"""Explicit, content-addressed blind exports. Never expose the source checkout."""
import hashlib
import os
import shutil
import tempfile
from pathlib import Path
from .core import digest

ALLOWED = {"instruction.md", "task.toml", "environment", "tests", "solution"}
FORBIDDEN = {".git", ".env", "gold", "provenance", "node_modules", ".venv", "__pycache__"}


def file_manifest(path):
    manifest = {}
    total = 0
    for item in sorted(path.rglob("*")):
        relative = item.relative_to(path)
        if item.is_symlink():
            raise ValueError("Symlinks are not admitted in task bundles")
        if any(p in FORBIDDEN or p.startswith(".env.") for p in relative.parts):
            raise ValueError("Excluded metadata or credentials inside task bundle")
        if item.is_file():
            total += item.stat().st_size
            if total > 50_000_000:
                raise ValueError("Prototype bundle limit is 50 MB; use a smaller admitted task")
            manifest[str(relative)] = hashlib.sha256(item.read_bytes()).hexdigest()
    return manifest


def export_bundle(source, store_root, allowed_roots):
    source = Path(source).resolve()
    if not any(source.is_relative_to(Path(root).resolve()) for root in allowed_roots):
        raise ValueError("Task path is outside the configured task roots")
    if not source.is_dir() or not (source / "instruction.md").is_file() or not (source / "task.toml").is_file():
        raise ValueError("Select a Harbor task directory containing instruction.md and task.toml")
    bundles = Path(store_root) / "bundles"
    bundles.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="export-", dir=bundles) as temp:
        target = Path(temp)
        for name in ALLOWED:
            entry = source / name
            if entry.is_symlink():
                raise ValueError("Symlinked task components are not admitted")
            if entry.is_dir():
                # Reject symlinks before copy; never follow a link to external data.
                file_manifest(entry)
                shutil.copytree(entry, target / name)
            elif entry.is_file():
                shutil.copyfile(entry, target / name)
        manifest = file_manifest(target)
        sha = digest(manifest)
        destination = bundles / sha
        if not destination.exists():
            try:
                os.rename(target, destination)
            except FileExistsError:
                pass
        if file_manifest(destination) != manifest:
            raise ValueError("Existing bundle digest mismatch")
    return {"sha256": sha, "files": manifest, "format": "harbor", "bytes": sum((destination / f).stat().st_size for f in manifest)}


def verified_path(store, bundle):
    sha = bundle["sha256"]
    if len(sha) != 64 or any(c not in "0123456789abcdef" for c in sha):
        raise ValueError("Invalid bundle digest")
    path = store.root / "bundles" / sha
    if not path.is_dir() or digest(file_manifest(path)) != sha:
        raise ValueError("Task bundle has changed or is missing")
    return path
