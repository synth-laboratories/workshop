"""Bounded coverage allocation: do not hide six checks in one probe slot."""
import json


def allocate(contracts, experiments):
    if not contracts:
        return experiments[:2], []
    # Two independent containers already exist in the policy. Use both for
    # required coverage when the inventory exceeds one small group; no extra
    # container, model call or wall-clock allowance is introduced here.
    groups = [contracts] if len(contracts) <= 2 else [contracts[::2], contracts[1::2]]
    probes = []
    for group in groups:
        probes.append({
            'execution_kind': 'component',
            'hypothesis': 'Required third-party assumptions must hold under the task-resolved dependencies.',
            'objective': 'Run ONLY these source-derived contracts: ' + json.dumps(group) +
                '. Inspect actual returned keys, shapes, arguments and selected input values, not just imports. '
                'Do not run the full task. Keep minimal installed dependencies in a persistent temporary '
                'directory within this disposable container across steps; do not delete them between '
                'setup and measurement. Record each contract separately as checked or not_checked, '
                'with observations or a concrete blocker. One failed check must not prevent the others. '
                'Use bounded parallel HTTP requests for small task-listed record sets. '
                'Stay within the shared 120-second deadline.',
            'confirmation': 'An observed mismatch in the exact source-consumed property establishes '
                'that component incompatibility only. Unchecked contracts remain unknown; successful '
                'imports or a substitute operation do not confirm the required behavior.'})
    remaining = 2 - len(probes)
    deferred = ['Speculative experiment deferred for required contract coverage: ' + item['hypothesis']
                for item in experiments[remaining:]]
    return probes + experiments[:remaining], deferred
