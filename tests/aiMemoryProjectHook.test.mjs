import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const windows = process.platform === 'win32';
test('项目钩子只接纳本项目目录，拒绝其他目录、相似前缀及链接逃逸', { skip: !windows }, () => {
  const fixture = mkdtempSync(join(tmpdir(), 'cc-memory-hook-'));
  const root = join(fixture, 'project');
  const scripts = join(root, 'scripts');
  const outside = join(fixture, 'project-other');
  mkdirSync(scripts, { recursive: true });
  mkdirSync(outside);
  writeFileSync(join(root, '.ai-memory.toml'), 'workspace = "default"\nproject = "crewvela"\n');
  const script = join(scripts, 'ai-memory-project-hook.ps1');
  copyFileSync(fileURLToPath(new URL('../scripts/ai-memory-project-hook.ps1', import.meta.url)), script);
  const run = (body, cwd = root, agent = 'codex') => spawnSync('pwsh', [
    '-NoProfile', '-File', script, '-Agent', agent, '-Event', 'session-start',
    '-MemoryExecutable', join(fixture, 'missing-ai-memory.exe'),
    '-DataDir', join(fixture, 'data'), '-ServerUrl', 'http://127.0.0.1:49374',
    '-PolicyFile', join(fixture, 'policy.json'),
  ], { input: typeof body === 'string' ? body : JSON.stringify(body), cwd, encoding: 'utf8', timeout: 15000 });
  const admitted = (result) => {
    assert.equal(result.error, undefined);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /configured executable is missing/);
  };
  const denied = (result) => {
    assert.equal(result.error, undefined);
    assert.equal(result.status, 0);
    assert.equal(result.stdout.trim(), '{}');
    assert.doesNotMatch(result.stderr, /configured executable is missing/);
  };
  try {
    for (const agent of ['claude-code', 'codex', 'kimi-code', 'zcode']) {
      admitted(run({ cwd: root }, outside, agent));
      denied(run({ cwd: outside }, root, agent));
    }
    admitted(run({ cwd: scripts }));
    admitted(run({ cwd: root.toUpperCase() }));
    admitted(run({}));
    denied(run({}, outside));
    denied(run({ cwd: '../project' }));
    denied(run({ cwd: join(root, '..', 'project-other') }));
    denied(run({ cwd: ['invalid'] }));
    denied(run({ cwd: root, tool_name: 'Edit', tool_input: { file_path: join(outside, 'README.md') } }));
    denied(run({ cwd: root, tool_name: 'Read', tool_input: { paths: [join(root, 'README.md'), join(outside, 'README.md')] } }));
    denied(run({ cwd: root, tool_name: 'MultiEdit', tool_input: { edits: [{ filePath: join(outside, 'README.md') }] } }));
    denied(run({ cwd: root, tool_name: 'Bash', tool_input: { workdir: outside } }));
    // Authorization is separate from a marker: neither alone broadens capture.
    writeFileSync(join(outside, '.ai-memory.toml'), 'workspace = "other"\nproject = "beta"\n');
    denied(run({ cwd: outside }));
    writeFileSync(join(fixture, 'policy.json'), JSON.stringify({ version: 1, roots: [root, outside] }));
    admitted(run({ cwd: outside }));
    denied(run({ cwd: outside, tool_input: { file_path: join(root, 'README.md') } }));
    writeFileSync(join(scripts, '.ai-memory.toml'), 'workspace = "other"\nproject = "nested"\n');
    denied(run({ cwd: scripts }));
    denied(run({ cwd: root, tool_input: { file_path: join(scripts, 'README.md') } }));
    writeFileSync(join(fixture, 'policy.json'), JSON.stringify({ version: 1, roots: [root, outside, scripts] }));
    admitted(run({ cwd: scripts }));
    denied(run({ cwd: scripts, tool_input: { file_path: join(root, 'README.md') } }));
    writeFileSync(join(fixture, 'policy.json'), '{invalid');
    denied(run({ cwd: root }));
    writeFileSync(join(fixture, 'policy.json'), JSON.stringify({ version: 1, roots: [] }));
    admitted(run({ cwd: root }));
    denied(run({ cwd: outside }));
    denied(run('not json'));
    denied(run('[]'));
    symlinkSync(outside, join(root, 'linked-project'), 'junction');
    denied(run({ cwd: join(root, 'linked-project') }));
    denied(run({ cwd: root, tool_name: 'Write', tool_input: { file_path: join(root, 'linked-project', 'new.txt') } }));
    // File symlinks require Developer Mode or elevation on some Windows hosts.
    try {
      symlinkSync(join(outside, 'missing.txt'), join(root, 'linked-file.txt'), 'file');
      denied(run({ cwd: root, tool_name: 'Read', tool_input: { file_path: join(root, 'linked-file.txt') } }));
    } catch (error) {
      if (!['EPERM', 'EACCES'].includes(error.code)) throw error;
      console.warn('FILE_SYMLINK_TEST_SKIPPED: Windows did not permit creating a file symlink.');
    }
  } finally {
    // rm unlinks a junction; it does not traverse its target.
    rmSync(fixture, { recursive: true, force: true });
  }
});
