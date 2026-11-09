import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,mkdirSync,writeFileSync,rmSync,existsSync,chmodSync} from 'node:fs';
import {execFileSync,spawnSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const root=fileURLToPath(new URL('../',import.meta.url));
const source=readFileSync(path.join(root,'start-first-run-test.command'),'utf8');
function bashPath() {
  if (process.platform!=='win32') return '/bin/bash';
  const gitPaths=execFileSync('where.exe',['git'],{encoding:'utf8'}).trim().split(/\r?\n/);
  for (const git of gitPaths) {
    const bash=path.resolve(path.dirname(git),'..','bin','bash.exe');
    if (existsSync(bash)) return bash;
  }
  throw new Error('macOS launcher regression needs Git Bash on Windows');
}
const bash=bashPath();
function fixture(run) {
  const base=mkdtempSync(path.join(tmpdir(),'crewvela mac 中文 '));
  try {
    const launcher=path.join(base,'start-first-run-test.command');
    writeFileSync(launcher,source);
    const argvFile=path.join(base,'received-args');
    const env={...process.env,CREWVELA_TEST_ARGV:argvFile};
    const makeApp=relative=>{
      const app=path.join(base,relative);
      const binary=path.join(app,'Contents','MacOS','CrewVela');
      mkdirSync(path.dirname(binary),{recursive:true});
      writeFileSync(binary,'#!/bin/bash\nprintf "%s\\0" "$@" > "$CREWVELA_TEST_ARGV"\nexit 23\n');
      chmodSync(binary,0o755);
      return app;
    };
    const launch=(args=[])=>spawnSync(bash,[launcher,...args],{cwd:base,env,encoding:'utf8'});
    const received=()=>readFileSync(argvFile,'utf8').split('\0').filter(Boolean);
    const physical=execFileSync(bash,['-c','cd -- "$1" && pwd -P','bash',base],{encoding:'utf8'}).trim();
    run({base,launcher,makeApp,launch,received,physical,argvFile});
  } finally {rmSync(base,{recursive:true,force:true});}
}

test('Finder launcher has valid Bash syntax, LF endings and executable Git metadata',()=>{
  assert.ok(source.startsWith('#!/bin/bash\n'));
  assert.doesNotMatch(source,/\r/);
  execFileSync(bash,['-n',path.join(root,'start-first-run-test.command')]);
  const mode=execFileSync('git',['ls-files','--stage','--','start-first-run-test.command'],{cwd:root,encoding:'utf8'});
  assert.match(mode,/^100755 /);
});
test('Finder launcher discovers bundled app and always passes the isolated root including spaces and Chinese',()=>fixture(({makeApp,launch,received,physical})=>{
  makeApp('src-tauri/target/aarch64-apple-darwin/release/bundle/macos/CrewVela · 协帆.app');
  const result=launch();
  assert.equal(result.status,23,result.stderr);
  assert.deepEqual(received(),['--test-data-dir',`${physical}/.test-data`]);
}));
test('explicit app and test directory stay literal and binary errors propagate',()=>fixture(({makeApp,launch,received,physical,base})=>{
  makeApp('custom 应用.app');
  const directory='another 中文 $(touch must-not-exist)';
  const result=launch(['--app','custom 应用.app','--data-dir',directory]);
  assert.equal(result.status,23,result.stderr);
  assert.deepEqual(received(),['--test-data-dir',`${physical}/${directory}`]);
  assert.equal(existsSync(path.join(base,'must-not-exist')),false);
}));
test('missing app and invalid arguments fail without opening a normal instance or creating test data',()=>fixture(({launch,argvFile,base})=>{
  for (const args of [[],['--app'],['--data-dir'],['--unknown'],['--app','']]) {
    const result=launch(args);
    assert.equal(result.status,2,JSON.stringify({args,...result}));
    assert.equal(existsSync(argvFile),false);
    assert.equal(existsSync(path.join(base,'.test-data')),false);
  }
  assert.equal(launch(['--help']).status,0);
  assert.equal(existsSync(argvFile),false);
}));
