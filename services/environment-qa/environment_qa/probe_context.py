"""Hash-checked transfer of the planner's evidence into the QA-only sandbox."""
import json
from pathlib import Path
from .core import digest

def prepare(store,reference,destination):
    path=(store.root/reference['path']).resolve()
    if not path.is_relative_to(store.root.resolve()):raise ValueError('Unsafe probe context path')
    files=json.loads(path.read_text())
    if digest(files)!=reference['sha256']:raise ValueError('Probe context digest mismatch')
    copied=[]
    for name,body in files.items():
        rel=Path(name)
        if not rel.parts or rel.is_absolute() or '..' in rel.parts:raise ValueError('Unsafe probe evidence path')
        if rel.parts[0] not in {'external','cached-runtime','evidence'}:continue
        target=destination/rel;target.parent.mkdir(parents=True,exist_ok=True)
        target.write_text(body)
        copied.append({'path':name,'sha256':digest(body)})
    # The staged filenames are intentionally projections, not original checkout
    # paths. Give the executor an explicit map so it need not spend its limited
    # actions searching for pyproject.toml when it is dependency-01.txt.
    inventory=json.loads(files.get('evidence/dependency-inventory.json','{}'))
    source_index=[{key:record[key] for key in ('document','url','projection','sha256') if key in record}
                  for record in inventory.get('records',[]) if record.get('document') in files][:32]
    return {'context_digest':reference['sha256'],'files':copied,'source_index':source_index,'notice':'QA-only evidence projection; not original task-agent access. Complete source originals remain in the host evidence manifest.'}
