const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/App.tsx'), 'utf8');
const actions = source.slice(source.indexOf('  const archiveProjectSessions ='), source.indexOf('  const launchTerminal ='));
function harness(invoke) {
  const sessions = [{ id: 'one', title: 'One', projectId: 'a' }, { id: 'two', title: 'Two', projectId: 'a' }, { id: 'three', title: 'Three', projectId: 'b' }];
  const liveSessions = sessions.slice(0, 2), tabSessions = liveSessions;
  const selectedSessionId = 'one', window = { confirm: () => true }, useCallback = fn => fn;
  let selection = selectedSessionId, refreshes = 0;
  const errors = [], showError = message => errors.push(message);
  const setSelectedSessionId = value => { selection = typeof value === 'function' ? value(selection) : value; };
  const refreshSessions = async () => { refreshes++; };
  const api = eval('(function(){' + ts.transpile(actions + '\nreturn { archiveProjectSessions, stopAll, deleteAllSessions };', { target: ts.ScriptTarget.ES2022 }) + '})')();
  return { ...api, select: setSelectedSessionId, errors, state: () => ({ selection, refreshes }) };
}
test('project archive does not clear a later selection outside the archived project', async () => {
  let release; const h = harness(() => new Promise(resolve => { release = resolve; }));
  const pending = h.archiveProjectSessions({ id: 'a', name: 'A' });
  h.select('three'); release(); await pending;
  assert.equal(h.state().selection, 'three');
  assert.equal(h.state().refreshes, 1);
});
test('batch delete clears only successfully deleted current selection and reports failures', async () => {
  const h = harness(async (_, { sessionId }) => { if (sessionId === 'two') throw Error('dirty worktree'); });
  h.select('two'); await h.deleteAllSessions();
  assert.equal(h.state().selection, 'two');
  assert.match(h.errors[0], /Two.*dirty worktree/);
  h.select('one'); await h.deleteAllSessions();
  assert.equal(h.state().selection, null);
  h.select('three'); await h.deleteAllSessions();
  assert.equal(h.state().selection, 'three');
});
test('batch stop reports refused stop while continuing other sessions and refreshing', async t => {
  t.mock.method(console, 'error', () => {});
  const stopped = [];
  const h = harness(async (_, { sessionId }) => { stopped.push(sessionId); if (sessionId === 'one') throw Error('exit unverified'); });
  await h.stopAll();
  assert.deepEqual(stopped, ['one', 'two']);
  assert.match(h.errors[0], /One.*exit unverified/);
  assert.equal(h.state().refreshes, 1);
});
