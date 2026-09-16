#!/usr/bin/env python3
"""Stage the pinned Containers authority and a relocatable Python beside Workshop.

No developer PATH or credential store is consulted by the installed launcher.
The build uses a wheel, never an editable checkout. Reuse requires the same
source fingerprint, version, and Python pin.
"""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, tempfile, tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
PYTHON = '3.12.11'

def run(args, **kwargs):
    return subprocess.run(args, check=True, timeout=kwargs.pop("timeout", 600), **kwargs)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--containers', type=pathlib.Path, default=pathlib.Path(os.environ.get('CONTAINERS_ROOT', str(ROOT.parent/'containers'))))
    parser.add_argument('--synth-ai', type=pathlib.Path, default=pathlib.Path(os.environ.get('SYNTH_AI_PROJECT_ROOT', str(ROOT.parent/'synth-ai'))))
    parser.add_argument('--output', type=pathlib.Path, default=ROOT/'runtime-distributions/trace')
    args = parser.parse_args()
    source = args.containers.resolve(); sdk_source = args.synth_ai.resolve(); output = args.output.resolve()
    version = tomllib.loads((source/'pyproject.toml').read_text())['project']['version']
    pinned = (ROOT/'apps/synth_desktop/src-tauri/synth-containers-version.txt').read_text().strip()
    if version != pinned: raise SystemExit(f'Containers source {version} does not match Workshop pin {pinned}')
    sdk_version = tomllib.loads((sdk_source/'pyproject.toml').read_text())['project']['version']
    sdk_pinned = (ROOT/'apps/synth_desktop/src-tauri/synth-ai-version.txt').read_text().strip()
    if sdk_version != sdk_pinned: raise SystemExit(f'Synth SDK source {sdk_version} does not match Workshop pin {sdk_pinned}')
    sdk_revision = (ROOT/'apps/synth_desktop/src-tauri/synth-ai-revision.txt').read_text().strip()
    actual_sdk_revision = subprocess.check_output(['git','-C',str(sdk_source),'rev-parse','HEAD'], text=True).strip()
    if actual_sdk_revision != sdk_revision: raise SystemExit(f'Synth SDK source {actual_sdk_revision} does not match Workshop pin {sdk_revision}')
    if subprocess.check_output(['git','-C',str(sdk_source),'status','--porcelain'], text=True).strip():
        raise SystemExit(f'Synth SDK source must be clean: {sdk_source}')
    digest = hashlib.sha256()
    for path in sorted([source/'pyproject.toml', *source.joinpath('src').rglob('*.py')]):
        digest.update(str(path.relative_to(source)).encode()); digest.update(path.read_bytes())
    for path in sorted([sdk_source/'pyproject.toml', *sdk_source.joinpath('synth_ai').rglob('*.py')]):
        digest.update(('synth-ai/'+str(path.relative_to(sdk_source))).encode()); digest.update(path.read_bytes())
    bridge_source = ROOT/'scripts/synth-swarms-sdk-bridge.py'
    digest.update(b'synth-swarms-sdk-bridge.py'); digest.update(bridge_source.read_bytes())
    fingerprint = digest.hexdigest()
    receipt = output/'trace-runtime/build.json'
    if receipt.exists():
        old = json.loads(receipt.read_text())
        if old.get('sourceFingerprint') == fingerprint and old.get('python') == PYTHON and old.get('layoutVersion') == 4:
            run([str(output/'bin/synth-trace'), 'version']); run([str(output/'bin/synth-swarms-sdk'),'capabilities'],input=b'{"baseUrl":"http://127.0.0.1:1","apiKey":"stage-probe","payload":{}}',stdout=subprocess.DEVNULL); print(receipt); return
    output.mkdir(parents=True, exist_ok=True)
    cache = ROOT/'.test-tmp/trace-package-cache';cache.mkdir(parents=True,exist_ok=True)
    env = {**os.environ, 'UV_CACHE_DIR':str(cache), 'TMPDIR':str(cache)}
    with tempfile.TemporaryDirectory(dir=output, prefix='.trace-stage-') as temporary:
        stage = pathlib.Path(temporary); runtime = stage/'trace-runtime';runtime.mkdir()
        run(['uv','python','install',PYTHON,'--install-dir',str(runtime/'python'),'--no-bin'],env=env)
        python = next((runtime/'python').glob('*/bin/python3')).resolve()
        for alias in (runtime/'python').iterdir():
            if alias.is_symlink() and pathlib.Path(os.readlink(alias)).is_absolute():
                destination = alias.resolve();alias.unlink();alias.symlink_to(os.path.relpath(destination,alias.parent))
        run(['uv','build','--wheel','--out-dir',str(stage/'wheels'),str(source)],env=env)
        wheel = next((stage/'wheels').glob('*.whl'))
        run(['uv','pip','install','--python',str(python),'--target',str(runtime/'site-packages'),str(wheel)],env=env)
        sdk_wheels = stage/'sdk-wheels'; sdk_wheels.mkdir()
        run(['uv','build','--wheel','--out-dir',str(sdk_wheels),str(sdk_source)],env=env)
        sdk_wheel = next(sdk_wheels.glob('*.whl'))
        run(['uv','pip','install','--python',str(python),'--target',str(runtime/'site-packages'),str(sdk_wheel)],env=env)
        shutil.copy2(bridge_source, runtime/'synth-swarms-sdk-bridge.py')
        python_relative = python.relative_to(runtime)
        manifest = {'schemaVersion':'synth.trace-runtime.v1','layoutVersion':4,'version':version,'synthAiVersion':sdk_version,'python':PYTHON,'sourceFingerprint':fingerprint,
                    'wheelSha256':hashlib.sha256(wheel.read_bytes()).hexdigest(),'synthAiWheelSha256':hashlib.sha256(sdk_wheel.read_bytes()).hexdigest(),'pythonExecutable':str(python_relative)}
        (runtime/'build.json').write_text(json.dumps(manifest,indent=2)+'\n')
        # Launcher locates every dependency from its own packaged location.
        launcher = '#!/bin/sh\nset -eu\nHERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\n'
        launcher += 'export PYTHONNOUSERSITE=1\nexport PYTHONDONTWRITEBYTECODE=1\n'
        launcher += 'export PYTHONPATH="$HERE/../trace-runtime/site-packages"\n'
        launcher += 'if [ "$#" -eq 1 ] && [ "$1" = "version" ]; then\n'
        launcher += f'  exec "$HERE/../trace-runtime/{python_relative}" -c \'from importlib.metadata import version; print(version("synth-containers"))\'\nfi\n'
        launcher += f'exec "$HERE/../trace-runtime/{python_relative}" -m synth_containers.tracing.cli "$@"\n'
        (stage/'bin').mkdir();(stage/'bin/synth-trace').write_text(launcher);(stage/'bin/synth-trace').chmod(0o755)
        swarm_launcher = '#!/bin/sh\nset -eu\nHERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\n'
        swarm_launcher += 'export PYTHONNOUSERSITE=1\nexport PYTHONDONTWRITEBYTECODE=1\n'
        swarm_launcher += 'export PYTHONPATH="$HERE/../trace-runtime/site-packages"\n'
        swarm_launcher += f'exec "$HERE/../trace-runtime/{python_relative}" "$HERE/../trace-runtime/synth-swarms-sdk-bridge.py" "$@"\n'
        (stage/'bin/synth-swarms-sdk').write_text(swarm_launcher);(stage/'bin/synth-swarms-sdk').chmod(0o755)
        run([str(stage/'bin/synth-trace'),'research-query','--help'],env=env,stdout=subprocess.DEVNULL)
        old = output/'.trace-runtime-previous'
        if old.exists(): shutil.rmtree(old)
        if (output/'trace-runtime').exists(): (output/'trace-runtime').rename(old)
        try:
            runtime.rename(output/'trace-runtime');(output/'bin').mkdir(exist_ok=True)
            (stage/'bin/synth-trace').replace(output/'bin/synth-trace')
            (stage/'bin/synth-swarms-sdk').replace(output/'bin/synth-swarms-sdk')
        except BaseException:
            if (output/'trace-runtime').exists(): shutil.rmtree(output/'trace-runtime')
            if old.exists():old.rename(output/'trace-runtime')
            raise
        if old.exists():shutil.rmtree(old)
    run([str(output/'bin/synth-trace'),'version']);print(receipt)

if __name__ == '__main__':main()
