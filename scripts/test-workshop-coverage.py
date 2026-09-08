#!/usr/bin/env python3
"""Real isolated-runtime coverage checks; no provider calls or user credentials.

Exercises shared compatibility handlers, native-owned browser lifetime, and
headless desktop-state CAS. The only web page is a loopback fixture served by
this process. Use a dedicated instance data directory.
"""
import argparse
import http.server
import json
from pathlib import Path
import runpy
import threading
import uuid

MCP = runpy.run_path(str(Path(__file__).with_name('test-workshop-runtime.py')))['MCP']

class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'<html><head><title>Workshop coverage</title></head><body><h1>Coverage fixture</h1><button onclick="document.querySelector(\'output\').textContent=\'Count: 1\'">Increment</button><output>Count: 0</output></body></html>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--data-root', required=True, type=Path)
    parser.add_argument('--receipt', required=True, type=Path)
    args = parser.parse_args()
    root = args.data_root.resolve(strict=True)
    assert 'artifacts' in root.parts and 'capabilities' in root.parts, 'dedicated capability test instance required'
    config = root / 'context/settings.json'
    before = config.read_bytes() if config.exists() else None
    settings = json.loads(before or b'{}')
    settings.setdefault('mcpGroupEnabled', {})['browser'] = True
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps(settings))
    config.chmod(0o600)
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Page)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    client = None
    browser = None
    try:
        client = MCP(args.binary.resolve(), root)
        tools = client.request('tools/list', {})['tools']
        names = {tool['name'] for tool in tools}
        for name in ['container_prepare_rollout', 'container_start_prepared_rollout', 'visual_authoring_context', 'visual_bind_data_source', 'visual_capture_review', 'desktop_state_get', 'desktop_state_update', 'browser_create_session', 'browser_click']:
            assert name in names, name
        initial = client.call('runtime_status')
        client.call('runtime_control', {'action': 'detach'})
        client.call('container_list')
        visual = client.call('visual_create', {'template_id': 'diagram.mermaid.v1', 'title': 'Coverage proof', 'content': 'flowchart LR\n A[Input] --> B[Shared runtime] --> C[Output]'})['visual']
        owner = visual['workspaceId']
        assert owner and not visual.get('sessionId')
        authoring = client.call('visual_authoring_context', {'visual_id': visual['id']})
        assert authoring['visual']['id'] == visual['id']
        fork = client.call('visual_fork', {'visual_id': visual['id'], 'title': 'Shared fork'})['visual']
        assert fork['workspaceId'] == owner and not fork.get('sessionId')
        state = client.call('desktop_state_get')['entries']['synth.preferences.v1']
        preferences = json.loads(state['value'])
        preferences['appearance']['theme'] = 'dark'
        changed = client.call('desktop_state_update', {'key': 'synth.preferences.v1', 'value': json.dumps(preferences), 'expectedRevision': state['revision']})
        assert changed['revision'] == state['revision'] + 1
        client.call('desktop_state_update', {'key': 'synth.preferences.v1', 'value': json.dumps(preferences), 'expectedRevision': state['revision']}, error=True)
        preferences['approvalPolicy'] = 'untrusted' if preferences.get('approvalPolicy') == 'never' else 'never'
        preferences['approvalMode'] = 'ask' if preferences.get('approvalMode') == 'allow-all' else 'allow-all'
        client.call('desktop_state_update', {'key': 'synth.preferences.v1', 'value': json.dumps(preferences), 'expectedRevision': changed['revision']}, error=True)
        client.call('desktop_state_commit', {'input': {}}, error=True)
        client.call('human_annotation_manage', {'operation': 'human_annotation_campaign_adjudicate', 'arguments': {}}, error=True)
        client.call('secrets_manage', {'operation': 'request_env_import'}, error=True)
        browser = client.call('browser_create_session', {'profile': 'coverage-' + uuid.uuid4().hex})
        target = {'session_id': browser['sessionId'], 'tab_id': browser['tabId']}
        client.call('browser_navigate', {**target, 'url': f'http://127.0.0.1:{server.server_port}/'})
        snapshot = client.call('browser_snapshot', target)
        assert 'Coverage fixture' in json.dumps(snapshot)
        client.call('browser_click', {**target, 'target': {'locator': {'role': 'button', 'name': 'Increment', 'exact': True}}})
        after_click = client.call('browser_snapshot', target)
        assert 'Count: 1' in json.dumps(after_click), after_click
        # Policy rejection must retain the existing managed session.
        client.call('browser_navigate', {**target, 'url': 'https://example.com'}, error=True)
        client.close()
        client = MCP(args.binary.resolve(), root)
        tabs = client.call('browser_list_tabs', {'session_id': browser['sessionId']})['tabs']
        assert any(tab['tabId'] == browser['tabId'] for tab in tabs)
        shot = client.call('browser_screenshot', target)
        assert Path(shot['path']).is_file()
        final = client.call('runtime_status')
        assert final['processId'] == initial['processId'] and not final['desktopAttached']
        assert client.call('desktop_state_get')['entries']['synth.preferences.v1'] == changed
        receipt = {'toolCount': len(names), 'runtime': final, 'visualId': visual['id'], 'forkId': fork['id'], 'workspaceId': owner, 'browserScreenshot': shot['path'], 'checks': ['shared-authoring-and-fork', 'headless-preferences-cas', 'permission-preference-denial', 'facade-human-decision-denial', 'real-browser-click-and-snapshot', 'browser-origin-denial-preserves-session', 'browser-survives-mcp-client-reconnect'], 'providerCalls': False}
        args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps(receipt, indent=2))
    finally:
        if client:
            try:
                if browser: client.call('browser_close_session', {'session_id': browser['sessionId']})
            finally: client.close()
        server.shutdown()
        server.server_close()
        if before is None: config.unlink(missing_ok=True)
        else: config.write_bytes(before)

if __name__ == '__main__':
    main()
