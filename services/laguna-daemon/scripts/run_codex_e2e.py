#!/usr/bin/env python3
"""Run destructive-safe, real Codex CLI gates against the native server.

The harness creates an isolated temporary workspace, bounds every subprocess,
and uses approval bypass only inside that disposable directory. It never
enables server-hosted shell or patch execution.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:7434/v1")
    parser.add_argument("--model", default=MODEL)
    parser.add_argument("--api-key-env", default="SYNTH_LAGUNA_API_KEY")
    parser.add_argument("--timeout", type=int, default=240)
    parser.add_argument(
        "--cases",
        default="text,shell,apply_patch,mcp_echo,mcp_sum",
        help="Comma-separated case names.",
    )
    return parser.parse_args()


def run_bounded(command: list[str], *, cwd: Path, env: dict[str, str], timeout: int) -> tuple[int, str]:
    process = subprocess.Popen(
        command,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    try:
        output, _ = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            output, _ = process.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            output, _ = process.communicate()
        return 124, output
    return process.returncode, output


def event_objects(output: str) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    for line in output.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and isinstance(value.get("type"), str):
            result.append(value)
    return result


def main() -> int:
    args = parse_args()
    api_key = os.environ.get(args.api_key_env)
    if not api_key:
        raise SystemExit(f"Set {args.api_key_env} to the native server bearer token.")
    codex = shutil.which("codex")
    if not codex:
        raise SystemExit("codex is not installed or not on PATH.")
    repo = Path(__file__).resolve().parents[1]
    mcp_server = repo / "tests" / "fixtures" / "mcp" / "contract_server.py"
    python = Path(os.environ.get("PYTHON", sys.executable)).resolve()
    selected = [value.strip() for value in args.cases.split(",") if value.strip()]
    results: list[dict[str, Any]] = []

    with tempfile.TemporaryDirectory(prefix="laguna-codex-e2e-") as temp:
        workspace = Path(temp)
        (workspace / "config.txt").write_text("status=before\n", encoding="utf-8")
        mcp_log = workspace / "mcp.jsonl"
        env = dict(os.environ)
        env["LAGUNA_E2E_KEY"] = api_key
        env["FIXTURE_MCP_LOG"] = str(mcp_log)

        prompts = {
            "text": "Do not call tools. Reply exactly CODEX_TEXT_OK.",
            "shell": "Run pwd exactly once, then reply exactly CODEX_SHELL_OK.",
            "apply_patch": (
                "Use only apply_patch to change the exact line status=before to status=after "
                "in config.txt, with no space after +/- markers. Then reply exactly "
                "CODEX_APPLY_PATCH_OK."
            ),
            "mcp_echo": (
                "Call laguna_fixture.fixture_echo exactly once with value native-mcp, then "
                "reply exactly CODEX_MCP_ECHO_OK."
            ),
            "mcp_sum": (
                "Call laguna_fixture.fixture_sum exactly once with a=19 and b=23, then "
                "reply exactly CODEX_MCP_SUM_OK."
            ),
        }
        expected = {
            "text": "CODEX_TEXT_OK",
            "shell": "CODEX_SHELL_OK",
            "apply_patch": "CODEX_APPLY_PATCH_OK",
            "mcp_echo": "CODEX_MCP_ECHO_OK",
            "mcp_sum": "CODEX_MCP_SUM_OK",
        }

        for case in selected:
            if case not in prompts:
                results.append({"case": case, "passed": False, "error": "unknown case"})
                continue
            command = [
                codex,
                "exec",
                "--ignore-user-config",
                "--json",
                "--ephemeral",
                "--skip-git-repo-check",
                "--dangerously-bypass-approvals-and-sandbox",
                "-C",
                str(workspace),
                "-m",
                args.model,
                "-c",
                "model_provider=laguna_e2e",
                "-c",
                'model_providers.laguna_e2e.name="Native Laguna E2E"',
                "-c",
                f'model_providers.laguna_e2e.base_url="{args.base_url}"',
                "-c",
                'model_providers.laguna_e2e.env_key="LAGUNA_E2E_KEY"',
                "-c",
                'model_providers.laguna_e2e.wire_api="responses"',
            ]
            if case.startswith("mcp_"):
                command.extend(
                    [
                        "-c",
                        f'mcp_servers.laguna_fixture.command="{python}"',
                        "-c",
                        f'mcp_servers.laguna_fixture.args=["{mcp_server}"]',
                        "-c",
                        f'mcp_servers.laguna_fixture.env={{FIXTURE_MCP_LOG="{mcp_log}"}}',
                    ]
                )
            command.append(prompts[case])
            started = time.monotonic()
            code, output = run_bounded(command, cwd=workspace, env=env, timeout=args.timeout)
            events = event_objects(output)
            rendered = json.dumps(events, ensure_ascii=False)
            passed = code == 0 and expected[case] in rendered
            details: dict[str, Any] = {
                "case": case,
                "passed": passed,
                "exit_code": code,
                "duration_seconds": round(time.monotonic() - started, 3),
                "event_count": len(events),
            }
            if case == "apply_patch":
                details["file_content"] = (workspace / "config.txt").read_text(encoding="utf-8").strip()
                passed = passed and details["file_content"] == "status=after"
                details["passed"] = passed
            if case.startswith("mcp_"):
                calls = [json.loads(line) for line in mcp_log.read_text(encoding="utf-8").splitlines()] if mcp_log.exists() else []
                details["mcp_calls"] = calls
                tool_name = "fixture_echo" if case == "mcp_echo" else "fixture_sum"
                passed = passed and sum(call.get("name") == tool_name for call in calls) == 1
                details["passed"] = passed
            if not passed:
                details["output_tail"] = output[-4000:]
            results.append(details)

    report = {"base_url": args.base_url, "model": args.model, "results": results}
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0 if results and all(result["passed"] for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
