const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/App.tsx'), 'utf8');
const action = source.slice(source.indexOf('  const handleProviderReorder ='), source.indexOf('  const selectableProviders ='));
function harness() {
  const providers = [{ id: 'a', name: 'old A' }, { id: 'b', name: 'B' }];
  const providerReorderInFlight = { current: false };
  const calls = [], errors = [];
  let current = providers;
  const setProviders = value => { current = typeof value === 'function' ? value(current) : value; };
  const showError = message => errors.push(message);
  const invoke = (command, args) => new Promise((resolve, reject) => calls.push({ command, args, resolve, reject }));
  const reorder = eval('(function(){' + ts.transpile(action + '\nreturn handleProviderReorder;', { target: ts.ScriptTarget.ES2022 }) + '})')();
  return { reorder, calls, errors, set: setProviders, current: () => current };
}
test('failed provider reorder preserves concurrent edits, additions and deletions', async t => {
  t.mock.method(console, 'error', () => {});
  const h = harness();
  const first = h.reorder(['b', 'a']);
  await h.reorder(['a', 'b']);
  assert.equal(h.calls.length, 1);
  h.set([{ id: 'a', name: 'edited A' }, { id: 'c', name: 'new C' }]);
  h.calls[0].reject(Error('save failed')); await first;
  assert.deepEqual(h.current(), [{ id: 'a', name: 'edited A' }, { id: 'c', name: 'new C' }]);
  assert.match(h.errors[0], /save failed/);
});
test('provider reorder rejects duplicate IDs and releases the gate after success', async () => {
  const h = harness();
  await h.reorder(['a', 'a']);
  assert.equal(h.calls.length, 0);
  const first = h.reorder(['b', 'a']);
  h.calls[0].resolve(); await first;
  assert.deepEqual(h.current().map(value => value.id), ['b', 'a']);
  const second = h.reorder(['a', 'b']);
  h.calls[1].resolve(); await second;
  assert.deepEqual(h.current().map(value => value.id), ['a', 'b']);
});
