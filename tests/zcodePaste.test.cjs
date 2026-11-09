const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/components/zcodePaste.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);

test('composer paste classification: files win over text, plain text stays native, empty falls back to system clipboard', async () => {
  const { classifyComposerPaste } = await modulePromise;
  // 截图/资源管理器复制/浏览器图片：WebView clipboardData.files 非空 → 附件通路。
  assert.equal(classifyComposerPaste([{ name: 'image.png', size: 1 }], ''), 'files');
  assert.equal(classifyComposerPaste([{ name: 'a.txt', size: 1 }], '同贴文本'), 'files');
  // 纯文本：不拦截，走原生插入。
  assert.equal(classifyComposerPaste([], 'hello'), 'text');
  // 文本与文件皆空：可能是 WebView 未暴露的位图/文件，交后端直读系统剪贴板。
  assert.equal(classifyComposerPaste([], ''), 'system');
});

test('attachment refs merge dedupes, keeping existing order first', async () => {
  const { mergeAttachmentRefs } = await modulePromise;
  assert.deepEqual(mergeAttachmentRefs(['C:/a.png'], ['C:/a.png', 'C:/b.png']), ['C:/a.png', 'C:/b.png']);
  assert.deepEqual(mergeAttachmentRefs([], ['x', 'x']), ['x']);
  assert.deepEqual(mergeAttachmentRefs(['C:/tmp/0.png', 'C:/tmp/1.png'], ['C:/tmp/1.png', 'C:/tmp/2.png']), ['C:/tmp/0.png', 'C:/tmp/1.png', 'C:/tmp/2.png']);
});
