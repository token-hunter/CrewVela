const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const ts = require('typescript');

const source = fs.readFileSync(path.join(__dirname, '../src/browserPanelState.ts'), 'utf8');
const output = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
}).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);

test('完整 http/https 地址直通', async () => {
  const { normalizeAddressInput } = await modulePromise;
  assert.equal(normalizeAddressInput('  https://example.com/docs?a=1 '), 'https://example.com/docs?a=1');
  assert.equal(normalizeAddressInput('http://example.com'), 'http://example.com');
});

test('localhost 本地地址补 http', async () => {
  const { normalizeAddressInput } = await modulePromise;
  assert.equal(normalizeAddressInput('localhost:3000'), 'http://localhost:3000');
  assert.equal(normalizeAddressInput('127.0.0.1:8080/admin'), 'http://127.0.0.1:8080/admin');
});

test('裸域名补 https', async () => {
  const { normalizeAddressInput } = await modulePromise;
  assert.equal(normalizeAddressInput('github.com/crewvela'), 'https://github.com/crewvela');
  assert.equal(normalizeAddressInput('a.b.example.co.uk/x'), 'https://a.b.example.co.uk/x');
});

test('非域名输入走搜索', async () => {
  const { normalizeAddressInput } = await modulePromise;
  assert.equal(
    normalizeAddressInput('rust cargo 教程'),
    `https://www.bing.com/search?q=${encodeURIComponent('rust cargo 教程')}`,
  );
  assert.equal(
    normalizeAddressInput('how to print'),
    `https://www.bing.com/search?q=${encodeURIComponent('how to print')}`,
  );
});

test('空输入返回 null', async () => {
  const { normalizeAddressInput } = await modulePromise;
  assert.equal(normalizeAddressInput(''), null);
  assert.equal(normalizeAddressInput('   '), null);
});

test('状态持久化往返与异常兜底', async (t) => {
  const m = await modulePromise;
  // 无 localStorage 环境（Node 默认）读取出默认值。
  assert.deepEqual(m.loadBrowserPanelState(), { url: '' });
  const backing = new Map();
  globalThis.localStorage = {
    getItem: (key) => (backing.has(key) ? backing.get(key) : null),
    setItem: (key, value) => backing.set(key, String(value)),
    removeItem: (key) => backing.delete(key),
  };
  t.after(() => {
    delete globalThis.localStorage;
  });
  m.saveBrowserPanelState({ url: 'https://example.com' });
  assert.deepEqual(m.loadBrowserPanelState(), { url: 'https://example.com' });
  // 损坏 JSON 落回默认，不让浏览面板崩掉。
  globalThis.localStorage.setItem('crewvela-browser-panel-state-v1', '{oops');
  assert.deepEqual(m.loadBrowserPanelState(), { url: '' });
});
