import assert from 'node:assert/strict';
import test from 'node:test';
import { CLI_RUNTIMES, launchChoices, launchModeLabel, isCliRuntime } from '../src/runtimeLaunch.ts';
test('六种 CLI 的模式目录独立，原生聊天与不存在的模式不混入', () => {
  assert.equal(CLI_RUNTIMES.length, 6);
  assert.equal(isCliRuntime('codex_chat'), false);
  assert.deepEqual(launchChoices('unsupported'), []);
  assert.equal(launchChoices('pi').some(choice => choice.id === 'yolo'), false);
  assert.match(launchModeLabel('kimi_code', 'yolo'), /Ask When Needed/);
  assert.match(launchModeLabel('kimi_code', 'auto'), /Never Ask/);
  assert.match(launchModeLabel('codex', 'unknown'), /未知/);
  assert.equal(launchModeLabel('codex', null), null);
});
