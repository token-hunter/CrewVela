const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/nativeChatTabOrder.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2020 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);

test('move before and after in both directions without altering input', async () => {
  const { reorderNativeChatSlots: move } = await modulePromise;
  const tabs = ['a', 'b', 'c'];
  assert.deepEqual(move(tabs, tabs, 'a', 'c', true), ['b', 'c', 'a']);
  assert.deepEqual(move(tabs, tabs, 'c', 'a', false), ['c', 'a', 'b']);
  assert.deepEqual(move(tabs, tabs, 'a', 'b', false), tabs);
  assert.deepEqual(move(tabs, tabs, 'foreign', 'a', true), tabs);
  assert.deepEqual(tabs, ['a', 'b', 'c']);
});
test('persist workspace isolation and restore stable slots after handoff and reopen', async () => {
  const { loadNativeChatTabOrders: load, saveNativeChatTabOrders: save, orderedNativeChatSlots: order, reorderNativeChatSlots: move } = await modulePromise;
  let raw = null;
  const storage = { getItem: () => raw, setItem: (_, value) => { raw = value; } };
  const orders = { one: move([], ['slot-a', 'slot-b'], 'slot-b', 'slot-a', false), two: ['slot-a', 'slot-b'] };
  save(orders, storage);
  const restored = load(storage);
  assert.deepEqual(order(['slot-a', 'slot-b', 'new'], restored.one), ['slot-b', 'slot-a', 'new']);
  assert.deepEqual(order(['slot-a', 'slot-b'], restored.two), ['slot-a', 'slot-b']);
  assert.deepEqual(order(['slot-a'], restored.one), ['slot-a']);
  assert.deepEqual(order(['slot-a', 'slot-b'], restored.one), ['slot-b', 'slot-a']);
});
test('closed slots retain positions when visible tabs reorder', async () => {
  const { reorderNativeChatSlots: move } = await modulePromise;
  assert.deepEqual(move(['a', 'closed', 'b'], ['a', 'b', 'new'], 'new', 'a', false), ['new', 'closed', 'a', 'b']);
});
test('storage failures and corrupt data propagate instead of claiming success', async () => {
  const { loadNativeChatTabOrders: load, saveNativeChatTabOrders: save } = await modulePromise;
  assert.throws(() => save({}, { setItem: () => { throw Error('quota'); } }), /quota/);
  assert.throws(() => load({ getItem: () => '{' }));
  assert.throws(() => load({ getItem: () => '{"w":[3]}' }));
  assert.deepEqual(load({ getItem: () => '{"w":["a","a","b"]}' }), { w: ['a', 'b'] });
});
