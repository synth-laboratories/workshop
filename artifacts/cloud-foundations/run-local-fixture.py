"""Exercise only this fresh packaged instance and its local deterministic model."""
import json
import urllib.request
from pathlib import Path

root = Path(__file__).resolve().parents[2]
instance = root / '.test-tmp/instances/v09/cloud-local-e06'
connection = json.loads((instance / 'data/eval-driver.json').read_text())
assert connection['instanceName'] == 'cloud-local-e06'
assert connection['url'].startswith('http://127.0.0.1:')
output = root / '.test-tmp/packaged-evidence'
output.mkdir(parents=True, exist_ok=True)

def call(method, route, body=None):
    request = urllib.request.Request(connection['url'] + route,
        data=None if body is None else json.dumps(body).encode(), method=method,
        headers={'Authorization': 'Bearer ' + connection['token'], 'Content-Type':'application/json'})
    with urllib.request.urlopen(request, timeout=50) as response:
        return json.load(response)

session = 'local-e06-fixture'
health = call('GET', '/v1/health')
(output / 'packaged-health.json').write_text(json.dumps(health, indent=2)+'\n')
created = call('POST', '/v1/sessions', {
    'sessionId':session, 'workspace':str(instance/'workspace'),
    'provider':'local-laguna', 'model':'poolside/Laguna-XS-2.1-NVFP4-mlx',
    })
(output / 'packaged-session-created.json').write_text(json.dumps(created, indent=2)+'\n')
call('POST', f'/v1/sessions/{session}/select', {})
sent = call('POST', f'/v1/sessions/{session}/messages', {
    'provider':'local-laguna', 'model':'poolside/Laguna-XS-2.1-NVFP4-mlx',
    'body':'Do not call tools. Reply with the Local-only fixture result.', 'effort':'none'})
(output / 'packaged-message-sent.json').write_text(json.dumps(sent, indent=2)+'\n')
terminal = call('POST', f'/v1/sessions/{session}/wait_terminal', {'timeoutMs':40000})
(output / 'packaged-terminal.json').write_text(json.dumps(terminal, indent=2)+'\n')
exported = call('GET', f'/v1/sessions/{session}/export')
(output / 'packaged-local-export.json').write_text(json.dumps(exported, indent=2)+'\n')
print(json.dumps({'session':session, 'terminal':terminal.get('terminal'), 'kind':terminal.get('kind')}))
