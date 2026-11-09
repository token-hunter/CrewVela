const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const ts = require('typescript');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../src/ptyPolling.ts'), 'utf8');
function mountPolling() {
  const calls = [], events = [], timers = new Set();
  let resolvePoll;
  const module = { exports: {} };
  const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(output, {
    exports: module.exports,
    require: name => name === 'react' ? { useEffect() {} } : name.includes('core')
      ? { invoke: command => { calls.push(command); return new Promise(resolve => { resolvePoll = resolve; }); } }
      : { sessionDisplayTitle: session => session.title },
    window: { dispatchEvent: event => events.push(event) },
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
    setInterval: fn => { timers.add(fn); return fn; }, clearInterval: fn => timers.delete(fn), console,
  });
  return { subscribe: module.exports.subscribePtyPolling, calls, events, timers,
    tick: () => [...timers][0]?.(), resolve: result => resolvePoll(result) };
}
test('主网格和多个侧栏终端共享一次消费，隐藏终端仍收到完整分发', async () => {
  const p = mountPolling(); const snapshots = [[], [], []];
  const stops = snapshots.map(list => p.subscribe(sessions => list.push(sessions)));
  assert.equal(p.timers.size, 1);
  const pending = p.tick(); await p.tick();
  assert.deepEqual(p.calls, ['pty_poll_all']);
  const chunks = [{ appSessionId: 'main', bytes: [65] }, { appSessionId: 'side1', bytes: [66] }, { appSessionId: 'side2', bytes: [67] }];
  const sessions = chunks.map((chunk, i) => ({ id: chunk.appSessionId, status: 'running', ptyId: i, title: chunk.appSessionId, updatedAt: 1 }));
  p.resolve({ chunks, sessions }); await pending;
  assert.equal(p.events.length, 1); assert.equal(p.events[0].detail, chunks);
  for (const snapshot of snapshots) assert.equal(snapshot[0], sessions);
  stops[1](); assert.equal(p.timers.size, 1);
  const next = p.tick(); const exited = sessions.map(session => ({ ...session, status: 'stopped', ptyId: null, updatedAt: 2 }));
  p.resolve({ chunks: [], sessions: exited }); await next;
  assert.equal(snapshots[0].length, 2); assert.equal(snapshots[1].length, 1); assert.equal(snapshots[2].length, 2);
  stops[0](); stops[2](); assert.equal(p.timers.size, 0);
});
test('相同回调的两个订阅独立退出，在途轮询期间重挂载不会重复消费', async () => {
  const p = mountPolling(); const received = []; const callback = sessions => received.push(sessions);
  const stop1 = p.subscribe(callback), stop2 = p.subscribe(callback); stop1();
  const pending = p.tick(); stop2(); const stop3 = p.subscribe(callback); await p.tick();
  assert.equal(p.calls.length, 1);
  const sessions = [{ id: 'main', status: 'running', ptyId: 1, title: 'main', updatedAt: 1 }];
  p.resolve({ chunks: [], sessions }); await pending;
  assert.equal(received.length, 1); stop3(); assert.equal(p.timers.size, 0);
});
