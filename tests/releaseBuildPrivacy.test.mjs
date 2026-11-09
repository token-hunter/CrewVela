import assert from 'node:assert/strict';
import test from 'node:test';
import { releaseBuildEnvironment, isReleaseBuild } from '../scripts/tauri-cli.mjs';

test('发行路径映射保留含空格路径与原有编译参数，不改动调用方环境', () => {
  const env = { CARGO_ENCODED_RUSTFLAGS: '-C\x1ftarget-cpu=generic', CARGO_HOME: 'C:\\cache\\cargo' };
  const result = releaseBuildEnvironment(env, 'C:\\Users\\builder\\source with spaces', 'C:\\Users\\builder');
  const flags = result.CARGO_ENCODED_RUSTFLAGS.split('\x1f');
  assert.deepEqual(flags.slice(0, 2), ['-C', 'target-cpu=generic']);
  assert.ok(flags.includes('--remap-path-prefix=C:\\Users\\builder\\source with spaces=/crewvela/source'));
  assert.ok(flags.includes('--remap-path-prefix=C:/Users/builder/source with spaces=/crewvela/source'));
  assert.ok(flags.includes('--remap-path-prefix=\\\\?\\C:\\Users\\builder=/crewvela/home'));
  assert.equal(env.CARGO_ENCODED_RUSTFLAGS, '-C\x1ftarget-cpu=generic');
  assert.equal(result.CARGO_PROFILE_RELEASE_STRIP, 'debuginfo');
  assert.ok(flags.findIndex(x => x.endsWith('=/crewvela/source')) > flags.findIndex(x => x.endsWith('=/crewvela/home')));
});

test('Mac 路径和自定义缓存根参与映射；仅 release build 启用', () => {
  const result = releaseBuildEnvironment({ RUSTFLAGS: '-C opt-level=3', RUSTUP_HOME: '/opt/toolchains' }, '/Users/builder/My Project', '/Users/builder');
  const flags = result.CARGO_ENCODED_RUSTFLAGS.split('\x1f');
  assert.deepEqual(flags.slice(0, 2), ['-C', 'opt-level=3']);
  assert.ok(flags.includes('--remap-path-prefix=/opt/toolchains=/crewvela/rustup'));
  assert.ok(flags.includes('--remap-path-prefix=/Users/builder/My Project=/crewvela/source'));
  assert.equal(isReleaseBuild(['build', '--no-bundle']), true);
  assert.equal(isReleaseBuild(['dev']), false);
  assert.equal(isReleaseBuild(['build', '--debug']), false);
  assert.equal(isReleaseBuild(['bundle', '--bundles', 'nsis']), false);
});
