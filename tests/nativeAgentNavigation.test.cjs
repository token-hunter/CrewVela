const { test } = require('node:test');
const assert = require('node:assert/strict');
const { isRetiredRuntime } = require('../src/runtimeTaxonomy.ts');
const { retainedSwitchSource } = require('../src/sessionSwitch.ts');
const fs = require('node:fs');
const path = require('node:path');
const ts = require('typescript');

// Run the actual App callback with a cold connection; no React renderer or
// adapter needs to resolve before we can assert what the user sees.
function selectionHarness(target, resume, visited = []) {
  const source = fs.readFileSync(path.join(__dirname, '../src/App.tsx'), 'utf8');
  const ast = ts.createSourceFile('App.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  let callback;
  function visit(node) {
    if (ts.isVariableDeclaration(node) && node.name.getText(ast) === 'selectSession') callback = node.initializer.arguments[0];
    ts.forEachChild(node, visit);
  }
  visit(ast);
  assert.ok(callback, 'App must retain its session selection callback');
  const js = ts.transpileModule(`const select = ${callback.getText(ast)};`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const state = { selected: 'pi', loading: null, commands: [], errors: [] };
  const bindings = {
    isRetiredRuntime,
    retainedSwitchSource,
    sessions: [target], sidebarTerminalIds: [], UI_PREVIEW: false, switchRecords: [], visitedNativeSessions: visited,
    pendingSessionResumes: { current: new Set() },
    isNativeChatSession: item => item.providerId === `local-runtime-${item.agentRuntimeKind}`,
    isStructuredZCodeCli: () => false, isSessionActive: item => item.status === 'running',
    isRuntimeEnabled: () => true,
    setSelectedSessionId: id => { state.selected = id; },
    setMultiAgentWorkspaceProjectId: () => {},
    setLoadingSessionId: value => { state.loading = typeof value === 'function' ? value(state.loading) : value; },
    setSessions: () => {}, showError: error => state.errors.push(error), refreshSessions: async () => {},
    invoke: (command, args) => { state.commands.push([command, args]); return resume(); },
  };
  const select = new Function(...Object.keys(bindings), `${js}\nreturn select;`)(...Object.values(bindings));
  return { state, select };
}

const native = { id: 'codex', agentRuntimeKind: 'codex_chat', providerId: 'local-runtime-codex_chat', kind: 'terminal', status: 'stopped' };

test('unknown historical runtime can be selected for reading without launching a process', async () => {
  const target = { ...native, id: 'historical', agentRuntimeKind: 'removed_runtime', providerId: 'external' };
  const { state, select } = selectionHarness(target, async () => { throw new Error('must never launch'); });
  await select(target.id);
  assert.equal(state.selected, target.id);
  assert.deepEqual(state.commands, []);
  assert.deepEqual(state.errors, []);
});

test('cold native navigation selects the destination before connection work, without generic resume', async () => {
  let release;
  const cold = new Promise(resolve => { release = resolve; });
  const { state, select } = selectionHarness(native, () => cold);
  const pending = select(native.id);
  try {
    assert.equal(state.selected, 'codex', 'a cold ACP connection must not keep the Pi chat visible');
    assert.deepEqual(state.commands, [], 'native pane owns its asynchronous start and connection status');
  } finally { release(native); await pending; }
});

test('a broken Codex adapter cannot make navigation fail or return to the source chat', async () => {
  const { state, select } = selectionHarness(native, async () => { throw new Error('ACP 适配器已退出：连接已关闭'); });
  await select(native.id);
  assert.equal(state.selected, 'codex');
  assert.deepEqual(state.errors, [], 'adapter errors belong to the selected native pane');
  assert.deepEqual(state.commands, []);
});

test('ordinary CLI sessions select immediately and retain visible failure feedback', async () => {
  const target = { ...native, id: 'cli', agentRuntimeKind: 'codex', providerId: 'external' };
  const { state, select } = selectionHarness(target, async () => { throw new Error('CLI failed'); });
  await select(target.id);
  assert.equal(state.selected, 'cli', 'failed resume keeps the selected session and its retry view visible');
  assert.equal(state.loading, null);
  assert.deepEqual(state.commands, [['resume_session', { sessionId: 'cli' }]]);
  assert.deepEqual(state.errors, ['恢复会话失败：Error: CLI failed']);
});

test('ordinary CLI resume cannot overwrite later navigation or start the same process twice', async () => {
  const target = { ...native, id: 'cli', agentRuntimeKind: 'codex', providerId: 'external' };
  let release;
  const cold = new Promise(resolve => { release = resolve; });
  const { state, select } = selectionHarness(target, () => cold);
  const pending = select(target.id);
  assert.equal(state.selected, 'cli');
  await select(target.id);
  assert.equal(state.commands.length, 1);
  await select(null);
  release(target); await pending;
  assert.equal(state.selected, null);
  assert.equal(state.loading, null);
});

test('an already mounted stopped native chat still resumes, but failure leaves its destination visible', async () => {
  const { state, select } = selectionHarness(native, async () => { throw new Error('ACP 适配器已退出：连接已关闭'); }, [native.id]);
  await select(native.id);
  assert.equal(state.selected, 'codex');
  assert.deepEqual(state.commands, [['resume_session', { sessionId: 'codex' }]]);
  assert.match(state.errors[0], /ACP 适配器已退出：连接已关闭/);
});

test('a background native resume cannot take selection back after the user navigates away', async () => {
  let release;
  const cold = new Promise(resolve => { release = resolve; });
  const { state, select } = selectionHarness(native, () => cold, [native.id]);
  const pending = select(native.id);
  assert.equal(state.selected, native.id);
  await select(null);
  release(native);
  await pending;
  assert.equal(state.selected, null);
});

test('ZCode stopped chats select immediately and still restore on first visit', async () => {
  const target = { ...native, id: 'zcode', agentRuntimeKind: 'zcode', providerId: 'local-runtime-zcode' };
  const { state, select } = selectionHarness(target, async () => target);
  await select(target.id);
  assert.equal(state.selected, target.id);
  assert.deepEqual(state.commands, [['resume_session', { sessionId: 'zcode' }]]);
});
