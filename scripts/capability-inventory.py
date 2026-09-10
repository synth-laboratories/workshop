#!/usr/bin/env python3
"""Reproducible declaration inventory. Static evidence, never a parity claim.

Reads only source code and package manifests. Does not launch an app, load client
configuration, inspect credentials, or call a provider. --check detects drift.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / "apps/synth_desktop/src-tauri/src"
OUT = ROOT / "docs/engineering/capability-inventory.json"
REPORT = OUT.with_suffix(".md")


def location(path: Path, text: str, offset: int) -> dict:
    return {"path": str(path.relative_to(ROOT)), "line": text.count("\n", 0, offset) + 1}


def inventory() -> dict:
    entries: list[dict] = []
    files: dict[str, str] = {}

    def read(path: Path) -> str:
        data = path.read_bytes()
        files[str(path.relative_to(ROOT))] = hashlib.sha256(data).hexdigest()
        return data.decode()

    def add(surface: str, name: str, path: Path, text: str, offset: int, **details):
        source = location(path, text, offset)
        entries.append({
            "entryId": f"{surface}:{source['path']}:{name}:{source['line']}",
            "surface": surface,
            "name": name,
            "source": source,
            "classification": "unreviewed",
            "operationId": None,
            "domainOwner": None,
            "handlerVerified": False,
            "parityVerified": False,
            **details,
        })

    specta = RUST / "contract/specta.rs"
    text = read(specta)
    # Keep the search inside the real collect_commands! registration. Capture
    # nested module paths; do not equate Commands constants with live handlers.
    registration = re.search(r"collect_commands!\s*\[(.*?)\]\s*\)", text, re.S)
    if registration is None:
        raise ValueError("Tauri collect_commands! registration shape changed; update extractor")
    for match in re.finditer(r"(?m)^\s*(?:crate::)?([A-Za-z_][A-Za-z0-9_:]*)\s*,", registration[1]):
        add("tauri_registration", match[1], specta, text, registration.start(1) + match.start())

    constants = RUST / "contract/commands.rs"
    text = read(constants)
    for match in re.finditer(r'pub const\s+(\w+):\s*&\s*\x27static\s+str\s*=\s*"([^"\n]+)"', text):
        add("command_constant", match[2], constants, text, match.start(), constant=match[1])

    for path in sorted([*(RUST / "bin").glob("*_mcp.rs"), *(RUST / "adapters/mcp/operations").glob("*.rs"), RUST / "browser/operations.rs"]):
        text = read(path)
        # Literal declaration candidates, including helper functions. Require
        # inputSchema before the next tool-name key. Generated/dynamic tools
        # are listed as an extraction limitation rather than guessed.
        for match in re.finditer(r'"name"\s*:\s*"([A-Za-z0-9_.-]+)"', text):
            end = text.find('"name"', match.end())
            section = text[match.end():end if end >= 0 else len(text)]
            if not re.search(r'"inputSchema"\s*:', section):
                continue
            add("mcp_literal_candidate", match[1], path, text, match.start(),
                possibleGenericFacade=bool(re.search(r'"operation"\s*:\s*\{', section)))

    # Typed declarations project into tools/list at runtime. Record their
    # declared identities, without inferring execution or migration status.
    for path in sorted((RUST / "domains").rglob("*.rs")):
        text = read(path)
        explicit = r'const ID:.*?=\s*"([^"]+)";\s*const MCP_NAME:.*?=\s*"([^"]+)";'
        macros = r'operation!\(\s*\w+\s*,\s*\w+\s*,\s*\w+\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"'
        for pattern in (explicit, macros):
            for match in re.finditer(pattern, text):
                add("typed_mcp_declaration", match[2], path, text, match.start(),
                    operationId=match[1])
    for path in [RUST / "contract/capabilities.rs", RUST / "adapters/workshop.rs",
                 *sorted((RUST / "agent_integration").glob("*.rs")),
                 *sorted((RUST / "session/acp").glob("*.rs")), RUST / "contract/desktop_dispatch.rs",
                 RUST / "contract/desktop_policy.rs", RUST / "platform/desktop_runtime.rs"]:
        read(path)

    generated_path = RUST / "contract/desktop_tools.json"
    generated = read(generated_path)
    for tool in json.loads(generated)["tools"]:
        add("generated_desktop_mcp_declaration", tool["name"], generated_path, generated,
            generated.index(json.dumps(tool["name"])), operationId=tool["_meta"]["workshop/operationId"],
            classification="generated_projection_subject_to_desktop_policy")

    hosted = RUST / "session/codex/home.rs"
    text = read(hosted)
    for match in re.finditer(r'"(synth_\w+)"\s*=>\s*"((?:\\.|[^"\\])*)"', text):
        decoded = json.loads('"' + match[2] + '"')
        names = re.fullmatch(r"enabled_tools\s*=\s*(\[.*\])\s*", decoded, re.S)
        if not names:
            continue
        for name in json.loads(names[1]):
            add("hosted_tool_allowlist", f"{match[1]}:{name}", hosted, text, match.start(),
                server=match[1], tool=name)

    # Routes include those in helper functions. Conditional/prefix routes and
    # dynamic dispatch are preserved as explicit follow-up requirements.
    for path in sorted(RUST.rglob("*.rs")):
        if "/third_party/" in str(path):
            continue
        raw = path.read_text()
        if not re.search(r'\("(?:GET|POST|PUT|PATCH|DELETE)"\s*,\s*"/', raw):
            continue
        text = read(path)
        for match in re.finditer(r'\("(GET|POST|PUT|PATCH|DELETE)"\s*,\s*"([^"\n]+)"\)', text):
            add("literal_route_candidate", f"{match[1]} {match[2]}", path, text, match.start())

    renderer = ROOT / "apps/synth_desktop/src/renderer/src"
    for path in sorted(renderer.rglob("*")):
        if path.suffix not in {".ts", ".tsx"} or "generated" in path.parts:
            continue
        raw = path.read_text()
        matches = list(re.finditer(r"\bcommands\.([A-Za-z_]\w*)\s*\(", raw))
        matches += list(re.finditer(r'''\binvoke(?:<[^>\n]+>)?\s*\(\s*["']([^"']+)["']''', raw))
        if not matches:
            continue
        text = read(path)
        for match in sorted(matches, key=lambda item: item.start()):
            add("renderer_call_candidate", match[1], path, text, match.start())

    manifest = ROOT / "package.json"
    text = read(manifest)
    for name, command in sorted(json.loads(text).get("scripts", {}).items()):
        if name.startswith(("desktop:", "mcp:")) or name == "workshop":
            offset = text.index(json.dumps(name))
            add("package_command", name, manifest, text, offset, command=command)

    entries.sort(key=lambda row: (row["surface"], row["source"]["path"], row["source"]["line"], row["name"]))
    counts: dict[str, int] = {}
    for entry in entries:
        counts[entry["surface"]] = counts.get(entry["surface"], 0) + 1
    return {
        "schemaVersion": "workshop.capability-inventory.v1",
        "evidenceKind": "static_source_declarations",
        "sourceDigest": hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest(),
        "sourceFiles": dict(sorted(files.items())),
        "counts": counts,
        "limitations": [
            "Counts are declaration occurrences, not unique product operations or coverage percentages.",
            "MCP literal candidates may include test declarations; actual tools/list must verify them.",
            "Desktop MCP projection is enumerated; dynamic legacy aliases, conditional routes, shell CLIs and direct UI/agent mutations still require owner review.",
            "Only literal renderer calls are enumerated; wrapper and computed calls require owner review.",
            "Hosted tool allowlists include literal match arms only; block/computed configuration needs review.",
            "No handler execution, authentication, side effect, installed-client compatibility or parity has been verified.",
            "Source hashes identify inspected working files; the implementation plan records the selected base commit.",
        ],
        "entries": entries,
    }


def report(data: dict) -> str:
    lines = [
        "# Workshop capability inventory baseline", "",
        "Generated by `python3 scripts/capability-inventory.py`. Do not edit this report.", "",
        "This is a declaration inventory. **No operation is marked as migrated or proven.**", "",
        f"Inspected source digest: `{data['sourceDigest']}`. The implementation plan records the base commit.", "",
        "| Surface | Declaration occurrences |", "| --- | ---: |",
    ]
    lines += [f"| {surface} | {count} |" for surface, count in data["counts"].items()]
    lines += ["", "## Evidence limitations", ""]
    lines += [f"- {item}" for item in data["limitations"]]
    lines += ["", "## Owner review queue", "",
              "Use `capability-inventory.json` for exact source locations and null owner/operation mappings.",
              "A reviewed migration ledger must map these occurrences to domain operations, aliases, or justified internal mechanics.",
              "The generator never assigns semantic equivalence based on similar names.", "",
              "First slice: inspect visual template discovery, creation/binding, presentation, capture and interaction.",
              "Prioritize direct dependencies on hosted session identity and independently maintained tool schemas.", ""]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Check generated files without writing")
    args = parser.parse_args()
    data = inventory()
    outputs = {OUT: json.dumps(data, indent=2, ensure_ascii=False) + "\n", REPORT: report(data)}
    stale = []
    for path, expected in outputs.items():
        if args.check:
            if not path.exists() or path.read_text() != expected:
                stale.append(str(path.relative_to(ROOT)))
        else:
            path.write_text(expected)
    if stale:
        print("Stale capability inventory: " + ", ".join(stale), file=sys.stderr)
        return 1
    print(json.dumps({"checked": args.check, "counts": data["counts"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
