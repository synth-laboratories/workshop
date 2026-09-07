import argparse
import fcntl
import json
from pathlib import Path
from .bundles import export_bundle
from .core import Store
from .worker import step, recover


def main():
    parser = argparse.ArgumentParser(description="Workshop Environment QA prototype")
    parser.add_argument("--store", type=Path, required=True)
    sub = parser.add_subparsers(dest="command", required=True)
    serve = sub.add_parser("serve")
    serve.add_argument("--task-root", action="append", type=Path, required=True)
    serve.add_argument("--port", type=int, default=7338)
    serve.add_argument("--provider-budget-usd",type=float,help="Explicitly authorized aggregate UI run allowance; never replenished on restart")
    serve.add_argument("--allowance-id",help="Unique ID for this user-authorized budget")
    run = sub.add_parser("run")
    run.add_argument("task", type=Path)
    run.add_argument("--mode", choices=["automated", "hitl"])
    run.add_argument("--profile", choices=["tbench-hitl", "tbench-non-hitl", "k3-non-hitl"])
    run.add_argument("--charter", choices=["terminal-bench", "reb-systems", "reb-visuals"], default="terminal-bench")
    run.add_argument("--reviewer", choices=["rules", "ai"], default="rules")
    run.add_argument("--harbor", action="store_true")
    run.add_argument("--budget-usd", type=float, default=0)
    run.add_argument("--task-goals", default="")
    run.add_argument("--parent")
    run.add_argument("--request-key")
    run.add_argument("--enqueue", action="store_true", help="Submit to an already running service without taking its worker lock")
    run.add_argument("--full", action="store_true", help="Run the versioned full QA DAG, including agent and adversarial runtime trials")
    run.add_argument("--targeted", action="store_true", help="Source-led QA with at most two hypothesis-driven experiments")
    run.add_argument("--policy", type=Path, help="Trusted JSON DAG policy override")
    inspect = sub.add_parser("inspect")
    inspect.add_argument("run_id")
    importer = sub.add_parser("import-run",help="Import a sealed run and verified evidence from a local store")
    importer.add_argument("source_store",type=Path)
    importer.add_argument("run_id")
    compare_parser = sub.add_parser("compare",help="Post-seal AI reference matching in a separate private scorer store")
    compare_parser.add_argument("prediction",type=Path)
    compare_parser.add_argument("gold",type=Path)
    compare_parser.add_argument("--output",type=Path,required=True)
    compare_parser.add_argument("--budget-usd",type=float,required=True)
    args = parser.parse_args()
    if args.command == "compare":
        from .matching import compare
        print(json.dumps(compare(json.loads(args.prediction.read_text()),json.loads(args.gold.read_text()),args.output,args.budget_usd),indent=2))
        return
    if args.command == "serve":
        from .server import serve
        serve(args.store, args.task_root, args.port,args.provider_budget_usd,args.allowance_id)
        return
    store = Store(args.store)
    if args.command == "import-run":
        from .importing import import_run
        print(import_run(Store(args.source_store),store,args.run_id)["id"])
        return
    if args.command == "inspect":
        print(json.dumps(store.get(args.run_id), indent=2))
        return
    bundle = export_bundle(args.task, store.root, [args.task.resolve()])
    lock = None
    if not args.enqueue:
        lock = (store.root / "worker.lock").open("a")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit("Service is running; use --enqueue")
        recover(store)
    pipeline = None
    if sum(bool(x) for x in (args.full,args.targeted,args.policy,args.profile)) > 1:
        parser.error('Choose only one of --profile, --full, --targeted, --policy')
    if args.profile:
        from .profiles import resolve
        args.mode, pipeline = resolve(args.profile, args.mode)
    if args.full or args.targeted or args.policy:
        from .policy import full_policy, targeted_policy, validate
        pipeline = validate(json.loads(args.policy.read_text())) if args.policy else targeted_policy() if args.targeted else full_policy()
    record = store.create(bundle, args.mode or "automated", args.charter, args.reviewer, args.harbor,
                          args.budget_usd, args.request_key, args.parent, args.task_goals, pipeline=pipeline)
    if not args.enqueue:
        if pipeline:
            from .dag import run_until_idle
            run_until_idle(store, record["id"])
        else:
            while step(store, record["id"]):
                pass
    print(json.dumps(store.get(record["id"]), indent=2))
    if lock:
        lock.close()


if __name__ == "__main__":
    main()
