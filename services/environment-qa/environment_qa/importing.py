"""Import a sealed local run and its verified evidence without changing the seal."""
import hashlib
import json
import shutil
from pathlib import Path
from .core import canonical, digest, verify_seal
from .bundles import verified_path


def import_run(source, destination, run_id):
    run = source.get(run_id)
    if not verify_seal(run): raise ValueError("Only sealed predictions can be imported")
    bundle = verified_path(source,run["bundle"])
    target = destination.root/"bundles"/bundle.name
    if not target.exists(): shutil.copytree(bundle,target)
    verified_path(destination,run["bundle"])
    refs = []
    for evidence in run["evidence"]:
        result = evidence["result"]
        refs.extend((a["path"],a["sha256"],False) for a in result.get("artifacts",[]))
        if result.get("context_ref"):
            ref = result["context_ref"]
            refs.append((ref["path"],ref["sha256"],True))
    for relative, expected, structured in refs:
        path = Path(relative)
        if path.is_absolute() or ".." in path.parts: raise ValueError("Unsafe artifact reference")
        original, copied = source.root/path,destination.root/path
        if original.is_symlink() or not original.resolve().is_relative_to(source.root): raise ValueError("Unsafe source artifact")
        if not copied.resolve().is_relative_to(destination.root): raise ValueError("Unsafe destination artifact")
        data = original.read_bytes()
        actual = digest(json.loads(data)) if structured else hashlib.sha256(data).hexdigest()
        if actual != expected: raise ValueError("Artifact digest mismatch")
        copied.parent.mkdir(parents=True,exist_ok=True)
        if copied.exists():
            if copied.read_bytes() != data: raise ValueError("Existing artifact differs")
        else:
            with copied.open("xb") as handle: handle.write(data)
    with destination.connect() as con:
        con.execute("BEGIN IMMEDIATE")
        prior = con.execute("SELECT document FROM runs WHERE id=?",(run_id,)).fetchone()
        if prior:
            if canonical(json.loads(prior[0])) != canonical(run): raise ValueError("Existing run differs")
        else:
            con.execute("INSERT INTO runs VALUES(?,?,?,?,?)",(run_id,"sealed-import-"+run_id,digest(run),run["revision"],canonical(run)))
            cursor = 0
            while True:
                events = source.events(run_id,cursor)
                if not events: break
                for event in events:
                    con.execute("INSERT INTO events(run_id,kind,at,revision,payload) VALUES(?,?,?,?,?)",(run_id,event["kind"],event["at"],event["revision"],canonical(event["payload"])))
                cursor = events[-1]["seq"]
    return run
