import test from 'node:test';
import assert from 'node:assert/strict';
import { downloadPercent, installationRunning, installationStep, supportsRuntimeInstallation, runtimeInstallationInfo } from '../src/runtimeInstallation.ts';

test('下载百分比仅由真实总字节计算；未知大小和验证阶段不显示假进度', () => {
  assert.equal(downloadPercent({ phase: 'downloading', downloaded: 64, total: 100 }), 64);
  assert.equal(downloadPercent({ phase: 'downloading', downloaded: 64, total: null }), undefined);
  assert.equal(downloadPercent({ phase: 'downloading', downloaded: 64, total: 0 }), undefined);
  assert.equal(downloadPercent({ phase: 'verifying', downloaded: 100, total: 100 }), undefined);
  assert.equal(downloadPercent({ phase: 'downloading', downloaded: 101, total: 100 }), 100);
});
test('步骤和忙碌状态区分校验、安装、登录与终止结果', () => {
  for (const phase of ['checking', 'downloading', 'installing', 'verifying']) assert.equal(installationRunning({ phase }), true);
  for (const phase of ['ready', 'failed', 'cancelled']) assert.equal(installationRunning({ phase }), false);
  assert.equal(installationStep({ phase: 'verifying' }), 2);
  assert.equal(installationStep({ phase: 'ready' }), 3);
});
test('七种终端 CLI 和七种原生聊天都有管理入口，退役 Runtime 不提供入口', () => {
  for (const kind of ['claude_code', 'codex', 'opencode', 'kimi_code', 'pi', 'antigravity', 'zcode_cli']) assert.equal(supportsRuntimeInstallation(kind), true, kind);
  for (const kind of ['zcode', 'claude_chat', 'codex_chat', 'kimi_chat', 'pi_chat', 'opencode_chat', 'deepseek_harness']) assert.equal(supportsRuntimeInstallation(kind), true, kind);
  assert.equal(supportsRuntimeInstallation('oh_my_pi'), false);
  assert.match(runtimeInstallationInfo('antigravity').checksum, /SHA-512/);
  assert.match(runtimeInstallationInfo('opencode').channel, /1\.x/);
  assert.match(runtimeInstallationInfo('opencode').checksum, /SHA512/);
  assert.match(runtimeInstallationInfo('kimi_code').checksum, /SHA256/);
  assert.match(runtimeInstallationInfo('pi').loginHint, /\/login/);
  assert.equal(runtimeInstallationInfo('codex').loginLabel, '打开 CLI 登录');
});
