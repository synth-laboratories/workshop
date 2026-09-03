from __future__ import annotations

import json
from pathlib import Path
from typing import Any

JSON = dict[str, Any]


def _template_index(visuals_root: Path | None) -> dict[str, JSON]:
    if visuals_root is None:
        return {}
    families = visuals_root / "families"
    if not families.exists():
        return {}
    canonical_root = families.resolve(strict=True)
    for candidate in families.rglob("*"):
        if candidate.is_symlink():
            raise ValueError(f"visual template registry refuses symlink: {candidate}")

    index: dict[str, JSON] = {}
    for manifest in sorted(families.rglob("template.json")):
        path = manifest.parent
        if not path.resolve(strict=True).is_relative_to(canonical_root):
            raise ValueError(f"visual template path escapes family root: {path}")
        data = json.loads(manifest.read_text(encoding="utf-8"))
        template_id = data.get("id") or path.name
        if template_id != path.name:
            raise ValueError(f"visual template id {template_id!r} does not match directory {path}")
        if template_id in index:
            raise ValueError(
                f"duplicate visual template id {template_id!r} in "
                f"{index[template_id]['path']} and {path}"
            )
        data["id"] = template_id
        data["path"] = str(path)
        data["shellPath"] = str(path / "shell.tsx") if (path / "shell.tsx").exists() else None
        example = path / "examples" / "fixture_binding.json"
        if example.exists():
            data["exampleBinding"] = json.loads(example.read_text(encoding="utf-8"))
        index[template_id] = data
    return dict(sorted(index.items()))


def list_visual_templates(visuals_root: Path | None) -> list[JSON]:
    return list(_template_index(visuals_root).values())


def resolve_visual_template(visuals_root: Path | None, template_id: str) -> JSON:
    try:
        return _template_index(visuals_root)[template_id]
    except KeyError:
        raise KeyError(template_id) from None
