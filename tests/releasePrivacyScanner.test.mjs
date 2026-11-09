import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

test('发行扫描拒绝 UTF-8/UTF-16 用户路径，报告不回显原值', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'crewvela privacy '));
  try {
    const scanner = fileURLToPath(new URL('../scripts/check-release-privacy.mjs', import.meta.url));
    const file = path.join(root, 'fixture.bin');
    const privatePath = 'C:\\Users\\private-builder\\project';
    await writeFile(file, Buffer.concat([Buffer.from('/Users/private-builder/code\0'), Buffer.from(privatePath, 'utf16le')]));
    const red = spawnSync(process.execPath, [scanner, file], { encoding: 'utf8' });
    assert.equal(red.status, 1, red.stderr);
    const report = JSON.parse(red.stdout);
    assert.deepEqual(report[0].findings.map(x => x.kind).sort(), ['mac-user-path', 'windows-user-path']);
    assert.ok(!red.stdout.includes('private-builder'));
    await writeFile(file, '/crewvela/source/src/main.rs');
    const green = spawnSync(process.execPath, [scanner, file], { encoding: 'utf8' });
    assert.equal(green.status, 0, green.stderr);
  } finally { await rm(root, { recursive: true, force: true }); }
});
