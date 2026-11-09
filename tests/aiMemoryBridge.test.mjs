import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, symlinkSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { projectArguments, withinProject, allowedRoots, authorizedProjectRoot, resolveProjectRoute } from '../scripts/ai-memory-mcp-bridge.mjs';

const fields = { workspace: {}, project: {}, cwd: {} };
test('静态桥给当前项目明确坐标，不补齐部分作用域、不覆盖显式或跨项目调用', () => {
  const args = { query: 'previous work' };
  assert.deepEqual(projectArguments(fields, args, '/cwd'), { ...args, workspace: 'default', project: 'crewvela', cwd: '/cwd' });
  assert.deepEqual(args, { query: 'previous work' });
  for (const special of [
    { workspace: 'other' }, { project: 'other' }, { workspace: 'other', project: 'named' },
    { global: true }, { scopes: [] }, { scope: 'global' }, { scope: 'profile' },
  ]) assert.deepEqual(projectArguments(fields, special), special);
  assert.deepEqual(projectArguments({}, args), args);
  assert.deepEqual(projectArguments(fields, args, '/cwd', { workspace: 'work', project: 'beta' }),
    { ...args, workspace: 'work', project: 'beta', cwd: '/cwd' });
});

test('授权根目录和真实 marker 必须同时存在，损坏策略拒绝捕获', () => {
  const fixture = realpathSync(mkdtempSync(join(tmpdir(), 'cc-memory-policy-')));
  const root = join(fixture, 'project');
  const other = join(fixture, 'other');
  const policy = join(fixture, 'policy.json');
  mkdirSync(join(root, 'child'), { recursive: true });
  mkdirSync(other);
  try {
    assert.deepEqual(allowedRoots(policy, root), [root]);
    assert.equal(authorizedProjectRoot(root, [root]), null);
    writeFileSync(join(root, '.ai-memory.toml'), 'workspace="work"\nproject="alpha"');
    writeFileSync(join(other, '.ai-memory.toml'), 'workspace="work"\nproject="beta"');
    assert.equal(authorizedProjectRoot(join(root, 'child'), [root]), root);
    writeFileSync(join(root, 'child', '.ai-memory.toml'), 'workspace="work"\nproject="nested"');
    assert.equal(authorizedProjectRoot(join(root, 'child'), [root]), null);
    assert.equal(authorizedProjectRoot(join(root, 'child'), [root, join(root, 'child')]), join(root, 'child'));
    assert.equal(authorizedProjectRoot(other, [root]), null);
    writeFileSync(policy, JSON.stringify({ version: 1, roots: [root, other] }));
    assert.equal(authorizedProjectRoot(other, allowedRoots(policy, root)), other);
    writeFileSync(policy, JSON.stringify({ version: 1, roots: [] }));
    assert.deepEqual(allowedRoots(policy, root), [root]);
    assert.equal(authorizedProjectRoot(other, allowedRoots(policy, root)), null);
    assert.equal(authorizedProjectRoot(root, [join(fixture, 'deleted'), root]), root);
    writeFileSync(policy, '{broken');
    assert.deepEqual(allowedRoots(policy), []);
    writeFileSync(policy, JSON.stringify({ version: 1, roots: ['relative'] }));
    assert.deepEqual(allowedRoots(policy), []);
    symlinkSync(other, join(root, 'outside-link'), process.platform === 'win32' ? 'junction' : 'dir');
    assert.equal(authorizedProjectRoot(join(root, 'outside-link'), [root]), null);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});

test('marker 坐标使用 CLI 检查结果且拒绝无效策略、远端路由和未准入事件', () => {
  const fixture = mkdtempSync(join(tmpdir(), 'cc-memory-route-'));
  writeFileSync(join(fixture, '.ai-memory.toml'), 'workspace="work"\nproject="beta"');
  const accepted = { marker_present: true, scope_resolution: 'explicit', server_resolution: 'install-default',
    policy_state: 'inactive', admits_capture: true, scope: { workspace: 'work', project: 'beta' } };
  const route = (report, status = 0) => resolveProjectRoute({ cwd: fixture, projectRoot: fixture,
    executable: 'fixture', dataDir: fixture, serverUrl: 'http://127.0.0.1:49374/mcp?flavor=moonshot',
    inspect(args, input) {
      assert.ok(args.includes('--check-capture'));
      assert.equal(args[args.indexOf('--server-url') + 1], 'http://127.0.0.1:49374');
      assert.equal(JSON.parse(input).cwd, fixture);
      return { status, stdout: JSON.stringify(report) };
    } });
  try {
    assert.deepEqual(route(accepted), { workspace: 'work', project: 'beta' });
    for (const invalid of [{ policy_state: 'invalid' }, { admits_capture: false }, { marker_present: false },
      { server_resolution: 'profile' }, { scope_resolution: 'server-derived' }, { scope: { workspace: 'work' } }]) {
      assert.equal(route({ ...accepted, ...invalid }), null);
    }
    assert.equal(route(accepted, 1), null);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});
test('桥接目录校验允许子目录，拒绝同名前缀的其他项目', () => {
  const fixture = mkdtempSync(join(tmpdir(), 'cc-memory-bridge-'));
  const root = join(fixture, 'project');
  mkdirSync(join(root, 'child'), { recursive: true });
  mkdirSync(join(fixture, 'project-other'));
  try {
    assert.equal(withinProject(root, root), true);
    assert.equal(withinProject(join(root, 'child'), root), true);
    assert.equal(withinProject(join(fixture, 'project-other'), root), false);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});

test('其他项目启动用户级桥时返回空工具，不连接不可达服务', async () => {
  const fixture = mkdtempSync(join(tmpdir(), 'cc-memory-outside-'));
  const client = new Client({ name: 'cc-memory-outside-test', version: '1' });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [fileURLToPath(new URL('../scripts/ai-memory-mcp-bridge.mjs', import.meta.url)), 'http://127.0.0.1:1/mcp'],
    cwd: fixture,
  });
  try {
    await client.connect(transport);
    const result = await client.listTools();
    assert.deepEqual(result.tools, []);
    const denied = await client.callTool({ name: 'memory_query', arguments: { query: 'anything' } });
    assert.equal(denied.isError, true);
  } finally {
    await client.close();
    rmSync(fixture, { recursive: true, force: true });
  }
});
