const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/codexContextUsage.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);
test('unknown and malformed context data never produce a percentage', async () => {
  const { contextUsageDetails } = await modulePromise;
  for (const usage of [null, undefined, {used: 1, size: 0}, {used: -1, size: 10}, {used: NaN, size: 10}, {used: 1.5, size: 10}, {used: 10, size: Infinity}]) assert.equal(contextUsageDetails(usage), null);
});
test('latest occupancy can decrease after compaction and zero is valid', async () => {
  const { contextUsageDetails } = await modulePromise;
  assert.equal(contextUsageDetails({used: 168000, size: 258000}).percent, 65);
  assert.equal(contextUsageDetails({used: 24000, size: 258000}).percent, 9);
  assert.equal(contextUsageDetails({used: 0, size: 258000}).remainingPercent, 100);
});
test('over-limit data is honest while ring and remaining are bounded', async () => {
  const { contextUsageDetails, formatContextTokens } = await modulePromise;
  assert.deepEqual(contextUsageDetails({used: 110, size: 100}), {used: 110, size: 100, percent: 110, ringPercent: 100, remainingPercent: 0});
  assert.equal(formatContextTokens(168000), '168k');
  assert.equal(formatContextTokens(999), '999');
});
