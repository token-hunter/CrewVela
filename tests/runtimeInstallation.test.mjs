import test from 'node:test';
import assert from 'node:assert/strict';
import { downloadPercent, installationRunning, installationStep, supportsRuntimeInstallation } from '../src/runtimeInstallation.ts';

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
