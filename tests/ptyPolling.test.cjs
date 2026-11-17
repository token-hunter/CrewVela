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
    require: name => name === 'react' ? { useEffect() {}, useSyncExternalStore: (subscribe, get) => ({ subscribe, get }) } : name.includes('core')
      ? { invoke: command => { calls.push(command); return new Promise(resolve => { resolvePoll = resolve; }); } }
      : { sessionDisplayTitle: session => session.title },
    window: { dispatchEvent: event => events.push(event) },
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
    setInterval: fn => { timers.add(fn); return fn; }, clearInterval: fn => timers.delete(fn), console,
  });
  return { subscribe: module.exports.subscribePtyPolling, observeWork: module.exports.useCliPtyWorking, calls, events, timers,
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

test('CLI work changes independently of live session status and follows the PTY identity', async () => {
  const p = mountPolling(), snapshots = [];
  const stop = p.subscribe(sessions => snapshots.push(sessions));
  const work = p.observeWork(7);
  const changes = [];
  const unsubscribe = work.subscribe(() => changes.push(work.get()));
  const sessions = [{ id: 'codex', status: 'running', ptyId: 7, title: 'Codex', updatedAt: 1 }];
  for (const workingCliPtyIds of [[], [7], [7], [], [8]]) {
    const pending = p.tick();
    p.resolve({ chunks: {}, sessions, workingCliPtyIds }); await pending;
  }
  assert.deepEqual(changes, [true, false, false]);
  assert.equal(snapshots.length, 1, 'display activity updates do not redraw the whole workspace');
  assert.equal(work.get(), false);
  assert.equal(p.observeWork(8).get(), true);
  unsubscribe(); stop();
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
