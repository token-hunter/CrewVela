const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '../src/sidebarShortcuts.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);

function withStorage(stub) {
  globalThis.window = { localStorage: stub };
  return () => { delete globalThis.window; };
}

test('配置会去重、丢弃未知 id，并按设置页导航顺序输出', async () => {
  const { sanitizeSidebarShortcuts, SIDEBAR_SHORTCUT_OPTIONS } = await modulePromise;
  assert.deepEqual(
    sanitizeSidebarShortcuts(['automation', 'mcp', 'bogus', 'modelEvaluation', 'automation']),
    ['automation'],
  );
  assert.deepEqual(sanitizeSidebarShortcuts('nope'), []);
  assert.deepEqual(sanitizeSidebarShortcuts(null), []);
  assert.deepEqual(sanitizeSidebarShortcuts([42, {}]), []);
  // 全部合法 id 都能保留
  assert.deepEqual(sanitizeSidebarShortcuts(SIDEBAR_SHORTCUT_OPTIONS.map((o) => o.id)), SIDEBAR_SHORTCUT_OPTIONS.map((o) => o.id));
});

test('没有存储值时回退默认：自动化', async () => {
  const { loadSidebarShortcuts, DEFAULT_SIDEBAR_SHORTCUTS } = await modulePromise;
  const stored = {};
  const cleanup = withStorage({
    getItem: (key) => (key in stored ? stored[key] : null),
    setItem: (key, value) => { stored[key] = String(value); },
  });
  try {
    assert.deepEqual(loadSidebarShortcuts(), DEFAULT_SIDEBAR_SHORTCUTS);
    assert.deepEqual(DEFAULT_SIDEBAR_SHORTCUTS, ['automation']);
  } finally { cleanup(); }
});

test('损坏或半损坏的存储值不会让侧栏崩溃', async () => {
  const { loadSidebarShortcuts } = await modulePromise;
  const cases = ['{not json', '"str"', '{"a":1}', JSON.stringify(['ghost', 'git'])];
  for (const raw of cases) {
    const cleanup = withStorage({ getItem: () => raw, setItem: () => {} });
    try {
      const loaded = loadSidebarShortcuts();
      assert.ok(Array.isArray(loaded));
      assert.ok(loaded.every((id) => typeof id === 'string'));
      if (raw === JSON.stringify(['ghost', 'git'])) assert.deepEqual(loaded, ['git']);
    } finally { cleanup(); }
  }
});

test('保存时写入净化后的 JSON，存储抛错被吞掉', async () => {
  const { saveSidebarShortcuts } = await modulePromise;
  const writes = [];
  let cleanup = withStorage({
    getItem: () => null,
    setItem: (key, value) => writes.push([key, value]),
  });
  try {
    saveSidebarShortcuts(['git', 'ghost', 'git', 'automation']);
    assert.equal(writes.length, 1);
    const [key, value] = writes[0];
    assert.equal(key, 'crewvela-sidebar-shortcuts');
    assert.deepEqual(JSON.parse(value), ['git', 'automation']);
  } finally { cleanup(); }
  cleanup = withStorage({ getItem: () => null, setItem: () => { throw new Error('quota'); } });
  try {
    saveSidebarShortcuts(['git']);
  } finally { cleanup(); }
});
