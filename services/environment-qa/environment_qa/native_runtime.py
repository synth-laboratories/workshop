"""Explicit digest-pinned QA trials with shared native provider custody."""
from __future__ import annotations

import asyncio
import importlib.metadata
import json
import os
import tomllib
from pathlib import Path


def run_native_probe(*, task, work, job, agent, environment, pipeline, timeout_seconds,
                     cancelled, executable):
    from synth_containers.harbor_environment import (
        HarborProviderCompatibility,
        inspect_harbor_package,
        register_harbor_environment,
    )
    from synth_containers.harbor_phase_limits import NativeHarborPhaseLimits
    from synth_containers.harbor_task_stage import stage_native_harbor_task
    from synth_containers.lifecycle_limits import LifecycleLimits
    from synth_containers.staged_harbor_execution import (
        cleanup_staged_harbor_jobs,
        execute_staged_harbor,
    )

    if importlib.metadata.version('synth-containers') != '0.4.3.dev2026091126':
        raise ValueError('Native QA requires the pinned dev26 containers runtime')
    if pipeline.get('backend') != 'docker':
        raise ValueError('Native QA currently admits owned Docker custody only')
    image = pipeline['native_image']
    root = (Path(work) / (job + '-native')).resolve()
    jobs = Path(work).resolve()
    manifest = tomllib.loads((task / 'task.toml').read_text())
    phases = NativeHarborPhaseLimits(
        manifest['agent']['timeout_sec'], manifest['verifier']['timeout_sec'],
    )
    limits = LifecycleLimits(
        overall_seconds=timeout_seconds, setup_seconds=min(300, timeout_seconds),
        work_seconds=timeout_seconds, verifier_seconds=min(120, timeout_seconds),
        publication_seconds=min(120, timeout_seconds),
        cleanup_seconds=min(90, timeout_seconds / 4),
    )
    secrets = tuple(environment[key] for key in (
        'QA_PROVIDER_KEY', 'OPENROUTER_API_KEY', 'QA_GATE_TOKEN',
    ) if environment.get(key))

    async def setup():
        release = register_harbor_environment(
            inspect_harbor_package(task), agent_image=image, verifier_image=image,
            provider=HarborProviderCompatibility(provider_id='docker', supports_separate_verifier=False),
        )
        if not release.validation.valid:
            raise ValueError('Native QA task admission failed: ' + ', '.join(release.validation.errors))
        return stage_native_harbor_task(
            release, root / 'task', resource_ttl_minutes=pipeline.get('native_resource_ttl_minutes', 60),
            docker_resource_custody=True, docker_egress_image=pipeline.get('native_docker_egress_image'),
            phase_limits=phases,
        )

    async def decode():
        # QA's existing oracle/no-op/repeat assessment remains the grader.
        return {'verifier_receipts': len(list((jobs / job).glob('*/result.json')))}

    async def publish(outcome):
        with (root / 'evidence.json').open('x') as handle:
            json.dump(outcome, handle, sort_keys=True, allow_nan=False)
            handle.flush()
            os.fsync(handle.fileno())
        return {'status': 'retained'}

    async def cleanup():
        return await cleanup_staged_harbor_jobs(
            jobs / job, provider='docker', output=root / 'cleanup.log',
            env=environment, redact=secrets,
        )

    outcome = asyncio.run(execute_staged_harbor(
        root, run_id=job, limits=limits, setup=setup, agent=agent, model=None,
        jobs_dir=jobs, job_name=job,
        max_output_bytes=pipeline.get('process_output_max_bytes', 16 * 1024 * 1024),
        decode=decode, publish=publish, cleanup=cleanup, env=environment,
        redact=secrets, executable=(executable,), should_cancel=cancelled,
    ))
    return outcome, root / 'process.log'
