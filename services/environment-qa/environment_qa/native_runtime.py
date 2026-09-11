"""Explicit digest-pinned QA trials with shared native provider custody."""
from __future__ import annotations

import asyncio
import importlib.metadata
import json
import os
import tomllib
from pathlib import Path


def run_native_probe(*, task, work, job, agent, environment, pipeline, timeout_seconds,
                     cancelled, executable, component=False):
    from synth_containers.bounded_process import run_bounded_process
    from synth_containers.harbor_environment import (
        HarborProviderCompatibility,
        _tree_digest,
        inspect_harbor_package,
        register_harbor_environment,
    )
    from synth_containers.harbor_phase_limits import NativeHarborPhaseLimits
    from synth_containers.harbor_task_stage import stage_native_harbor_task
    from synth_containers.lifecycle_limits import (
        DurableRolloutSupervisor,
        LifecycleLimits,
    )
    from synth_containers.staged_harbor_execution import (
        cleanup_staged_harbor_jobs,
        execute_staged_harbor,
    )

    if importlib.metadata.version('synth-containers') != '0.4.3.dev2026091126':
        raise ValueError('Native QA requires the pinned dev26 containers runtime')
    provider = pipeline.get('backend', 'docker')
    if provider not in {'docker', 'daytona'}:
        raise ValueError('Native QA requires Docker or Daytona')
    if provider == 'daytona' and pipeline.get('native_environment_digest') != _tree_digest(task / 'environment'):
        raise ValueError('Daytona QA requires the exact prepared environment context digest')
    image = pipeline['native_image']
    root = (Path(work) / (job + '-native')).resolve()
    jobs = Path(work).resolve()
    manifest = tomllib.loads((task / 'task.toml').read_text())
    phases = None if component else NativeHarborPhaseLimits(
        manifest['agent']['timeout_sec'], manifest['verifier']['timeout_sec'],
    )
    limits = LifecycleLimits(
        overall_seconds=timeout_seconds, setup_seconds=min(300, timeout_seconds),
        work_seconds=timeout_seconds, verifier_seconds=min(120, timeout_seconds),
        publication_seconds=min(120, timeout_seconds),
        cleanup_seconds=min(90, timeout_seconds / 4),
    )
    secrets = tuple(environment[key] for key in (
        'QA_PROVIDER_KEY', 'OPENROUTER_API_KEY', 'QA_GATE_TOKEN', 'DAYTONA_API_KEY',
    ) if environment.get(key))

    async def component_setup():
        import toml
        from harbor.models.task.config import TaskConfig
        from synth_containers.harbor_environment import _MAX_TREE_BYTES, _tree_files
        from synth_containers.harbor_task_stage import (
            _copy_bounded_file,
            native_harbor_environment_flags,
        )

        source_digest = _tree_digest(task)
        if any((task / 'environment' / name).exists() for name in ('docker-compose.yaml', 'docker-compose.yml', 'compose.yaml', 'compose.yml')):
            raise ValueError('Native component images cannot start additional Compose services')
        config = tomllib.loads((task / 'task.toml').read_text())
        if config.get('steps') or config['environment'].get('mounts') or config['environment'].get('volumes'):
            raise ValueError('Native component requires one task without host mounts')
        flags = native_harbor_environment_flags(
            task, provider=provider, image=image,
            resource_ttl_minutes=pipeline.get('native_resource_ttl_minutes',60),
            docker_resource_custody=provider == 'docker',
            docker_egress_image=pipeline.get('native_docker_egress_image') if provider == 'docker' else None,
        )
        config['environment'].update(docker_image=image, build_timeout_sec=300)
        encoded = toml.dumps(config)
        TaskConfig.model_validate_toml(encoded)
        target = root / 'task'
        target.mkdir(parents=True, exist_ok=False)
        copied = 0
        for source in _tree_files(task):
            relative = source.relative_to(task)
            if relative.as_posix() in {'environment/Dockerfile','task.toml'}:
                continue
            destination = target / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            copied += _copy_bounded_file(source, destination, _MAX_TREE_BYTES-copied)
        (target / 'task.toml').write_text(encoded)
        if _tree_digest(task) != source_digest:
            raise ValueError('Component source changed during staging')
        receipt = {'schema_version':'workshop.qa-native-component-stage.v1',
                   'source_package_digest':source_digest, 'staged_package_digest':_tree_digest(target),
                   'task_path':str(target), 'native_environment_flags':flags,
                   'verifier_executed':False, 'benchmark_score':None}
        with (root/'component-stage.json').open('x') as handle:
            json.dump(receipt,handle,sort_keys=True);handle.flush();os.fsync(handle.fileno())
        return receipt

    def validate_component(receipt):
        if receipt.get('verifier_executed') is not False or _tree_digest(Path(receipt['task_path'])) != receipt['staged_package_digest']:
            raise ValueError('Native component custody changed before launch')

    async def setup():
        release = register_harbor_environment(
            inspect_harbor_package(task), agent_image=image, verifier_image=image,
            provider=HarborProviderCompatibility(provider_id=provider, supports_separate_verifier=False),
        )
        if not release.validation.valid:
            raise ValueError('Native QA task admission failed: ' + ', '.join(release.validation.errors))
        return stage_native_harbor_task(
            release, root / 'task', resource_ttl_minutes=pipeline.get('native_resource_ttl_minutes', 60),
            docker_resource_custody=provider == 'docker', docker_egress_image=pipeline.get('native_docker_egress_image') if provider == 'docker' else None,
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
            jobs / job, provider=provider, output=root / 'cleanup.log',
            env=environment, redact=secrets,
        )

    async def execute_component():
        # Component hypotheses deliberately omit the grader. The generic
        # lifecycle verifier phase validates observation custody only.
        supervisor = DurableRolloutSupervisor(root, job, limits)
        outcome = {'execution_returncode': None, 'execution_error': None,
                   'cleanup': {'cleanup_status': 'pending'}, 'execution_kind': 'component',
                   'benchmark_score': None, 'benchmark_status': 'not_run'}
        try:
            try:
                if supervisor._phase_deadlines:
                    raise RuntimeError('Started native component requires reconciliation, not replay')
                staged = await supervisor.run_phase('setup', component_setup)
                validate_component(staged)

                async def work_phase():
                    validate_component(staged)
                    if cancelled(): raise RuntimeError('QA component cancelled before launch')
                    command = [executable, 'run', '--path', staged['task_path'],
                               *staged['native_environment_flags'], '--agent', agent,
                               '--jobs-dir', str(jobs), '--job-name', job,
                               '--n-concurrent', '1', '--n-attempts', '1', '--max-retries', '0',
                               '--timeout-multiplier', '1', '--environment-build-timeout-multiplier', '1',
                               '--disable-verification']
                    process = asyncio.create_task(run_bounded_process(
                        command, output=root / 'process.log', env=environment,
                        max_output_bytes=pipeline.get('process_output_max_bytes', 16*1024*1024), redact=secrets,
                    ))
                    try:
                        while not process.done():
                            if cancelled(): raise RuntimeError('QA component cancelled')
                            await asyncio.wait({process}, timeout=0.25)
                        return await process
                    finally:
                        if not process.done(): process.cancel()
                        await asyncio.gather(process, return_exceptions=True)

                outcome['execution_returncode'] = await supervisor.run_phase('work', work_phase)

                async def observations():
                    paths = list((jobs / job).glob('*/agent/qa-trajectory.json'))
                    return {'observation_artifacts': len(paths), 'grader_executed': False}

                outcome['observations'] = await supervisor.run_phase('verifier', observations)
                if outcome['execution_returncode']:
                    outcome['execution_error'] = 'HarborProcessFailed'
            except BaseException as error:
                outcome['execution_error'] = type(error).__name__
                supervisor.decide_stop(type(error).__name__)
            try:
                await supervisor.run_phase('publication', lambda: publish(outcome), preserve_evidence=True)
            except BaseException as error:
                outcome['publication_error'] = type(error).__name__
            try:
                outcome['cleanup'] = await supervisor.stop('component_complete', cleanup)
            except BaseException as error:
                outcome['cleanup'] = {'cleanup_status': 'pending', 'error_type': type(error).__name__}
            return outcome
        finally:
            supervisor.close()

    if component:
        outcome = asyncio.run(execute_component())
    else:
        outcome = asyncio.run(execute_staged_harbor(
            root, run_id=job, limits=limits, setup=setup, agent=agent, model=None,
            jobs_dir=jobs, job_name=job,
            max_output_bytes=pipeline.get('process_output_max_bytes', 16 * 1024 * 1024),
            decode=decode, publish=publish, cleanup=cleanup, env=environment,
            redact=secrets, executable=(executable,), should_cancel=cancelled,
        ))
    return outcome, root / 'process.log'
