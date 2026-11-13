// Native WKWebView + real stdio MCP acceptance; no model/API requests or user data.
// Run: node tests/browserControlMacos.native.mjs [absolute harness executable]
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtempSync, readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { nativeCommand } from '../src-tauri/resources/browser-control-macos.mjs';
if (process.platform !== 'darwin') throw new Error('Native WKWebView acceptance requires macOS');
const root = mkdtempSync(path.join(tmpdir(), 'crewvela-native-browser-'));
const output = fileURLToPath(new URL('../output/macos-browser-panel-agent/', import.meta.url)); mkdirSync(output, { recursive: true });
const fixture = `<!doctype html><meta charset="utf-8"><title>Native Panel Fixture</title><h1>WKWebView 验收</h1>
<form action="/submitted"><label>姓名<input name="name"></label><label>城市<select name="city"><option value="a">北京</option><option value="b">上海</option></select></label>
<label><input type="checkbox" name="agree">同意</label><button type="button" onclick="document.querySelector('output').textContent='已点击'">点击测试</button><button type="submit">提交</button></form>
<a href="/slow" target="_blank">慢速链接</a><output>等待点击</output><div id="shadow"></div>
<script>document.querySelector('#shadow').attachShadow({mode:'open'}).innerHTML='<button>Shadow Button</button>';</script>`;
const server = createServer((req, res) => {
  res.setHeader('content-type', 'text/html; charset=utf-8');
  if (req.url.startsWith('/submitted')) setTimeout(() => res.end(`<title>Submitted</title><h1>提交成功</h1><p>${new URL(req.url, 'http://local').searchParams.get('name')}</p>`), 800);
  else if (req.url === '/slow') setTimeout(() => res.end('<title>Slow Page</title><h1>慢速页面已加载</h1>'), 800);
  else res.end(fixture);
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const harness = spawn(process.argv[2] || 'src-tauri/target/debug/examples/macos_browser_panel_acceptance', [root], { stdio: ['ignore','pipe','pipe'] });
let harnessErrors = ''; harness.stderr.on('data', chunk => { harnessErrors += chunk; });
let client; const pending = new Map(); let nextId = 0;
async function stop(child) {
  if (!child || child.exitCode !== null) return;
  const done = new Promise(resolve => child.once('exit', resolve)); child.kill('SIGTERM');
  await Promise.race([done, new Promise(resolve => setTimeout(resolve, 1500))]);
  if (child.exitCode === null) child.kill('SIGKILL');
}
try {
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`harness timeout: ${harnessErrors}`)), 15000);
    createInterface({ input: harness.stdout }).on('line', line => { if (line.startsWith('READY ')) { clearTimeout(timer); resolve(); } });
    harness.once('exit', code => { clearTimeout(timer); reject(new Error(`harness exit ${code}: ${harnessErrors}`)); });
  });
  const config = path.join(root, 'browser-control/panel-macos.json');
  assert.equal((await nativeCommand(config, { action: 'state' })).url, null);
  client = spawn(process.execPath, ['src-tauri/resources/browser-control-macos.mjs'], { env: { ...process.env, CREWVELA_BROWSER_MACOS_CONFIG: config }, stdio: ['pipe','pipe','pipe'] });
  let clientErrors = ''; client.stderr.on('data', c => { clientErrors += c; });
  createInterface({ input: client.stdout }).on('line', line => { const result = JSON.parse(line); pending.get(result.id)?.(result); pending.delete(result.id); });
  const rpc = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++nextId; const timer = setTimeout(() => { pending.delete(id); reject(new Error(`RPC ${method} timed out ${clientErrors}`)); }, 30000);
    pending.set(id, result => { clearTimeout(timer); if (result.error) reject(new Error(JSON.stringify(result.error))); else resolve(result.result); });
    client.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
  const call = async (name, args = {}, expectError = false) => {
    const result = await rpc('tools/call', { name, arguments: args });
    if (expectError) { assert.equal(result.isError, true); return result; }
    assert.ok(!result.isError, `${name}: ${JSON.stringify(result)}`); return result;
  };
  const text = result => result.content.filter(c => c.type === 'text').map(c => c.text).join('\n');
  const findRef = (snapshot, label) => {
    const line = snapshot.split('\n').find(line => line.startsWith('- ') && line.includes(JSON.stringify(label)));
    assert.ok(line, `missing control ${label}: ${snapshot}`); return line.match(/\[ref=([^\]]+)\]/)[1];
  };
  await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'native-acceptance', version: '1' } });
  client.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  const tools = await rpc('tools/list'); assert.ok(tools.tools.some(t => t.name === 'browser_snapshot'));
  assert.equal((await nativeCommand(config, { action: 'state' })).url, null, 'discovery must not create browser');
  let snapshot = text(await call('browser_navigate', { url: base }));
  assert.match(snapshot, /WKWebView 验收/); assert.ok(findRef(snapshot, 'Shadow Button'));
  const stale = findRef(snapshot, '点击测试');
  snapshot = text(await call('browser_click', { ref: stale })); assert.match(snapshot, /已点击/);
  await call('browser_click', { ref: stale }, true);
  snapshot = text(await call('browser_snapshot'));
  const fields = [{ name: '姓名', type: 'textbox', ref: findRef(snapshot, '姓名'), value: 'Native Agent' },
    { name: '城市', type: 'combobox', ref: findRef(snapshot, '城市'), value: 'b' }, { name: '同意', type: 'checkbox', ref: findRef(snapshot, '同意'), value: 'true' }];
  snapshot = text(await call('browser_fill_form', { fields })); assert.match(snapshot, /Native Agent/); assert.match(snapshot, /\[checked\]/);
  const pngResult = await call('browser_take_screenshot'); const png = Buffer.from(pngResult.content[0].data, 'base64');
  assert.deepEqual([...png.subarray(0,8)], [137,80,78,71,13,10,26,10]); assert.ok(png.length > 1000);
  writeFileSync(path.join(output, 'native-panel.png'), png);
  snapshot = text(await call('browser_snapshot'));
  const beforeSubmit = Date.now(); snapshot = text(await call('browser_click', { ref: findRef(snapshot, '提交') }));
  assert.match(snapshot, /提交成功/); assert.match(snapshot, /Native Agent/); assert.ok(Date.now() - beforeSubmit >= 700);
  snapshot = text(await call('browser_navigate', { url: base }));
  const beforeLink = Date.now(); snapshot = text(await call('browser_click', { ref: findRef(snapshot, '慢速链接') }));
  assert.match(snapshot, /慢速页面已加载/); assert.ok(Date.now() - beforeLink >= 700);
  snapshot = text(await call('browser_navigate_back')); assert.match(snapshot, /WKWebView 验收/);
  await call('browser_press_key', { key: 'Control+a' }, true);
  await call('browser_navigate', { url: 'file:///etc/passwd' }, true);
  await call('browser_close'); assert.equal((await nativeCommand(config, { action: 'state' })).url, null);
  snapshot = text(await call('browser_navigate', { url: base })); assert.match(snapshot, /WKWebView 验收/);
  // Revoking the setting must invalidate the already-running MCP connection.
  writeFileSync(path.join(root, 'browser-control.json'), JSON.stringify({ enabled: false, preferPanel: true }));
  await call('browser_snapshot', {}, true);
  const auth = JSON.parse(readFileSync(config));
  const rejected = await fetch(`http://127.0.0.1:${auth.port}/command`, { method: 'POST', headers: { 'content-type':'application/json', Authorization:`Bearer ${auth.token}`, Origin: 'https://untrusted.invalid' }, body: JSON.stringify({ action:'state' }) });
  assert.equal(rejected.status, 403);
  writeFileSync(path.join(output, 'acceptance.txt'), 'PASS: real WKWebView + stdio MCP discovery, navigate, snapshot/ref, shadow DOM, click, batch form, delayed submit/link, back, PNG, stale refs, close/reopen, revocation and origin rejection.\n');
  console.log('PASS: real WKWebView and stdio MCP acceptance; screenshot at ' + path.join(output, 'native-panel.png'));
} finally {
  await stop(client); await stop(harness); await new Promise(resolve => server.close(resolve));
  if (harnessErrors) writeFileSync(path.join(output, 'native-stderr.log'), harnessErrors);
  console.log('Temporary acceptance root: ' + root);
}
