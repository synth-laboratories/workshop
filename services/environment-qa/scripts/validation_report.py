"""Read-only pilot receipts; completion is not benchmark quality acceptance."""
import argparse
from collections import Counter
import json
from pathlib import Path
import time

from environment_qa.core import Store, verify_seal
from environment_qa.freeze import engine_freeze


def report(store, run_id):
    run = store.get(run_id)
    with store.connect() as con:
        rows = con.execute("SELECT kind,payload FROM events WHERE run_id=? AND kind IN "
                           "('codex.process.started','codex.process.stopped','codex.thread.opened',"
                           "'codex.tool.result','codex.turn.failed','codex.turn/failed') ORDER BY seq", (run_id,)).fetchall()
    started, stopped, threads, tools, errors = set(), set(), {}, Counter(), []
    for row in rows:
        event = json.loads(row['payload'])
        key = (event['gate_id'], event['attempt'])
        data = event['payload']
        if row['kind'] == 'codex.process.started': started.add((*key, data['pid']))
        if row['kind'] == 'codex.process.stopped': stopped.add((*key, data['pid']))
        if row['kind'] == 'codex.thread.opened': threads[key] = data['threadId']
        if row['kind'] == 'codex.tool.result' and data.get('ok'): tools[(key, data['tool'])] += 1
        if row['kind'] in {'codex.turn.failed', 'codex.turn/failed'}: errors.append({'gate': key[0], 'detail': data})
    evidence_gates = {e['gate'] for e in run['evidence']}
    results_by_gate = {e['gate']: e['result'] for e in run['evidence']}
    contracts = next((e['result'] for e in run['evidence'] if e['gate'] == 'dependency-contracts'), {})
    cleanup = [c for e in run['evidence'] for c in e['result'].get('cleanup', [])]
    trials = [{'gate': e['gate'], **t} for e in run['evidence'] for t in e['result'].get('trials', [])]
    return {
        'id': run_id, 'status': run['status'], 'profile': run['policy']['pipeline'].get('profile'),
        'profile_version': run['policy']['pipeline'].get('profile_version'),
        'task_sha256': run['bundle']['sha256'], 'seal': run['seal'], 'seal_valid': verify_seal(run),
        'wall_seconds_including_review': round((run['seal']['at'] if run['seal'] else time.time()) - run['created_at'], 3),
        'task_verdict': run.get('verdict'), 'quality_acceptance': 'not_measured',
        'gate_counts': dict(Counter(g['status'] for g in run['gates'])),
        'missing_required_results': [g['id'] for g in run['gates'] if g['required'] and g['id'] not in evidence_gates],
        'non_success_gates': [{'gate': g['id'], 'status': g['status']} for g in run['gates'] if g['status'] != 'succeeded'],
        'execution_gaps': [g['id'] for g in run['gates']
                           if g['status'] in {'failed', 'inconclusive'}
                           and g['executor'] != 'disposition'
                           and not results_by_gate.get(g['id'], {}).get('assessment')],
        'app_servers_started': len(started), 'app_servers_stopped': len(stopped),
        'missing_stop_receipts': sorted(started - stopped),
        'thread_count': len(threads), 'unique_thread_count': len(set(threads.values())),
        'successful_tool_calls': dict(Counter({name: sum(n for (_, tool), n in tools.items() if tool == name)
                                               for name in {tool for _, tool in tools}})),
        'failed_turns': errors, 'retained_contracts': len(contracts.get('contracts', [])),
        'assumption_coverage': contracts.get('assumption_coverage', {}),
        'cleanup_receipts': cleanup, 'trials': trials,
        # The surface and the assurance are the two things a later report cannot
        # reconstruct if they are not carried out of the run here.
        'launch_surface': run['policy'].get('surface'),
        'decisions': [{'gate': i['gate_id'], 'actor': i.get('actor'), 'decision': i.get('decision'),
                       'assurance': i.get('actor_assurance')}
                      for i in run['interactions']],
        'verified_human_decisions': sum(i.get('actor') == 'local-human'
                                        and i.get('actor_assurance') == 'operator-token'
                                        for i in run['interactions']),
        'cap_usd': run['budget']['limit_usd'],
        'transport_bound_usd_not_invoice': run['budget'].get('transport', {}).get('reserved_usd'),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--store', type=Path, required=True)
    parser.add_argument('--run-id', action='append', required=True)
    parser.add_argument('--freeze', type=Path, required=True)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    frozen, current = json.loads(args.freeze.read_text()), engine_freeze()
    result = {'notice': 'Development integration receipts only. No gold score or acceptance claim.',
              'engine_matches_preflight': frozen['engine_sha256'] == current['engine_sha256'],
              'client_matches_preflight': frozen['client_sha256'] == current['client_sha256'],
              'engine_sha256': current['engine_sha256'],
              'runs': [report(Store(args.store), run_id) for run_id in args.run_id]}
    body = json.dumps(result, indent=2) + '\n'
    if args.out: args.out.write_text(body)
    else: print(body, end='')


if __name__ == '__main__': main()
