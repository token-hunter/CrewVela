const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/App.tsx'), 'utf8');
const closeSource = source.slice(source.indexOf('  const closeTab ='), source.indexOf('  const renameTab ='));
function harness(invoke, sessionsReady = true, switchRecordsReady = true) {
  let workspace = { tabs: [{ id: 'a', title: 'A' }, { id: 'b', title: 'B' }], activeTabId: 'a', assignments: { one: 'a', two: 'a', three: 'b' } };
  let sessions = [{ id: 'one', title: 'One' }, { id: 'two', title: 'Two' }, { id: 'three', title: 'Three' }];
  let selection = 'one';
  const closingWorkspaceRef = { current: false }, workspaceRef = { current: workspace }, sessionsRef = { current: sessions };
  const nativeViewLocation = { current: { tabId: 'a', tabIds: ['a', 'b'] } };
  const selectedSessionByWorkspace = { current: { a: 'one', b: 'three' } };
  const UI_PREVIEW = false, window = { confirm: () => true }, switchRecords = [];
  const errors = [];
  const showError = message => errors.push(message);
  const useCallback = fn => fn;
  const tabOf = id => workspace.assignments[id];
  const refreshSessions = async () => {};
  const rememberedWorkspaceSession = id => sessions.find(s => s.id === selectedSessionByWorkspace.current[id]);
  const setSessions = update => { sessions = update(sessions); sessionsRef.current = sessions; };
  const setWorkspace = update => { workspace = update(workspace); workspaceRef.current = workspace; };
  const setSelectedSessionId = id => { selection = id; };
  const setMultiAgentWorkspaceProjectId = () => {};
  const isNativeChatSession = () => false, isStructuredZCodeCli = () => false;
  const js = ts.transpile(closeSource + '\nreturn closeTab;', { target: ts.ScriptTarget.ES2022 });
  const close = eval('(function(){' + js + '})')();
  return { close, errors, state: () => ({ workspace, sessions, selection }), addSession: () => { setSessions(s => [...s, { id: 'new', title: 'New' }]); setWorkspace(w => ({ ...w, assignments: { ...w.assignments, new: 'a' } })); }, navigate: () => { nativeViewLocation.current.tabId = 'b'; setWorkspace(w => ({ ...w, activeTabId: 'b' })); selection = 'three'; } };
}

test('workspace close failure preserves tab and surviving session assignments', async () => {
  const h = harness(async (_, { sessionId }) => { if (sessionId === 'two') throw Error('worktree dirty'); });
  await h.close({ id: 'a', title: 'A' });
  assert.deepEqual(h.state().workspace.tabs.map(t => t.id), ['a', 'b']);
  assert.equal(h.state().workspace.assignments.two, 'a');
  assert.deepEqual(h.state().sessions.map(s => s.id), ['two', 'three']);
  assert.match(h.errors[0], /worktree dirty/);
});

test('successful close selects previous valid workspace and concurrent closes cannot delete last tab', async () => {
  let release; const gate = new Promise(resolve => { release = resolve; }); const calls = [];
  const h = harness(async (_, { sessionId }) => { calls.push(sessionId); await gate; });
  const pending = h.close({ id: 'a', title: 'A' });
  await h.close({ id: 'b', title: 'B' });
  assert.deepEqual(calls, ['one']);
  release(); await pending;
  assert.deepEqual(h.state().workspace.tabs.map(t => t.id), ['b']);
  assert.equal(h.state().selection, 'three');
  assert.deepEqual(calls, ['one', 'two']);
});

test('new session arriving during close keeps its workspace and navigation is not stolen', async () => {
  let release; const gate = new Promise(resolve => { release = resolve; });
  const h = harness(async () => gate);
  const pending = h.close({ id: 'a', title: 'A' });
  h.addSession(); h.navigate(); release(); await pending;
  assert.equal(h.state().workspace.tabs.length, 2);
  assert.equal(h.state().workspace.assignments.new, 'a');
  assert.equal(h.state().selection, 'three');
  assert.match(h.errors[0], /新会话/);
});

test('stored workspace assignments are untouched until both initial snapshots succeed', () => {
  const effect = source.slice(source.indexOf('  // 等待首次会话和接力快照'), source.indexOf('  const tabOf ='));
  for (const readiness of [[false, false], [true, false], [false, true]]) {
    const [sessionsReady, switchRecordsReady] = readiness;
    const sessions = [], switchRecords = [];
    let writes = 0;
    const setWorkspace = () => { writes++; };
    const useEffect = fn => fn();
    eval(ts.transpile(effect, { target: ts.ScriptTarget.ES2022 }));
    assert.equal(writes, 0, String(readiness));
  }
});


test('closing a workspace before either initial snapshot is ready cannot clear persisted ownership', async () => {
  for (const readiness of [[false, false], [false, true], [true, false]]) {
    let calls = 0;
    const h = harness(async () => { calls++; }, ...readiness);
    const before = JSON.stringify(h.state());
    await h.close({ id: 'a', title: 'A' });
    assert.equal(JSON.stringify(h.state()), before);
    assert.equal(calls, 0);
    assert.match(h.errors[0], /正在加载/);
  }
});
