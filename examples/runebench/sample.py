"""Package or restore one explicit, scripted recording; never include credentials."""
import argparse
import hashlib
import json
import re
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent
ALLOWED = {
    'matrix_run_manifest.json', 'events.jsonl',
    'episode/events.jsonl', 'episode/engine-states.jsonl', 'episode/states.jsonl',
    'episode/baseline.json', 'episode/cutoff.json', 'episode/job_result.json',
    *(f'episode/{actor}.mp4' for actor in ('maa', 'mab', 'mba', 'mbb')),
    *(f'episode/record-{actor}.log' for actor in ('maa', 'mab', 'mba', 'mbb')),
}

def package(run):
    run = run.resolve()
    if not run.is_dir() or not re.fullmatch(r'[A-Za-z0-9-]+', run.name):
        raise ValueError('Sample must be one explicitly selected run directory')
    manifest = json.loads((run / 'matrix_run_manifest.json').read_text())
    result = json.loads((run / 'episode/job_result.json').read_text())
    if manifest['config']['evidence_kind'] != 'scripted' or result['status'] != 'evaluated' or result['usage']['calls'] != 0:
        raise ValueError('Only a successful zero-model-call scripted sample may ship')
    for name in ALLOWED:
        path = run / name
        if not path.is_file() or path.is_symlink():
            raise ValueError(f'Missing regular sample file: {name}')
        if path.suffix != '.mp4':
            content = path.read_text()
            if any(marker in content for marker in ('/Users/', 'Bearer ', 'sk-or-', 'access_token', 'refresh_token', 'OPENROUTER_API_KEY=')):
                raise ValueError(f'Private/credential-shaped data in {name}; review before distribution')
    archive = HERE / 'sample.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        for name in sorted(ALLOWED):
            info = tar.gettarinfo(str(run / name), arcname=f'{run.name}/{name}')
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mode = 0o644
            with (run / name).open('rb') as source:
                tar.addfile(info, source)
    if archive.stat().st_size > 9_000_000:
        raise ValueError('Sample exceeds the small public-example size budget')
    policy = json.loads((HERE / 'manifest.json').read_text())
    policy['sample'] = {'file': archive.name, 'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'run': run.name, 'kind': 'scripted', 'modelCalls': 0}
    (HERE / 'manifest.json').write_text(json.dumps(policy, indent=2) + '\n')
    print(f'Packaged {run.name}: {archive.stat().st_size} bytes')

def restore():
    sample = json.loads((HERE / 'manifest.json').read_text()).get('sample')
    if not sample:
        raise ValueError('This checkout has no packaged sample yet')
    archive = HERE / sample['file']
    if hashlib.sha256(archive.read_bytes()).hexdigest() != sample['sha256']:
        raise ValueError('Sample checksum mismatch')
    destination = HERE / 'results'
    if (destination / sample['run']).exists():
        raise ValueError('Sample already restored; use review or rebuild instead of overwriting it')
    with tarfile.open(archive, 'r:gz') as tar:
        expected = {f"{sample['run']}/{name}" for name in ALLOWED}
        members = tar.getmembers()
        if len(members) != len(expected) or {m.name for m in members} != expected or any(not m.isfile() for m in members):
            raise ValueError('Unexpected sample members')
        destination.mkdir(exist_ok=True)
        tar.extractall(destination, filter='data')
    print(f"Restored verified sample {sample['run']}")

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--package', type=Path)
    args = parser.parse_args()
    package(args.package) if args.package else restore()
