const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/claudeCommandCatalog.ts'), 'utf8');
const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const modulePromise = import(`data:text/javascript;base64,${Buffer.from(output).toString('base64')}`);

test('commands are classified into builtin / skill / plugin groups by name', async () => {
  const { classifyClaudeCommand } = await modulePromise;
  assert.equal(classifyClaudeCommand('compact'), 'builtin');
  assert.equal(classifyClaudeCommand('model'), 'builtin');
  assert.equal(classifyClaudeCommand('deep-research'), 'builtin');
  assert.equal(classifyClaudeCommand('firecrawl-scrape'), 'skill');
  assert.equal(classifyClaudeCommand('code-review'), 'skill');
  assert.equal(classifyClaudeCommand('ponytail:ponytail-review'), 'plugin');
  assert.equal(classifyClaudeCommand('eli5:eli5'), 'plugin');
});

test('duplicate names collapse to one entry, preferring the described variant', async () => {
  const { dedupeClaudeCommands } = await modulePromise;
  const deduped = dedupeClaudeCommands([
    { name: 'code-review' },
    { name: 'code-review', description: 'Review the changes since a fixed point' },
    { name: 'review', description: 'builtin review' },
    { name: 'review', description: 'builtin review' },
    { name: 'compact', description: 'first' },
  ]);
  assert.equal(deduped.length, 3);
  assert.equal(deduped.find((command) => command.name === 'code-review').description, 'Review the changes since a fixed point');
  assert.equal(deduped.filter((command) => command.name === 'review').length, 1);
  assert.equal(deduped.find((command) => command.name === 'compact').description, 'first');
});

test('grouping orders builtin then skill then plugin and keeps upstream order inside groups', async () => {
  const { groupClaudeCommands } = await modulePromise;
  const grouped = groupClaudeCommands([
    { name: 'arch-map', description: 'skill' },
    { name: 'compact', description: 'builtin' },
    { name: 'ponytail:ponytail-help', description: 'plugin' },
    { name: 'agents', description: 'builtin' },
    { name: 'firecrawl-scrape', description: 'skill' },
  ]);
  assert.deepEqual(grouped.builtin.map((command) => command.name), ['compact', 'agents']);
  assert.deepEqual(grouped.skill.map((command) => command.name), ['arch-map', 'firecrawl-scrape']);
  assert.deepEqual(grouped.plugin.map((command) => command.name), ['ponytail:ponytail-help']);
});

test('real-world snapshot shape maps into menu commands', async () => {
  const { snapshotClaudeCommands, groupClaudeCommands } = await modulePromise;
  const commands = snapshotClaudeCommands({ commands: [
    { name: 'model', description: 'Set the AI model' },
    { name: 'webnovel-writer:webnovel-write', description: 'plugin' },
    { name: 'zoom-out', inputHint: 'topic' },
  ] });
  assert.equal(commands.length, 3);
  assert.deepEqual(groupClaudeCommands(commands).skill.map((command) => command.name), ['zoom-out']);
});
