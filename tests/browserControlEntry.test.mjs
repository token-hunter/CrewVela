import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { once } from 'node:events';

for (const throughAlias of [false, true]) {
  test(`浏览器 MCP 真实入口握手与工具发现：${throughAlias ? '迁移目录 junction/symlink' : '普通目录'}`, { timeout: 10000 }, async t => {
    const directory = mkdtempSync(path.join(tmpdir(), 'crewvela mcp 中文 '));
    const physical = path.join(directory, 'physical');
    const alias = path.join(directory, 'CrewVela');
    mkdirSync(physical);
    copyFileSync(fileURLToPath(new URL('../src-tauri/resources/browser-control-bridge.mjs', import.meta.url)), path.join(physical, 'bridge.mjs'));
    if (throughAlias) symlinkSync(physical, alias, process.platform === 'win32' ? 'junction' : 'dir');
    const engine = path.join(directory, 'engine.mjs');
    writeFileSync(engine, `import { createInterface } from 'node:readline';
createInterface({input: process.stdin}).on('line', line => {
 const m=JSON.parse(line); let result;
 if(m.method==='initialize') result={protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fixture',version:'1'}};
 if(m.method==='tools/list') result={tools:[{name:'browser_probe',inputSchema:{type:'object'}}]};
 if(result) process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\\n');
});`);
    const child = spawn(process.execPath, [path.join(throughAlias ? alias : physical, 'bridge.mjs'), process.execPath, engine, '--', '--isolated'], { stdio: ['pipe', 'pipe', 'pipe'] });
    const closed = once(child, 'close');
    const messages = [];
    const pending = new Map();
    const lines = createInterface({ input: child.stdout });
    lines.on('line', line => {
      const message = JSON.parse(line);
      messages.push(message);
      pending.get(message.id)?.resolve(message);
    });
    child.on('exit', (code, signal) => {
      for (const request of pending.values()) request.reject(new Error(`MCP 入口在返回协议响应前退出：${code ?? signal}`));
    });
    t.after(async () => {
      child.stdin.end();
      const timer = setTimeout(() => child.kill(), 1500);
      try { await closed; } finally { clearTimeout(timer); lines.close(); }
      assert.equal(path.dirname(path.resolve(directory)), path.resolve(tmpdir()));
      rmSync(directory, { recursive: true, force: true });
    });
    async function request(id, method, params) {
      const response = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`MCP ${method} 没有响应`)), 5000);
        pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
      });
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
      try {
        const message = await response;
        assert.equal(message.error, undefined);
        return message.result;
      } finally { pending.delete(id); }
    }
    await request(1, 'initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'fixture', version: '1' } });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} }) + '\n');
    const result = await request(2, 'tools/list', {});
    assert.deepEqual(result.tools.map(tool => tool.name), ['browser_probe']);
    assert.equal(messages.filter(message => message.error).length, 0);
  });
}
