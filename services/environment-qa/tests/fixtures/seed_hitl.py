"""Real TBench source checks in a disposable UI-test store; no model calls."""
import sys
import shutil
from pathlib import Path
from environment_qa.bundles import ALLOWED, export_bundle
from environment_qa.core import Store
from environment_qa.server import serve

root = Path(sys.argv[1])
tasks = [Path(p).resolve() for p in sys.argv[2:4]]
store = Store(root / "store")
for index, task in enumerate(tasks):
    # Another test runner may have generated __pycache__ in the corpus. Never
    # delete it or weaken production admission: stage only task components here.
    snapshot = root / f"snapshot-{index}"
    if not snapshot.exists():
        snapshot.mkdir()
        for name in ALLOWED:
            entry = task / name
            if entry.is_symlink():
                raise ValueError("Fixture source cannot contain symlinked components")
            if entry.is_dir():
                shutil.copytree(entry, snapshot / name, symlinks=True,
                                ignore=shutil.ignore_patterns("__pycache__"))
            elif entry.is_file():
                shutil.copyfile(entry, snapshot / name)
    store.create(export_bundle(snapshot, store.root, [root]), mode="hitl",
                 request_key=f"ui-test-{index}",
                 overlay="UI integration fixture on a real TBench snapshot. Rules-only, no Codex. Automated test decisions are not human adjudication.")
serve(store.root, tasks, 17340)
