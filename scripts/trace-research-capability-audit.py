#!/usr/bin/env python3
"""Non-secret proxy accounting for the retained RuneBench acceptance attempts."""
import datetime,json,sqlite3
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]/'artifacts/trace-research-e2e/runebench-proxy'
db=sqlite3.connect(ROOT/'synth.sqlite3');db.row_factory=sqlite3.Row
attempts=[]
for path in sorted(ROOT.glob('*.json')):
    if len(path.stem)!=32:continue
    attempt=json.loads(path.read_text())
    usage=[]
    for run_id in attempt.get('runIds',[]):
        row=db.execute('SELECT run_id,used_calls,used_input_tokens,used_output_tokens,used_cost_usd_micros,used_cost_known,status FROM secret_capabilities WHERE run_id=?',(run_id,)).fetchone()
        if row:usage.append(dict(row))
    attempts.append({'receipt':path.name,'processSuccess':attempt['processSuccess'],'usage':usage})
report={'recordedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'attempts':attempts,
        'allRecordedCapabilitiesNonUsable':all(row['status'] in ('revoked','exhausted','expired') for a in attempts for row in a['usage']),
        'reservedMaximumMicros':json.loads((ROOT/'reconstruction-budget.json').read_text()),
        'proxyReportedCostMicros':sum(row['used_cost_usd_micros'] for a in attempts for row in a['usage']),
        'accountBilling':None,'note':'Proxy-reported usage and conservative reservations are distinct; account billing is unavailable.'}
(ROOT/'final-capability-audit.json').write_text(json.dumps(report,indent=2))
print(json.dumps({k:report[k] for k in ('allRecordedCapabilitiesNonUsable','reservedMaximumMicros','proxyReportedCostMicros')}))
