const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/components/zcodeMessageActions.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);
test('selection references preserve unicode, newlines and fenced text using native userselect JSON', async () => {
  const { addConversationQuote, withConversationQuotes } = await modulePromise;
  const quotes = addConversationQuote([], '中文\n```\n"引用"');
  const prompt = withConversationQuotes('问题', quotes);
  assert.deepEqual(JSON.parse(prompt.split('```userselect\n')[1].split('\n```')[0]), quotes);
  assert.equal(withConversationQuotes('问题', []), '问题');
  assert.equal(withConversationQuotes('', quotes).startsWith('# userselect:'), true);
});
test('native selection limits fail visibly without truncating or mutating current references', async () => {
  const { addConversationQuote } = await modulePromise;
  const existing = [{ text: 'a'.repeat(8000) }, { text: 'b'.repeat(8000) }];
  assert.throws(() => addConversationQuote([], 'x'.repeat(8001)));
  assert.throws(() => addConversationQuote(existing, 'x'));
  assert.throws(() => addConversationQuote(Array.from({length:8}, () => ({text:'x'})), 'y'));
  assert.equal(existing.length, 2);
});
test('editing requires the native row action and stable user identity', async () => {
  const { canEditUserMessage } = await modulePromise;
  assert.equal(canEditUserMessage({kind:'userInput',entityId:'old'}), false);
  assert.equal(canEditUserMessage({kind:'userInput',entityId:'old',actions:{canEdit:true,editDisposition:'fork'}}), false);
  assert.equal(canEditUserMessage({kind:'userInput',entityId:'latest',actions:{canEdit:true,editDisposition:"rewind"}}), true);
  assert.equal(canEditUserMessage({kind:'assistantText',entityId:'assistant',actions:{canEdit:true,editDisposition:"rewind"}}), false);
  assert.equal(canEditUserMessage({kind:'userInput',actions:{canEdit:true,editDisposition:"rewind"}}), false);
});
