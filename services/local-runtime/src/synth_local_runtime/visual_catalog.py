from __future__ import annotations

import json
from pathlib import Path
from typing import Any

JSON = dict[str, Any]


def list_visual_templates(visuals_root: Path | None) -> list[JSON]:
    if visuals_root is None or not visuals_root.exists():
        return []
    templates_dir = visuals_root / "templates"
    if not templates_dir.exists():
        return []
    out: list[JSON] = []
    for path in sorted(templates_dir.iterdir()):
        meta = path / "template.json"
        if not meta.exists():
            continue
        try:
            data = json.loads(meta.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        data.setdefault("id", path.name)
        data["path"] = str(path)
        out.append(data)
    return out


def resolve_visual_template(visuals_root: Path | None, template_id: str) -> JSON:
    if visuals_root is None:
        raise KeyError(template_id)
    path = visuals_root / "templates" / template_id
    meta = path / "template.json"
    if not meta.exists():
        raise KeyError(template_id)
    data = json.loads(meta.read_text(encoding="utf-8"))
    data.setdefault("id", template_id)
    data["path"] = str(path)
    data["shellPath"] = str(path / "shell.tsx") if (path / "shell.tsx").exists() else None
    example = path / "examples" / "fixture_binding.json"
    if example.exists():
        data["exampleBinding"] = json.loads(example.read_text(encoding="utf-8"))
    return data
