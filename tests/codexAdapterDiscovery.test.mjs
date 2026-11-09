import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { resolveAdapterEntry } from '../tools/codex-acp-wrapper.mjs';

async function installFixture(root, version) {
  const directory = path.join(root, '@agentclientprotocol/codex-acp');
  await mkdir(path.join(directory, 'dist'), { recursive: true });
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ version }));
  const entry = path.join(directory, 'dist/index.js');
  await writeFile(entry, '// fixture');
  return entry;
}

test('安装形态从 npm exec 的 PATH 发现固定适配器，支持空格并跳过其他版本', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'crewvela adapter '));
  try {
    const wrong = path.join(root, 'other/node_modules');
    const expected = path.join(root, 'pinned/node_modules');
    await installFixture(wrong, '2.0.0');
    const entry = await installFixture(expected, '1.11.0');
    assert.equal(await resolveAdapterEntry([path.join(wrong, '.bin'), path.join(expected, '.bin')].join(path.delimiter)), entry);
    await assert.rejects(resolveAdapterEntry(path.join(wrong, '.bin')), /Pinned codex-acp 1\.11\.0/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('全局 npm 和 Unix Node 前缀的安装无需源码目录', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'crewvela global '));
  try {
    const globalPrefix = path.join(root, 'global');
    const entry = await installFixture(path.join(globalPrefix, 'node_modules'), '1.11.0');
    assert.equal(await resolveAdapterEntry(globalPrefix), entry);
    const unixPrefix = path.join(root, 'unix');
    const unixEntry = await installFixture(path.join(unixPrefix, 'lib/node_modules'), '1.11.0');
    assert.equal(await resolveAdapterEntry(path.join(unixPrefix, 'bin')), unixEntry);
  } finally { await rm(root, { recursive: true, force: true }); }
});
