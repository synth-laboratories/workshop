"""Shared host process custody for Harbor probes; provider cleanup stays separate."""
from __future__ import annotations

import asyncio
from collections.abc import Callable, Mapping
from pathlib import Path


def run_probe_process(
    command: list[str], *, output: Path, environment: Mapping[str, str], cwd: Path,
    timeout_seconds: float, cancelled: Callable[[], bool], max_output_bytes: int,
) -> tuple[int, bool, str | None]:
    from synth_containers.bounded_process import ProcessOutputLimit, run_bounded_process

    async def execute() -> tuple[int, bool, str | None]:
        try:
            async with asyncio.timeout(timeout_seconds):
                task = asyncio.create_task(run_bounded_process(
                    command, output=output, env=environment, cwd=cwd,
                    max_output_bytes=max_output_bytes,
                    redact=tuple(environment[name] for name in (
                        "QA_PROVIDER_KEY", "OPENROUTER_API_KEY", "QA_GATE_TOKEN"
                    ) if environment.get(name)),
                ))
                try:
                    while not task.done():
                        if cancelled():
                            return -1, True, "cancelled"
                        await asyncio.wait({task}, timeout=0.2)
                    return await task, False, None
                finally:
                    if not task.done():
                        task.cancel()
                        try:
                            await task
                        except asyncio.CancelledError:
                            pass
        except TimeoutError:
            return -1, True, "deadline_exceeded"
        except ProcessOutputLimit:
            return -1, True, "process_output_limit"
    return asyncio.run(execute())
