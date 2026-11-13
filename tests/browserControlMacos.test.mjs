import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { MacOSPanelBridge, MACOS_PANEL_TOOLS, nativeCommand } from '../src-tauri/resources/browser-control-macos.mjs';

async function ready(bridge) {
  await bridge.receive({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } });
  bridge.receive({ jsonrpc: '2.0', method: 'notifications/initialized' });
}
const call = (bridge, id, name, args = {}) => bridge.receive({ jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } });

test('macOS handshake and discovery never create a browser or launch external engines', async () => {
  const messages = []; let calls = 0;
  const bridge = new MacOSPanelBridge({ command: async () => { calls++; }, write: m => messages.push(m) });
  await ready(bridge);
  await bridge.receive({ jsonrpc: '2.0', id: 2, method: 'tools/list' });
  assert.equal(calls, 0); assert.equal(messages[0].result.protocolVersion, '2025-06-18');
  assert.ok(messages[1].result.tools.some(t => t.name === 'browser_take_screenshot'));
  assert.ok(!messages[1].result.tools.some(t => t.name === 'browser_file_upload'));
  bridge.close();
});

test('closed/unopened tab list does not create a page and close sends only close', async () => {
  const actions = [], messages = [];
  const bridge = new MacOSPanelBridge({ command: async c => actions.push(c.action), write: m => messages.push(m) });
  await ready(bridge); await call(bridge, 2, 'browser_tabs', { action: 'list' });
  assert.deepEqual(actions, []);
  await call(bridge, 3, 'browser_close'); assert.deepEqual(actions, ['close']);
  assert.equal(messages.at(-1).result.isError, undefined); bridge.close();
});

test('snapshot waits for native loading even while previous document is complete', async () => {
  let loading = 2, snapshots = 0; const messages = [];
  const bridge = new MacOSPanelBridge({ command: async c => {
    if (c.action === 'state') return { url: 'https://example.com', loading: loading-- > 0 };
    if (c.action === 'eval') {
      if (c.script.includes(')("info",')) return { ok: true, value: { ready: 'complete', documentId: 'new' } };
      snapshots++; return { ok: true, value: 'loaded page' };
    }
  }, write: m => messages.push(m) });
  await ready(bridge); await call(bridge, 2, 'browser_snapshot');
  assert.equal(snapshots, 1); assert.ok(loading < 0);
  assert.equal(messages.at(-1).result.content[0].text, 'loaded page'); bridge.close();
});

test('operation error does not retry on another browser or report success', async () => {
  const messages = [], actions = [];
  const bridge = new MacOSPanelBridge({ command: async c => { actions.push(c.action); if (c.action === 'eval') return { ok: false, error: 'stale ref' }; }, write: m => messages.push(m) });
  await ready(bridge); await call(bridge, 2, 'browser_click', { ref: 'old' });
  assert.deepEqual(actions, ['ensure', 'eval']);
  assert.equal(messages.at(-1).result.isError, true); assert.match(messages.at(-1).result.content[0].text, /stale ref/); bridge.close();
});

test('cancellation of queued requests skips side effects and releases queue', async () => {
  const messages = [], actions = [];
  const bridge = new MacOSPanelBridge({ command: async c => actions.push(c.action), write: m => messages.push(m) });
  await ready(bridge);
  const pending = call(bridge, 2, 'browser_snapshot');
  bridge.receive({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 2 } });
  await pending; assert.deepEqual(actions, []); assert.equal(messages.at(-1).error.code, -32800);
  await bridge.receive({ jsonrpc: '2.0', id: 3, method: 'tools/list' });
  assert.equal(messages.at(-1).result.tools.length, MACOS_PANEL_TOOLS.length); bridge.close();
});

test('native HTTP adapter validates endpoint credentials and propagates server failures', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'crewvela-mac-bridge-'));
  const file = path.join(dir, 'panel-macos.json'); let requests = 0;
  const token = '12345678-1234-1234-1234-123456789abc';
  const server = createServer((req, res) => {
    requests++; assert.equal(req.url, '/command'); assert.equal(req.headers.authorization, `Bearer ${token}`);
    assert.equal(req.headers.origin, undefined); res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ error: 'control disabled' }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    writeFileSync(file, JSON.stringify({ port: server.address().port, token: 'bad' }));
    await assert.rejects(nativeCommand(file, { action: 'ensure' }), /配置无效/); assert.equal(requests, 0);
    writeFileSync(file, JSON.stringify({ port: server.address().port, token }));
    await assert.rejects(nativeCommand(file, { action: 'ensure' }), /control disabled/); assert.equal(requests, 1);
  } finally { await new Promise(resolve => server.close(resolve)); rmSync(dir, { recursive: true, force: true }); }
});
