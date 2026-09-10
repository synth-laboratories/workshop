"""Read-only recovery command for a source archive already downloaded by pip."""
import re


def command(package):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+', package):
        raise ValueError('Invalid package name')
    return "python3 - <<'QA_ARCHIVE_PY'\n" + SCRIPT.replace('PACKAGE_LITERAL', repr(package)) + '\nQA_ARCHIVE_PY'


SCRIPT = '''import hashlib, json, tarfile
from pathlib import Path
package = PACKAGE_LITERAL
print('QA SOURCE-ONLY RECOVERY: no archive member is executed or extracted; this is not a runtime API measurement.')
budget = 32000
seen = set()
for archive in sorted(Path('/tmp').glob('pip-*/*')):
    if not archive.is_file() or not archive.name.lower().replace('_', '-').startswith(package.lower().replace('_', '-') + '-'):
        continue
    if not archive.name.endswith(('.tar.gz', '.tgz')) or archive.stat().st_size > 5_000_000:
        continue
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest in seen:
        continue
    seen.add(digest)
    print(json.dumps({'archive': str(archive), 'sha256': digest, 'package': package}))
    with tarfile.open(archive, 'r:gz') as stream:
        members = []
        for index, member in enumerate(stream):
            if index >= 2000:
                break
            if member.isfile() and 0 < member.size <= 65536 and member.name.endswith(('.py', '.pyx', '.toml')):
                members.append(member)
        members.sort(key=lambda member: (not member.name.endswith('.py'), member.name))
        for member in members[:24]:
            if budget <= 0:
                break
            handle = stream.extractfile(member)
            data = handle.read(min(member.size, budget, 8000))
            budget -= len(data)
            print(json.dumps({'member': member.name, 'source_excerpt': data.decode('utf-8', errors='replace'), 'omitted_bytes': member.size - len(data)}))
    break
if not seen:
    print('No eligible downloaded source archive found; compatibility remains untested.')
'''
