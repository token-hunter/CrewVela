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
  assert.equal(supportsRuntimeInstallation('codex'), true);
  assert.equal(supportsRuntimeInstallation('claude_code'), true);
  assert.equal(supportsRuntimeInstallation('zcode_cli'), false);
});
test('五种终端 CLI 支持安装；原生聊天和暂缓的 ZCode 不新增安装入口', () => {
  for (const kind of ['claude_code', 'codex', 'opencode', 'kimi_code', 'pi']) assert.equal(supportsRuntimeInstallation(kind), true);
  for (const kind of ['zcode_cli', 'zcode', 'claude_chat', 'codex_chat', 'kimi_chat', 'pi_chat', 'opencode_chat', 'deepseek_harness']) assert.equal(supportsRuntimeInstallation(kind), false);
  assert.match(runtimeInstallationInfo('opencode').channel, /1\.x/);
  assert.match(runtimeInstallationInfo('opencode').checksum, /SHA512/);
  assert.match(runtimeInstallationInfo('kimi_code').checksum, /SHA256/);
  assert.match(runtimeInstallationInfo('pi').loginHint, /\/login/);
  assert.equal(runtimeInstallationInfo('codex').loginLabel, '打开 CLI 登录');
});
